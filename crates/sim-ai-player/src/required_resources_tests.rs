//! The player AI systems must fail loudly when a resource they require is
//! missing, rather than silently defaulting (ticket 29). Each case builds a
//! fully installed world, removes one resource, and asserts the first
//! system run panics naming exactly that resource.

use std::any::type_name;
use std::panic::{AssertUnwindSafe, catch_unwind};

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ExecutorKind;
use sim_components::{Ball, MatchClock, PendingBallTouches};
use sim_math::Tuning;
use sim_physics::{PitchControlGrid, SimRng};
use sim_rules::{CurrentTick, PendingPass, PendingShot};

use crate::{install_ai_player_resources, kick_execution_system, player_decision_system};

/// A world holding every resource the player AI systems require.
fn installed_world() -> World {
    let mut world = World::new();
    sim_physics::install_physics_resources(&mut world, 1);
    sim_rules::install_rules_resources(&mut world);
    install_ai_player_resources(&mut world);
    world.insert_resource(Ball {
        spin: 0.0,
        state: sim_components::BallState::Free,
        possessor: None,
        last_touch: None,
        touch_history: [None, None],
        kick_velocity: None,
    });
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(MatchClock {
        elapsed_ticks: 0,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(Tuning::default());
    world
}

/// True when `message` contains `full` as a whole type path. A plain
/// `contains` would let a panic about `sim_components::BallAirborne`
/// satisfy a check for `sim_components::Ball`, so the match must not be
/// followed by another identifier character.
fn names_resource(message: &str, full: &str) -> bool {
    message.match_indices(full).any(|(start, _)| {
        message[start + full.len()..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
    })
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
    let full = type_name::<T>();
    assert!(
        names_resource(&message, full),
        "panic should name `{full}`, got: {message}"
    );
}

#[test]
fn decision_requires_pitch_control_grid() {
    assert_requires::<PitchControlGrid, _>(player_decision_system);
}

#[test]
fn decision_requires_ball() {
    assert_requires::<Ball, _>(player_decision_system);
}

#[test]
fn kick_execution_requires_pending_pass() {
    assert_requires::<PendingPass, _>(kick_execution_system);
}

#[test]
fn kick_execution_requires_pending_shot() {
    assert_requires::<PendingShot, _>(kick_execution_system);
}

#[test]
fn kick_execution_requires_current_tick() {
    assert_requires::<CurrentTick, _>(kick_execution_system);
}

#[test]
fn kick_execution_requires_match_clock() {
    assert_requires::<MatchClock, _>(kick_execution_system);
}

#[test]
fn kick_execution_requires_rng() {
    assert_requires::<SimRng, _>(kick_execution_system);
}

#[test]
fn kick_execution_requires_tuning() {
    assert_requires::<Tuning, _>(kick_execution_system);
}
