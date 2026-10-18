//! Integration tests for the event-emit pipeline: every event
//! `seq` over a real `Simulation` run is gapless; `pass` events
//! carry correct `kick_tick` / `success` / `to`; `possession_change`
//! fires on team change; ticket-10 events (`goal` with scorer /
//! assister / running score, `shot` / `save`, `kickoff`,
//! `half_time`, `full_time`) fire on the right transitions. Short
//! match windows keep the tests fast (full-match coverage is in the
//! `#[ignore]`d test at the bottom).

#![allow(
    clippy::float_cmp,
    reason = "tests compare fresh values against the constants they were built from"
)]

use crate::{EventLog, MatchEvent, PossessionChangeCause, Simulation, check_gapless_seq};
use sim_components::TeamId;

fn pos(x: f32, y: f32) -> sim_math::Vec2 {
    sim_math::Vec2::new(x, y)
}

#[test]
fn events_emitted_in_short_window_have_gapless_seq() {
    let mut sim = Simulation::new(12345);
    for _ in 0..3_000 {
        sim.tick();
    }
    let log = sim
        .world()
        .get_resource::<EventLog>()
        .cloned()
        .expect("EventLog is registered");
    if !log.events.is_empty() {
        assert!(
            check_gapless_seq(&log).is_none(),
            "seq is not gapless: {} events",
            log.events.len()
        );
    }
}

#[test]
fn pass_event_has_correct_kick_tick_and_success() {
    // Build a pass event directly to pin the payload shape.
    let mut world = crate::tests::installed_world();
    world.insert_resource(sim_components::MatchClock {
        elapsed_ticks: 1_000,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(EventLog::new());
    let clock = world
        .resource::<sim_components::MatchClock>()
        .clone();
    let event = crate::build_pass_event(
        0,
        1_000,
        &clock,
        TeamId(0),
        sim_components::PlayerId::from_bits(11),
        Some(sim_components::PlayerId::from_bits(22)),
        true,
        900,
        pos(50.0, 34.0),
        pos(60.0, 34.0),
    );
    let mut log = world.resource_mut::<EventLog>();
    log.append(event);
    let log = world.resource::<EventLog>();
    let MatchEvent::Pass { payload, envelope } = &log.events[0] else {
        panic!("expected pass event");
    };
    assert_eq!(payload.kick_tick, 900, "kick_tick must be the kick tick");
    assert_eq!(envelope.tick, 1_000, "envelope tick must be the resolution tick");
    assert!(payload.success, "pass to a teammate is success");
    assert_eq!(payload.to, Some(sim_components::PlayerId::from_bits(22)));
    // team 0 kicker; the team field reflects the kicker's team.
    assert_eq!(envelope.team, Some(TeamId(0)));
}

#[test]
fn possession_change_event_has_cause_and_teams() {
    let mut world = crate::tests::installed_world();
    world.insert_resource(sim_components::MatchClock {
        elapsed_ticks: 2_000,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(EventLog::new());
    let clock = world
        .resource::<sim_components::MatchClock>()
        .clone();
    let event = crate::build_possession_change_event(
        0,
        2_000,
        &clock,
        Some(TeamId(0)),
        TeamId(1),
        PossessionChangeCause::OutOfBounds,
        None,
    );
    let mut log = world.resource_mut::<EventLog>();
    log.append(event);
    let log = world.resource::<EventLog>();
    let MatchEvent::PossessionChange { payload, .. } = &log.events[0] else {
        panic!("expected possession_change event");
    };
    assert_eq!(payload.from_team, TeamId(0));
    assert_eq!(payload.to_team, TeamId(1));
    assert_eq!(payload.cause, PossessionChangeCause::OutOfBounds);
}

/// Ticket 10: the first ticks settle `PreMatch → Kickoff → InPlay`
/// and emit exactly one `kickoff` with `reason = match_start`, a
/// taker, and both attack directions (home `+1`, away `-1`).
#[test]
fn kickoff_emitted_on_first_tick_with_match_start_reason() {
    let mut sim = Simulation::new(12345);
    for _ in 0..3 {
        sim.tick();
    }
    let log = sim
        .world()
        .get_resource::<EventLog>()
        .cloned()
        .expect("EventLog is registered");
    let kickoffs: Vec<_> = log
        .events
        .iter()
        .filter(|e| matches!(e, MatchEvent::Kickoff { .. }))
        .collect();
    assert_eq!(kickoffs.len(), 1, "exactly one kickoff on match start");
    let MatchEvent::Kickoff { envelope, payload } = kickoffs[0] else {
        panic!("expected kickoff event");
    };
    assert_eq!(envelope.team, Some(TeamId(0)), "home kicks off first");
    assert!(
        payload.taker.is_some(),
        "kickoff names a taker in a live match"
    );
    assert_eq!(
        payload.reason,
        sim_components::KickoffReason::MatchStart
    );
    assert_eq!(
        (payload.attack_home, payload.attack_away),
        (Some(1), Some(-1)),
        "half-start kickoff carries both attack directions"
    );
}

/// Ticket 10 / A7: folding the `goal` events per team over a short
/// window equals `Match.score` at every point (here: at the end).
#[test]
fn goal_running_score_equals_fold_over_short_window() {
    let mut sim = Simulation::new(12345);
    for _ in 0..3_000 {
        sim.tick();
    }
    let (score, home_goals, away_goals) = {
        let world = sim.world();
        let score = world
            .get_resource::<sim_components::Match>()
            .map_or((0, 0), |m| m.score);
        let log = world
            .get_resource::<EventLog>()
            .expect("EventLog is registered");
        let mut home = 0_u8;
        let mut away = 0_u8;
        for event in &log.events {
            if let MatchEvent::Goal { envelope, payload } = event {
                if envelope.team == Some(TeamId(0)) {
                    home += 1;
                } else {
                    away += 1;
                }
                // Running score in the event equals the count of goals
                // per team at emission time.
                assert_eq!(
                    payload.score,
                    (home, away),
                    "goal event running score must equal the per-team goal count"
                );
            }
        }
        (score, home, away)
    };
    assert_eq!(
        (home_goals, away_goals),
        score,
        "final score must equal the count of goal events per team (A7)"
    );
}

/// Ticket 10: `emit_goal_events` attributes scorer / assister from the
/// stashed touch window and emits the `possession_change` with cause
/// `goal` to the conceding team.
#[test]
fn goal_emitter_attributes_scorer_and_possession_change() {
    use bevy_ecs::prelude::Schedule;
    use sim_components::{LastTouch, MatchClock, TouchKind};
    use sim_rules::{GoalForEmit, GoalResolved};

    let mut world = crate::tests::installed_world();
    world.insert_resource(MatchClock {
        elapsed_ticks: 4_500,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(EventLog::new());
    let scorer_e = world.spawn(()).id();
    let assister_e = world.spawn(()).id();
    let scorer = LastTouch::by_player(TeamId(0), scorer_e, TouchKind::Play).with_tick(4_500);
    let assist = LastTouch::by_player(TeamId(0), assister_e, TouchKind::Play).with_tick(4_400);
    world.insert_resource(GoalForEmit(Some(GoalResolved {
        scorer_team: TeamId(0),
        score: (1, 0),
        last_touch: Some(scorer),
        prev_touch: Some(assist),
        older_touch: None,
        goal_tick: 4_500,
        ball_pos: sim_math::Vec2::new(105.0, 34.0),
    })));

    let mut schedule = Schedule::default();
    schedule.add_systems(crate::simulation::event_emit::emit_goal_events);
    schedule.run(&mut world);
    // Drain like the end-of-chain pass resolver does.
    world.resource_mut::<GoalForEmit>().0 = None;

    let log = world.resource::<EventLog>();
    assert_eq!(log.events.len(), 2, "goal + possession_change(goal)");
    let MatchEvent::Goal { envelope, payload } = &log.events[0] else {
        panic!("expected goal event first");
    };
    assert_eq!(envelope.team, Some(TeamId(0)));
    assert_eq!(payload.scorer, scorer.player_id);
    assert_eq!(payload.assister, assist.player_id);
    assert!(!payload.own_goal);
    assert_eq!(payload.score, (1, 0));
    let MatchEvent::PossessionChange { payload, .. } = &log.events[1] else {
        panic!("expected possession_change(goal) second");
    };
    assert_eq!(payload.cause, PossessionChangeCause::Goal);
    assert_eq!(payload.to_team, TeamId(1), "conceding team restarts");
    assert_eq!(log.possession_team, Some(TeamId(1)));
}

/// Ticket 10: a pending shot gathered by the opposing goalkeeper
/// resolves to `saved` plus a companion `save` event; a slow ball is
/// caught.
#[test]
fn shot_resolves_to_saved_with_catch_for_slow_ball() {
    use bevy_ecs::prelude::Schedule;
    use sim_components::{
        Ball, BallMarker, BallState, LastTouch, MatchClock, Player, Position, Role, RoleComponent,
        TeamIdComponent, TouchKind, Velocity,
    };
    use sim_rules::{PendingShot, PendingShotState};

    let mut world = crate::tests::installed_world();
    world.insert_resource(MatchClock {
        elapsed_ticks: 5_000,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(EventLog::new());
    let shooter = world.spawn(()).id();
    let keeper = world
        .spawn((
            Player {
                team_id: TeamId(1),
                intent: None,
            },
            TeamIdComponent(TeamId(1)),
            RoleComponent(Role::Goalkeeper),
        ))
        .id();
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world
        .entity_mut(ball_entity)
        .insert(Position(sim_math::Vec2::new(100.0, 34.0)));
    world
        .entity_mut(ball_entity)
        .insert(Velocity(sim_math::Vec2::zero()));
    world.insert_resource(Ball {
        spin: 0.0,
        state: BallState::Possessed,
        possessor: Some(keeper),
        last_touch: Some(LastTouch::by_player(
            TeamId(0),
            shooter,
            TouchKind::Play,
        )),
        touch_history: [None, None],
        kick_velocity: None,
    });
    world.insert_resource(PendingShot(Some(PendingShotState {
        shooter,
        shooter_team: TeamId(0),
        kick_tick: 4_900,
        kick_pos: sim_math::Vec2::new(80.0, 34.0),
    })));

    let mut schedule = Schedule::default();
    schedule.add_systems(crate::simulation::event_emit::resolve_shot_events);
    schedule.run(&mut world);

    let log = world.resource::<EventLog>();
    assert_eq!(log.events.len(), 2, "shot + save");
    let MatchEvent::Shot { payload, .. } = &log.events[0] else {
        panic!("expected shot event first");
    };
    assert!(payload.on_target);
    assert_eq!(payload.outcome, sim_components::ShotOutcome::Saved);
    let MatchEvent::Save { envelope, payload } = &log.events[1] else {
        panic!("expected save event second");
    };
    assert_eq!(envelope.team, Some(TeamId(1)), "keeper team credited");
    assert_eq!(
        payload.keeper,
        sim_components::PlayerId::from_entity(keeper)
    );
    assert_eq!(
        payload.shooter,
        sim_components::PlayerId::from_entity(shooter)
    );
    assert_eq!(
        payload.kind,
        sim_components::SaveKind::Catch,
        "stationary ball is caught, not parried"
    );
}

/// Ticket 23: a pending cross resolves to a `cross` event and, when the
/// attacker wins the aerial duel at the target point, a companion
/// `header` event on the next `seq` (same tick).
#[test]
fn cross_resolves_to_cross_and_header_events() {
    use bevy_ecs::prelude::Schedule;
    use sim_components::{MatchClock, PlayerId, TeamId};
    use sim_rules::CurrentTick;
    use sim_math::Vec2;

    use crate::simulation::event_emit::{
        emit_cross_header_events, CrossResolved, HeaderResolved, PendingCross,
    };
    use crate::simulation::events::{CrossOutcome, HeaderKind, HeaderOutcome};

    let mut world = crate::tests::installed_world();
    world.insert_resource(MatchClock {
        elapsed_ticks: 7_000,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(EventLog::new());
    world.insert_resource(CurrentTick(7_000));

    let crosser = PlayerId::from_bits(11);
    let headerer = PlayerId::from_bits(22);
    world.insert_resource(PendingCross(Some(CrossResolved {
        player_id: crosser,
        team: TeamId(0),
        target: Vec2::new(94.0, 34.0),
        outcome: CrossOutcome::Completed,
        header: Some(HeaderResolved {
            player: headerer,
            kind: HeaderKind::Shot,
            outcome: HeaderOutcome::Completed,
        }),
    })));

    let mut schedule = Schedule::default();
    schedule.add_systems(emit_cross_header_events);
    schedule.run(&mut world);

    let log = world.resource::<EventLog>();
    assert_eq!(log.events.len(), 2, "cross then header");
    assert_eq!(log.next_seq, 2);
    assert!(
        check_gapless_seq(log).is_none(),
        "cross + header must be gapless"
    );

    let MatchEvent::Cross { envelope, payload } = &log.events[0] else {
        panic!("expected cross event first");
    };
    assert_eq!(envelope.seq, 0);
    assert_eq!(envelope.tick, 7_000);
    assert_eq!(envelope.team, Some(TeamId(0)), "crosser team credited");
    assert_eq!(payload.player, crosser);
    assert_eq!(payload.target, [94.0, 34.0]);
    assert_eq!(payload.outcome, CrossOutcome::Completed);

    let MatchEvent::Header { envelope, payload } = &log.events[1] else {
        panic!("expected header event second");
    };
    assert_eq!(envelope.seq, 1);
    assert_eq!(envelope.tick, 7_000);
    assert_eq!(envelope.team, Some(TeamId(0)), "headerer team credited");
    assert_eq!(payload.player, headerer);
    assert_eq!(payload.kind, HeaderKind::Shot);
    assert_eq!(payload.outcome, HeaderOutcome::Completed);
}

/// Ticket 23: a cross that was punched / intercepted produces a `cross`
/// event with no companion `header`.
#[test]
fn cross_without_header_emits_only_cross() {
    use bevy_ecs::prelude::Schedule;
    use sim_components::{MatchClock, PlayerId, TeamId};
    use sim_rules::CurrentTick;
    use sim_math::Vec2;

    use crate::simulation::event_emit::{
        emit_cross_header_events, CrossResolved, PendingCross,
    };
    use crate::simulation::events::CrossOutcome;

    let mut world = crate::tests::installed_world();
    world.insert_resource(MatchClock {
        elapsed_ticks: 7_000,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(EventLog::new());
    world.insert_resource(CurrentTick(7_000));

    world.insert_resource(PendingCross(Some(CrossResolved {
        player_id: PlayerId::from_bits(11),
        team: TeamId(1),
        target: Vec2::new(94.0, 34.0),
        outcome: CrossOutcome::Punched,
        header: None,
    })));

    let mut schedule = Schedule::default();
    schedule.add_systems(emit_cross_header_events);
    schedule.run(&mut world);

    let log = world.resource::<EventLog>();
    assert_eq!(log.events.len(), 1, "only the cross event");
    assert_eq!(log.next_seq, 1);
    let MatchEvent::Cross { payload, .. } = &log.events[0] else {
        panic!("expected cross event");
    };
    assert_eq!(payload.outcome, CrossOutcome::Punched);
}

/// Ticket 10: detection and emission compose in one schedule run —
/// `goal_detection_system` credits the score and stashes the window,
/// then the emitters write the `goal` event (with scorer) and the
/// `possession_change` with cause `goal`, and the pass resolver drains
/// the signal.
#[test]
fn goal_detection_and_emission_compose_in_one_schedule() {
    use bevy_ecs::prelude::*;
    use sim_components::{
        Ball, BallMarker, BallState, LastTouch, Match, MatchClock, MatchState, PendingBallTouches,
        Position, TouchKind, Velocity,
    };

    let mut world = crate::tests::installed_world();
    world.insert_resource(MatchClock {
        elapsed_ticks: 4_500,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(Match {
        id: 1,
        home_team: bevy_ecs::prelude::Entity::PLACEHOLDER,
        away_team: bevy_ecs::prelude::Entity::PLACEHOLDER,
        score: (0, 0),
        state: MatchState::InPlay,
        seed: 7,
    });
    world.insert_resource(EventLog::new());
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(sim_rules::PendingPass::default());
    world.insert_resource(sim_rules::GoalForEmit::default());
    world.insert_resource(sim_rules::CurrentTick(4_500));
    let toucher = world.spawn(()).id();
    let touch = LastTouch::by_player(TeamId(0), toucher, TouchKind::Play).with_tick(4_500);
    world.insert_resource(Ball {
        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touch: Some(touch),
        touch_history: [None, None],
        kick_velocity: None,
    });
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world
        .entity_mut(ball_entity)
        .insert(Position(sim_math::Vec2::new(105.5, 34.0)));
    world
        .entity_mut(ball_entity)
        .insert(Velocity(sim_math::Vec2::zero()));

    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            sim_rules::apply_ball_touched_system,
            sim_rules::goal_detection_system,
            crate::simulation::event_emit::emit_goal_events,
            crate::simulation::event_emit::resolve_pass_events,
        )
            .chain(),
    );
    schedule.run(&mut world);

    assert_eq!(
        world.resource::<Match>().score,
        (1, 0),
        "home credited by goal position"
    );
    let log = world.resource::<EventLog>();
    assert_eq!(log.events.len(), 2, "goal + possession_change(goal)");
    let MatchEvent::Goal { payload, .. } = &log.events[0] else {
        panic!("expected goal event first");
    };
    assert_eq!(payload.scorer, touch.player_id, "scorer is the last Play touch");
    assert_eq!(payload.assister, None, "no previous touch, no assister");
    assert!(!payload.own_goal);
    assert_eq!(payload.score, (1, 0), "running score after the goal");
    assert!(
        world.resource::<sim_rules::GoalForEmit>().0.is_none(),
        "goal signal drained by the end of the chain"
    );
}

/// Ticket 10: the post-goal kickoff emits `reason = after_goal` with
/// no attack directions.
#[test]
fn kickoff_restart_emits_after_goal_without_attack_directions() {
    use bevy_ecs::prelude::Schedule;
    use sim_components::MatchClock;
    use sim_rules::{KickoffForEmit, KickoffResolved};

    let mut world = crate::tests::installed_world();
    world.insert_resource(MatchClock {
        elapsed_ticks: 5_100,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(EventLog::new());
    let taker = world.spawn(()).id();
    world.insert_resource(KickoffForEmit(Some(KickoffResolved {
        taker_team: TeamId(1),
        taker,
        at: sim_math::Vec2::new(52.5, 34.0),
    })));

    let mut schedule = Schedule::default();
    schedule.add_systems(crate::simulation::event_emit::emit_kickoff_restart_events);
    schedule.run(&mut world);

    let log = world.resource::<EventLog>();
    assert_eq!(log.events.len(), 1);
    let MatchEvent::Kickoff { envelope, payload } = &log.events[0] else {
        panic!("expected kickoff event");
    };
    assert_eq!(envelope.team, Some(TeamId(1)));
    assert_eq!(
        payload.taker,
        Some(sim_components::PlayerId::from_entity(taker))
    );
    assert_eq!(payload.reason, sim_components::KickoffReason::AfterGoal);
    assert_eq!((payload.attack_home, payload.attack_away), (None, None));
}
