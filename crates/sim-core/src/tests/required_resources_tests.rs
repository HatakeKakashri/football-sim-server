//! Every system in the simulation schedule must fail loudly when a
//! resource it requires is missing, rather than silently defaulting
//! (ticket 29). Each case builds a world through `WorldInstaller`, removes
//! one resource, and asserts the first system run panics naming exactly
//! that resource.

use std::any::type_name;
use std::panic::{AssertUnwindSafe, catch_unwind};

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ExecutorKind;
use sim_components::{Ball, MatchClock};
use sim_math::Tuning;
use sim_physics::SimRng;
use sim_rules::{
    CurrentTick, GoalForEmit, KickoffForEmit, OobRestartForEmit, PendingOutOfBounds, PendingShot,
    PendingTackle,
};

use crate::WorldInstaller;
use crate::simulation::event_emit::{
    PendingCross, emit_cross_header_events, emit_goal_events, emit_kickoff_restart_events,
    emit_oob_and_restart_events, emit_possession_change_on_acquisition,
    emit_possession_change_on_oob, emit_tackle_events, resolve_pass_events, resolve_shot_events,
};
use crate::simulation::trace_emit::trace_emission_system;

/// A world holding every resource the schedule's systems require.
fn installed_world() -> World {
    let mut world = World::new();
    WorldInstaller::new(1).install_resources(&mut world);
    // `MatchClock` is installed with the match entities, not by the
    // installer, so supply it directly.
    world.insert_resource(MatchClock {
        elapsed_ticks: 0,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world
}

/// Run `system` once in a world that lacks `T`; assert it panics and the
/// panic names `T`. The single-threaded executor keeps the original panic
/// payload (the multi-threaded one replaces it with a generic message).
fn assert_requires<T: Resource, M>(system: impl IntoSystemConfigs<M>) {
    let mut world = installed_world();
    world.remove_resource::<T>();
    let mut schedule = Schedule::default();
    schedule.set_executor_kind(ExecutorKind::SingleThreaded);
    schedule.add_systems(system);

    let payload = catch_unwind(AssertUnwindSafe(|| schedule.run(&mut world)))
        .expect_err("system ran without a resource it requires");
    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .unwrap_or_default();
    let short = type_name::<T>().rsplit("::").next().unwrap_or_default();
    assert!(
        message.contains(short),
        "panic should name `{short}`, got: {message}"
    );
}

/// One `#[test]` per (system, resource) pair.
macro_rules! requires {
    ($($name:ident: $system:ident needs $resource:ty;)+) => {
        $(
            #[test]
            fn $name() {
                assert_requires::<$resource, _>($system);
            }
        )+
    };
}

requires! {
    pass_events_need_pending_oob: resolve_pass_events needs PendingOutOfBounds;
    pass_events_need_goal_slot: resolve_pass_events needs GoalForEmit;
    pass_events_need_tick: resolve_pass_events needs CurrentTick;
    possession_acquisition_needs_tick: emit_possession_change_on_acquisition needs CurrentTick;
    possession_oob_needs_pending_oob: emit_possession_change_on_oob needs PendingOutOfBounds;
    possession_oob_needs_tick: emit_possession_change_on_oob needs CurrentTick;
    goal_events_need_goal_slot: emit_goal_events needs GoalForEmit;
    goal_events_need_tick: emit_goal_events needs CurrentTick;
    kickoff_events_need_kickoff_slot: emit_kickoff_restart_events needs KickoffForEmit;
    kickoff_events_need_tick: emit_kickoff_restart_events needs CurrentTick;
    shot_events_need_pending_shot: resolve_shot_events needs PendingShot;
    shot_events_need_goal_slot: resolve_shot_events needs GoalForEmit;
    shot_events_need_oob_slot: resolve_shot_events needs OobRestartForEmit;
    shot_events_need_tuning: resolve_shot_events needs Tuning;
    shot_events_need_rng: resolve_shot_events needs SimRng;
    shot_events_need_tick: resolve_shot_events needs CurrentTick;
    tackle_events_need_pending_tackle: emit_tackle_events needs PendingTackle;
    tackle_events_need_tick: emit_tackle_events needs CurrentTick;
    cross_events_need_pending_cross: emit_cross_header_events needs PendingCross;
    cross_events_need_tick: emit_cross_header_events needs CurrentTick;
    oob_restart_events_need_oob_slot: emit_oob_and_restart_events needs OobRestartForEmit;
    oob_restart_events_need_tick: emit_oob_and_restart_events needs CurrentTick;
    trace_emission_needs_ball: trace_emission_system needs Ball;
}
