//! Goalkeeper mechanic pure functions (PRD §6.2, Phase 4 (d), ticket 23).
//! 
//! Pure functions that turn the remaining goalkeeper attributes into
//! mechanic parameters. Every consumer reads through the response curve
//! [`sim_math::curve`] and the fit multiplier [`sim_math::effective_attribute`],
//! so direction-of-effect is guaranteed by the curve's strict monotonicity
//! and is unit-tested per attribute (PRD §13.1 `A10`).
//!
//! # Mechanics
//!
//! - `Throwing` → hand-distribution accuracy and range.
//! - `Kicking` → kick-distribution accuracy and range.
//! - `Punching` → cross clearance by punching.
//! - `Aerial Reach` → reach radius for shots and crosses.
//! - `Communication` † → reduces defender positioning and marking error
//!   in the keeper's own half (error factor falls as Communication rises).
//! - `Agility` † → lateral movement in goal and dive reach on saves.
//! - `Concentration` † → save-lapse probability, rising with time since
//!   the keeper's last touch and falling as Concentration rises.
//!
//! The †-marked attributes (Communication, Agility, Concentration) are
//! proposal interpretations (PRD §6.2); their direction of effect is
//! unit-tested even though the full live-match consumer lands in a later
//! phase.

#![allow(
    clippy::cast_possible_truncation,
    reason = "effective attributes are bounded to [0, 200] (attribute ≤ 200, multiplier ≤ 1.0), so f32 -> i32 is lossless"
)]

use crate::Attribute;
use sim_math::{curve, effective_attribute, Tuning};

/// Index of `Attribute::Throwing` in `Attribute::ALL` /
/// `Tuning::curve_params`. Mirrors the attribute declaration order; the
/// cross-crate consistency is asserted by
/// `sim_components::attribute_tests::a10_curve_monotonicity_all_attributes`.
const THROWING_IDX: usize = Attribute::Throwing as usize;
const KICKING_IDX: usize = Attribute::Kicking as usize;
const PUNCHING_IDX: usize = Attribute::Punching as usize;
const AERIAL_REACH_IDX: usize = Attribute::AerialReach as usize;
const COMMUNICATION_IDX: usize = Attribute::Communication as usize;
const AGILITY_IDX: usize = Attribute::Agility as usize;
const CONCENTRATION_IDX: usize = Attribute::Concentration as usize;

/// Hand-distribution accuracy in `[0, 1]`: the likelihood a thrown
/// distribution reaches its target. Rises monotonically with `Throwing`.
///
/// The `Throwing` curve-table entry is rising (`min: 0.05`, `max:
/// 0.95` at the default tuning), so the raw curve output *is* the
/// accuracy.
#[must_use]
pub fn throwing_accuracy(throwing: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(throwing, fit_multiplier);
    curve(effective as i32, &tuning.curve_params[THROWING_IDX])
}

/// Hand-distribution range in metres. Rises monotonically with
/// `Throwing`, mapped to a `15..=40` m band.
#[must_use]
pub fn throwing_range(throwing: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(throwing, fit_multiplier);
    let quality = curve(effective as i32, &tuning.curve_params[THROWING_IDX]);
    (15.0 + quality.mul_add(25.0, 0.0)).clamp(15.0, 40.0)
}

/// Kick-distribution accuracy in `[0, 1]`: the likelihood a kicked
/// distribution reaches its target. Rises monotonically with `Kicking`.
#[must_use]
pub fn kicking_accuracy(kicking: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(kicking, fit_multiplier);
    curve(effective as i32, &tuning.curve_params[KICKING_IDX])
}

/// Kick-distribution range in metres. Rises monotonically with
/// `Kicking`, mapped to a `25..=70` m band.
#[must_use]
pub fn kicking_range(kicking: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(kicking, fit_multiplier);
    let quality = curve(effective as i32, &tuning.curve_params[KICKING_IDX]);
    (25.0 + quality.mul_add(45.0, 0.0)).clamp(25.0, 70.0)
}

/// Cross clearance probability by punching in `[0, 1]`. Rises
/// monotonically with `Punching`: a better puncher is more likely to
/// clear an incoming cross rather than parry or drop it.
#[must_use]
pub fn punching_clearance_probability(punching: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(punching, fit_multiplier);
    curve(effective as i32, &tuning.curve_params[PUNCHING_IDX])
}

/// Aerial-reach radius in metres: how far the goalkeeper can reach for
/// shots and crosses. Rises monotonically with `Aerial Reach`, mapped to
/// a `2.0..=6.0` m band.
#[must_use]
pub fn aerial_reach_radius(aerial_reach: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(aerial_reach, fit_multiplier);
    let quality = curve(effective as i32, &tuning.curve_params[AERIAL_REACH_IDX]);
    (2.0 + quality.mul_add(4.0, 0.0)).clamp(2.0, 6.0)
}

/// Defender positioning / marking error factor in `[0, 1]` applied to
/// defenders in the keeper's own half. Falls monotonically with
/// `Communication`: better communication ⇒ smaller error factor.
///
/// The `Communication` curve rises (positioning quality); the error
/// factor is its inverse, so higher `Communication` ⇒ lower error.
#[must_use]
pub fn communication_defender_error_factor(
    communication: u8,
    fit_multiplier: f32,
    tuning: &Tuning,
) -> f32 {
    let effective = effective_attribute(communication, fit_multiplier);
    let quality = curve(effective as i32, &tuning.curve_params[COMMUNICATION_IDX]);
    (1.0 - quality).clamp(0.0, 1.0)
}

/// Dive-reach distance in metres: lateral reach on a save. Rises
/// monotonically with `Agility`, mapped to a `1.0..=5.0` m band.
#[must_use]
pub fn agility_dive_reach(agility: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(agility, fit_multiplier);
    let quality = curve(effective as i32, &tuning.curve_params[AGILITY_IDX]);
    (1.0 + quality.mul_add(4.0, 0.0)).clamp(1.0, 5.0)
}

/// Save-lapse probability in `[0, 1]`: the chance the goalkeeper lapses
/// on a save. Falls monotonically with `Concentration` (a more
/// concentrated keeper lapses less often) and rises monotonically with
/// `time_since_last_touch_ticks` (lapse risk grows the longer the keeper
/// has gone without touching the ball).
///
/// `Concentration`'s curve-table entry is falling (`min: 0.02`, `max:
/// 0.70` at the default tuning), so the raw curve output *is* the base
/// lapse probability: higher `Concentration` ⇒ lower base lapse.
#[allow(
    clippy::too_long_first_doc_paragraph,
    reason = "the opening paragraph states the full documented effect of Concentration in one place; splitting it would weaken clarity"
)]
#[must_use]
pub fn concentration_save_lapse_probability(
    concentration: u8,
    time_since_last_touch_ticks: u64,
    fit_multiplier: f32,
    tuning: &Tuning,
) -> f32 {
    let effective = effective_attribute(concentration, fit_multiplier);
    // Base lapse probability: falling in `Concentration`, in [0.02, 0.70].
    let base_lapse = curve(effective as i32, &tuning.curve_params[CONCENTRATION_IDX]);
    // Lapse risk grows with time since the keeper's last touch, up to a
    // doubling after ten seconds (600 ticks). `time_since_last_touch_ticks`
    // is a whole-tick count; realistic match values are far below 2^24,
    // so the cast is exact in practice, and the clamp bounds the ratio.
    #[expect(
        clippy::cast_precision_loss,
        reason = "time_since_last_touch_ticks is a whole-tick count; realistic match values are far below 2^24, and the subsequent clamp bounds the ratio to [0, 1]"
    )]
    let time_factor =
        1.0 + (time_since_last_touch_ticks as f32 / 600.0).clamp(0.0, 1.0);
    (base_lapse * time_factor).clamp(0.0, 1.0)
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

    // === Throwing ===

    #[test]
    fn throwing_accuracy_is_strictly_rising_in_throwing() {
        let tuning = tuning();
        let mut prev = throwing_accuracy(1, fit(), &tuning);
        for throwing in 2..=200u8 {
            let value = throwing_accuracy(throwing, fit(), &tuning);
            assert!(
                value > prev + TOL,
                "throwing_accuracy not strictly rising at Throwing {throwing}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn throwing_accuracy_stays_within_curve_bounds() {
        let tuning = tuning();
        for throwing in 1..=200u8 {
            let value = throwing_accuracy(throwing, fit(), &tuning);
            assert!(
                (0.05..=0.95).contains(&value),
                "throwing_accuracy {throwing} = {value} outside [0.05, 0.95]"
            );
        }
    }

    #[test]
    fn throwing_range_is_strictly_rising_in_throwing() {
        let tuning = tuning();
        let mut prev = throwing_range(1, fit(), &tuning);
        for throwing in 2..=200u8 {
            let value = throwing_range(throwing, fit(), &tuning);
            assert!(
                value > prev + TOL,
                "throwing_range not strictly rising at Throwing {throwing}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn throwing_range_stays_in_band() {
        let tuning = tuning();
        for throwing in 1..=200u8 {
            let value = throwing_range(throwing, fit(), &tuning);
            assert!(
                (15.0..=40.0).contains(&value),
                "throwing_range {throwing} = {value} outside [15, 40]"
            );
        }
    }

    // === Kicking ===

    #[test]
    fn kicking_accuracy_is_strictly_rising_in_kicking() {
        let tuning = tuning();
        let mut prev = kicking_accuracy(1, fit(), &tuning);
        for kicking in 2..=200u8 {
            let value = kicking_accuracy(kicking, fit(), &tuning);
            assert!(
                value > prev + TOL,
                "kicking_accuracy not strictly rising at Kicking {kicking}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn kicking_range_is_strictly_rising_in_kicking() {
        let tuning = tuning();
        let mut prev = kicking_range(1, fit(), &tuning);
        for kicking in 2..=200u8 {
            let value = kicking_range(kicking, fit(), &tuning);
            assert!(
                value > prev + TOL,
                "kicking_range not strictly rising at Kicking {kicking}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn kicking_range_stays_in_band() {
        let tuning = tuning();
        for kicking in 1..=200u8 {
            let value = kicking_range(kicking, fit(), &tuning);
            assert!(
                (25.0..=70.0).contains(&value),
                "kicking_range {kicking} = {value} outside [25, 70]"
            );
        }
    }

    // === Punching ===

    #[test]
    fn punching_clearance_probability_is_strictly_rising_in_punching() {
        let tuning = tuning();
        let mut prev = punching_clearance_probability(1, fit(), &tuning);
        for punching in 2..=200u8 {
            let value = punching_clearance_probability(punching, fit(), &tuning);
            assert!(
                value > prev + TOL,
                "punching_clearance_probability not strictly rising at Punching {punching}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn punching_clearance_probability_stays_within_curve_bounds() {
        let tuning = tuning();
        for punching in 1..=200u8 {
            let value = punching_clearance_probability(punching, fit(), &tuning);
            assert!(
                (0.05..=0.95).contains(&value),
                "punching_clearance_probability {punching} = {value} outside [0.05, 0.95]"
            );
        }
    }

    // === Aerial Reach ===

    #[test]
    fn aerial_reach_radius_is_strictly_rising_in_aerial_reach() {
        let tuning = tuning();
        let mut prev = aerial_reach_radius(1, fit(), &tuning);
        for aerial_reach in 2..=200u8 {
            let value = aerial_reach_radius(aerial_reach, fit(), &tuning);
            assert!(
                value > prev + TOL,
                "aerial_reach_radius not strictly rising at Aerial Reach {aerial_reach}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn aerial_reach_radius_stays_in_band() {
        let tuning = tuning();
        for aerial_reach in 1..=200u8 {
            let value = aerial_reach_radius(aerial_reach, fit(), &tuning);
            assert!(
                (2.0..=6.0).contains(&value),
                "aerial_reach_radius {aerial_reach} = {value} outside [2, 6]"
            );
        }
    }

    // === Communication ===

    #[test]
    fn communication_defender_error_factor_is_strictly_falling_in_communication() {
        let tuning = tuning();
        let mut prev = communication_defender_error_factor(1, fit(), &tuning);
        for communication in 2..=200u8 {
            let value = communication_defender_error_factor(communication, fit(), &tuning);
            assert!(
                value < prev - TOL,
                "communication_defender_error_factor not strictly falling at Communication {communication}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn communication_defender_error_factor_stays_in_unit_interval() {
        let tuning = tuning();
        for communication in 1..=200u8 {
            let value = communication_defender_error_factor(communication, fit(), &tuning);
            assert!(
                (0.0..=1.0).contains(&value),
                "communication_defender_error_factor {communication} = {value} outside [0, 1]"
            );
        }
    }

    // === Agility ===

    #[test]
    fn agility_dive_reach_is_strictly_rising_in_agility() {
        let tuning = tuning();
        let mut prev = agility_dive_reach(1, fit(), &tuning);
        for agility in 2..=200u8 {
            let value = agility_dive_reach(agility, fit(), &tuning);
            assert!(
                value > prev + TOL,
                "agility_dive_reach not strictly rising at Agility {agility}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn agility_dive_reach_stays_in_band() {
        let tuning = tuning();
        for agility in 1..=200u8 {
            let value = agility_dive_reach(agility, fit(), &tuning);
            assert!(
                (1.0..=5.0).contains(&value),
                "agility_dive_reach {agility} = {value} outside [1, 5]"
            );
        }
    }

    // === Concentration ===

    #[test]
    fn concentration_save_lapse_probability_is_falling_in_concentration() {
        let tuning = tuning();
        let time = 300; // 5 seconds since last touch
        let mut prev = concentration_save_lapse_probability(1, time, fit(), &tuning);
        for concentration in 2..=200u8 {
            let value = concentration_save_lapse_probability(concentration, time, fit(), &tuning);
            assert!(
                value <= prev + TOL,
                "concentration_save_lapse_probability not non-increasing in Concentration {concentration}: {prev} -> {value}"
            );
            prev = value;
        }
        // Concentration must reduce the lapse overall (strictly falling curve).
        let low = concentration_save_lapse_probability(1, time, fit(), &tuning);
        let high = concentration_save_lapse_probability(200, time, fit(), &tuning);
        assert!(
            high < low - TOL,
            "lapse at Concentration 200 ({high}) must be below lapse at 1 ({low})"
        );
    }

    #[test]
    fn concentration_save_lapse_probability_rises_with_time_since_last_touch() {
        let tuning = tuning();
        let conc = 100;
        let recent = concentration_save_lapse_probability(conc, 0, fit(), &tuning);
        let stale = concentration_save_lapse_probability(conc, 1200, fit(), &tuning);
        assert!(
            stale > recent + TOL,
            "lapse after 1200 ticks ({stale}) must exceed lapse at 0 ticks ({recent})"
        );
    }

    #[test]
    fn concentration_save_lapse_probability_stays_in_unit_interval() {
        let tuning = tuning();
        for concentration in 1..=200u8 {
            for time in [0, 300, 600, 1200] {
                let value = concentration_save_lapse_probability(concentration, time, fit(), &tuning);
                assert!(
                    (0.0..=1.0).contains(&value),
                    "concentration_save_lapse_probability {concentration}@{time} = {value} outside [0, 1]"
                );
            }
        }
    }
}
