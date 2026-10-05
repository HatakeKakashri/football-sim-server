//! Tests for `out_of_bounds_system` (Law 9): throw-in, corner, goal-kick,
//! plus the touch-clear-on-OOB wiring (`last_touch_team` is captured in
//! the event payload before `last_touch` is cleared).

use bevy_ecs::prelude::*;
use sim_components::{
    Ball, BallMarker, BallOutOfBoundsType, BallState, LastTouch, Player, Position, RuleEvent,
    TeamId, TeamIdComponent, TouchKind, Velocity,
};
use sim_math::Vec2;

use crate::tests::ball_fixture;
use crate::{CurrentTick, out_of_bounds_system};

/// Build a Ball resource with `last_touch = Some(LastTouch{team, toucher, Play})`.
/// Use this in OOB tests so the debug-build `last_touch.is_some()` assertion
/// in `out_of_bounds_system` doesn't fire — the wiring is correct
/// (kickoff / kick / restart placement always sets it before OOB can fire),
/// but unit tests for the OOB system alone are not wired through those
/// producers.
fn ball_fixture_with_touch(toucher: bevy_ecs::prelude::Entity, team: TeamId) -> Ball {
    let mut b = ball_fixture(Vec2::zero(), BallState::Free);
    b.last_touch = Some(LastTouch {
        team,
        player: Some(toucher),
        kind: TouchKind::Play,
    });
    b
}

#[test]
fn test_out_of_bounds_detection_throw_in() {
    let mut world = World::new();

    // Toucher player on team 0 (set up so OOB has a `last_touch` to capture).
    let toucher = world.spawn(()).id();
    world.entity_mut(toucher).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world.entity_mut(toucher).insert(TeamIdComponent(TeamId(0)));

    // Ball at bottom touchline (y=0, x=20) with low velocity - physics bounced it back
    // Throw-in: ball crosses touchline (y < 0 or y > PITCH_WIDTH)
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(ball_fixture_with_touch(toucher, TeamId(0)));
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
    assert!(
        ball.last_touch.is_none(),
        "OOB must clear last_touch after capturing it"
    );

    let rule_event = world.entity(ball_entity).get::<RuleEvent>();
    assert!(rule_event.is_some(), "RuleEvent should be emitted");
    if let Some(RuleEvent::OutOfBounds {
        oob_type,
        last_touch_team,
    }) = rule_event
    {
        assert!(
            matches!(oob_type, BallOutOfBoundsType::ThrowIn),
            "Ball at y=0 (touchline) should be ThrowIn"
        );
        assert_eq!(
            *last_touch_team,
            Some(TeamId(0)),
            "OOB event payload must capture the last toucher's team"
        );
    } else {
        panic!("expected RuleEvent::OutOfBounds, got {rule_event:?}");
    }
}

#[test]
fn test_out_of_bounds_detection_corner() {
    let mut world = World::new();

    let toucher = world.spawn(()).id();
    world.entity_mut(toucher).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world.entity_mut(toucher).insert(TeamIdComponent(TeamId(0)));

    // Ball at bottom-left corner (x=0.1, y=0.1) with low velocity
    // Both x and y are at the boundary, so it's a corner
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(ball_fixture_with_touch(toucher, TeamId(0)));
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
    if let Some(RuleEvent::OutOfBounds { oob_type, .. }) = rule_event {
        assert!(
            matches!(oob_type, BallOutOfBoundsType::Corner),
            "Ball at corner position should be Corner"
        );
    } else {
        panic!("expected RuleEvent::OutOfBounds, got {rule_event:?}");
    }
}

#[test]
fn test_out_of_bounds_goal_kick() {
    let mut world = World::new();

    let toucher = world.spawn(()).id();
    world.entity_mut(toucher).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world.entity_mut(toucher).insert(TeamIdComponent(TeamId(0)));

    // Ball in goal area on left side
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(ball_fixture_with_touch(toucher, TeamId(0)));
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
    if let Some(RuleEvent::OutOfBounds { oob_type, .. }) = rule_event {
        assert!(matches!(oob_type, BallOutOfBoundsType::GoalKick));
    } else {
        panic!("expected RuleEvent::OutOfBounds, got {rule_event:?}");
    }
}
