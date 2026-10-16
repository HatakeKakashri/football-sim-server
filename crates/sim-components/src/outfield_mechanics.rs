//! Outfield mechanic pure functions (PRD §6.2, Phase 4 (b), ticket 21).
//!
//! Each function maps one consumed attribute — through the response
//! curve [`sim_math::curve`] and the fit multiplier
//! [`sim_math::effective_attribute`] — to the mechanic parameter that
//! attribute drives. Direction-of-effect is guaranteed by the curve's
//! strict monotonicity and is unit-tested per attribute (PRD §13.1
//! `A10`; Marking and Positioning are unit-only here, Passing and
//! Tackling additionally get full-match sweeps in ticket 29).
//!
//! The four consumers:
//! - `Passing` → pass error spread (narrows as passing improves).
//! - `Tackling` → tackle success probability (rises as tackling improves).
//! - `Marking` → distance-to-mark error (shrinks as marking improves).
//! - `Positioning` → target-position error (shrinks as positioning improves).
//!
//! The `effective_attribute` → `curve` casts below are non-truncating:
//! `effective` is bounded to `[0, 200]` (attribute ≤ 200, multiplier ≤ 1.0),
//! which fits exactly in `f32`'s 23-bit mantissa and casts losslessly to `i32`.
#![allow(
    clippy::cast_possible_truncation,
    reason = "effective attributes are bounded to [0, 200] before the cast, so f32 -> i32 is lossless"
)]

use crate::Attribute;
use sim_math::{curve, effective_attribute, Tuning};

/// Index of `Attribute::Passing` in `Attribute::ALL` /
/// `Tuning::curve_params`. Mirrors the attribute declaration order; the
/// cross-crate consistency is asserted by
/// `sim_components::attribute_tests::a10_curve_monotonicity_all_attributes`.
const PASSING_IDX: usize = Attribute::Passing as usize;
/// Index of `Attribute::Tackling`.
const TACKLING_IDX: usize = Attribute::Tackling as usize;
/// Index of `Attribute::Marking`.
const MARKING_IDX: usize = Attribute::Marking as usize;
/// Index of `Attribute::Positioning`.
const POSITIONING_IDX: usize = Attribute::Positioning as usize;

/// Pass error spread in `[0, 1]`: the probability a pass deviates from
/// its intended target.
///
/// Falls monotonically with `Passing` — better passers are more
/// accurate, so the `pass` event's `to` field (the intended receiver)
/// is populated more often.
///
/// The `Passing` curve-table entry is falling
/// (`min: 0.02`, `max: 0.70` at the default tuning), so the raw curve
/// output *is* the error spread: high `Passing` → low error.
#[must_use]
pub fn pass_error_spread(passing: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(passing, fit_multiplier);
    curve(effective as i32, &tuning.curve_params[PASSING_IDX])
}

/// Tackle success probability in `[0, 1]`.
///
/// Rises monotonically with `Tackling` — better tacklers win more
/// challenges.
///
/// The `Tackling` curve-table entry is rising
/// (`min: 0.05` base probability, `max: 0.95` at the default tuning),
/// so the raw curve output *is* the success probability. The base
/// probability (attribute near zero) and ceiling live on this curve
/// record, which is part of the tuning struct (PRD §11).
#[must_use]
pub fn tackle_success_probability(tackling: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(tackling, fit_multiplier);
    curve(effective as i32, &tuning.curve_params[TACKLING_IDX])
}

/// Mark distance-to-mark error in metres.
///
/// Falls monotonically with `Marking` — a better marker stays closer
/// to their assigned opponent (smaller zonal cover error).
///
/// The `Marking` curve-table entry is falling, so the raw curve output
/// *is* the error in metres.
#[must_use]
pub fn mark_distance_error(marking: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(marking, fit_multiplier);
    curve(effective as i32, &tuning.curve_params[MARKING_IDX])
}

/// Defensive target-position error in metres against the ball and
/// threats.
///
/// Falls monotonically with `Positioning` — a better positioned
/// defender takes up a more accurate target position.
///
/// The `Positioning` curve-table entry is falling, so the raw curve
/// output *is* the error in metres.
#[must_use]
pub fn target_position_error(positioning: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(positioning, fit_multiplier);
    curve(effective as i32, &tuning.curve_params[POSITIONING_IDX])
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f32 = 1e-6;

    fn tuning() -> Tuning {
        Tuning::default()
    }

    fn fit() -> f32 {
        1.0
    }

    /// Tolerance for "strictly monotonic" step checks; the logistic
    /// curve is strictly monotonic on integer attributes, so adjacent
    /// steps differ by more than this.
    #[test]
    fn pass_error_spread_is_strictly_falling_in_passing() {
        let tuning = tuning();
        let mut prev = pass_error_spread(1, fit(), &tuning);
        for passing in 2..=200u8 {
            let value = pass_error_spread(passing, fit(), &tuning);
            assert!(
                value < prev - TOL,
                "pass_error_spread not strictly falling at Passing {passing}: {prev} -> {value}"
            );
            prev = value;
        }
        assert!(
            prev > TOL,
            "pass_error_spread must stay above zero at Passing 200 ({prev})"
        );
    }

    #[test]
    fn pass_error_spread_stays_within_curve_bounds() {
        let tuning = tuning();
        for passing in 1..=200u8 {
            let value = pass_error_spread(passing, fit(), &tuning);
            assert!(
                (0.02..=0.70).contains(&value),
                "pass_error_spread {passing} = {value} outside [0.02, 0.70]"
            );
        }
    }

    #[test]
    fn pass_error_spread_honors_fit_multiplier() {
        let tuning = tuning();
        // Lower fit multiplier ⇒ lower effective attribute ⇒ higher
        // error spread for a falling curve.
        let full = pass_error_spread(100, 1.00, &tuning);
        let partial = pass_error_spread(100, 0.75, &tuning);
        assert!(
            full < partial - TOL,
            "pass error at full fit ({full}) must be below partial fit ({partial})"
        );
    }

    #[test]
    fn tackle_success_probability_is_strictly_rising_in_tackling() {
        let tuning = tuning();
        let mut prev = tackle_success_probability(1, fit(), &tuning);
        for tackling in 2..=200u8 {
            let value = tackle_success_probability(tackling, fit(), &tuning);
            assert!(
                value > prev + TOL,
                "tackle_success_probability not strictly rising at Tackling {tackling}: {prev} -> {value}"
            );
            prev = value;
        }
        assert!(
            prev < 1.0 - TOL,
            "tackle_success_probability must stay below one at Tackling 200 ({prev})"
        );
    }

    #[test]
    fn tackle_success_probability_stays_within_curve_bounds() {
        let tuning = tuning();
        for tackling in 1..=200u8 {
            let value = tackle_success_probability(tackling, fit(), &tuning);
            assert!(
                (0.05..=0.95).contains(&value),
                "tackle_success_probability {tackling} = {value} outside [0.05, 0.95]"
            );
        }
    }

    #[test]
    fn tackle_success_probability_honors_fit_multiplier() {
        let tuning = tuning();
        // Lower fit multiplier ⇒ lower effective attribute ⇒ lower
        // success probability for a rising curve.
        let full = tackle_success_probability(100, 1.00, &tuning);
        let partial = tackle_success_probability(100, 0.75, &tuning);
        assert!(
            full > partial + TOL,
            "tackle probability at full fit ({full}) must exceed partial fit ({partial})"
        );
    }

    #[test]
    fn mark_distance_error_is_strictly_falling_in_marking() {
        let tuning = tuning();
        let mut prev = mark_distance_error(1, fit(), &tuning);
        for marking in 2..=200u8 {
            let value = mark_distance_error(marking, fit(), &tuning);
            assert!(
                value < prev - TOL,
                "mark_distance_error not strictly falling at Marking {marking}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn mark_distance_error_stays_within_curve_bounds() {
        let tuning = tuning();
        for marking in 1..=200u8 {
            let value = mark_distance_error(marking, fit(), &tuning);
            assert!(
                (0.02..=0.70).contains(&value),
                "mark_distance_error {marking} = {value} outside [0.02, 0.70]"
            );
        }
    }

    #[test]
    fn target_position_error_is_strictly_falling_in_positioning() {
        let tuning = tuning();
        let mut prev = target_position_error(1, fit(), &tuning);
        for positioning in 2..=200u8 {
            let value = target_position_error(positioning, fit(), &tuning);
            assert!(
                value < prev - TOL,
                "target_position_error not strictly falling at Positioning {positioning}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn target_position_error_stays_within_curve_bounds() {
        let tuning = tuning();
        for positioning in 1..=200u8 {
            let value = target_position_error(positioning, fit(), &tuning);
            assert!(
                (0.02..=0.70).contains(&value),
                "target_position_error {positioning} = {value} outside [0.02, 0.70]"
            );
        }
    }
}
