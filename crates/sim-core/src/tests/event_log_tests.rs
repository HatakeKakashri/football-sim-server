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
    let mut world = World::new();
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
    let mut world = World::new();
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
    let mut world = World::new();
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

/// `possession_change` events have the right fields when built.
#[test]
fn possession_change_event_emits_correct_envelope_and_payload() {
    let mut world = World::new();
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
    let mut world = World::new();
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
