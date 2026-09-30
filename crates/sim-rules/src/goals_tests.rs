//! Tests for `goal_detection_system` (Law 10): home goal, away goal,
//! dead-ball guard (no double scoring).

use bevy_ecs::prelude::*;
use sim_components::{BallMarker, BallState, MatchClock, Position, Velocity};
use sim_math::Vec2;

use crate::tests::ball_fixture;
use crate::{CurrentTick, goal_detection_system};

#[test]
fn test_goal_scored_home_team() {
    let mut world = World::new();

    // Match is now a Resource (Phase C §4.3).
    world.insert_resource(sim_components::Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: sim_components::MatchState::InPlay,
        seed: 12345,
    });
    let match_entity = world.spawn(()).id();
    world.entity_mut(match_entity).insert(MatchClock {
        elapsed_ticks: 30 * 60,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });

    // Create ball entity in home goal (x = 105.5, y = 34)
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(ball_fixture(Vec2::new(105.5, 34.0), BallState::Free));
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(105.5, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

    let mut schedule = Schedule::default();
    schedule.add_systems(goal_detection_system);

    world.insert_resource(CurrentTick(100));
    schedule.run(&mut world);

    let m = world.resource::<sim_components::Match>();
    assert_eq!(m.score.0, 1);
    assert_eq!(m.score.1, 0);

    let ball = world.resource::<sim_components::Ball>();
    assert_eq!(ball.state, BallState::Dead);

    // Verify KickoffRestart was scheduled
    let pending = world.get_resource::<crate::PendingRestart>();
    assert!(pending.is_some());
}

#[test]
fn test_goal_scored_away_team() {
    let mut world = World::new();

    world.insert_resource(sim_components::Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: sim_components::MatchState::InPlay,
        seed: 12345,
    });
    let match_entity = world.spawn(()).id();
    world.entity_mut(match_entity).insert(MatchClock {
        elapsed_ticks: 30 * 60,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });

    // Create ball entity in away goal (x = -0.5, y = 34)
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(ball_fixture(Vec2::new(-0.5, 34.0), BallState::Free));
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(-0.5, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

    let mut schedule = Schedule::default();
    schedule.add_systems(goal_detection_system);

    world.insert_resource(CurrentTick(100));
    schedule.run(&mut world);

    let m = world.resource::<sim_components::Match>();
    assert_eq!(m.score.0, 0);
    assert_eq!(m.score.1, 1);
}

#[test]
fn test_no_double_goal_when_ball_dead() {
    let mut world = World::new();

    world.insert_resource(sim_components::Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: sim_components::MatchState::InPlay,
        seed: 12345,
    });
    let match_entity = world.spawn(()).id();
    world.entity_mut(match_entity).insert(MatchClock {
        elapsed_ticks: 30 * 60,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });

    // Create ball entity in home goal with Dead state
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(ball_fixture(Vec2::new(105.5, 34.0), BallState::Dead));
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(105.5, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

    let mut schedule = Schedule::default();
    schedule.add_systems(goal_detection_system);

    world.insert_resource(CurrentTick(100));
    schedule.run(&mut world);

    // Score should still be 0-0 because ball was already Dead
    let m = world.resource::<sim_components::Match>();
    assert_eq!(m.score.0, 0);
    assert_eq!(m.score.1, 0);
}