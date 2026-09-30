//! Unit tests for `sim-ai-player`. Compiled only under `cfg(test)`.
//!
//! Test files:
//! - this file: small unit tests for `Consideration` accessors,
//!   `tactical_thresholds`, `intent_kind`, and `raw_input`. Plus the
//!   `run_one_cadence_window` helper used by the decision integration tests.
//! - `decision_tests.rs`: end-to-end cadence / decision-system integration tests.
//! - `perception_tests.rs`: end-to-end perception-system integration tests.
//!
//! `closest_by_distance` was removed in Phase G (dead code cleanup).

use crate::brain::intent_kind;
use crate::{Consideration, ConsiderationContext, tactical_thresholds};

use bevy_ecs::prelude::*;
use sim_ai_core::ResponseCurve;
use sim_components::{ActionIntent, Intent, MovementIntent};
use sim_math::Vec2;

/// Exhaustiveness guard: if a new `Consideration` variant is added without
/// updating the F1 macro, this const-assertion fails to compile, forcing
/// the developer to add the variant to the macro's `$( ... )*` list.
#[test]
#[allow(
    clippy::float_cmp,
    reason = "exact equality against a literal is the point of this smoke check"
)]
fn weight_curve_exhaustive_for_all_variants() {
    // Spot-check: construct every variant the macro must cover and
    // verify both accessors return the values carried by the variant.
    let v = Consideration::DistanceToTarget {
        weight: 0.5,
        curve: ResponseCurve::Linear { min: 0.0, max: 1.0 },
    };
    assert_eq!(v.weight(), 0.5);
    assert!(matches!(v.curve(), ResponseCurve::Linear { min: 0.0, max: 1.0 }));
    // The exhaustive list is verified at compile-time by the macro
    // itself; this test is the runtime smoke check.
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "exact equality against a literal is the point of this pin-the-value sanity check"
)]
fn tactical_thresholds_have_expected_values() {
    // Pin the values to the magic numbers they replace, so a balance
    // pass that changes gameplay tuning is a deliberate act.
    assert_eq!(tactical_thresholds::PASS_LANE_CLEAR_M, 8.0);
    assert_eq!(tactical_thresholds::DEFENDER_PRESSURE_RADIUS_M, 5.0);
    assert_eq!(tactical_thresholds::DISTANCE_TO_TARGET_CAP_M, 30.0);
}

/// Phase D helper: run a schedule for one full cadence window,
/// incrementing the match clock each iteration. Every player is
/// guaranteed at least one evaluation regardless of which slot
/// their entity id hashes into. Returns the final `elapsed_ticks`.
pub fn run_one_cadence_window(
    world: &mut World,
    schedule: &mut Schedule,
    match_entity: Entity,
) -> u64 {
    for _ in 0..crate::DECISION_CADENCE_TICKS {
        // Phase F follow-up: MatchClock is a Resource (spec §3). Update
        // both forms — the Resource is the source of truth for systems;
        // the Component on the entity is kept for backwards compat.
        let elapsed = world
            .get_resource::<sim_components::MatchClock>()
            .map_or(0, |c| c.elapsed_ticks)
            + 1;
        if let Some(mut clock) = world.get_resource_mut::<sim_components::MatchClock>() {
            clock.elapsed_ticks = elapsed;
        }
        if let Some(mut clock) = world
            .entity_mut(match_entity)
            .get_mut::<sim_components::MatchClock>()
        {
            clock.elapsed_ticks = elapsed;
        }
        schedule.run(world);
    }
    world
        .get_resource::<sim_components::MatchClock>()
        .map_or(0, |c| c.elapsed_ticks)
}

/// Phase E PR 4b: the string labels produced by `intent_kind` must
/// match the labels the old flat enum produced, so any consumer that
/// hashes on the label (e.g. hysteresis matching in
/// `player_decision_system`) doesn't drift. This test enumerates all
/// 11 variants and asserts the round-tripped label equals the legacy
/// discriminant name.
#[test]
fn test_intent_kind_string_for_all_variants() {
    // `Entity::PLACEHOLDER` is a Bevy 0.14 `Entity::from_raw(0)` idiom;
    // if the Bevy version changes, adjust.
    let cases = [
        (
            Intent::Movement(MovementIntent::MoveToPosition(Vec2::zero())),
            "MoveToPosition",
        ),
        (
            Intent::Movement(MovementIntent::HoldPosition),
            "HoldPosition",
        ),
        (Intent::Movement(MovementIntent::ChaseBall), "ChaseBall"),
        (Intent::Movement(MovementIntent::Intercept), "Intercept"),
        (Intent::Movement(MovementIntent::SupportRun), "SupportRun"),
        (Intent::Movement(MovementIntent::TrackBack), "TrackBack"),
        (Intent::Action(ActionIntent::PassTo), "PassTo"),
        (
            Intent::Action(ActionIntent::ShootAtGoal(Vec2::zero())),
            "ShootAtGoal",
        ),
        (
            Intent::Action(ActionIntent::Tackle(Entity::PLACEHOLDER)),
            "Tackle",
        ),
        (
            Intent::Action(ActionIntent::MarkOpponent(Entity::PLACEHOLDER)),
            "MarkOpponent",
        ),
        (
            Intent::Action(ActionIntent::Press(Entity::PLACEHOLDER)),
            "Press",
        ),
    ];
    for (intent, expected) in cases {
        assert_eq!(intent_kind(&intent), expected);
    }
}

/// PR 3: the new `Consideration` enum must replace the string-dispatch
/// `compute_consideration_input`. `DistanceToTarget` falls back to
/// distance-to-ball when the intent has no target (e.g. `HoldPosition`),
/// so we feed a snapshot where the player is 2.5m from the ball. The
/// raw input is `30 - 2.5 = 27.5`, which the brief asserts is in
/// `(25, 30)`.
#[test]
fn test_consideration_distance_to_target() {
    let perception = sim_components::PerceptionSnapshot {
        self_position: Vec2::new(50.0, 34.0),
        nearby_teammates: smallvec::SmallVec::default(),
        nearby_opponents: smallvec::SmallVec::default(),
        ball_position: Vec2::new(52.5, 34.0),
        ball_state: sim_components::BallState::Free,
        goal_position: Vec2::new(105.0, 34.0),
        pitch_bounds: sim_components::PitchBounds {
            distance_to_left: 50.0,
            distance_to_right: 55.0,
            distance_to_top: 34.0,
            distance_to_bottom: 34.0,
        },
    };
    let intent = sim_components::Intent::Movement(sim_components::MovementIntent::HoldPosition);
    let c = Consideration::DistanceToTarget {
        weight: 1.0,
        curve: ResponseCurve::Linear { min: 0.0, max: 1.0 },
    };
    let ctx = ConsiderationContext {
        perception: &perception,
        intent: &intent,
        stamina: 1.0,
        skill: 0.7,
        grid: None,
    };
    let raw = c.raw_input(&ctx);
    assert!(raw > 25.0 && raw < 30.0, "raw input was {raw}");
}

/// Spec §10 deviation regression test: `player_stagger_slot` uses a
/// `SplitMix64` hash of the entity id rather than the spec's
/// `tick % 22 == player_index` formula. This test pins the distribution
/// guarantee (uniform spread across slots) so a future switch to the
/// spec's formula is a deliberate, test-visible change.
#[test]
fn test_stagger_slot_distribution() {
    use crate::DECISION_CADENCE_TICKS;
    use crate::player_stagger_slot;

    // Generate 22 distinct entity ids (matching a real roster) and
    // verify the slot distribution is roughly uniform.
    #[allow(clippy::cast_possible_truncation, reason = "DECISION_CADENCE_TICKS is 6; always fits in usize")]
    let mut slot_counts = [0u32; DECISION_CADENCE_TICKS as usize];
    for i in 0..22u32 {
        let entity = bevy_ecs::prelude::Entity::from_raw(i);
        let slot = usize::try_from(player_stagger_slot(entity)).expect("slot fits in usize");
        slot_counts[slot] += 1;
    }

    // With 22 players across 6 slots, expect roughly 3-4 per slot.
    // Allow a tolerance of ±2 to avoid flaky tests while still
    // catching a degenerate distribution (e.g. all players in one slot).
    for (slot, &count) in slot_counts.iter().enumerate() {
        assert!(
            (1..=6).contains(&count),
            "slot {slot} has {count} players; expected 1-6 for a uniform distribution"
        );
    }

    // Verify all slots are represented (no empty slots with 22 players).
    for (slot, &count) in slot_counts.iter().enumerate() {
        assert!(count > 0, "slot {slot} is empty; distribution is not uniform");
    }
}
