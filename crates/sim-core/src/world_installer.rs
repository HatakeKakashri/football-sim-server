//! `WorldInstaller`: the single place that inserts every resource the
//! simulation's systems require.
//!
//! All systems declare their resources as required `Res<T>` (or
//! `ResMut<T>`); there are no silent `Option<Res<T>>` fallbacks. If a
//! world is missing a resource a system needs, the schedule panics
//! loudly instead of degrading to a default. This module is the single
//! edit surface for the resource set, used by
//! [`Simulation::new`](super::Simulation::new), test worlds, and CLI
//! entry points.

use bevy_ecs::prelude::*;
use sim_ai_player::DecisionEvaluationCount;
use sim_components::{
    Ball, BallAirborne, BallState, MatchClock, PendingBallTouches,
};
use sim_math::{PitchDimensions, Tuning};
use sim_physics::{PitchControlGrid, SimRng};
use sim_rules::{
    CurrentTick, GoalForEmit, KickoffForEmit, OobRestartForEmit, PendingOutOfBounds,
    PendingPass, PendingRestart, PendingShot, PendingTackle,
};

use crate::simulation::event_emit::PendingCross;
use crate::simulation::lifecycle::{FirstHalfAddedTime, HalfTimeEntryTick};
use crate::EventLog;

/// Insert every resource that at least one registered system reads as a
/// required `Res<T>` / `ResMut<T>`.
///
/// Call this once after creating the `World` and before adding systems
/// to the schedule. Resources that are conditionally set during play
/// (e.g. `PendingRestart`, which is only filled after a goal) are
/// inserted with their `default()` value so systems can branch on
/// their contents without panicking on absence.
///
/// # Panics
///
/// Panics if any resource is already present (Bevy's
/// `insert_resource` overwrites silently, but we call it once per
/// world, so a duplicate insertion indicates a caller bug).
pub fn install_all_resources(world: &mut World, seed: u64) {
    // Pitch geometry — required by `ball_physics_system`.
    world.insert_resource(PitchDimensions::standard());
    // Pitch control grid — mutated in place by `pitch_control_system`.
    world.insert_resource(PitchControlGrid::build_standard());
    // Stochastic RNG — seed derived from the match seed.
    world.insert_resource(SimRng::new(seed));
    // Per-player decision evaluation counter.
    world.insert_resource(DecisionEvaluationCount::default());

    // Ball state resource (single source of truth for spin/state/possessor/etc.).
    world.insert_resource(Ball {
        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touch: None,
        touch_history: [None, None],
        kick_velocity: None,
    });
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(BallAirborne::default());
    world.insert_resource(PendingOutOfBounds::default());
    world.insert_resource(OobRestartForEmit::default());
    world.insert_resource(PendingPass::default());
    world.insert_resource(GoalForEmit::default());
    world.insert_resource(KickoffForEmit::default());
    world.insert_resource(PendingShot::default());
    world.insert_resource(PendingTackle::default());
    world.insert_resource(PendingCross::default());
    world.insert_resource(HalfTimeEntryTick { tick: None });
    world.insert_resource(FirstHalfAddedTime(0));
    world.insert_resource(EventLog::new());
    world.insert_resource(Tuning::default());

    // Match clock — the source of truth for elapsed ticks / half /
    // added time / running flag.
    world.insert_resource(MatchClock {
        elapsed_ticks: 0,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });

    // Current tick — inserted at 0 before the first tick; overwritten
    // each tick by `Simulation::tick`.
    world.insert_resource(CurrentTick(0));

    // Pending restart — set by `goal_detection_system` after a goal,
    // consumed by `restart_system`. Default (no pending event) means
    // no restart is awaiting resolution.
    world.insert_resource(PendingRestart::default());
}
