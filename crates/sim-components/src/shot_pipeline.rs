//! Shot-pipeline mechanics (PRD Phase 4 (a), ticket 20).
//!
//! Pure functions that turn raw attributes into shot and save mechanic
//! parameters. Every consumer reads through the response curve
//! (`sim_math::curve`) and the fit multiplier
//! (`sim_math::effective_attribute`), so direction-of-effect is guaranteed
//! by the curve's strict monotonicity. The mechanic parameters feed the
//! shot-outcome composer ([`resolve_shot_outcome`]) and are unit-tested for
//! monotonicity (PRD §13.1 A10).
//!
//! # Mechanic parameters
//!
//! - `Shooting` → shot power and accuracy (both rise with the attribute).
//! - `Finishing` → conversion quality, higher close to goal and lower under
//!   pressure.
//! - `Reflexes` → save probability on close and fast shots.
//! - `Anticipation` → keeper-to-shooter positioning error (shrinks with the
//!   attribute).
//! - `Rushing Out` → rush-out angle-closing quality.

use crate::{Attribute, SaveKind, ShotOutcome};
use sim_math::{curve, effective_attribute, Tuning};

/// Ball speed (m/s) at or below which a goalkeeper catches (rather than
/// parries) a resolved open-play shot. `Punch` is reserved for cross
/// clearances (aerial play, post-MVP).
pub const SAVE_CATCH_MAX_SPEED_MPS: f32 = 0.25;

/// Index of `Attribute::Shooting` in `Attribute::ALL` / `Tuning::curve_params`.
const SHOOTING_IDX: usize = Attribute::Shooting as usize;
/// Index of `Attribute::Finishing`.
const FINISHING_IDX: usize = Attribute::Finishing as usize;
/// Index of `Attribute::Reflexes`.
const REFLEXES_IDX: usize = Attribute::Reflexes as usize;
/// Index of `Attribute::Anticipation`.
const ANTICIPATION_IDX: usize = Attribute::Anticipation as usize;
/// Index of `Attribute::RushingOut`.
const RUSHING_OUT_IDX: usize = Attribute::RushingOut as usize;

/// Evaluate the response curve for `attribute` (scaled by `fit_multiplier`)
/// and optionally invert the output so the mechanic parameter moves in the
/// documented direction regardless of the curve table's `rising` flag.
///
/// The `Tuning::curve_params` entry for `Shooting` is falling (it models
/// shot error); power and accuracy must rise with `Shooting`, so those
/// consumers invert. `Anticipation`'s curve rises (positioning quality),
/// but the mechanic parameter is an *error* that must shrink, so it is
/// inverted too.
#[allow(
    clippy::cast_possible_truncation,
    reason = "effective attribute is bounded to [0, 200] (attribute ≤ 200, multiplier ≤ 1.0), so the cast is non-truncating"
)]
#[must_use]
fn mechanic_param(attribute: u8, fit_multiplier: f32, index: usize, invert: bool, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(attribute, fit_multiplier);
    // `effective` is bounded to [0, 200] (attribute ≤ 200, multiplier ≤ 1.0),
    // so the cast is non-truncating.
    let value = curve(effective as i32, &tuning.curve_params[index]);
    if invert {
        1.0 - value
    } else {
        value
    }
}

/// Shot power factor in `[0, 1]`. Rises monotonically with `Shooting`.
///
/// The `Shooting` curve-table entry is falling (shot error), so the
/// output is inverted: a higher `Shooting` attribute yields a harder
/// shot (harder for the keeper to save).
#[must_use]
pub fn shot_power(shooting: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    mechanic_param(shooting, fit_multiplier, SHOOTING_IDX, true, tuning)
}

/// Shot accuracy in `[0, 1]`: likelihood the shot is on target. Rises
/// monotonically with `Shooting`. Inverted for the same reason as power.
#[must_use]
pub fn shot_accuracy(shooting: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    mechanic_param(shooting, fit_multiplier, SHOOTING_IDX, true, tuning)
}

/// Finishing conversion quality in `[0, 1]`: how likely a well-placed
/// shot beats the keeper. Rises monotonically with `Finishing`, is
/// higher close to goal and lower under pressure.
///
/// `distance_to_goal_m` is the shooter's distance to the goal they are
/// attacking; `pressure` is a `0..=1` estimate of defensive pressure
/// (0 = none, 1 = maximum). Both are multipliers on the finishing curve
/// output, so monotonicity in `Finishing` holds at any fixed distance /
/// pressure.
#[allow(
    clippy::too_many_arguments,
    reason = "finishing conversion is a function of five documented inputs; a builder would scatter the semantics"
)]
#[must_use]
pub fn finishing_conversion(
    finishing: u8,
    fit_multiplier: f32,
    distance_to_goal_m: f32,
    pressure: f32,
    tuning: &Tuning,
) -> f32 {
    let base = mechanic_param(finishing, fit_multiplier, FINISHING_IDX, false, tuning);
    // Close to goal: conversion is highest at the goal line and fades
    // to half at 30 m.
    let distance_factor =
        1.0 - (distance_to_goal_m / 30.0).clamp(0.0, 1.0).mul_add(0.5, 0.0);
    // Defensive pressure reduces conversion, up to half at maximum.
    let pressure_factor = 1.0 - pressure.clamp(0.0, 1.0).mul_add(0.5, 0.0);
    (base * distance_factor * pressure_factor).clamp(0.0, 1.0)
}

/// Goalkeeper save probability on a close/fast shot in `[0, 1]`. Rises
/// monotonically with `Reflexes`; higher close to goal and for faster
/// shots (a close, fast shot is exactly where reflexes matter most).
///
/// `ball_speed_mps` is the shot speed in metres per second.
#[allow(
    clippy::too_long_first_doc_paragraph,
    reason = "the opening paragraph states the full documented effect of Reflexes in one sentence; splitting it would weaken clarity"
)]
#[allow(
    clippy::too_many_arguments,
    reason = "save probability is a function of four documented inputs"
)]
#[must_use]
pub fn save_probability(
    reflexes: u8,
    fit_multiplier: f32,
    distance_to_goal_m: f32,
    ball_speed_mps: f32,
    tuning: &Tuning,
) -> f32 {
    let base = mechanic_param(reflexes, fit_multiplier, REFLEXES_IDX, false, tuning);
    // Close shots: up to +20% save probability right on the goal line.
    let close_factor =
        1.0 + (1.0 - (distance_to_goal_m / 30.0).clamp(0.0, 1.0)).mul_add(0.2, 0.0);
    // Fast shots: up to +20% at 25 m/s and above.
    let fast_factor = 1.0 + (ball_speed_mps / 25.0).clamp(0.0, 1.0).mul_add(0.2, 0.0);
    (base * close_factor * fast_factor).clamp(0.0, 1.0)
}

/// Keeper-to-shooter positioning error in metres. Falls monotonically
/// with `Anticipation`: better anticipation → smaller error, in
/// `[0.25, 4.75]` m.
#[must_use]
pub fn keeper_positioning_error(anticipation: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    // The Anticipation curve rises (positioning quality); error is its
    // inverse.
    let quality = mechanic_param(anticipation, fit_multiplier, ANTICIPATION_IDX, false, tuning);
    5.0 * (1.0 - quality)
}

/// Rush-out angle-closing quality in `[0, 1]`. Rises monotonically with
/// `Rushing Out`: a better rusher closes the shooting angle more
/// effectively.
#[must_use]
pub fn rush_out_quality(rushing_out: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    mechanic_param(rushing_out, fit_multiplier, RUSHING_OUT_IDX, false, tuning)
}

/// Keeper save chance combining `Reflexes` (base), `Anticipation`
/// (positioning) and `Rushing Out` (angle closing).
///
/// Anticipation shrinks positioning error, which raises the save chance
/// (up to +20%). Rushing Out raises it further on close shots (up to
/// +15% inside the penalty area).
#[allow(
    clippy::too_many_arguments,
    reason = "keeper save chance combines three attributes plus shot context"
)]
#[must_use]
pub fn keeper_save_chance(
    reflexes: u8,
    anticipation: u8,
    rushing_out: u8,
    fit_multiplier: f32,
    distance_to_goal_m: f32,
    ball_speed_mps: f32,
    tuning: &Tuning,
) -> f32 {
    let base =
        save_probability(reflexes, fit_multiplier, distance_to_goal_m, ball_speed_mps, tuning);
    let error = keeper_positioning_error(anticipation, fit_multiplier, tuning);
    // Error in [0.25, 4.75]; `1.0 - error/5.0` is in [0.05, 0.95].
    let anticipation_factor =
        1.0 + (1.0 - error / 5.0).clamp(0.0, 1.0).mul_add(0.2, 0.0);
    let quality = rush_out_quality(rushing_out, fit_multiplier, tuning);
    // Rush out matters most close to goal (inside the 16.5 m penalty area).
    let close = 1.0 - (distance_to_goal_m / 16.5).clamp(0.0, 1.0);
    let rush_factor = 1.0 + quality.mul_add(0.15, 0.0).mul_add(close, 0.0);
    (base * anticipation_factor * rush_factor).clamp(0.0, 1.0)
}

/// Result of resolving one shot through the Phase 4 pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShotResolution {
    /// Whether the shot was on target.
    pub on_target: bool,
    /// How the shot resolved.
    pub outcome: ShotOutcome,
}

/// Compose a shot outcome from the mechanic parameters and two RNG rolls
/// in `[0, 1)`.
///
/// `accuracy` (Shooting) sets the on-target likelihood. `power` (Shooting)
/// and `conversion` (Finishing) reduce the keeper's effective save chance
/// (a hard, well-placed shot is harder to save); `save_chance`
/// ([`keeper_save_chance`], driven by Reflexes/Anticipation/Rushing Out)
/// raises it.
///
/// # Parameters
///
/// - `roll_on_target` — uniform draw for the on-target test.
/// - `roll_save` — uniform draw for the save-versus-goal test (only
///   consumed when the shot is on target).
#[allow(
    clippy::too_many_arguments,
    reason = "the shot composer takes one parameter per documented mechanic input plus two rolls"
)]
#[must_use]
pub fn resolve_shot_outcome(
    accuracy: f32,
    power: f32,
    conversion: f32,
    save_chance: f32,
    roll_on_target: f32,
    roll_save: f32,
) -> ShotResolution {
    let on_target = roll_on_target < accuracy.clamp(0.0, 1.0);
    if !on_target {
        return ShotResolution {
            on_target: false,
            outcome: ShotOutcome::OffTarget,
        };
    }
    // Effective save chance: keeper save probability, reduced by shot
    // power (harder to save) and finishing conversion (better placed).
    let effective_save = (save_chance.clamp(0.0, 1.0)
        * (1.0 - conversion.clamp(0.0, 1.0))
        * 0.5f32.mul_add(-power.clamp(0.0, 1.0), 1.0))
    .clamp(0.0, 1.0);
    let outcome = if roll_save < effective_save {
        ShotOutcome::Saved
    } else {
        ShotOutcome::Goal
    };
    ShotResolution { on_target: true, outcome }
}

/// Save kind from ball speed (open-play shots only; `Punch` is reserved
/// for cross clearances).
#[must_use]
pub fn save_kind_from_speed(ball_speed_mps: f32) -> SaveKind {
    if ball_speed_mps <= SAVE_CATCH_MAX_SPEED_MPS {
        SaveKind::Catch
    } else {
        SaveKind::Parry
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "mechanic parameters are computed from integer-literal attributes through a strictly-monotonic curve; comparisons use exact f32 equality on values built from the same literals"
)]
mod tests {
    use super::*;

    const TOL: f32 = 1e-6;

    fn tuning() -> Tuning {
        Tuning::default()
    }

    fn fit() -> f32 {
        1.0
    }

    #[test]
    fn shot_power_is_strictly_rising_in_shooting() {
        let tuning = tuning();
        let mut prev = shot_power(1, fit(), &tuning);
        for shooting in 2..=200 {
            let value = shot_power(shooting, fit(), &tuning);
            assert!(
                value > prev + TOL,
                "shot_power not strictly rising at Shooting {shooting}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn shot_accuracy_is_strictly_rising_in_shooting() {
        let tuning = tuning();
        let mut prev = shot_accuracy(1, fit(), &tuning);
        for shooting in 2..=200 {
            let value = shot_accuracy(shooting, fit(), &tuning);
            assert!(
                value > prev + TOL,
                "shot_accuracy not strictly rising at Shooting {shooting}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn finishing_conversion_is_strictly_rising_in_finishing() {
        let tuning = tuning();
        // Close to goal, no pressure.
        let mut prev = finishing_conversion(1, fit(), 6.0, 0.0, &tuning);
        for finishing in 2..=200 {
            let value = finishing_conversion(finishing, fit(), 6.0, 0.0, &tuning);
            assert!(
                value > prev + TOL,
                "finishing_conversion not strictly rising at Finishing {finishing}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn finishing_conversion_is_higher_close_to_goal() {
        let tuning = tuning();
        let close = finishing_conversion(100, fit(), 6.0, 0.0, &tuning);   // inside the box
        let far = finishing_conversion(100, fit(), 28.0, 0.0, &tuning);    // well outside
        assert!(
            close > far + TOL,
            "conversion close to goal ({close}) must exceed far ({far})"
        );
    }

    #[test]
    fn finishing_conversion_is_lower_under_pressure() {
        let tuning = tuning();
        let unpressured = finishing_conversion(100, fit(), 6.0, 0.0, &tuning);
        let pressured = finishing_conversion(100, fit(), 6.0, 1.0, &tuning);
        assert!(
            pressured < unpressured - TOL,
            "conversion under pressure ({pressured}) must be lower than unpressured ({unpressured})"
        );
    }

    /// Save probability is monotonic (non-decreasing) in Reflexes. It can
    /// saturate at 1.0 on close/fast shots at high Reflexes, so we assert
    /// non-decreasing plus an overall rise rather than strict step-by-step.
    #[test]
    fn save_probability_is_monotonic_rising_in_reflexes() {
        let tuning = tuning();
        let mut prev = save_probability(1, fit(), 6.0, 25.0, &tuning);
        let first = prev;
        for reflexes in 2..=200 {
            let value = save_probability(reflexes, fit(), 6.0, 25.0, &tuning);
            assert!(
                value >= prev - TOL,
                "save_probability not non-decreasing at Reflexes {reflexes}: {prev} -> {value}"
            );
            prev = value;
        }
        assert!(
            prev > first + TOL,
            "save_probability must rise overall from Reflexes 1 ({first}) to 200 ({prev})"
        );
    }

    #[test]
    fn save_probability_is_higher_close_and_fast() {
        let tuning = tuning();
        let close_fast = save_probability(100, fit(), 6.0, 25.0, &tuning);
        let far_slow = save_probability(100, fit(), 28.0, 2.0, &tuning);
        assert!(
            close_fast > far_slow + TOL,
            "save probability close/fast ({close_fast}) must exceed far/slow ({far_slow})"
        );
    }

    #[test]
    fn keeper_positioning_error_is_strictly_falling_in_anticipation() {
        let tuning = tuning();
        let mut prev = keeper_positioning_error(1, fit(), &tuning);
        for anticipation in 2..=200 {
            let value = keeper_positioning_error(anticipation, fit(), &tuning);
            assert!(
                value < prev - TOL,
                "positioning error not strictly falling at Anticipation {anticipation}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn rush_out_quality_is_strictly_rising_in_rushing_out() {
        let tuning = tuning();
        let mut prev = rush_out_quality(1, fit(), &tuning);
        for rushing_out in 2..=200 {
            let value = rush_out_quality(rushing_out, fit(), &tuning);
            assert!(
                value > prev + TOL,
                "rush_out_quality not strictly rising at Rushing Out {rushing_out}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    /// Keeper save chance is monotonic (non-decreasing) in each goalkeeper
    /// attribute, holding the others fixed. It can saturate at 1.0, so we
    /// assert non-decreasing plus an overall rise for each attribute.
    #[test]
    fn keeper_save_chance_is_monotonic_rising_in_goalkeeper_attributes() {
        let tuning = tuning();
        macro_rules! assert_attribute_rises {
            ($attr:expr, $label:expr, $first:expr) => {{
                let mut prev = $first;
                for v in 2..=200u8 {
                    let value = $attr(v);
                    assert!(
                        value >= prev - TOL,
                        "keeper_save_chance not non-decreasing in {} at {v}: {prev} -> {value}",
                        $label
                    );
                    prev = value;
                }
                assert!(
                    prev > $first + TOL,
                    "keeper_save_chance must rise overall in {}",
                    $label
                );
            }};
        }
        assert_attribute_rises!(
            |r| keeper_save_chance(r, 100, 100, fit(), 6.0, 25.0, &tuning),
            "Reflexes",
            keeper_save_chance(1, 100, 100, fit(), 6.0, 25.0, &tuning)
        );
        assert_attribute_rises!(
            |a| keeper_save_chance(100, a, 100, fit(), 6.0, 25.0, &tuning),
            "Anticipation",
            keeper_save_chance(100, 1, 100, fit(), 6.0, 25.0, &tuning)
        );
        assert_attribute_rises!(
            |r| keeper_save_chance(100, 100, r, fit(), 6.0, 25.0, &tuning),
            "Rushing Out",
            keeper_save_chance(100, 100, 1, fit(), 6.0, 25.0, &tuning)
        );
    }

    #[test]
    fn resolve_shot_outcome_on_target_follows_accuracy() {
        let tuning = tuning();
        let accuracy = shot_accuracy(150, fit(), &tuning);
        // Low roll → on target; high roll → off target.
        let low = resolve_shot_outcome(accuracy, 0.5, 0.5, 0.5, 0.1, 0.9);
        assert!(low.on_target, "low roll against accuracy {accuracy} should be on target");
        let high = resolve_shot_outcome(accuracy, 0.5, 0.5, 0.5, 0.99, 0.1);
        assert!(!high.on_target, "high roll against accuracy {accuracy} should be off target");
    }

    #[test]
    fn resolve_shot_outcome_saved_follows_save_chance() {
        // On-target for sure (accuracy 1.0), low power/conversion → save
        // chance dominates.
        let high_save = resolve_shot_outcome(1.0, 0.0, 0.0, 0.9, 0.1, 0.1);
        assert_eq!(high_save.outcome, ShotOutcome::Saved);
        let low_save = resolve_shot_outcome(1.0, 0.0, 0.0, 0.1, 0.1, 0.9);
        assert_eq!(low_save.outcome, ShotOutcome::Goal);
    }

    #[test]
    fn shot_power_and_accuracy_honor_fit_multiplier() {
        let tuning = tuning();
        // Full fit (1.00) ≥ partial fit (0.75) for the same attribute.
        let full = shot_power(100, 1.00, &tuning);
        let partial = shot_power(100, 0.75, &tuning);
        assert!(
            full >= partial - TOL,
            "shot power at full fit ({full}) must be at least partial fit ({partial})"
        );
    }

    #[test]
    fn save_kind_from_speed_splits_catch_and_parry() {
        assert_eq!(save_kind_from_speed(0.0), SaveKind::Catch);
        assert_eq!(save_kind_from_speed(SAVE_CATCH_MAX_SPEED_MPS), SaveKind::Catch);
        assert_eq!(
            save_kind_from_speed(SAVE_CATCH_MAX_SPEED_MPS + 0.001),
            SaveKind::Parry
        );
        assert_eq!(save_kind_from_speed(25.0), SaveKind::Parry);
    }

    #[test]
    fn mechanic_parameters_stay_in_valid_ranges() {
        let tuning = tuning();
        for attr in 1..=200 {
            assert!((0.0..=1.0).contains(&shot_power(attr, fit(), &tuning)), "shot_power {attr}");
            assert!(
                (0.0..=1.0).contains(&shot_accuracy(attr, fit(), &tuning)),
                "shot_accuracy {attr}"
            );
            assert!(
                (0.0..=1.0).contains(&finishing_conversion(attr, fit(), 6.0, 0.0, &tuning)),
                "finishing_conversion {attr}"
            );
            assert!(
                (0.0..=1.0).contains(&save_probability(attr, fit(), 6.0, 25.0, &tuning)),
                "save_probability {attr}"
            );
            assert!(
                (0.0..=1.0).contains(&rush_out_quality(attr, fit(), &tuning)),
                "rush_out_quality {attr}"
            );
            // Positioning error is in metres, not unit interval.
            assert!(keeper_positioning_error(attr, fit(), &tuning) > 0.0);
        }
    }
}
