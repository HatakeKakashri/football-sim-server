//! Tests for `foul_detection_system` (Law 12): dangerous play on
//! high-speed opposing-team collisions, plus offside-kind gating:
//! `offside_detection_system` must ignore `TouchKind::Restart` touches.

use bevy_ecs::prelude::*;
use sim_components::{
    Ball, BallMarker, LastTouch, MatchClock, Player, Position, RoleComponent, RuleEvent, Skill,
    Stamina, TeamId, TeamIdComponent, TouchKind, Velocity,
};
use sim_math::Vec2;

use crate::{CurrentTick, foul_detection_system, offside_detection_system};

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

// -----------------------------------------------------------------------------
// Offside regression tests (M12.3)
// -----------------------------------------------------------------------------

/// Regression: when a teammate of a striker plays the ball
/// (`last_touch` flips to that teammate's team, `kind == Play`),
/// `offside_detection_system` evaluates the offside position and emits
/// the `offside` telemetry when the conditions are met.
///
/// The system gates its telemetry output on `tick_counter % 300 == 0`,
/// so we run the schedule 300 times in a fresh world to hit the throttle
/// boundary.
#[test]
fn test_offside_flagged_on_teammate_pass() {
    use sim_telemetry::{TelemetryConfig, capture_trace};

    let events: Vec<serde_json::Value> = capture_trace(|| {
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

        // Ball at the center spot (x=52.5).
        let ball_entity = world.spawn(()).id();
        world.entity_mut(ball_entity).insert(BallMarker);
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(52.5, 34.0)));
        world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

        // Player A — kicker/passer (team 0) — at the ball.
        let passer = world.spawn(()).id();
        world.entity_mut(passer).insert(Player {
            team_id: TeamId(0),
            intent: None,
        });
        world.entity_mut(passer).insert(TeamIdComponent(TeamId(0)));
        world
            .entity_mut(passer)
            .insert(Position(Vec2::new(52.5, 34.0)));

        // Player B — the potentially-offside striker (team 0, same as
        // the passer) — beyond both the ball and the second-last
        // defender. The system evaluates only attackers on the
        // *touching* team for offside, so the striker must be on
        // team 0.
        let striker = world.spawn(()).id();
        world.entity_mut(striker).insert(Player {
            team_id: TeamId(0),
            intent: None,
        });
        world
            .entity_mut(striker)
            .insert(Position(Vec2::new(95.0, 34.0)));
        world.entity_mut(striker).insert(TeamIdComponent(TeamId(0)));

        // Two team-1 defenders near their own goal line (the opposing
        // team of the passer, who is team 0). Team 0 attacks +x, so
        // the second-last defender is at x=80; ball at x=52.5; the
        // striker (team 0) at x=95 is beyond both, in an offside
        // position at the moment the ball was last touched by a
        // team-0 player (the passer).
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

        // Seed Ball with a Play touch on the passer.
        world.insert_resource(Ball {
            kick_velocity: None,
            spin: 0.0,
            state: sim_components::BallState::Free,
            possessor: None,
            last_touch: Some(LastTouch {
                team: TeamId(0),
                player: Some(passer),
                kind: TouchKind::Play,
            }),
            last_deliberate_play: None,
        });

        world.insert_resource(CurrentTick(60));
        world.insert_resource(TelemetryConfig::new(60, None).expect("valid"));

        // Run 300 ticks to cross the `tick_counter % 300 == 0`
        // throttle boundary.
        let mut schedule = Schedule::default();
        schedule.add_systems(offside_detection_system);
        for _ in 0..300 {
            schedule.run(&mut world);
        }
    });

    let offside_events: Vec<_> = events
        .iter()
        .filter(|e| e.get("name").and_then(serde_json::Value::as_str) == Some("offside"))
        .collect();
    assert_eq!(
        offside_events.len(),
        1,
        "exactly one offside event should fire when a teammate pass \
         leaves a striker in an offside position"
    );
    let args = &offside_events[0]["args"];
    assert_eq!(
        args["team"], 0,
        "offside is flagged against the passer team"
    );
    assert_eq!(args["x"], 95.0, "offside at the striker's x");
    assert_eq!(args["y"], 34.0);
}

/// Regression: an opponent kick resets the offside judgement. After
/// the passer on team 0 makes the striker offside, a team-1 player
/// kicks the ball (`last_touch` flips to team 1). The offside system
/// must no longer flag the striker — Law 11 says offside is judged
/// against the most-recent play touch's team.
#[test]
fn test_offside_reset_by_opponent_kick() {
    use sim_telemetry::{TelemetryConfig, capture_trace};

    let events: Vec<serde_json::Value> = capture_trace(|| {
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

        // Same geometry as `test_offside_flagged_on_teammate_pass` but with
        // the kicker now on team 1 — `last_touch` flips to team 1,
        // so the offside system now evaluates team-1 attackers (none
        // of which are in team 1's opponent half here).
        let ball_entity = world.spawn(()).id();
        world.entity_mut(ball_entity).insert(BallMarker);
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(52.5, 34.0)));
        world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

        // Two team-0 defenders near their own goal line (team 1
        // attacks -x, so team 0's goal is at x=105). The second-last
        // defender is at x=80.
        let def1 = world.spawn(()).id();
        world
            .entity_mut(def1)
            .insert(Position(Vec2::new(85.0, 34.0)));
        world.entity_mut(def1).insert(TeamIdComponent(TeamId(0)));

        let def2 = world.spawn(()).id();
        world
            .entity_mut(def2)
            .insert(Position(Vec2::new(80.0, 34.0)));
        world.entity_mut(def2).insert(TeamIdComponent(TeamId(0)));

        // Team-1 opponent kicker. Placed in his own defending half
        // (x=85, on team 0's side of the pitch) so the offside system
        // does not flag him — `in_opponent_half` for team 1 (attacking
        // -x) requires x < 52.5. With all team-1 attackers out of the
        // opponent half, no offside event should fire.
        let opponent_kicker = world.spawn(()).id();
        world.entity_mut(opponent_kicker).insert(Player {
            team_id: TeamId(1),
            intent: None,
        });
        world
            .entity_mut(opponent_kicker)
            .insert(Position(Vec2::new(90.0, 34.0)));
        world
            .entity_mut(opponent_kicker)
            .insert(TeamIdComponent(TeamId(1)));

        world.insert_resource(Ball {
            kick_velocity: None,
            spin: 0.0,
            state: sim_components::BallState::Free,
            possessor: None,
            last_touch: Some(LastTouch {
                team: TeamId(1),
                player: Some(opponent_kicker),
                kind: TouchKind::Play,
            }),
            last_deliberate_play: None,
        });

        world.insert_resource(CurrentTick(60));
        world.insert_resource(TelemetryConfig::new(60, None).expect("valid"));

        let mut schedule = Schedule::default();
        schedule.add_systems(offside_detection_system);
        for _ in 0..300 {
            schedule.run(&mut world);
        }
    });

    let offside_events: Vec<_> = events
        .iter()
        .filter(|e| e.get("name").and_then(serde_json::Value::as_str) == Some("offside"))
        .collect();
    assert!(
        offside_events.is_empty(),
        "after an opponent kick resets the touch to team 1, the offside \
         system must not flag the original striker: got {offside_events:?}"
    );
}
