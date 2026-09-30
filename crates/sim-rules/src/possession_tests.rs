//! Tests for `possession_resolution_system`: closest-player selection,
//! possessor entity identity, possession clearing, and `last_touched_by`
//! pass-tracking.

use bevy_ecs::prelude::*;
use sim_components::{
    Ball, BallMarker, BallState, Player, Position, RoleComponent, Skill, Stamina, TeamId,
    TeamIdComponent, Velocity,
};
use sim_math::Vec2;

use crate::possession_resolution_system;

#[test]
fn test_possession_resolution() {
    let mut world = World::new();

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(Ball {
        kick_velocity: None,

        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touched_by: None,
    });
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(50.0, 34.0)));

    let player_entity = world.spawn(()).id();
    world.entity_mut(player_entity).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world
        .entity_mut(player_entity)
        .insert(Position(Vec2::new(50.5, 34.0)));
    world
        .entity_mut(player_entity)
        .insert(Velocity(Vec2::zero()));
    world.entity_mut(player_entity).insert(Stamina(0.8));
    world
        .entity_mut(player_entity)
        .insert(RoleComponent(sim_components::Role::Striker));
    world.entity_mut(player_entity).insert(Skill(0.8));
    world
        .entity_mut(player_entity)
        .insert(TeamIdComponent(TeamId(0)));

    let mut schedule = Schedule::default();
    schedule.add_systems(possession_resolution_system);
    schedule.run(&mut world);

    let ball = world.resource::<Ball>();
    assert_eq!(ball.state, BallState::Possessed);
}

/// Test that possession is resolved to the actual player entity, not
/// `Entity::PLACEHOLDER`.
#[test]
fn test_possession_resolved_to_real_entity() {
    let mut world = World::new();

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(Ball {
        kick_velocity: None,

        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touched_by: None,
    });
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(50.0, 34.0)));

    let player_entity = world.spawn(()).id();
    world.entity_mut(player_entity).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world
        .entity_mut(player_entity)
        .insert(Position(Vec2::new(50.3, 34.0)));
    world
        .entity_mut(player_entity)
        .insert(Velocity(Vec2::zero()));
    world.entity_mut(player_entity).insert(Stamina(0.8));
    world
        .entity_mut(player_entity)
        .insert(RoleComponent(sim_components::Role::Striker));
    world.entity_mut(player_entity).insert(Skill(0.8));
    world
        .entity_mut(player_entity)
        .insert(TeamIdComponent(TeamId(0)));

    let mut schedule = Schedule::default();
    schedule.add_systems(possession_resolution_system);
    schedule.run(&mut world);

    let ball = world.resource::<Ball>();
    assert_eq!(ball.state, BallState::Possessed);
    assert_eq!(
        ball.possessor,
        Some(player_entity),
        "Ball.possessor must be the real player entity, not PLACEHOLDER"
    );
    // last_touched_by should also be set (player is within 1.0 m)
    assert_eq!(ball.last_touched_by, Some(player_entity));
}

/// Test that possession is cleared when no player is within 1.5 m.
#[test]
fn test_possession_cleared_when_ball_free() {
    let mut world = World::new();

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(Ball {
        kick_velocity: None,

        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touched_by: None,
    });
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(50.0, 34.0)));

    let player_entity = world.spawn(()).id();
    world.entity_mut(player_entity).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world
        .entity_mut(player_entity)
        .insert(Position(Vec2::new(55.0, 34.0)));
    world
        .entity_mut(player_entity)
        .insert(Velocity(Vec2::zero()));
    world.entity_mut(player_entity).insert(Stamina(0.8));
    world
        .entity_mut(player_entity)
        .insert(RoleComponent(sim_components::Role::Striker));
    world.entity_mut(player_entity).insert(Skill(0.8));
    world
        .entity_mut(player_entity)
        .insert(TeamIdComponent(TeamId(0)));

    let mut schedule = Schedule::default();
    schedule.add_systems(possession_resolution_system);
    schedule.run(&mut world);

    let ball = world.resource::<Ball>();
    assert_eq!(ball.state, BallState::Free);
    assert_eq!(
        ball.possessor, None,
        "Ball should have no possessor when no player is within 1.5 m"
    );
}

/// Test that `last_touched_by` is updated when a player touches the ball
/// (simulates a pass scenario).
#[test]
fn test_offside_last_touch_tracked() {
    let mut world = World::new();

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(Ball {
        kick_velocity: None,

        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touched_by: None,
    });
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(50.0, 34.0)));

    let player_a = world.spawn(()).id();
    world.entity_mut(player_a).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world
        .entity_mut(player_a)
        .insert(Position(Vec2::new(50.2, 34.0)));
    world.entity_mut(player_a).insert(Velocity(Vec2::zero()));
    world.entity_mut(player_a).insert(Stamina(0.8));
    world
        .entity_mut(player_a)
        .insert(RoleComponent(sim_components::Role::Striker));
    world.entity_mut(player_a).insert(Skill(0.8));
    world
        .entity_mut(player_a)
        .insert(TeamIdComponent(TeamId(0)));

    let mut schedule = Schedule::default();
    schedule.add_systems(possession_resolution_system);
    schedule.run(&mut world);

    let ball = world.resource::<Ball>();
    assert_eq!(
        ball.last_touched_by,
        Some(player_a),
        "Player A (closest to ball) should be recorded as last toucher"
    );

    // Now move the ball to player B (simulating a pass)
    {
        let mut em = world.entity_mut(ball_entity);
        let mut pos = em.get_mut::<Position>().unwrap();
        pos.0 = Vec2::new(80.0, 34.0);
    }
    {
        let mut em = world.entity_mut(ball_entity);
        let mut pos = em.get_mut::<Position>().unwrap();
        pos.0 = Vec2::new(80.0, 34.0);
    }
    // Phase C §4.3: Ball state lives on the Resource.
    {
        let mut ball = world.resource_mut::<Ball>();
        ball.state = BallState::Free;
        ball.possessor = None;
    }

    // Player B is now close to the new ball position
    let player_b = world.spawn(()).id();
    world.entity_mut(player_b).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world
        .entity_mut(player_b)
        .insert(Position(Vec2::new(80.3, 34.0)));
    world.entity_mut(player_b).insert(Velocity(Vec2::zero()));
    world.entity_mut(player_b).insert(Stamina(0.9));
    world
        .entity_mut(player_b)
        .insert(RoleComponent(sim_components::Role::CentralMidfielder));
    world.entity_mut(player_b).insert(Skill(0.7));
    world
        .entity_mut(player_b)
        .insert(TeamIdComponent(TeamId(0)));

    // Player A is far from the new ball position
    {
        let mut em = world.entity_mut(player_a);
        let mut pos = em.get_mut::<Position>().unwrap();
        pos.0 = Vec2::new(50.0, 34.0);
    }

    schedule.run(&mut world);

    let ball = world.resource::<Ball>();
    assert_eq!(
        ball.last_touched_by,
        Some(player_b),
        "After pass, last_touched_by should be player B (new closest)"
    );
    assert_eq!(ball.possessor, Some(player_b));
    assert_eq!(ball.state, BallState::Possessed);
}