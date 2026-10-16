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

use crate::physical_mechanics::held_ball_retention_probability;
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
/// Index of `Attribute::Dribbling` in `Attribute::ALL` /
/// `Tuning::curve_params`. Mirrors the attribute declaration order; the
/// cross-crate consistency is asserted by
/// `sim_components::attribute_tests::a10_curve_monotonicity_all_attributes`.
const DRIBBLING_IDX: usize = Attribute::Dribbling as usize;
/// Index of `Attribute::Crossing`.
const CROSSING_IDX: usize = Attribute::Crossing as usize;
/// Index of `Attribute::Heading`.
const HEADING_IDX: usize = Attribute::Heading as usize;
/// Index of `Attribute::Bravery`.
const BRAVERY_IDX: usize = Attribute::Bravery as usize;

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

/// Ball-control retention probability in `[0, 1]`: the carrier's chance
/// to keep the ball against an incoming challenge while dribbling.
///
/// Rises monotonically with `Dribbling` — a better dribbler retains
/// possession more often under pressure.
///
/// The `Dribbling` curve-table entry is rising (`min: 0.05`, `max:
/// 0.95` at the default tuning), so the raw curve output *is* the
/// retention probability.
#[must_use]
pub fn dribble_retention_probability(
    dribbling: u8,
    fit_multiplier: f32,
    tuning: &Tuning,
) -> f32 {
    let effective = effective_attribute(dribbling, fit_multiplier);
    curve(effective as i32, &tuning.curve_params[DRIBBLING_IDX])
}

/// Cross accuracy in `[0, 1]`: the likelihood a cross reaches its
/// intended target point.
///
/// Rises monotonically with `Crossing` — a better crosser hits the
/// target point more often, which feeds the airborne duel at that point
/// (PRD §7.4).
///
/// The `Crossing` curve-table entry is rising (`min: 0.05`, `max:
/// 0.95` at the default tuning), so the raw curve output *is* the
/// accuracy.
#[must_use]
pub fn cross_accuracy(crossing: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(crossing, fit_multiplier);
    curve(effective as i32, &tuning.curve_params[CROSSING_IDX])
}

/// Header accuracy in `[0, 1]`: the likelihood a header hits its
/// intended target.
///
/// Rises monotonically with `Heading` — a better headerer directs the
/// ball more accurately at the aerial target point.
///
/// The `Heading` curve-table entry is rising (`min: 0.05`, `max: 0.95`
/// at the default tuning), so the raw curve output *is* the accuracy.
#[must_use]
pub fn header_accuracy(heading: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(heading, fit_multiplier);
    curve(effective as i32, &tuning.curve_params[HEADING_IDX])
}

/// Header power in `[0, 1]`: the pace and placement of the header.
///
/// Rises monotonically with `Heading` — a more powerful header is
/// harder for the defender or goalkeeper to deal with.
///
/// The `Heading` curve-table entry is rising, so the raw curve output
/// *is* the power.
#[must_use]
pub fn header_power(heading: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(heading, fit_multiplier);
    curve(effective as i32, &tuning.curve_params[HEADING_IDX])
}

/// Probability the attacker wins an aerial duel at the target point
/// (PRD §7.4).
///
/// The attacker's `Heading` (power) and `Strength` contend with the
/// defender's `Heading` (accuracy) and `Strength`. The attacker's win
/// probability rises monotonically with the attacker's attributes and
/// falls monotonically with the defender's, so it is a genuine
/// probabilistic duel rather than a deterministic comparison.
#[must_use]
pub fn aerial_duel_attacker_win_probability(
    attacker_heading: u8,
    attacker_strength: u8,
    defender_heading: u8,
    defender_strength: u8,
    fit_multiplier: f32,
    tuning: &Tuning,
) -> f32 {
    let attacker = header_power(attacker_heading, fit_multiplier, tuning)
        .mul_add(
            held_ball_retention_probability(attacker_strength, fit_multiplier, tuning),
            0.0,
        );
    let defender = header_accuracy(defender_heading, fit_multiplier, tuning).mul_add(
        held_ball_retention_probability(defender_strength, fit_multiplier, tuning),
        0.0,
    );
    let total = attacker + defender;
    if total <= 0.0 {
        return 0.5;
    }
    attacker / total
}

/// Bravery decision weight in `[0, 1]`: a player's willingness to
/// contest duels, block shots, and attempt challenges.
///
/// Rises monotonically with `Bravery` — a braver player is more likely
/// to commit to a contest, which weights the tackle / press / challenge
/// decision (PRD §6.2).
///
/// The `Bravery` curve-table entry is rising (`min: 0.05`, `max: 0.95`
/// at the default tuning), so the raw curve output *is* the decision
/// weight.
#[must_use]
pub fn bravery_contest_weight(bravery: u8, fit_multiplier: f32, tuning: &Tuning) -> f32 {
    let effective = effective_attribute(bravery, fit_multiplier);
    curve(effective as i32, &tuning.curve_params[BRAVERY_IDX])
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

    // === Dribbling ===

    #[test]
    fn dribble_retention_probability_is_strictly_rising_in_dribbling() {
        let tuning = tuning();
        let mut prev = dribble_retention_probability(1, fit(), &tuning);
        for dribbling in 2..=200u8 {
            let value = dribble_retention_probability(dribbling, fit(), &tuning);
            assert!(
                value > prev + TOL,
                "dribble_retention_probability not strictly rising at Dribbling {dribbling}: {prev} -> {value}"
            );
            prev = value;
        }
        assert!(
            prev < 1.0 - TOL,
            "dribble_retention_probability must stay below one at Dribbling 200 ({prev})"
        );
    }

    #[test]
    fn dribble_retention_probability_stays_within_curve_bounds() {
        let tuning = tuning();
        for dribbling in 1..=200u8 {
            let value = dribble_retention_probability(dribbling, fit(), &tuning);
            assert!(
                (0.05..=0.95).contains(&value),
                "dribble_retention_probability {dribbling} = {value} outside [0.05, 0.95]"
            );
        }
    }

    #[test]
    fn dribble_retention_probability_honors_fit_multiplier() {
        let tuning = tuning();
        let full = dribble_retention_probability(100, 1.00, &tuning);
        let partial = dribble_retention_probability(100, 0.75, &tuning);
        assert!(
            full > partial + TOL,
            "retention at full fit ({full}) must exceed partial fit ({partial})"
        );
    }

    // === Crossing ===

    #[test]
    fn cross_accuracy_is_strictly_rising_in_crossing() {
        let tuning = tuning();
        let mut prev = cross_accuracy(1, fit(), &tuning);
        for crossing in 2..=200u8 {
            let value = cross_accuracy(crossing, fit(), &tuning);
            assert!(
                value > prev + TOL,
                "cross_accuracy not strictly rising at Crossing {crossing}: {prev} -> {value}"
            );
            prev = value;
        }
        assert!(
            prev < 1.0 - TOL,
            "cross_accuracy must stay below one at Crossing 200 ({prev})"
        );
    }

    #[test]
    fn cross_accuracy_stays_within_curve_bounds() {
        let tuning = tuning();
        for crossing in 1..=200u8 {
            let value = cross_accuracy(crossing, fit(), &tuning);
            assert!(
                (0.05..=0.95).contains(&value),
                "cross_accuracy {crossing} = {value} outside [0.05, 0.95]"
            );
        }
    }

    #[test]
    fn cross_accuracy_honors_fit_multiplier() {
        let tuning = tuning();
        let full = cross_accuracy(100, 1.00, &tuning);
        let partial = cross_accuracy(100, 0.75, &tuning);
        assert!(
            full > partial + TOL,
            "cross accuracy at full fit ({full}) must exceed partial fit ({partial})"
        );
    }

    // === Heading ===

    #[test]
    fn header_accuracy_is_strictly_rising_in_heading() {
        let tuning = tuning();
        let mut prev = header_accuracy(1, fit(), &tuning);
        for heading in 2..=200u8 {
            let value = header_accuracy(heading, fit(), &tuning);
            assert!(
                value > prev + TOL,
                "header_accuracy not strictly rising at Heading {heading}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn header_accuracy_stays_within_curve_bounds() {
        let tuning = tuning();
        for heading in 1..=200u8 {
            let value = header_accuracy(heading, fit(), &tuning);
            assert!(
                (0.05..=0.95).contains(&value),
                "header_accuracy {heading} = {value} outside [0.05, 0.95]"
            );
        }
    }

    #[test]
    fn header_power_is_strictly_rising_in_heading() {
        let tuning = tuning();
        let mut prev = header_power(1, fit(), &tuning);
        for heading in 2..=200u8 {
            let value = header_power(heading, fit(), &tuning);
            assert!(
                value > prev + TOL,
                "header_power not strictly rising at Heading {heading}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn header_power_stays_within_curve_bounds() {
        let tuning = tuning();
        for heading in 1..=200u8 {
            let value = header_power(heading, fit(), &tuning);
            assert!(
                (0.05..=0.95).contains(&value),
                "header_power {heading} = {value} outside [0.05, 0.95]"
            );
        }
    }

    // === Aerial duel ===

    #[test]
    fn aerial_duel_attacker_win_rises_with_attacker_heading() {
        let tuning = tuning();
        let fit = 1.0;
        let mut prev = aerial_duel_attacker_win_probability(1, 100, 100, 100, fit, &tuning);
        for heading in 2..=200u8 {
            let value = aerial_duel_attacker_win_probability(heading, 100, 100, 100, fit, &tuning);
            assert!(
                value > prev + TOL,
                "duel win prob not rising in attacker Heading at {heading}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn aerial_duel_attacker_win_falls_with_defender_heading() {
        let tuning = tuning();
        let fit = 1.0;
        let mut prev = aerial_duel_attacker_win_probability(100, 100, 1, 100, fit, &tuning);
        for heading in 2..=200u8 {
            let value = aerial_duel_attacker_win_probability(100, 100, heading, 100, fit, &tuning);
            assert!(
                value < prev - TOL,
                "duel win prob not falling in defender Heading at {heading}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn aerial_duel_attacker_win_rises_with_attacker_strength() {
        let tuning = tuning();
        let fit = 1.0;
        let mut prev =
            aerial_duel_attacker_win_probability(100, 1, 100, 100, fit, &tuning);
        for strength in 2..=200u8 {
            let value =
                aerial_duel_attacker_win_probability(100, strength, 100, 100, fit, &tuning);
            assert!(
                value > prev + TOL,
                "duel win prob not rising in attacker Strength at {strength}: {prev} -> {value}"
            );
            prev = value;
        }
    }

    #[test]
    fn aerial_duel_attacker_win_stays_in_unit_interval() {
        let tuning = tuning();
        let fit = 1.0;
        // A bounded grid over the attribute range is enough to confirm the
        // duel probability is always a valid [0, 1] value; an exhaustive
        // 200^4 sweep would be prohibitively slow in a unit test.
        for ah in [1u8, 50, 100, 150, 200] {
            for as_ in [1u8, 50, 100, 150, 200] {
                for dh in [1u8, 50, 100, 150, 200] {
                    for ds in [1u8, 50, 100, 150, 200] {
                        let value =
                            aerial_duel_attacker_win_probability(ah, as_, dh, ds, fit, &tuning);
                        assert!(
                            (0.0..=1.0).contains(&value),
                            "duel win prob {value} outside [0, 1] for ({ah}, {as_}, {dh}, {ds})"
                        );
                    }
                }
            }
        }
    }

    // === Bravery ===

    #[test]
    fn bravery_contest_weight_is_strictly_rising_in_bravery() {
        let tuning = tuning();
        let mut prev = bravery_contest_weight(1, fit(), &tuning);
        for bravery in 2..=200u8 {
            let value = bravery_contest_weight(bravery, fit(), &tuning);
            assert!(
                value > prev + TOL,
                "bravery_contest_weight not strictly rising at Bravery {bravery}: {prev} -> {value}"
            );
            prev = value;
        }
        assert!(
            prev < 1.0 - TOL,
            "bravery_contest_weight must stay below one at Bravery 200 ({prev})"
        );
    }

    #[test]
    fn bravery_contest_weight_stays_within_curve_bounds() {
        let tuning = tuning();
        for bravery in 1..=200u8 {
            let value = bravery_contest_weight(bravery, fit(), &tuning);
            assert!(
                (0.05..=0.95).contains(&value),
                "bravery_contest_weight {bravery} = {value} outside [0.05, 0.95]"
            );
        }
    }

    #[test]
    fn bravery_contest_weight_honors_fit_multiplier() {
        let tuning = tuning();
        let full = bravery_contest_weight(100, 1.00, &tuning);
        let partial = bravery_contest_weight(100, 0.75, &tuning);
        assert!(
            full > partial + TOL,
            "bravery weight at full fit ({full}) must exceed partial fit ({partial})"
        );
    }
}
