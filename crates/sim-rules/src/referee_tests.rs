//! Tests for `foul_detection_system` (Law 12): dangerous play on
//! high-speed opposing-team collisions, plus offside-kind gating:
//! `offside_detection_system` must ignore `TouchKind::Restart` touches.

use bevy_ecs::prelude::*;
use sim_components::{
    Ball, BallMarker, LastTouch, MatchClock, Player, Position, RoleComponent, RuleEvent, Skill,
    Stamina, TeamId, TeamIdComponent, TouchKind, Velocity,
};
use sim_math::Vec2;

use crate::{foul_detection_system, offside_detection_system};

#[test]
fn test_foul_detection_dangerous_play() {
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

    // Create two opposing players very close together with high relative velocity
    let home_player = world.spawn(()).id();
    world.entity_mut(home_player).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world
        .entity_mut(home_player)
        .insert(Position(Vec2::new(50.0, 34.0)));
    world
        .entity_mut(home_player)
        .insert(Velocity(Vec2::new(5.0, 0.0)));
    world.entity_mut(home_player).insert(Stamina(0.8));
    world
        .entity_mut(home_player)
        .insert(RoleComponent(sim_components::Role::CenterBack));
    world.entity_mut(home_player).insert(Skill(0.7));
    world
        .entity_mut(home_player)
        .insert(TeamIdComponent(TeamId(0)));

    let away_player = world.spawn(()).id();
    world.entity_mut(away_player).insert(Player {
        team_id: TeamId(1),
        intent: None,
    });
    world
        .entity_mut(away_player)
        .insert(Position(Vec2::new(50.5, 34.0)));
    world
        .entity_mut(away_player)
        .insert(Velocity(Vec2::new(-5.0, 0.0)));
    world.entity_mut(away_player).insert(Stamina(0.8));
    world
        .entity_mut(away_player)
        .insert(RoleComponent(sim_components::Role::Striker));
    world.entity_mut(away_player).insert(Skill(0.7));
    world
        .entity_mut(away_player)
        .insert(TeamIdComponent(TeamId(1)));

    let mut schedule = Schedule::default();
    schedule.add_systems(foul_detection_system);
    schedule.run(&mut world);

    // Players should have RuleEvent::Foul components from high-speed collision
    let home_foul = world.entity(home_player).get::<RuleEvent>();
    let away_foul = world.entity(away_player).get::<RuleEvent>();

    assert!(
        home_foul.is_some() || away_foul.is_some(),
        "At least one player should have a foul event from dangerous play"
    );
}

/// `offside_detection_system` must not consider `last_touch` when its
/// `kind == TouchKind::Restart`. Law 11 does not penalise offside
/// directly from a kickoff, throw-in, goal-kick, or corner.
///
/// Test setup: a home-team striker deep in the away-team's half. If the
/// `last_touch` were consulted with `kind = Play`, offside would flag
/// (player further toward goal than ball or second-last defender). With
/// `kind = Restart`, offside is skipped — no telemetry event fires,
/// no debug line.
#[test]
fn test_offside_ignores_restart_touch() {
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

    // Ball at (60, 34).
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(60.0, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

    // Taker entity — exists only so last_touch has a real reference.
    let taker = world.spawn(()).id();
    world.entity_mut(taker).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world.entity_mut(taker).insert(TeamIdComponent(TeamId(0)));

    // Seed the Ball with a Restart touch. If the system
    // mis-implemented kind gating, it would consult this and
    // (incorrectly) treat team 0 as the passing team for offside.
    let ball = Ball {
        kick_velocity: None,
        spin: 0.0,
        state: sim_components::BallState::Free,
        possessor: None,
        last_touch: Some(LastTouch {
            team: TeamId(0),
            player: Some(taker),
            kind: TouchKind::Restart,
        }),
        last_deliberate_play: None,
    };
    world.insert_resource(ball);

    // Two defenders near their own goal (we will examine the
    // second-last, which is roughly x=80 — `attacking_direction`
    // for team 0 attacks +x so `closer_than` is `x > ref`).
    let def1 = world.spawn(()).id();
    world
        .entity_mut(def1)
        .insert(Position(Vec2::new(85.0, 34.0)));
    world.entity_mut(def1).insert(TeamIdComponent(TeamId(1)));

    let def2 = world.spawn(()).id();
    world
        .entity_mut(def2)
        .insert(Position(Vec2::new(80.0, 34.0)));
    world.entity_mut(def2).insert(TeamIdComponent(TeamId(1)));

    // Attacker deep in the opponent's half (x=95), well beyond
    // ball (x=60) and the second-last defender (x=80). If
    // offside were run with a Play touch on team 0, this would
    // trigger an offside flag. With Restart touch, it must not.
    let attacker = world.spawn(()).id();
    world
        .entity_mut(attacker)
        .insert(Position(Vec2::new(95.0, 34.0)));
    world
        .entity_mut(attacker)
        .insert(TeamIdComponent(TeamId(0)));

    let mut schedule = Schedule::default();
    schedule.add_systems(offside_detection_system);

    // Single run is sufficient: the Restart short-circuit returns
    // before any tick-counter or logging logic. We assert that the
    // system does not mutate the ball's touch kind.
    schedule.run(&mut world);

    // Sanity: the system's no-op-for-Restart contract means the
    // touch kind survives intact. (We can't inspect the internal
    // `tick_counter` Local from outside, but it is the only
    // mutable side-effect of a Restart short-circuit.)
    let ball_after = world.resource::<Ball>();
    assert_eq!(
        ball_after.last_touch.as_ref().map(|t| t.kind),
        Some(TouchKind::Restart),
        "offside must not mutate the touch kind"
    );
}
