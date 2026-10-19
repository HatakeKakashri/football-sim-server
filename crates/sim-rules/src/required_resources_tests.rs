//! A rule system must fail loudly when a resource it requires is missing,
//! rather than silently defaulting (ticket 29). Each case builds a world
//! with everything installed, removes one resource, and asserts the first
//! system run panics naming exactly that resource.

use std::any::type_name;
use std::panic::{AssertUnwindSafe, catch_unwind};

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ExecutorKind;
use sim_components::{BallState, Match, MatchClock, MatchState, PendingBallTouches};
use sim_math::Vec2;

use crate::tests::ball_fixture;
use crate::{
    CurrentTick, PendingRuleOutcome, foul_detection_system,
    goal_detection_system, install_rules_resources, match_duration_enforcement_system,
    minimum_player_count_system, possession_resolution_system, restart_system,
    stoppage_tracking_system,
};

/// A world holding every resource the rule systems require.
fn installed_world() -> World {
    let mut world = World::new();
    install_rules_resources(&mut world);
    world.insert_resource(ball_fixture(Vec2::new(52.5, 34.0), BallState::Free));
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: MatchState::InPlay,
        seed: 1,
    });
    world.insert_resource(MatchClock {
        elapsed_ticks: 0,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
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
fn possession_resolution_requires_current_tick() {
    assert_requires::<CurrentTick, _>(possession_resolution_system);
}

#[test]
fn stoppage_tracking_requires_current_tick() {
    assert_requires::<CurrentTick, _>(stoppage_tracking_system);
}

#[test]
fn foul_detection_requires_current_tick() {
    assert_requires::<CurrentTick, _>(foul_detection_system);
}

#[test]
fn minimum_player_count_requires_current_tick() {
    assert_requires::<CurrentTick, _>(minimum_player_count_system);
}

#[test]
fn match_duration_enforcement_requires_current_tick() {
    assert_requires::<CurrentTick, _>(match_duration_enforcement_system);
}

#[test]
fn goal_detection_requires_current_tick() {
    assert_requires::<CurrentTick, _>(goal_detection_system);
}

#[test]
fn goal_detection_requires_goal_slot() {
    assert_requires::<PendingRuleOutcome, _>(goal_detection_system);
}

#[test]
fn restart_requires_current_tick() {
    assert_requires::<CurrentTick, _>(restart_system);
}

#[test]
fn restart_requires_oob_restart_slot() {
    assert_requires::<PendingRuleOutcome, _>(restart_system);
}

#[test]
fn restart_requires_kickoff_slot() {
    assert_requires::<PendingRuleOutcome, _>(restart_system);
}
