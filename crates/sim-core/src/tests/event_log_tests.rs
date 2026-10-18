//! Tests for the event log: `EventLog`, the `pass` and
//! `possession_change` event writers, the `seq` gapless invariant,
//! and the wire shape (PRD §9).
//!
//! The tests are split into:
//! - `event_log_invariant` — gapless-monotonic `seq` over a
//!   real `Simulation` run.
//! - `pass_event_resolved_at_acquisition` — a `PassTo` kick is
//!   resolved when the ball is acquired.
//! - `possession_change_fires_on_team_change` — a `possession_change`
//!   event fires when possession moves between teams.
//! - `oob_emits_pass_then_possession_change` — OOB resolves the
//!   pending pass with `to = None` and `success = false`, and
//!   emits `possession_change` with cause `out_of_bounds`.

#![allow(
    clippy::float_cmp,
    reason = "tests compare fresh values against the constants they were built from"
)]

use bevy_ecs::prelude::*;
use sim_components::{Ball, BallMarker, BallState, LastTouch, Player, Position, TouchKind, Velocity};
use crate::{
    EventLog, MatchEvent, PossessionChangeCause, Simulation, check_gapless_seq,
};
use sim_math::Vec2;

fn pos(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

/// Drive a `Simulation` to a given tick count, then return the
/// event log for assertions.
fn play_to_tick(seed: u64, n: u64) -> (Simulation, EventLog) {
    let mut sim = Simulation::new(seed);
    for _ in 0..n {
        sim.tick();
    }
    let log = sim
        .world()
        .get_resource::<EventLog>()
        .cloned()
        .expect("invariant: EventLog is registered in every Simulation");
    (sim, log)
}

/// Drive a `Simulation` to `FullTime` (or `tick_cap`) and return
/// the log + final state hash.
fn play_to_full_time(seed: u64) -> (Simulation, EventLog, u64) {
    let mut sim = Simulation::new(seed);
    let tick_cap = 400_000_u64;
    let mut reached_full = false;
    for _ in 0..tick_cap {
        sim.tick();
        let world = sim.world();
        let state = world.resource::<sim_components::Match>().state;
        if state == sim_components::MatchState::FullTime {
            reached_full = true;
            break;
        }
    }
    assert!(
        reached_full,
        "seed {seed} did not reach FullTime within the tick cap"
    );
    let log = sim
        .world()
        .get_resource::<EventLog>()
        .cloned()
        .expect("EventLog must be registered");
    let hash = sim.get_state_hash();
    (sim, log, hash)
}

/// Two `Simulation` runs with the same seed produce the same
/// event log. This is the A6 (determinism) requirement on the
/// event log.
#[test]
fn event_log_is_deterministic_for_same_seed() {
    // A short tick window keeps the test fast while still
    // exercising the schedule enough to emit a handful of
    // events. The full-match determinism check is `#[ignore]`'d
    // below.
    let (_, log_a) = play_to_tick(12345, 5_000);
    let (_, log_b) = play_to_tick(12345, 5_000);
    assert_eq!(
        log_a.next_seq, log_b.next_seq,
        "same seed must produce the same number of events"
    );
    for (i, (a, b)) in log_a.events.iter().zip(log_b.events.iter()).enumerate() {
        assert_eq!(a.envelope().seq, b.envelope().seq, "seq at {i} differs");
        assert_eq!(a.envelope().tick, b.envelope().tick, "tick at seq {i}");
    }
}

/// `seq` is strictly gapless and monotonic across a match window.
#[test]
fn event_log_seq_is_gapless_and_monotonic() {
    let (_, log) = play_to_tick(12345, 5_000);
    assert!(check_gapless_seq(&log).is_none(), "event log has seq gaps");
    for (i, event) in log.events.iter().enumerate() {
        let expected = u64::try_from(i).expect("invariant: index fits u64");
        assert_eq!(
            event.envelope().seq,
            expected,
            "event #{i} has seq {} (expected {expected})",
            event.envelope().seq
        );
    }
}

/// Full-match determinism on the event log. `#[ignore]`'d because
/// it takes minutes per run; run explicitly when verifying
/// end-to-end behaviour.
///
/// ```text
/// cargo test -p sim-core --lib -- --ignored event_log_is_deterministic_full_match
/// ```
#[test]
#[ignore = "full-match event-log determinism check is minutes-long; run only when verifying end-to-end behaviour"]
fn event_log_is_deterministic_full_match() {
    let (_, log_a, _) = play_to_full_time(12345);
    let (_, log_b, _) = play_to_full_time(12345);
    assert_eq!(
        log_a.next_seq, log_b.next_seq,
        "same seed must produce the same number of events"
    );
    for (a, b) in log_a.events.iter().zip(log_b.events.iter()) {
        assert_eq!(a.envelope().seq, b.envelope().seq);
        assert_eq!(a.envelope().tick, b.envelope().tick);
    }
}

/// Ticket 10 over a full match: every `goal` has a scorer with a
/// running score that folds correctly (A7); the lifecycle events
/// (`kickoff` × half starts + after goals, one `half_time`, one
/// trailing `full_time`) are present; `full_time` carries the event
/// count and the live state hash. `#[ignore]`d (minutes-long).
///
/// ```text
/// cargo test -p sim-core --lib -- --ignored full_match_lifecycle_events_and_score_fold
/// ```
#[test]
#[ignore = "full-match lifecycle-event check is minutes-long; run only when verifying end-to-end behaviour"]
fn full_match_lifecycle_events_and_score_fold() {
    use sim_components::{KickoffReason, TeamId};

    let (sim, log, hash) = play_to_full_time(12345);
    assert!(
        check_gapless_seq(&log).is_none(),
        "event log has seq gaps"
    );

    // Lifecycle shape: one match-start kickoff, one second-half
    // kickoff, exactly one half_time, exactly one trailing full_time.
    // After-goal kickoffs are at most one per goal (a goal inside the
    // last 60 ticks of a half never restarts before the whistle).
    let mut match_starts = 0_u32;
    let mut second_halfs = 0_u32;
    let mut after_goals = 0_u32;
    let mut half_times = 0_u32;
    for event in &log.events {
        if let MatchEvent::Kickoff { payload, .. } = event {
            match payload.reason {
                KickoffReason::MatchStart => match_starts += 1,
                KickoffReason::SecondHalf => second_halfs += 1,
                KickoffReason::AfterGoal => after_goals += 1,
            }
        }
        if matches!(event, MatchEvent::HalfTime { .. }) {
            half_times += 1;
        }
    }
    assert_eq!(match_starts, 1, "one match-start kickoff");
    assert_eq!(second_halfs, 1, "one second-half kickoff");
    assert_eq!(half_times, 1, "one half_time event");

    // Goal fold (A7): every goal has a scorer, the running score in
    // each event equals the per-team count at emission time, and the
    // totals equal the final score.
    let mut home = 0_u8;
    let mut away = 0_u8;
    let mut goals = 0_u32;
    for event in &log.events {
        if let MatchEvent::Goal { envelope, payload } = event {
            goals += 1;
            assert!(
                payload.scorer.is_some(),
                "every goal event has a scorer"
            );
            if payload.own_goal {
                assert_eq!(
                    payload.assister, None,
                    "own goals carry no assister"
                );
            }
            if envelope.team == Some(TeamId(0)) {
                home += 1;
            } else {
                away += 1;
            }
            assert_eq!(
                payload.score,
                (home, away),
                "goal running score must equal the per-team goal count"
            );
        }
    }
    assert!(
        after_goals <= goals,
        "at most one after-goal kickoff per goal"
    );
    let score = sim
        .world()
        .resource::<sim_components::Match>()
        .score;
    assert_eq!(
        (home, away),
        score,
        "final score must equal the count of goal events per team (A7)"
    );

    // Trailing full_time: event count includes itself, state hash
    // matches the live hash.
    let MatchEvent::FullTime { envelope, payload } = log.events.last().expect("log is non-empty")
    else {
        panic!("last event must be full_time");
    };
    assert_eq!(envelope.team, None);
    assert_eq!(payload.score, score);
    assert_eq!(
        payload.event_count,
        u64::try_from(log.events.len()).expect("invariant: len fits u64"),
        "event_count includes the full_time event itself"
    );
    assert_eq!(
        payload.state_hash, hash,
        "full_time state_hash matches Simulation::get_state_hash"
    );
}

/// Every event has the envelope fields populated (PRD §9.1).
#[test]
fn events_have_populated_envelope() {
    let (_, log) = play_to_tick(12345, 5_000);
    for event in &log.events {
        let env = event.envelope();
        // `half` is 1 or 2.
        assert!(env.half == 1 || env.half == 2, "half must be 1 or 2");
        // `minute` is 45 or 90.
        assert!(
            env.minute == 45 || env.minute == 90,
            "minute must be 45 or 90"
        );
        // `added_minute` is None or a positive integer.
        if let Some(n) = env.added_minute {
            assert!(n >= 1, "added_minute must be ≥ 1 when present");
            assert!(n <= 10, "added_minute must be ≤ 10 per PRD §8.4");
        }
        assert!(env.tick <= 400_000, "tick must be within the cap");
    }
}

/// When possession changes team, a `possession_change` event fires.
#[test]
fn possession_change_fires_on_team_change() {
    // Build a world where the cache already says "team 0", then
    // transfer possession to team 1 via direct resource writes.
    // This bypasses the per-tick possession-resolution system to
    // pin the event emission on a clean team change.
    let mut world = crate::tests::installed_world();
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.entity_mut(ball_entity).insert(Position(pos(50.0, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));
    world.insert_resource(Ball {
        state: BallState::Possessed,
        possessor: None,
        last_touch: Some(LastTouch::by_player(
            sim_components::TeamId(0),
            Entity::from_raw(1),
            TouchKind::Play,
        )),
        spin: 0.0,
        touch_history: [None, None],
        kick_velocity: None,
    });
    world.insert_resource(sim_components::MatchClock {
        elapsed_ticks: 100,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(sim_rules::PendingPass::default());
    world.insert_resource(sim_rules::PendingOutOfBounds::default());
    world.insert_resource(sim_rules::CurrentTick(100));
    // Pre-seed the cache to team 0 so the first team change
    // (0 → 1) emits a possession_change. The "first acquisition
    // from None" path is intentionally a no-op (the cache is
    // initialised without a corresponding event); this test
    // exercises the cache-aware team-change path.
    let mut log = EventLog::new();
    log.possession_team = Some(sim_components::TeamId(0));
    world.insert_resource(log);

    // Spawn a team-1 player (the new possessor). Team 0 is not
    // needed because the cache already says "0".
    let t1_player = world
        .spawn((
            Player {
                team_id: sim_components::TeamId(1),
                intent: None,
            },
            sim_components::TeamIdComponent(sim_components::TeamId(1)),
        ))
        .id();

    // Mark the ball as possessed by team 1, then run the
    // possession-change emission system.
    {
        let mut ball = world.resource_mut::<Ball>();
        ball.possessor = Some(t1_player);
        ball.state = BallState::Possessed;
    }

    let mut schedule = Schedule::default();
    schedule.add_systems(
        crate::simulation::event_emit::emit_possession_change_on_acquisition,
    );
    schedule.run(&mut world);

    let log = world.resource::<EventLog>();
    assert!(
        log.possession_team == Some(sim_components::TeamId(1)),
        "possession_team cache should reflect the new team"
    );
    assert_eq!(
        log.events.len(),
        1,
        "exactly one possession_change should fire (0 → 1 transition)"
    );
    let event = &log.events[0];
    let MatchEvent::PossessionChange { payload, .. } = event else {
        panic!("expected possession_change, got {event:?}");
    };
    assert_eq!(payload.to_team, sim_components::TeamId(1));
    assert_eq!(payload.from_team, sim_components::TeamId(0));
    assert_eq!(payload.cause, PossessionChangeCause::Interception);
}

/// `possession_change` does NOT fire when the new possessor is on
/// the same team as the cache. This is the "team didn't change"
/// path: the cache stays at the same team, no event.
#[test]
fn possession_change_does_not_fire_on_same_team() {
    let mut world = crate::tests::installed_world();
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.entity_mut(ball_entity).insert(Position(pos(50.0, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));
    world.insert_resource(Ball {
        state: BallState::Possessed,
        possessor: None,
        last_touch: None,
        spin: 0.0,
        touch_history: [None, None],
        kick_velocity: None,
    });
    world.insert_resource(sim_components::MatchClock {
        elapsed_ticks: 100,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(sim_rules::CurrentTick(100));
    let mut log = EventLog::new();
    log.possession_team = Some(sim_components::TeamId(0));
    world.insert_resource(log);

    let t0_player_b = world
        .spawn((
            Player {
                team_id: sim_components::TeamId(0),
                intent: None,
            },
            sim_components::TeamIdComponent(sim_components::TeamId(0)),
        ))
        .id();
    {
        let mut ball = world.resource_mut::<Ball>();
        ball.possessor = Some(t0_player_b);
        ball.state = BallState::Possessed;
    }
    let mut schedule = Schedule::default();
    schedule.add_systems(
        crate::simulation::event_emit::emit_possession_change_on_acquisition,
    );
    schedule.run(&mut world);
    let log = world.resource::<EventLog>();
    assert_eq!(
        log.events.len(),
        0,
        "no event should fire when the new possessor is on the same team"
    );
}

/// `pass` events have the right fields when built.
#[test]
fn pass_event_emits_correct_envelope_and_payload() {
    use sim_components::PlayerId;
    let mut world = crate::tests::installed_world();
    world.insert_resource(Ball {
        state: BallState::Free,
        possessor: None,
        last_touch: None,
        spin: 0.0,
        touch_history: [None, None],
        kick_velocity: None,
    });
    world.insert_resource(sim_components::MatchClock {
        elapsed_ticks: 200,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(EventLog::new());
    let kicker = PlayerId::from_bits(7);
    let receiver = PlayerId::from_bits(9);
    let clock = world.resource::<sim_components::MatchClock>().clone();
    let event = crate::build_pass_event(
        0, // next_seq
        200,
        &clock,
        sim_components::TeamId(0),
        kicker,
        Some(receiver),
        true,
        100,
        pos(50.0, 34.0),
        pos(60.0, 34.0),
    );
    {
        let mut log = world.resource_mut::<EventLog>();
        log.append(event);
    }
    let log = world.resource::<EventLog>();
    assert_eq!(log.next_seq, 1);
    assert_eq!(log.events.len(), 1);
    let MatchEvent::Pass { payload, envelope } = &log.events[0] else {
        panic!("expected pass event");
    };
    assert_eq!(envelope.seq, 0);
    assert_eq!(envelope.tick, 200);
    assert_eq!(envelope.half, 1);
    assert_eq!(envelope.minute, 45);
    assert_eq!(envelope.added_minute, None);
    assert_eq!(envelope.team, Some(sim_components::TeamId(0)));
    assert_eq!(payload.from, kicker);
    assert_eq!(payload.to, Some(receiver));
    assert!(payload.success);
    assert_eq!(payload.kick_tick, 100);
    assert_eq!(payload.start, [50.0, 34.0]);
    assert_eq!(payload.end, [60.0, 34.0]);
}

/// `tackle` events are emitted at every tackle attempt with `tackler`,
/// `carrier` and `success` (PRD §9.2, ticket 21).
#[test]
fn tackle_event_emitted_with_tackler_carrier_and_success() {
    let mut world = crate::tests::installed_world();
    world.insert_resource(sim_components::MatchClock {
        elapsed_ticks: 200,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(sim_rules::CurrentTick(200));
    world.insert_resource(sim_rules::PendingTackle::default());
    world.insert_resource(EventLog::new());

    let tackler = world
        .spawn((
            Player {
                team_id: sim_components::TeamId(0),
                intent: None,
            },
            sim_components::TeamIdComponent(sim_components::TeamId(0)),
        ))
        .id();
    let carrier = world.spawn(()).id();

    world
        .resource_mut::<sim_rules::PendingTackle>()
        .set(sim_rules::TackleAttempt {
            tackler,
            carrier,
            success: true,
        });

    let mut schedule = Schedule::default();
    schedule.add_systems(crate::simulation::event_emit::emit_tackle_events);
    schedule.run(&mut world);

    let log = world.resource::<EventLog>();
    assert_eq!(log.events.len(), 1, "exactly one tackle event");
    let MatchEvent::Tackle { payload, envelope } = &log.events[0] else {
        panic!("expected tackle event, got {:?}", log.events[0]);
    };
    assert_eq!(envelope.seq, 0);
    assert_eq!(envelope.tick, 200);
    assert_eq!(envelope.team, Some(sim_components::TeamId(0)));
    assert_eq!(
        payload.tackler,
        sim_components::PlayerId::from_entity(tackler)
    );
    assert_eq!(
        payload.carrier,
        sim_components::PlayerId::from_entity(carrier)
    );
    assert!(payload.success);
}

/// `possession_change` events have the right fields when built.
#[test]
fn possession_change_event_emits_correct_envelope_and_payload() {
    let mut world = crate::tests::installed_world();
    world.insert_resource(sim_components::MatchClock {
        elapsed_ticks: 100,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(EventLog::new());
    let clock = world.resource::<sim_components::MatchClock>().clone();
    let event = crate::build_possession_change_event(
        0,
        100,
        &clock,
        Some(sim_components::TeamId(0)),
        sim_components::TeamId(1),
        PossessionChangeCause::Interception,
        None,
    );
    {
        let mut log = world.resource_mut::<EventLog>();
        log.append(event);
    }
    let log = world.resource::<EventLog>();
    let MatchEvent::PossessionChange { payload, envelope } = &log.events[0] else {
        panic!("expected possession_change event");
    };
    assert_eq!(envelope.seq, 0);
    assert_eq!(envelope.team, Some(sim_components::TeamId(1)));
    assert_eq!(payload.from_team, sim_components::TeamId(0));
    assert_eq!(payload.to_team, sim_components::TeamId(1));
    assert_eq!(payload.cause, PossessionChangeCause::Interception);
}

/// Events are serialised to the PRD §9.1+§9.2 wire shape (flat
/// object with envelope + payload fields at the same level).
#[test]
fn event_wire_shape_matches_prd() {
    use sim_components::PlayerId;
    let mut world = crate::tests::installed_world();
    world.insert_resource(sim_components::MatchClock {
        elapsed_ticks: 7421,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(EventLog::new());
    let clock = world.resource::<sim_components::MatchClock>().clone();
    let event = crate::build_pass_event(
        0,
        7421,
        &clock,
        sim_components::TeamId(0),
        PlayerId::from_bits(7),
        Some(PlayerId::from_bits(9)),
        true,
        7000,
        pos(50.0, 34.0),
        pos(60.0, 34.0),
    );
    {
        let mut log = world.resource_mut::<EventLog>();
        log.append(event);
    }
    let log = world.resource::<EventLog>();
    let json = serde_json::to_string(&log.events[0]).expect("serialise");
    // Envelope fields are at the top level.
    assert!(json.contains("\"seq\":0"), "seq not at top level: {json}");
    assert!(
        json.contains("\"tick\":7421"),
        "tick not at top level: {json}"
    );
    assert!(json.contains("\"half\":1"), "half not at top level: {json}");
    assert!(json.contains("\"minute\":45"), "minute not at top level: {json}");
    assert!(
        json.contains("\"added_minute\":null"),
        "added_minute not at top level: {json}"
    );
    assert!(json.contains("\"type\":\"pass\""), "type not at top level: {json}");
    // Payload fields are at the top level.
    assert!(json.contains("\"kick_tick\":7000"));
    assert!(json.contains("\"success\":true"));
    // The envelope's `type` field is not duplicated; only one
    // `type` key exists, set to the event-type string.
    let type_count = json.matches("\"type\"").count();
    assert_eq!(type_count, 1, "exactly one `type` key expected: {json}");
}

/// Build a tiny hand-crafted scenario that drives the OOB→restart
/// event pipeline end to end: out-of-bounds ball, OOB detector
/// fires, `restart_system` resolves the taker and the placement,
/// the event emitter writes `out_of_bounds` and `restart` back-to-back.
///
/// The pipeline runs inside a real `Bevy` `Schedule` and uses the
/// rule systems + event emitters in the same order the production
/// schedule uses (see `sim-core/src/simulation/construct.rs`).
#[allow(clippy::too_many_lines, reason = "end-to-end OOB→restart scenario setup")]
fn run_oob_scenario(
    ball_pos: Vec2,
    ball_vel: Vec2,
    oob_type: sim_components::BallOutOfBoundsType,
    last_touch: LastTouch,
) -> EventLog {
    use crate::simulation::SimulationSet;

    let mut world = crate::tests::installed_world();

    // Ball entity + position / velocity / marker.
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.entity_mut(ball_entity).insert(Position(ball_pos));
    world.entity_mut(ball_entity).insert(Velocity(ball_vel));

    // Toucher entity (for the OOB detector's last-touch assertion
    // and the restart's taker-team computation).
    let toucher = world.spawn(()).id();
    world.entity_mut(toucher).insert(Player {
        team_id: last_touch.team,
        intent: None,
    });
    world
        .entity_mut(toucher)
        .insert(sim_components::TeamIdComponent(last_touch.team));

    // Taker-team entity (the team opposite the toucher's team).
    let taker_team = if last_touch.team == sim_components::TeamId(0) {
        sim_components::TeamId(1)
    } else {
        sim_components::TeamId(0)
    };
    let taker_player = world.spawn(()).id();
    world.entity_mut(taker_player).insert(Player {
        team_id: taker_team,
        intent: None,
    });
    world
        .entity_mut(taker_player)
        .insert(sim_components::TeamIdComponent(taker_team));

    // Ball resource with the touch recorded.
    world.insert_resource(Ball {
        state: BallState::Free,
        possessor: None,
        last_touch: Some(last_touch),
        spin: 0.0,
        touch_history: [None, None],
        kick_velocity: None,
    });

    world.insert_resource(sim_components::MatchClock {
        elapsed_ticks: 1_234,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(sim_rules::CurrentTick(1_234));
    world.insert_resource(sim_rules::PendingOutOfBounds::default());
    world.insert_resource(sim_rules::OobRestartForEmit::default());
    world.insert_resource(sim_rules::PendingPass::default());
    world.insert_resource(sim_components::PendingBallTouches::default());
    world.insert_resource(sim_components::Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: sim_components::MatchState::InPlay,
        seed: 12345,
    });
    world.insert_resource(EventLog::new());

    // Pre-seed the OOB signal the way `out_of_bounds_system` would.
    world.resource_mut::<sim_rules::PendingOutOfBounds>().set(
        sim_rules::OutOfBoundsEvent {
            oob_type,
            last_touch: Some(last_touch),
        },
    );

    // Force the ball into the `Dead` state so `restart_system`
    // takes the OOB arm.
    world.resource_mut::<Ball>().state = BallState::Dead;

    let mut schedule = Schedule::default();
    schedule.configure_sets(
        (
            SimulationSet::Perception,
            SimulationSet::Decision,
            SimulationSet::Execution,
            SimulationSet::Physics,
            SimulationSet::Possession,
            SimulationSet::Rules,
            SimulationSet::MatchAdmin,
        )
            .chain(),
    );
    // Register only the systems relevant to the OOB→restart chain
    // (skip the upstream sets — the test isn't driving perception
    // or physics).
    schedule.add_systems(
        (
            sim_rules::apply_ball_touched_system,
            sim_rules::out_of_bounds_system,
            sim_rules::goal_detection_system,
            sim_rules::foul_detection_system,
            sim_rules::restart_system,
            crate::simulation::event_emit::emit_oob_and_restart_events,
            crate::simulation::event_emit::emit_possession_change_on_oob,
            crate::simulation::event_emit::emit_possession_change_on_acquisition,
            crate::simulation::event_emit::resolve_pass_events,
        )
            .chain()
            .in_set(SimulationSet::Rules),
    );

    schedule.run(&mut world);
    world.remove_resource::<EventLog>().expect("EventLog is set")
}

/// OOB fires on this tick → `out_of_bounds` is emitted first, then
/// the matching `restart` event (PRD §9.2, ticket 09). The pair
/// shares the same tick and the `restart.seq == out_of_bounds.seq +
/// 1` invariant holds.
#[test]
fn oob_emits_out_of_bounds_and_restart_pair() {
    use sim_components::BallOutOfBoundsType;
    let toucher = Entity::from_raw(11);
    let last_touch = LastTouch::by_player(
        sim_components::TeamId(0),
        toucher,
        TouchKind::Play,
    );
    let log = run_oob_scenario(
        pos(20.0, 0.0), // touchline OOB
        Vec2::zero(),
        BallOutOfBoundsType::ThrowIn,
        last_touch,
    );

    // Find the OOB event and the restart event among any
    // possession_change / pass events the pipeline also emits.
    let mut oob_event: Option<&crate::OutOfBoundsEventPayload> = None;
    let mut restart_event: Option<&crate::RestartEventPayload> = None;
    for event in &log.events {
        match event {
            crate::MatchEvent::OutOfBounds { payload, envelope } => {
                assert_eq!(envelope.tick, 1_234, "OOB event tick = current");
                oob_event = Some(payload);
            }
            crate::MatchEvent::Restart { payload, envelope } => {
                assert_eq!(envelope.tick, 1_234, "restart event tick = current");
                restart_event = Some(payload);
            }
            _ => {}
        }
    }
    let oob = oob_event.expect("out_of_bounds event was emitted");
    let restart = restart_event.expect("restart event was emitted");

    // The pair must be adjacent in the log (no other event
    // interleaved between them).
    let oob_idx = log
        .events
        .iter()
        .position(|e| matches!(e, crate::MatchEvent::OutOfBounds { .. }))
        .expect("OOB event present");
    let restart_idx = log
        .events
        .iter()
        .position(|e| matches!(e, crate::MatchEvent::Restart { .. }))
        .expect("restart event present");
    assert_eq!(
        restart_idx,
        oob_idx + 1,
        "restart must immediately follow out_of_bounds (ticket 09 ordering)"
    );

    // PRD §9.2 payload checks.
    assert_eq!(oob.restart, crate::RestartKind::ThrowIn);
    assert!(oob.last_touch.is_some(), "last_touch must be carried");
    assert_eq!(oob.exit_point[0], 20.0, "exit_point carries the ball pos at detection");
    assert_eq!(restart.kind, crate::RestartKind::ThrowIn, "kind matches OOB restart");
    // Throw-in at touchline y=0 → restart at (20, 0).
    assert_eq!(restart.at[0], 20.0);
    assert_eq!(restart.at[1], 0.0);
    // `taker` is on the team opposite the last toucher (Law 9).
    // last_touch.team = 0 → taker_team = 1; the taker player_id
    // resolves to the entity bits minted by `PlayerId::from_entity`.
    let expected_taker_team = if last_touch.team == sim_components::TeamId(0) {
        sim_components::TeamId(1)
    } else {
        sim_components::TeamId(0)
    };
    let restart_envelope_team = log
        .events
        .iter()
        .find_map(|e| match e {
            crate::MatchEvent::Restart { envelope, .. } => Some(envelope.team),
            _ => None,
        })
        .expect("restart envelope team present");
    assert_eq!(
        restart_envelope_team,
        Some(expected_taker_team),
        "restart envelope.team = taker team (Law 9)"
    );
    assert!(
        restart.taker.is_some(),
        "restart event must carry a taker PlayerId when a player exists on the taker team"
    );

    // Seq invariant: restart.seq == out_of_bounds.seq + 1.
    let oob_seq = log
        .events
        .iter()
        .find(|e| matches!(e, crate::MatchEvent::OutOfBounds { .. }))
        .map(|e| e.envelope().seq)
        .expect("OOB event has a seq");
    let restart_seq = log
        .events
        .iter()
        .find(|e| matches!(e, crate::MatchEvent::Restart { .. }))
        .map(|e| e.envelope().seq)
        .expect("restart event has a seq");
    assert_eq!(
        restart_seq,
        oob_seq + 1,
        "restart.seq == out_of_bounds.seq + 1"
    );
}

/// The `restart` taker is on the team OPPOSITE the last toucher
/// for every restart type (PRD §8.2 Law 9).
#[test]
fn restart_taker_is_on_team_opposite_last_toucher() {
    use sim_components::BallOutOfBoundsType;
    let last_touch_team = sim_components::TeamId(0);

    // Spawn a fixture per scenario so each test gets its own world.
    let run = |oob_type| {
        let last_touch = LastTouch::by_player(
            last_touch_team,
            Entity::from_raw(99),
            TouchKind::Play,
        );
        let log = run_oob_scenario(
            match oob_type {
                BallOutOfBoundsType::ThrowIn => pos(20.0, 0.0),
                BallOutOfBoundsType::GoalKick => pos(105.0, 34.0),
                BallOutOfBoundsType::Corner => pos(105.0, 1.0),
            },
            Vec2::zero(),
            oob_type,
            last_touch,
        );
        log.events
            .iter()
            .find_map(|e| match e {
                crate::MatchEvent::Restart { envelope, .. } => Some(envelope.team),
                _ => None,
            })
            .expect("restart event emitted")
    };

    assert_eq!(
        run(BallOutOfBoundsType::ThrowIn),
        Some(sim_components::TeamId(1)),
        "last_touch.team = 0 → restart envelope.team = 1 (opposite)"
    );
    assert_eq!(
        run(BallOutOfBoundsType::GoalKick),
        Some(sim_components::TeamId(1)),
        "last_touch.team = 0 → restart envelope.team = 1 (opposite)"
    );
    assert_eq!(
        run(BallOutOfBoundsType::Corner),
        Some(sim_components::TeamId(1)),
        "last_touch.team = 0 → restart envelope.team = 1 (opposite)"
    );
}

/// Across all restart scenarios (throw-in / goal-kick / corner)
/// the event log's `seq` is still gapless and monotonic — the
/// OOB→restart pair fits into the existing invariant.
#[test]
fn oob_and_restart_events_keep_seq_gapless() {
    use sim_components::BallOutOfBoundsType;
    let last_touch = LastTouch::by_player(
        sim_components::TeamId(1),
        Entity::from_raw(13),
        TouchKind::Play,
    );
    let log = run_oob_scenario(
        pos(0.0, 1.0), // corner
        Vec2::zero(),
        BallOutOfBoundsType::Corner,
        last_touch,
    );
    assert!(
        check_gapless_seq(&log).is_none(),
        "OOB+restart emissions must not break the gapless seq invariant"
    );
    // The log must contain at least one OOB and one restart event.
    assert!(
        log.events
            .iter()
            .any(|e| matches!(e, crate::MatchEvent::OutOfBounds { .. })),
        "at least one out_of_bounds event"
    );
    assert!(
        log.events
            .iter()
            .any(|e| matches!(e, crate::MatchEvent::Restart { .. })),
        "at least one restart event"
    );
}
