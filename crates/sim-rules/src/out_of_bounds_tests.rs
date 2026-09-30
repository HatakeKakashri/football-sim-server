//! Tests for `out_of_bounds_system` (Law 9): throw-in, corner, goal-kick.

use bevy_ecs::prelude::*;
use sim_components::{
    BallMarker, BallOutOfBoundsType, BallState, Position, RuleEvent, Velocity,
};
use sim_math::Vec2;

use crate::tests::ball_fixture;
use crate::{CurrentTick, out_of_bounds_system};

#[test]
fn test_out_of_bounds_detection_throw_in() {
    let mut world = World::new();

    // Ball at bottom touchline (y=0, x=20) with low velocity - physics bounced it back
    // Throw-in: ball crosses touchline (y < 0 or y > PITCH_WIDTH)
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(ball_fixture(Vec2::new(20.0, 0.0), BallState::Free));
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(20.0, 0.0)));
    world
        .entity_mut(ball_entity)
        .insert(Velocity(Vec2::new(0.0, 0.0)));

    let mut schedule = Schedule::default();
    schedule.add_systems(out_of_bounds_system);

    // Run with current_tick = 0
    world.insert_resource(CurrentTick(0));
    schedule.run(&mut world);

    let ball = world.resource::<sim_components::Ball>();
    assert_eq!(ball.state, BallState::Dead);

    let rule_event = world.entity(ball_entity).get::<RuleEvent>();
    assert!(rule_event.is_some(), "RuleEvent should be emitted");
    if let Some(RuleEvent::OutOfBounds(oob_type)) = rule_event {
        assert!(
            matches!(oob_type, BallOutOfBoundsType::ThrowIn),
            "Ball at y=0 (touchline) should be ThrowIn"
        );
    }
}

#[test]
fn test_out_of_bounds_detection_corner() {
    let mut world = World::new();

    // Ball at bottom-left corner (x=0.1, y=0.1) with low velocity
    // Both x and y are at the boundary, so it's a corner
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(ball_fixture(Vec2::new(0.1, 0.1), BallState::Free));
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(0.1, 0.1)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

    let mut schedule = Schedule::default();
    schedule.add_systems(out_of_bounds_system);

    world.insert_resource(CurrentTick(0));
    schedule.run(&mut world);

    let ball = world.resource::<sim_components::Ball>();
    assert_eq!(ball.state, BallState::Dead);

    let rule_event = world.entity(ball_entity).get::<RuleEvent>();
    assert!(rule_event.is_some());
    if let Some(RuleEvent::OutOfBounds(oob_type)) = rule_event {
        assert!(
            matches!(oob_type, BallOutOfBoundsType::Corner),
            "Ball at corner position should be Corner"
        );
    }
}

#[test]
fn test_out_of_bounds_goal_kick() {
    let mut world = World::new();

    // Ball in goal area on left side
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(ball_fixture(Vec2::new(-0.5, 34.0), BallState::Free));
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(-0.5, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

    let mut schedule = Schedule::default();
    schedule.add_systems(out_of_bounds_system);

    world.insert_resource(CurrentTick(0));
    schedule.run(&mut world);

    let rule_event = world.entity(ball_entity).get::<RuleEvent>();
    assert!(rule_event.is_some());
    if let Some(RuleEvent::OutOfBounds(oob_type)) = rule_event {
        assert!(matches!(oob_type, BallOutOfBoundsType::GoalKick));
    }
}