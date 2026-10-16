//! Physical & mental mechanics (PRD Phase 4 (c), ticket 22).
//!
//! Pure functions that turn the `Speed`, `Fitness` and `Strength`
//! attributes into mechanic parameters, each read through the response
//! curve (`sim_math::curve`) and the fit multiplier
//! (`sim_math::effective_attribute`). Direction-of-effect is guaranteed
//! by the curve's strict monotonicity and is unit-tested per attribute
//! (PRD §13.1 `A10`; these three are unit-only, not in the full-match
//! sweep — ticket 29).
//!
//! # Mechanics
//!
//! - `Speed` → sprint speed in m/tick (rising curve, scaled to a band
//!   below the physics speed cap). Lower with the ball than without
//!   (ball-carrier discount).
//! - `Fitness` → stamina drain multiplier (falling curve): higher
//!   fitness ⇒ lower drain per costful action (running, pressing,
//!   tackling) and a recovery rate. Both live in the tuning struct (§11).
//! - `Strength` → held-ball retention probability in [0, 1] (rising
//!   curve): the carrier's chance to keep the ball against an incoming
//!   tackle. (The goalkeeper contested-claims variant lands in ticket 24.)
//!
//! # Aggression
//!
//! Press and tackle stamina costs are scaled by `Aggression`. That
//! scaling lands in ticket 25; here the aggression factor is a
//! placeholder of 1.0.

#![allow(
    clippy::cast_possible_truncation,
    reason = "effective attributes are bounded to [0, 200] (attribute ≤ 200, multiplier ≤ 1.0), so f32 -> i32 is lossless"
)]

use sim_math::{curve, effective_attribute, Tuning};

use crate::Attribute;

/// Index of `Attribute::OutfieldSpeed` in `Attribute::ALL` /
/// `Tuning::curve_params`. Mirrors the attribute declaration order; the
/// cross-crate consistency is asserted by
/// `sim_components::attribute_tests::a10_curve_monotonicity_all_attributes`.
const SPEED_IDX: usize = Attribute::OutfieldSpeed as usize;
/// Index of `Attribute::OutfieldFitness`.
const FITNESS_IDX: usize = Attribute::OutfieldFitness as usize;
/// Index of `Attribute::OutfieldStrength`.
const STRENGTH_IDX: usize = Attribute::OutfieldStrength as usize;

/// Sprint speed band: the curve output maps to this band (m/s), so even
/// a maxed `Speed` stays below the physics speed cap and leaves headroom
/// for the ball-carrier discount.
const SPEED_MIN_MPS: f32 = 5.0;
const SPEED_MAX_MPS: f32 = 10.0;

/// Ball-carrier speed discount: a player in possession sprints at this
/// fraction of their free speed (lower with the ball than without).
const BALL_CARRIER_SPEED_FACTOR: f32 = 0.8;

/// Aggression → stamina-cost multiplier for press/tackle. Placeholder:
/// scaled by 1.0 here; ticket 25 wires it to the `Aggression` attribute.
#[must_use]
pub const fn aggression_stamina_factor(_aggression: u8) -> f32 {
    1.0
}

/// Sprint speed in metres per tick. Rises monotonically with `Speed`
/// through the response curve and fit multiplier.
///
/// `with_ball` applies the ball-carrier discount, so possession speed is
/// strictly below free speed at every attribute value.
#[must_use]
pub fn sprint_speed(speed: u8, fit_multiplier: f32, tuning: &Tuning, with_ball: bool) -> f32 {
    let effective = effective_attribute(speed, fit_multiplier);
    let curve_val = curve(effective as i32, &tuning.curve_params[SPEED_IDX]); // rising, [0, 1]
    let base_mps = (SPEED_MAX_MPS - SPEED_MIN_MPS).mul_add(curve_val, SPEED_MIN_MPS);
    let base = base_mps / 60.0; // m/s → m/tick
    let factor = if with_ball {
        BALL_CARRIER_SPEED_FACTOR
    } else {
        1.0
    };
    base * factor
}

/// Stamina drain multiplier from `Fitness` (falling curve) in [0, 1].
///
/// Higher fitness ⇒ lower curve output ⇒ lower effective drain per
/// costful action. Multiplied by the base per-action drain in the
/// execution system.
#[must_use]
pub fn fitness_drain_factor(fitness: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(fitness, fit_multiplier);
    // `effective` is bounded to [0, 200], so the cast is non-truncating.
    curve(effective as i32, &tuning.curve_params[FITNESS_IDX]) // falling, [0, 1]
}

/// Held-ball retention probability in [0, 1] from `Strength` (rising
/// curve). The carrier's chance to keep the ball against an incoming
/// tackle; higher `Strength` ⇒ higher retention.
#[must_use]
pub fn held_ball_retention_probability(strength: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(strength, fit_multiplier);
    // `effective` is bounded to [0, 200], so the cast is non-truncating.
    curve(effective as i32, &tuning.curve_params[STRENGTH_IDX]) // rising, [0, 1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_math::Tuning;

    const TOL: f32 = 1e-6;

    fn tuning() -> Tuning {
        Tuning::default()
    }

    fn fit() -> f32 {
        1.0
    }

    // === Speed ===

    #[test]
    fn sprint_speed_is_strictly_rising_in_speed() {
        let tuning = tuning();
        let mut prev = sprint_speed(1, fit(), &tuning, false);
        for speed in 2..=200u8 {
            let value = sprint_speed(speed, fit(), &tuning, false);
            assert!(
                value > prev + TOL,
                "sprint_speed not strictly rising at Speed {speed}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn sprint_speed_is_lower_with_the_ball() {
        let tuning = tuning();
        for speed in [1, 50, 100, 150, 200] {
            let free = sprint_speed(speed, fit(), &tuning, false);
            let with_ball = sprint_speed(speed, fit(), &tuning, true);
            assert!(
                with_ball < free - TOL,
                "Speed {speed}: with-ball speed {with_ball} must be below free speed {free}"
            );
            assert!(
                free.mul_add(-BALL_CARRIER_SPEED_FACTOR, with_ball).abs() < TOL,
                "Speed {speed}: with-ball speed must equal free * {BALL_CARRIER_SPEED_FACTOR}"
            );
        }
    }

    #[test]
    fn sprint_speed_stays_under_physics_speed_cap() {
        // 10 m/s rescaled to m/tick is `sim_physics::MAX_PLAYER_SPEED`.
        let cap = 10.0 / 60.0;
        let tuning = tuning();
        for speed in 1..=200u8 {
            let free = sprint_speed(speed, fit(), &tuning, false);
            assert!(
                free <= cap + TOL,
                "Speed {speed}: free speed {free} above cap {cap}"
            );
        }
    }

    #[test]
    fn sprint_speed_honors_fit_multiplier() {
        let tuning = tuning();
        // Lower fit multiplier ⇒ lower effective attribute ⇒ lower speed
        // for the rising curve.
        let full = sprint_speed(100, 1.00, &tuning, false);
        let partial = sprint_speed(100, 0.75, &tuning, false);
        assert!(
            full > partial + TOL,
            "free speed at full fit ({full}) must exceed partial fit ({partial})"
        );
    }

    // === Fitness ===

    #[test]
    fn fitness_drain_factor_is_strictly_falling_in_fitness() {
        let tuning = tuning();
        let mut prev = fitness_drain_factor(1, fit(), &tuning);
        for fitness in 2..=200u8 {
            let value = fitness_drain_factor(fitness, fit(), &tuning);
            assert!(
                value < prev - TOL,
                "fitness_drain_factor not strictly falling at Fitness {fitness}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn fitness_drain_factor_stays_in_unit_interval() {
        let tuning = tuning();
        for fitness in 1..=200u8 {
            let value = fitness_drain_factor(fitness, fit(), &tuning);
            assert!(
                (0.0..=1.0).contains(&value),
                "fitness_drain_factor {fitness} = {value} outside [0, 1]"
            );
        }
    }

    #[test]
    fn fitness_drain_factor_honors_fit_multiplier() {
        let tuning = tuning();
        // Lower fit multiplier ⇒ lower effective attribute ⇒ higher drain
        // factor for the falling curve.
        let full = fitness_drain_factor(100, 1.00, &tuning);
        let partial = fitness_drain_factor(100, 0.75, &tuning);
        assert!(
            full < partial - TOL,
            "drain factor at full fit ({full}) must be below partial fit ({partial})"
        );
    }

    // === Strength ===

    #[test]
    fn held_ball_retention_is_strictly_rising_in_strength() {
        let tuning = tuning();
        let mut prev = held_ball_retention_probability(1, fit(), &tuning);
        for strength in 2..=200u8 {
            let value = held_ball_retention_probability(strength, fit(), &tuning);
            assert!(
                value > prev + TOL,
                "held_ball_retention_probability not strictly rising at Strength {strength}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn held_ball_retention_stays_in_unit_interval() {
        let tuning = tuning();
        for strength in 1..=200u8 {
            let value = held_ball_retention_probability(strength, fit(), &tuning);
            assert!(
                (0.0..=1.0).contains(&value),
                "held_ball_retention_probability {strength} = {value} outside [0, 1]"
            );
        }
    }

    #[test]
    fn held_ball_retention_honors_fit_multiplier() {
        let tuning = tuning();
        // Lower fit multiplier ⇒ lower effective attribute ⇒ lower
        // retention for the rising curve.
        let full = held_ball_retention_probability(100, 1.00, &tuning);
        let partial = held_ball_retention_probability(100, 0.75, &tuning);
        assert!(
            full > partial + TOL,
            "retention at full fit ({full}) must exceed partial fit ({partial})"
        );
    }

    #[allow(
        clippy::float_cmp,
        reason = "aggression_stamina_factor returns the exact literal constant 1.0; equality is exact"
    )]
    #[test]
    fn aggression_stamina_factor_is_placeholder_one() {
        assert_eq!(aggression_stamina_factor(1), 1.0);
        assert_eq!(aggression_stamina_factor(200), 1.0);
    }
}
