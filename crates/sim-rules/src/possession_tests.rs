//! Tests for `possession_resolution_system`: closest-player selection,
//! possessor entity identity, possession clearing, and `last_touch`
//! pass-tracking.

use bevy_ecs::prelude::*;
use sim_components::{
    Ball, BallMarker, BallState, Player, Position, RoleComponent, Skill, Stamina, TeamId,
    TeamIdComponent, Velocity,
};
use sim_math::Vec2;

use crate::possession_resolution_system;
use crate::touch::apply_ball_touched_system;

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
        last_touch: None,

        touch_history: [None, None],
    });
    world.insert_resource(sim_components::PendingBallTouches::default());
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(50.0, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

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
    schedule.add_systems((possession_resolution_system, apply_ball_touched_system).chain());
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
        last_touch: None,

        touch_history: [None, None],
    });
    world.insert_resource(sim_components::PendingBallTouches::default());
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(50.0, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

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
    schedule.add_systems((possession_resolution_system, apply_ball_touched_system).chain());
    schedule.run(&mut world);

    let ball = world.resource::<Ball>();
    assert_eq!(ball.state, BallState::Possessed);
    assert_eq!(
        ball.possessor,
        Some(player_entity),
        "Ball.possessor must be the real player entity, not PLACEHOLDER"
    );
    // last_touch should also be set on possession change (Play kind)
    assert_eq!(
        ball.last_touch,
        Some(sim_components::LastTouch::by_player(TeamId(0), player_entity, sim_components::TouchKind::Play))
    );
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
        last_touch: None,

        touch_history: [None, None],
    });
    world.insert_resource(sim_components::PendingBallTouches::default());
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
    schedule.add_systems((possession_resolution_system, apply_ball_touched_system).chain());
    schedule.run(&mut world);

    let ball = world.resource::<Ball>();
    assert_eq!(ball.state, BallState::Free);
    assert_eq!(
        ball.possessor, None,
        "Ball should have no possessor when no player is within 1.5 m"
    );
}

/// Test that `last_touch` is updated when possession changes between
/// players (interception / pass-receiving scenario). The new possessor
/// becomes the `LastTouch` entity with `kind = Play`.
#[test]
fn test_possession_change_registers_play_touch() {
    let mut world = World::new();

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(Ball {
        kick_velocity: None,

        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touch: None,

        touch_history: [None, None],
    });
    world.insert_resource(sim_components::PendingBallTouches::default());
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(50.0, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

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
    schedule.add_systems((possession_resolution_system, apply_ball_touched_system).chain());
    schedule.run(&mut world);

    let ball = world.resource::<Ball>();
    assert_eq!(
        ball.last_touch,
        Some(sim_components::LastTouch::by_player(TeamId(0), player_a, sim_components::TouchKind::Play)),
        "Player A (gains possession) should be recorded as last toucher with Play kind"
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
        ball.last_touch,
        Some(sim_components::LastTouch::by_player(TeamId(0), player_b, sim_components::TouchKind::Play)),
        "After pass, last_touch should be player B with Play kind"
    );
    assert_eq!(ball.possessor, Some(player_b));
    assert_eq!(ball.state, BallState::Possessed);
}

/// Interception scenario: A (team 0) kicks the ball; B (team 1) is within
/// proximity when the ball arrives and gains possession by
/// `possession_resolution_system`. The `last_touch` must point at B with
/// `kind = Play`.
#[test]
fn test_interception_registers_play_touch_on_new_possessor() {
    let mut world = World::new();

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        // Simulate "A just kicked it" — last_touch on A from
        // kick_execution_system.
        state: BallState::Free,
        possessor: None,
        last_touch: Some(sim_components::LastTouch {
            // Placeholder pre-seed for "A just kicked it": the test
            // does not care about the kicker's identity or team —
            // only that B's acquisition overwrites the record.
            team: TeamId(0),
            player: None,
            player_id: None,
            kind: sim_components::TouchKind::Play,
            tick: 0,
        }),
        touch_history: [None, None],
    });
    world.insert_resource(sim_components::PendingBallTouches::default());
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(70.0, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

    // Player B (team 1) within 1.5 m of the ball.
    let player_b = world.spawn(()).id();
    world.entity_mut(player_b).insert(Player {
        team_id: TeamId(1),
        intent: None,
    });
    world
        .entity_mut(player_b)
        .insert(Position(Vec2::new(70.4, 34.0)));
    world.entity_mut(player_b).insert(Velocity(Vec2::zero()));
    world.entity_mut(player_b).insert(Skill(0.8));
    world.entity_mut(player_b).insert(Stamina(0.9));
    world
        .entity_mut(player_b)
        .insert(RoleComponent(sim_components::Role::CenterBack));
    world
        .entity_mut(player_b)
        .insert(TeamIdComponent(TeamId(1)));

    let mut schedule = Schedule::default();
    schedule.add_systems((possession_resolution_system, apply_ball_touched_system).chain());
    schedule.run(&mut world);

    let ball = world.resource::<Ball>();
    assert_eq!(ball.possessor, Some(player_b));
    assert_eq!(
        ball.last_touch,
        Some(sim_components::LastTouch::by_player(TeamId(1), player_b, sim_components::TouchKind::Play)),
        "interception must register B as a Play touch, not leave the \
         last_touch on the previous kicker"
    );
}
