//! Feature 002 integration tests: a real `Simulation` traced to an in-memory
//! Chrome trace, plus isolated emission-system behaviour.

#![allow(
    clippy::float_cmp,
    reason = "tests compare values against the identical arithmetic they were derived from"
)]

use std::collections::BTreeSet;

use bevy_ecs::prelude::*;
use serde_json::Value;
use sim_components::{
    Ball, BallMarker, BallState, Card, CardColor, Position, Referee, StoppageEvent, Velocity,
};
use sim_math::Vec2;
use sim_telemetry::{TelemetryConfig, TraceGate, capture_trace};

use crate::Simulation;
use crate::simulation::trace_emit::trace_emission_system;

const BALL_PID: u64 = 1;

/// One snapshot per second. Cadence tests pin this explicitly so they do not
/// depend on the CLI default, which is a separate (size) decision.
fn every_second() -> TelemetryConfig {
    TelemetryConfig::new(60, None).expect("valid")
}

fn traced_sim(config: TelemetryConfig) -> Simulation {
    let mut sim = Simulation::new(42);
    sim.enable_telemetry(config);
    sim
}

fn run(sim: &mut Simulation, ticks: u64) -> Vec<Value> {
    capture_trace(|| {
        for _ in 0..ticks {
            sim.tick();
        }
    })
}

fn counters<'a>(events: &'a [Value], pid: u64, name: &str) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|e| e["ph"] == "C" && e["pid"] == pid && e["name"] == name)
        .collect()
}

fn player_pids(events: &[Value]) -> BTreeSet<u64> {
    events
        .iter()
        .filter(|e| e["name"] == "process_name")
        .filter(|e| {
            let name = e["args"]["name"].as_str().unwrap_or("");
            name.starts_with("Home ") || name.starts_with("Away ")
        })
        .filter_map(|e| e["pid"].as_u64())
        .collect()
}

/// Map a trace `ts` (µs of sim time plus a sub-millisecond per-event sequence
/// offset) back to the tick it belongs to.
fn tick_of(ts: u64) -> u64 {
    (ts * 60 + 500_000) / 1_000_000
}

#[test]
fn each_snapshot_records_every_player_and_the_ball() {
    let mut sim = traced_sim(every_second());
    let events = run(&mut sim, 130); // clock ticks 0..=129 → snapshots at 0, 60, 120
    let pids = player_pids(&events);
    assert_eq!(pids.len(), 22, "one process per player");
    for pid in &pids {
        for track in ["x", "y", "vx", "vy", "stamina"] {
            assert_eq!(counters(&events, *pid, track).len(), 3, "pid {pid} {track}");
        }
    }
    for track in ["x", "y", "vx", "vy", "spin"] {
        assert_eq!(counters(&events, BALL_PID, track).len(), 3, "ball {track}");
    }
    assert!(
        !events.iter().any(|e| e["args"]["name"] == "Referee"),
        "no Referee process when no Referee component exists"
    );
}

#[test]
fn every_player_is_traced_exactly_once_per_decision_window() {
    let mut sim = traced_sim(every_second());
    let events = run(&mut sim, 130);
    let decisions: Vec<&Value> = events
        .iter()
        .filter(|e| {
            e["ph"] == "B"
                && e["name"]
                    .as_str()
                    .is_some_and(|n| n.starts_with("decision @ tick "))
        })
        .collect();
    assert_eq!(
        decisions.len(),
        22 * 3,
        "22 players × windows at 0, 60, 120"
    );
    for pid in player_pids(&events) {
        let per_player = decisions.iter().filter(|e| e["pid"] == pid).count();
        assert_eq!(per_player, 3, "pid {pid}");
    }
    let ticks: BTreeSet<u64> = decisions
        .iter()
        .filter_map(|e| e["name"].as_str()?.rsplit(' ').next()?.parse().ok())
        .collect();
    assert!(
        ticks.iter().all(|t| t % 60 < 6),
        "decisions only inside windows: {ticks:?}"
    );
}

#[test]
fn trace_covers_every_action_and_consideration_of_the_default_brain() {
    // T010: nothing the brain can evaluate may be missing from the trace.
    let brain = crate::simulation::brain_default::default_utility_brain();
    // Independent oracle: the nine actions documented on `default_utility_brain`.
    let expected_actions: BTreeSet<&str> = [
        "MoveToPosition",
        "ChaseBall",
        "PassTo",
        "ShootAtGoal",
        "Tackle",
        "MarkOpponent",
        "Press",
        "SupportRun",
        "HoldPosition",
    ]
    .into_iter()
    .collect();
    let expected_considerations: BTreeSet<&str> = brain
        .actions
        .iter()
        .flat_map(|a| {
            a.considerations
                .iter()
                .map(sim_ai_player::Consideration::name)
        })
        .collect();
    assert_eq!(brain.actions.len(), expected_actions.len());
    assert!(expected_considerations.len() >= 10);

    let mut sim = traced_sim(TelemetryConfig::default());
    let events = run(&mut sim, 10); // ≥ one full decision window: every player evaluated
    let actions: BTreeSet<&str> = events
        .iter()
        .filter(|e| e["ph"] == "B" && e["args"]["chosen"].is_boolean())
        .filter_map(|e| e["name"].as_str())
        .collect();
    let considerations: BTreeSet<&str> = events
        .iter()
        .filter(|e| e["ph"] == "i" && e["args"]["curve"].is_string())
        .filter_map(|e| e["name"].as_str())
        .collect();
    assert_eq!(actions, expected_actions);
    assert_eq!(considerations, expected_considerations);
}

#[test]
fn full_range_adds_every_tick_without_double_recording() {
    let config = TelemetryConfig::new(60, Some((100, 160))).expect("valid");
    let mut sim = traced_sim(config);
    let events = run(&mut sim, 200);
    let got: Vec<u64> = counters(&events, BALL_PID, "x")
        .iter()
        .filter_map(|e| e["ts"].as_u64())
        .map(tick_of)
        .collect();
    let mut expected: Vec<u64> = [0_u64, 60, 180].into_iter().chain(100..=160).collect();
    expected.sort_unstable();
    assert_eq!(
        got, expected,
        "one snapshot per recorded tick (120 is in both sets)"
    );
}

#[test]
fn timestamps_never_go_backwards() {
    let mut sim = traced_sim(every_second());
    let events = run(&mut sim, 70);
    let stamps: Vec<u64> = events
        .iter()
        .filter(|e| e["ph"] != "M")
        .filter_map(|e| e["ts"].as_u64())
        .collect();
    assert!(stamps.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn telemetry_disabled_emits_nothing() {
    let mut sim = Simulation::new(42);
    assert!(run(&mut sim, 130).is_empty());
}

#[test]
fn tracing_does_not_change_the_final_state_hash() {
    let mut traced = traced_sim(TelemetryConfig::new(1, None).expect("valid"));
    let mut plain = Simulation::new(42);
    let events = run(&mut traced, 300);
    for _ in 0..300 {
        plain.tick();
    }
    assert!(!events.is_empty(), "guard against a vacuous comparison");
    assert_eq!(traced.get_state_hash(), plain.get_state_hash());
}

#[test]
fn referee_events_appear_once_when_a_referee_exists() {
    let mut sim = traced_sim(TelemetryConfig::new(10, None).expect("valid"));
    let offender = sim.ball_entity();
    sim.world_mut().spawn(Referee {
        stoppage_events: vec![StoppageEvent {
            tick: 3,
            reason: "injury".to_string(),
        }],
        cards: vec![Card {
            player: offender,
            color: CardColor::Red,
            tick: 5,
        }],
    });
    let events = run(&mut sim, 35);
    let named = |n: &str| events.iter().filter(|e| e["name"] == n).collect::<Vec<_>>();
    assert_eq!(
        named("card").len(),
        1,
        "emitted once, not on every recorded tick"
    );
    assert_eq!(named("card")[0]["args"]["color"], "Red");
    assert_eq!(named("stoppage").len(), 1);
    assert_eq!(named("stoppage")[0]["args"]["reason"], "injury");
    assert!(
        events
            .iter()
            .any(|e| e["name"] == "process_name" && e["args"]["name"] == "Referee")
    );
}

#[test]
fn stoppages_queued_on_different_recorded_ticks_are_all_reported() {
    // `added_time_calculation_system` drains `Referee.stoppage_events` every
    // tick, so each recorded tick sees only what was queued since the last
    // drain. Every one of them must reach the trace, not just the first.
    let mut world = World::new();
    let referee = world
        .spawn(Referee {
            stoppage_events: vec![StoppageEvent {
                tick: 0,
                reason: "injury".to_string(),
            }],
            cards: Vec::new(),
        })
        .id();
    let mut schedule = Schedule::default();
    schedule.add_systems(trace_emission_system);
    let config = TelemetryConfig::new(1, None).expect("valid");
    let mut gate = TraceGate::default();
    let events = capture_trace(|| {
        for tick in 0..3_u64 {
            gate.advance(tick, &config, 6);
            world.insert_resource(gate);
            if tick > 0 {
                world
                    .entity_mut(referee)
                    .get_mut::<Referee>()
                    .expect("invariant: referee entity exists")
                    .stoppage_events
                    .push(StoppageEvent {
                        tick,
                        reason: format!("queued at {tick}"),
                    });
            }
            schedule.run(&mut world);
            // The drain that follows the emission system in the real schedule.
            world
                .entity_mut(referee)
                .get_mut::<Referee>()
                .expect("invariant: referee entity exists")
                .stoppage_events
                .clear();
        }
    });
    let reasons: Vec<&str> = events
        .iter()
        .filter(|e| e["name"] == "stoppage")
        .filter_map(|e| e["args"]["reason"].as_str())
        .collect();
    assert_eq!(reasons, ["injury", "queued at 1", "queued at 2"]);
}

#[test]
fn ball_change_events_fire_only_when_state_or_possessor_changes() {
    let mut world = World::new();
    world.insert_resource(Ball {
        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touched_by: None,
        kick_velocity: None,
    });
    world.spawn((
        BallMarker,
        Position(Vec2::new(1.0, 2.0)),
        Velocity(Vec2::zero()),
    ));
    let holder = world.spawn(()).id();
    let mut schedule = Schedule::default();
    schedule.add_systems(trace_emission_system);
    let config = TelemetryConfig::new(1, None).expect("valid");
    let mut gate = TraceGate::default();
    let events = capture_trace(|| {
        for tick in 0..3 {
            gate.advance(tick, &config, 6);
            world.insert_resource(gate);
            if tick == 1 {
                let mut ball = world.resource_mut::<Ball>();
                ball.state = BallState::Possessed;
                ball.possessor = Some(holder);
            }
            schedule.run(&mut world);
        }
    });
    let changes: Vec<&Value> = events
        .iter()
        .filter(|e| e["name"] == "state_change")
        .collect();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["args"]["from"], "Free");
    assert_eq!(changes[0]["args"]["to"], "Possessed");
    assert_eq!(changes[0]["ts"].as_u64().map(tick_of), Some(1));
    let poss: Vec<&Value> = events
        .iter()
        .filter(|e| e["name"] == "possession_change")
        .collect();
    assert_eq!(poss.len(), 1);
    assert_eq!(poss[0]["args"]["from"], "none");
    assert_eq!(poss[0]["args"]["to"], holder.index().to_string());
    assert_eq!(counters(&events, BALL_PID, "x").len(), 3);
}
