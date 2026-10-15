//! Tests for `foul_detection_system` (Law 12): dangerous play on
//! high-speed opposing-team collisions.
//!
//! Plus tests for `stoppage_tracking_system` (PRD §8.4): dead-ball ticks
//! accrue one queue entry each while the match is in play.

use bevy_ecs::prelude::*;
use sim_components::{
    Ball, BallState, Match, MatchClock, MatchState, Player, Position, Referee, RoleComponent,
    RuleEvent, Skill, Stamina, TeamId, TeamIdComponent, Velocity,
};
use sim_math::Vec2;

use crate::CurrentTick;
use crate::foul_detection_system;
use crate::referee::{DEAD_BALL_STOPPAGE_REASON, stoppage_tracking_system};

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

/// Minimal world for `stoppage_tracking_system`: a `Ball` resource in the
/// given state, a `Match` resource in the given state, one `Referee`
/// entity, and the tick counter.
fn stoppage_test_world(ball_state: BallState, match_state: MatchState) -> (World, Entity) {
    let mut world = World::new();
    world.insert_resource(Ball {
        spin: 0.0,
        state: ball_state,
        possessor: None,
        last_touch: None,
        touch_history: [None, None],
        kick_velocity: None,
    });
    world.insert_resource(Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: match_state,
        seed: 7,
    });
    world.insert_resource(CurrentTick(42));
    let referee = world
        .spawn(Referee {
            stoppage_events: Vec::new(),
            cards: Vec::new(),
        })
        .id();
    (world, referee)
}

fn run_stoppage_tracking(world: &mut World) {
    let mut schedule = Schedule::default();
    schedule.add_systems(stoppage_tracking_system);
    schedule.run(world);
}

/// A `Dead` ball while `InPlay` accrues exactly one stoppage tick.
#[test]
fn test_stoppage_tracking_counts_dead_ball_tick_in_play() {
    let (mut world, referee) = stoppage_test_world(BallState::Dead, MatchState::InPlay);
    run_stoppage_tracking(&mut world);
    let queue = &world
        .entity(referee)
        .get::<Referee>()
        .expect("invariant: referee entity exists")
        .stoppage_events;
    assert_eq!(queue.len(), 1, "one dead-ball tick accrues one entry");
    assert_eq!(queue[0].tick, 42);
    assert_eq!(queue[0].reason, DEAD_BALL_STOPPAGE_REASON);
}

/// Consecutive dead-ball ticks accumulate (the goal-celebration delay
/// accrues tick-by-tick through the 60-tick kickoff wait).
#[test]
fn test_stoppage_tracking_accumulates_across_ticks() {
    let (mut world, referee) = stoppage_test_world(BallState::Dead, MatchState::InPlay);
    for _ in 0..60 {
        run_stoppage_tracking(&mut world);
    }
    let queue = &world
        .entity(referee)
        .get::<Referee>()
        .expect("invariant: referee entity exists")
        .stoppage_events;
    assert_eq!(queue.len(), 60, "60 dead ticks accrue 60 entries");
}

/// A live ball accrues nothing.
#[test]
fn test_stoppage_tracking_ignores_live_ball() {
    let (mut world, referee) = stoppage_test_world(BallState::Free, MatchState::InPlay);
    run_stoppage_tracking(&mut world);
    let queue = &world
        .entity(referee)
        .get::<Referee>()
        .expect("invariant: referee entity exists")
        .stoppage_events;
    assert!(queue.is_empty(), "no stoppage while the ball is live");
}

/// A `Dead` ball outside `InPlay` (half-time break, kickoff) accrues
/// nothing — stoppage is per-half.
#[test]
fn test_stoppage_tracking_ignores_dead_ball_outside_play() {
    for state in [MatchState::HalfTime, MatchState::Kickoff, MatchState::FullTime] {
        let (mut world, referee) = stoppage_test_world(BallState::Dead, state);
        run_stoppage_tracking(&mut world);
        let queue = &world
            .entity(referee)
            .get::<Referee>()
            .expect("invariant: referee entity exists")
            .stoppage_events;
        assert!(
            queue.is_empty(),
            "no stoppage accrual while {state:?} even with a dead ball"
        );
    }
}
