//! Tests for `goal_detection_system` (Law 10): home goal, away goal,
//! dead-ball guard (no double scoring), touch-clear-on-goal, and
//! restart-kind registration by `restart_system`.

use bevy_ecs::prelude::*;
use sim_components::{
    BallMarker, BallState, LastTouch, MatchClock, Player, Position, RuleEvent, TouchKind, Velocity,
};
use sim_math::Vec2;

use crate::tests::ball_fixture;
use crate::{CurrentTick, PendingRestart, goal_detection_system, restart_system};

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

    // Toucher player on team 1 (so the goal is credited to the
    // scorer = home by ball position, with last_touch_team = 1
    // captured into the event payload).
    let toucher = world.spawn(()).id();
    world.entity_mut(toucher).insert(Player {
        team_id: sim_components::TeamId(1),
        intent: None,
    });
    world
        .entity_mut(toucher)
        .insert(sim_components::TeamIdComponent(sim_components::TeamId(1)));

    // Create ball entity in home goal (x = 105.5, y = 34)
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    let mut b = ball_fixture(Vec2::new(105.5, 34.0), BallState::Free);
    b.last_touch = Some(LastTouch {
        entity: toucher,
        kind: TouchKind::Play,
    });
    world.insert_resource(b);
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
    assert!(
        ball.last_touch.is_none(),
        "goal must clear last_touch after capturing it"
    );

    // Verify KickoffRestart was scheduled
    let pending = world.get_resource::<PendingRestart>();
    assert!(pending.is_some());

    // The Goal event should carry last_touch_team = team 1 (the
    // away toucher), while scorer_team remains home (0) by ball
    // position.
    let event = world.entity(ball_entity).get::<RuleEvent>();
    if let Some(RuleEvent::Goal {
        scorer_team,
        last_touch_team,
        ..
    }) = event
    {
        assert_eq!(*scorer_team, sim_components::TeamId(0));
        assert_eq!(*last_touch_team, Some(sim_components::TeamId(1)));
    } else {
        panic!("expected RuleEvent::Goal, got {event:?}");
    }
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

    // Toucher player on team 0.
    let toucher = world.spawn(()).id();
    world.entity_mut(toucher).insert(Player {
        team_id: sim_components::TeamId(0),
        intent: None,
    });
    world
        .entity_mut(toucher)
        .insert(sim_components::TeamIdComponent(sim_components::TeamId(0)));

    // Create ball entity in away goal (x = -0.5, y = 34)
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    let mut b = ball_fixture(Vec2::new(-0.5, 34.0), BallState::Free);
    b.last_touch = Some(LastTouch {
        entity: toucher,
        kind: TouchKind::Play,
    });
    world.insert_resource(b);
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

    let event = world.entity(ball_entity).get::<RuleEvent>();
    if let Some(RuleEvent::Goal {
        scorer_team,
        last_touch_team,
        ..
    }) = event
    {
        assert_eq!(*scorer_team, sim_components::TeamId(1));
        assert_eq!(*last_touch_team, Some(sim_components::TeamId(0)));
    } else {
        panic!("expected RuleEvent::Goal, got {event:?}");
    }
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

/// Delayed kickoff restart (after goal) registers the ball placement
/// as a `TouchKind::Restart` touch.
#[test]
fn test_kickoff_restart_registers_restart_touch() {
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
    world.entity_mut(match_entity).insert(MatchClock {
        elapsed_ticks: 30 * 60,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });

    // Set up pending kickoff restart.
    world.insert_resource(PendingRestart {
        event: Some(RuleEvent::KickoffRestart),
        restart_tick: 60,
        // Home scored the goal → away conceded → kickoff taker
        // must be away (team 1).
        scorer_team: Some(sim_components::TeamId(0)),
    });

    // Ball entity after the goal — Dead state, position is
    // irrelevant because the system will move it.
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(ball_fixture(Vec2::zero(), BallState::Dead));
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(105.5, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

    // An away-team player — the conceded-team kickoff taker
    // (Law 10: team that conceded takes the kickoff).
    let kicker = world.spawn(()).id();
    world.entity_mut(kicker).insert(Player {
        team_id: sim_components::TeamId(1),
        intent: None,
    });
    world
        .entity_mut(kicker)
        .insert(sim_components::TeamIdComponent(sim_components::TeamId(1)));

    let mut schedule = Schedule::default();
    schedule.add_systems(restart_system);
    world.insert_resource(CurrentTick(60));
    schedule.run(&mut world);

    let ball = world.resource::<sim_components::Ball>();
    assert_eq!(ball.state, BallState::Free);
    assert_eq!(
        ball.last_touch,
        Some(LastTouch {
            entity: kicker,
            kind: TouchKind::Restart,
        }),
        "delayed kickoff restart must register the conceded-team \
         kickoff taker (Law 10), not always the home team"
    );
    assert!(
        world.get_resource::<PendingRestart>().is_none(),
        "PendingRestart is cleared after consumption"
    );
}

/// Verify that the goal scorer feeds into the post-goal kickoff:
/// home scores → away concedes → kickoff taker must come from
/// team 1.
#[test]
fn test_goal_scorer_threads_into_post_goal_kickoff_taker() {
    let mut world = World::new();

    world.insert_resource(sim_components::Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: sim_components::MatchState::InPlay,
        seed: 12345,
    });

    // Ball in the home goal area — score for home.
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.insert_resource(ball_fixture(Vec2::new(105.5, 34.0), BallState::Free));
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(105.5, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

    // No toucher player required for the scorer-team assertion
    // (ball-position drives scorer_team, not last_touch).

    let mut schedule = Schedule::default();
    schedule.add_systems(goal_detection_system);
    world.insert_resource(CurrentTick(50));
    schedule.run(&mut world);

    // Now `PendingRestart` carries scorer_team = home (0).
    let pending = world
        .get_resource::<PendingRestart>()
        .expect("PendingRestart must be set after a goal");
    assert_eq!(
        pending.scorer_team,
        Some(sim_components::TeamId(0)),
        "goal must thread scorer_team into PendingRestart"
    );
}