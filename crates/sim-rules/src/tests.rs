//! Unit tests for `sim-rules`.
//!
//! Test files (sibling modules, all `#[cfg(test)]`):
//! - `out_of_bounds_tests.rs`: throw-in, corner, goal-kick OOB detection.
//! - `goals_tests.rs`: home/away goal scoring, dead-ball guard.
//! - `possession_tests.rs`: possession + last-touched-by tracking.
//! - `referee_tests.rs`: foul detection (dangerous play).
//! - `clock_tests.rs`: half-time enforcement.
//!
//! This file holds the shared `ball_fixture` helper and the
//! `kickoff_restart` test that bridges `goal_detection_system` (which
//! creates the `PendingRestart`) and `restart_system` (which consumes
//! it).

#![allow(
    clippy::float_cmp,
    reason = "tests compare outputs of identical arithmetic against the inputs they were derived from"
)]

use bevy_ecs::prelude::*;
use sim_components::{Ball, BallMarker, BallState, Position, RuleEvent, Velocity};
use sim_math::Vec2;

use crate::{CurrentTick, PendingRestart, restart_system};

/// Produces a test `Ball` resource with the given `state`.
/// All other fields are zeroed; `possessor` and `last_touched_by` are `None`.
///
/// Note: the legacy `pos: Vec2` parameter is intentionally retained (as
/// `_pos`) for backwards compatibility with the existing test call
/// sites. Ball position lives on the entity component (`Position`),
/// not on this Resource, so callers must still set the entity position
/// separately — the parameter does not propagate.
pub fn ball_fixture(_pos: Vec2, state: BallState) -> Ball {
    Ball {
        kick_velocity: None,
        spin: 0.0,
        state,
        possessor: None,
        last_touched_by: None,
    }
}

/// Integration test bridging `goal_detection_system` (which creates the
/// `PendingRestart`) and `restart_system` (which consumes it). Lives in
/// `tests.rs` because it touches both submodules' public systems.
#[test]
fn test_kickoff_restart_places_ball_at_center() {
    let mut world = World::new();

    world.insert_resource(sim_components::Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (1, 0),
        state: sim_components::MatchState::InPlay,
        seed: 12345,
    });
    let match_entity = world.spawn(()).id();
    world.entity_mut(match_entity).insert(sim_components::MatchClock {
        elapsed_ticks: 30 * 60,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });

    // Create ball entity in Dead state (after a goal)
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(ball_fixture(Vec2::new(105.5, 34.0), BallState::Dead));
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(105.5, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));
    // Stage a pending kick velocity as if `kick_execution_system` had
    // run earlier in the same tick — without the clear, this would
    // survive into the next Physics tick.
    world.resource_mut::<Ball>().kick_velocity = Some(Vec2::new(15.0, 0.0));

    // Set up pending restart for kickoff
    world.insert_resource(PendingRestart {
        event: Some(RuleEvent::KickoffRestart),
        restart_tick: 60, // Restart at tick 60
    });

    let mut schedule = Schedule::default();
    schedule.add_systems(restart_system);

    // Run at tick 60 - should trigger restart
    world.insert_resource(CurrentTick(60));
    schedule.run(&mut world);

    let ball = world.resource::<Ball>();
    let pos = world.entity(ball_entity).get::<Position>().unwrap();
    let vel = world.entity(ball_entity).get::<Velocity>().unwrap();
    assert_eq!(ball.state, BallState::Free);
    assert_eq!(pos.0, Vec2::new(crate::constants::CENTER_SPOT.0, crate::constants::CENTER_SPOT.1));
    assert_eq!(vel.0, Vec2::zero());
    // Regression: a pending kick velocity staged earlier in the tick must
    // not survive the restart — otherwise the next Physics-set tick would
    // impart a phantom impulse to the freshly-placed ball.
    assert!(
        ball.kick_velocity.is_none(),
        "kick_velocity must be cleared on restart, got {:?}",
        ball.kick_velocity
    );

    // PendingRestart should be cleared
    assert!(world.get_resource::<PendingRestart>().is_none());
}