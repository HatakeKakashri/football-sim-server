//! Tests for `goal_detection_system` (Law 10): home goal, away goal,
//! dead-ball guard (no double scoring), touch preservation on goal
//! (M8: the record stays on the ball), and restart-kind registration
//! by `restart_system`.

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleBuildSettings;
use sim_components::{
    Ball, BallMarker, BallState, Formation, LastTouch, MatchClock, Mentality, PendingBallTouches,
    Player, Position, RuleEvent, Tactic, Team, TeamId, TeamIdComponent, TouchKind, Velocity,
};
use sim_math::Vec2;

use crate::tests::ball_fixture;
use crate::{
    CurrentTick, GoalForEmit, PendingOutOfBounds, PendingRestart, goal_detection_system,
    out_of_bounds_system, restart_system, touch::apply_ball_touched_system,
};

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
    // scorer = home by ball position, with last_touch.team = 1
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
    b.last_touch = Some(LastTouch::by_player(sim_components::TeamId(1), toucher, TouchKind::Play));
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

    // Verify KickoffRestart was scheduled
    let pending = world.get_resource::<PendingRestart>();
    assert!(pending.is_some());

    // The Goal event should carry last_touch = the away toucher's
    // full record (team 1), while scorer_team remains home (0) by
    // ball position.
    let event = world.entity(ball_entity).get::<RuleEvent>();
    if let Some(RuleEvent::Goal {
        scorer_team,
        last_touch,
        ..
    }) = event
    {
        assert_eq!(*scorer_team, sim_components::TeamId(0));
        assert_eq!(
            *last_touch,
            Some(LastTouch::by_player(sim_components::TeamId(1), toucher, TouchKind::Play))
        );
    } else {
        panic!("expected RuleEvent::Goal, got {event:?}");
    }
}

/// Ticket 10: `goal_detection_system` stashes the resolved goal on
/// `GoalForEmit` so the `sim-core` emitter can write the `goal`
/// event (with scorer / assister attribution) in the same tick.
#[test]
fn test_goal_detection_stashes_emit_slot() {
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

    // Team-1 toucher kicks the ball into the home goal (x = 105.5):
    // credited to home by position — an own-goal shape that pins the
    // raw touch window the emitter later attributes.
    let toucher = world.spawn(()).id();
    world.entity_mut(toucher).insert(Player {
        team_id: sim_components::TeamId(1),
        intent: None,
    });
    world
        .entity_mut(toucher)
        .insert(sim_components::TeamIdComponent(sim_components::TeamId(1)));

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    let mut b = ball_fixture(Vec2::new(105.5, 34.0), BallState::Free);
    b.last_touch = Some(
        LastTouch::by_player(sim_components::TeamId(1), toucher, TouchKind::Play)
            .with_tick(90),
    );
    world.insert_resource(b);
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(105.5, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

    world.insert_resource(GoalForEmit::default());
    world.insert_resource(CurrentTick(100));
    let mut schedule = Schedule::default();
    schedule.add_systems(goal_detection_system);
    schedule.run(&mut world);

    let slot = world.resource::<GoalForEmit>();
    let resolved = slot.0.expect("goal must stash emit data");
    assert_eq!(
        resolved.scorer_team,
        sim_components::TeamId(0),
        "home credited by goal position"
    );
    assert_eq!(resolved.score, (1, 0), "running score after the goal");
    assert_eq!(
        resolved.last_touch,
        Some(
            LastTouch::by_player(sim_components::TeamId(1), toucher, TouchKind::Play)
                .with_tick(90)
        ),
        "emitter reads the scorer candidate from the stashed window"
    );
    assert_eq!(resolved.prev_touch, None, "no older touch in this fixture");
    assert_eq!(resolved.goal_tick, 100);
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
    b.last_touch = Some(LastTouch::by_player(sim_components::TeamId(0), toucher, TouchKind::Play));
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
        last_touch,
        ..
    }) = event
    {
        assert_eq!(*scorer_team, sim_components::TeamId(1));
        assert_eq!(
            *last_touch,
            Some(LastTouch::by_player(sim_components::TeamId(0), toucher, TouchKind::Play))
        );
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
    world.insert_resource(sim_components::PendingBallTouches::default());
    world.insert_resource(PendingOutOfBounds::default());
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
    schedule.add_systems((restart_system, apply_ball_touched_system).chain());
    world.insert_resource(CurrentTick(60));
    schedule.run(&mut world);

    let ball = world.resource::<sim_components::Ball>();
    assert_eq!(ball.state, BallState::Free);
    assert_eq!(
        ball.last_touch,
        Some(
            LastTouch::by_player(sim_components::TeamId(1), kicker, TouchKind::Restart)
                .with_tick(60)
        ),
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
    let mut b = ball_fixture(Vec2::new(105.5, 34.0), BallState::Free);
    // Seed a touch so the pre-touch guard (spec §E) is satisfied:
    // goal detection only fires after a registered touch. The scorer
    // threads off ball position, so the seeded record's identity is
    // not under test here.
    b.last_touch = Some(LastTouch::by_team(sim_components::TeamId(0), TouchKind::Play));
    world.insert_resource(b);
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

// -----------------------------------------------------------------------------
// End-to-end restart scenarios (M12.2)
// -----------------------------------------------------------------------------

/// Missed shot that crosses the goal line within the goal area → a
/// goal-kick is awarded to the defending team. The ball is placed at
/// the penalty spot of the goal it crossed (Law 9 / Law 16). Setup:
/// ball settles at (104.9, 34.0) inside the home goal area, low
/// velocity. Schedule `out_of_bounds_system` + `restart_system`.
#[test]
fn test_missed_shot_gives_goal_kick() {
    let mut world = World::new();

    let toucher = world.spawn(()).id();
    world.entity_mut(toucher).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world.entity_mut(toucher).insert(TeamIdComponent(TeamId(0)));

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(104.9, 34.0)));
    world
        .entity_mut(ball_entity)
        .insert(Velocity(Vec2::new(0.1, 0.0)));
    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touch: Some(LastTouch::by_player(TeamId(0), toucher, TouchKind::Play)),
        touch_history: [None, None],
    });
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(PendingOutOfBounds::default());
    world.insert_resource(CurrentTick(50));

    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            apply_ball_touched_system,
            out_of_bounds_system,
            restart_system,
        )
            .chain(),
    );
    schedule.run(&mut world);

    // Position confirms OOB fired (GoalKick spot is at x=94, y=34 for the
    // home goal). `restart_system` consumes the `PendingOutOfBounds`
    // signal after processing it — we verify the OOB classification
    // by position placement here; the signal was written, then cleared.
    let pos = world.entity(ball_entity).get::<Position>().unwrap();
    assert_eq!(
        pos.0,
        Vec2::new(94.0, 34.0),
        "goal-kick spot for home goal (Law 9)"
    );
    assert_eq!(
        world.resource::<PendingOutOfBounds>().get(),
        None,
        "the OOB signal must be consumed by restart_system"
    );

    // Ball placed at the home penalty spot.
    let pos = world.entity(ball_entity).get::<Position>().unwrap();
    assert_eq!(pos.0, Vec2::new(94.0, 34.0));
}

/// Shot deflected out over the goal line **outside** the goal area →
/// a corner is awarded. Ball settles at (104.9, 10.0), low velocity.
#[test]
fn test_deflected_shot_gives_corner() {
    let mut world = World::new();

    let toucher = world.spawn(()).id();
    world.entity_mut(toucher).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world.entity_mut(toucher).insert(TeamIdComponent(TeamId(0)));

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(104.9, 10.0)));
    world
        .entity_mut(ball_entity)
        .insert(Velocity(Vec2::new(0.1, 0.0)));
    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touch: Some(LastTouch::by_player(TeamId(0), toucher, TouchKind::Play)),
        touch_history: [None, None],
    });
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(PendingOutOfBounds::default());
    world.insert_resource(CurrentTick(60));

    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            apply_ball_touched_system,
            out_of_bounds_system,
            restart_system,
        )
            .chain(),
    );
    schedule.run(&mut world);

    // Position confirms OOB fired (corner spot at x=104, y=1 for the
    // bottom-right corner). Signal is consumed by restart_system.
    let pos = world.entity(ball_entity).get::<Position>().unwrap();
    assert_eq!(
        pos.0,
        Vec2::new(104.0, 1.0),
        "corner spot for bottom-right (Law 9)"
    );
    assert_eq!(
        world.resource::<PendingOutOfBounds>().get(),
        None,
        "the OOB signal must be consumed by restart_system"
    );

    // Ball placed at the corner arc on the bottom-right (y=10 is in
    // the bottom half of the pitch).
    let pos = world.entity(ball_entity).get::<Position>().unwrap();
    assert_eq!(pos.0, Vec2::new(104.0, 1.0));
}

/// Ball crosses the touchline with the last toucher on team 0. The
/// `PendingOutOfBounds` payload records the toucher's team (0);
/// after restart, the throw-in is awarded to team 1 (Law 9) and the
/// new Restart touch is queued in `PendingBallTouches`.
#[test]
fn test_throw_in_to_opposite_team() {
    let mut world = World::new();

    let toucher = world.spawn(()).id();
    world.entity_mut(toucher).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world.entity_mut(toucher).insert(TeamIdComponent(TeamId(0)));

    // Team 1 must have at least one player on the roster so
    // `first_player_on_team` resolves to a real entity rather than
    // `Entity::PLACEHOLDER` (which would yield `player: None` in the
    // Restart touch).
    let taker = world.spawn(()).id();
    world.entity_mut(taker).insert(Player {
        team_id: TeamId(1),
        intent: None,
    });
    world.entity_mut(taker).insert(TeamIdComponent(TeamId(1)));

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world
        .entity_mut(ball_entity)
        .insert(Position(Vec2::new(40.0, 0.0))); // touchline
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));
    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touch: Some(LastTouch::by_player(TeamId(0), toucher, TouchKind::Play)),
        touch_history: [None, None],
    });
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(PendingOutOfBounds::default());
    world.insert_resource(CurrentTick(70));

    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            apply_ball_touched_system,
            out_of_bounds_system,
            restart_system,
        )
            .chain(),
    );
    schedule.run(&mut world);

    // OOB signal payload must carry the original toucher. We verify
    // this on a fresh schedule that excludes `restart_system` (which
    // takes the signal out of the resource after consuming it).
    let mut world2 = World::new();
    world2.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touch: Some(LastTouch::by_player(TeamId(0), toucher, TouchKind::Play)),
        touch_history: [None, None],
    });
    world2.insert_resource(PendingBallTouches::default());
    world2.insert_resource(PendingOutOfBounds::default());
    let ball2 = world2.spawn(()).id();
    world2.entity_mut(ball2).insert(BallMarker);
    world2
        .entity_mut(ball2)
        .insert(Position(Vec2::new(40.0, 0.0)));
    world2.entity_mut(ball2).insert(Velocity(Vec2::zero()));
    world2.insert_resource(CurrentTick(70));

    let mut schedule2 = Schedule::default();
    schedule2.add_systems((apply_ball_touched_system, out_of_bounds_system).chain());
    schedule2.run(&mut world2);

    let signal = world2.resource::<PendingOutOfBounds>().get().copied();
    assert_eq!(
        signal.map(|event| event.last_touch),
        Some(Some(LastTouch::by_player(TeamId(0), toucher, TouchKind::Play))),
        "OutOfBounds payload must preserve the original toucher"
    );

    // Restart push attributes the throw-in to team 1 (Law 9: opposite of
    // last toucher). The push lands in the buffer this tick; the next
    // tick's consumer drains it.
    let pending = world.resource::<PendingBallTouches>();
    assert_eq!(
        pending.0,
        vec![
            LastTouch::by_player(TeamId(1), taker, TouchKind::Restart).with_tick(70)
        ],
        "throw-in is awarded to the team opposite of the last toucher"
    );
}

// -----------------------------------------------------------------------------
// OOB signal is visible to the restarter in the same run (issue 04)
// -----------------------------------------------------------------------------

/// `out_of_bounds_system` hands the restart signal to `restart_system`
/// through the `PendingOutOfBounds` resource, so the restarter sees it
/// even when **no `apply_deferred` sync point** separates the two
/// systems. Auto sync points are switched off here to prove the signal
/// no longer depends on deferred `Commands` being flushed.
#[test]
fn test_oob_restart_reads_signal_same_run_without_sync_point() {
    let mut world = World::new();

    let toucher = world.spawn((Player { team_id: TeamId(0), intent: None }, TeamIdComponent(TeamId(0)))).id();
    let taker = world.spawn((Player { team_id: TeamId(1), intent: None }, TeamIdComponent(TeamId(1)))).id();

    // Settled on the bottom touchline at x = 20 → throw-in.
    let ball_entity = world
        .spawn((BallMarker, Position(Vec2::new(20.0, 0.0)), Velocity(Vec2::zero())))
        .id();
    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touch: Some(LastTouch::by_player(TeamId(0), toucher, TouchKind::Play)),
        touch_history: [None, None],
    });
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(PendingOutOfBounds::default());
    world.insert_resource(CurrentTick(50));

    let mut schedule = Schedule::default();
    schedule.set_build_settings(ScheduleBuildSettings {
        auto_insert_apply_deferred: false,
        ..Default::default()
    });
    schedule.add_systems((out_of_bounds_system, restart_system).chain());
    schedule.run(&mut world);

    assert_eq!(
        world.resource::<Ball>().state,
        BallState::Free,
        "restart_system must resolve the OOB restart in the same run"
    );
    assert_eq!(
        world.entity(ball_entity).get::<Position>().map(|p| p.0),
        Some(Vec2::new(20.0, 0.0)),
        "throw-in is placed on the touchline where the ball went out"
    );
    assert_eq!(
        world.resource::<PendingBallTouches>().0,
        vec![LastTouch::by_player(TeamId(1), taker, TouchKind::Restart).with_tick(50)],
        "throw-in is taken by the team opposite the last toucher"
    );
    assert_eq!(
        world.resource::<PendingOutOfBounds>().get(),
        None,
        "the signal is consumed by the restart"
    );
}

// -----------------------------------------------------------------------------
// Scorer attribution follows the per-half attack direction (issue 02)
// -----------------------------------------------------------------------------

/// Spawn both teams with `home_direction` for the home team and the
/// opposite for the away team.
fn spawn_teams(world: &mut World, home_direction: i8) {
    for (id, direction) in [(0, home_direction), (1, -home_direction)] {
        world.spawn(Team::with_attack_direction(
            TeamId(id),
            format!("team-{id}"),
            Formation::FourFourTwo,
            Mentality::Balance,
            Tactic::Possession,
            Vec::new(),
            Vec::new(),
            direction,
        ));
    }
}

/// Run `goal_detection_system` once with the ball at `(ball_x, 34)` and
/// return the resulting score plus the scorer carried by the goal event.
fn score_for_ball_at(ball_x: f32, home_direction: i8) -> ((u8, u8), TeamId) {
    let mut world = World::new();
    world.insert_resource(sim_components::Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: sim_components::MatchState::InPlay,
        seed: 12345,
    });
    spawn_teams(&mut world, home_direction);

    let ball_entity = world.spawn((BallMarker, Position(Vec2::new(ball_x, 34.0)))).id();
    let mut ball = ball_fixture(Vec2::new(ball_x, 34.0), BallState::Free);
    ball.last_touch = Some(LastTouch::by_team(TeamId(0), TouchKind::Play));
    world.insert_resource(ball);
    world.insert_resource(CurrentTick(100));

    let mut schedule = Schedule::default();
    schedule.add_systems(goal_detection_system);
    schedule.run(&mut world);

    let score = world.resource::<sim_components::Match>().score;
    let Some(RuleEvent::Goal { scorer_team, .. }) = world.entity(ball_entity).get::<RuleEvent>()
    else {
        panic!("expected a RuleEvent::Goal for ball at x = {ball_x}");
    };
    (score, *scorer_team)
}

#[test]
fn test_first_half_goals_follow_attack_direction() {
    // Home attacks +x in the first half.
    assert_eq!(score_for_ball_at(105.5, 1), ((1, 0), TeamId(0)));
    assert_eq!(score_for_ball_at(-0.5, 1), ((0, 1), TeamId(1)));
}

#[test]
fn test_second_half_goals_follow_swapped_attack_direction() {
    // After the half-time swap home attacks -x: a ball in the x = 0
    // goal is a home goal and a ball in the x = 105 goal is an away goal.
    assert_eq!(score_for_ball_at(-0.5, -1), ((1, 0), TeamId(0)));
    assert_eq!(score_for_ball_at(105.5, -1), ((0, 1), TeamId(1)));
}
