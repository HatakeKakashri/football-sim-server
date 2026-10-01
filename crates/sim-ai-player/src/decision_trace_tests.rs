//! Decision tracing: the trace must expose every action and consideration a
//! player evaluated, mark the winner, and never change what is decided.

#![allow(
    clippy::float_cmp,
    reason = "tests compare traced values against the identical arithmetic they were derived from"
)]

use bevy_ecs::prelude::*;
use serde_json::Value;
use sim_ai_core::{ResponseCurve, geometric_mean};
use sim_components::{
    Intent, MatchClock, MovementIntent, PerceptionSnapshot, PitchBounds, Player, Role,
    RoleComponent, Skill, Stamina, TeamId, TeamIdComponent,
};
use sim_math::Vec2;
use sim_telemetry::{TelemetryConfig, TraceGate, capture_trace};

use crate::brain::intent_kind;
use crate::{
    Consideration, ConsiderationContext, DecisionEvaluationCount, PlayerAction, UtilityBrain,
    player_decision_system,
};

fn perception() -> PerceptionSnapshot {
    PerceptionSnapshot {
        self_position: Vec2::new(50.0, 34.0),
        nearby_teammates: smallvec::SmallVec::new(),
        nearby_opponents: smallvec::SmallVec::new(),
        ball_position: Vec2::new(60.0, 34.0),
        ball_state: sim_components::BallState::Free,
        goal_position: Vec2::new(105.0, 34.0),
        pitch_bounds: PitchBounds {
            distance_to_left: 50.0,
            distance_to_right: 55.0,
            distance_to_top: 34.0,
            distance_to_bottom: 34.0,
        },
    }
}

fn brain() -> UtilityBrain {
    let lin = ResponseCurve::Linear {
        min: 0.0,
        max: 30.0,
    };
    let log = ResponseCurve::Logistic {
        midpoint: 0.5,
        steepness: 4.0,
    };
    UtilityBrain {
        actions: vec![
            PlayerAction {
                intent: Intent::Movement(MovementIntent::HoldPosition),
                considerations: vec![Consideration::Stamina {
                    weight: 0.5,
                    curve: log.clone(),
                }],
            },
            PlayerAction {
                intent: Intent::Movement(MovementIntent::ChaseBall),
                considerations: vec![
                    Consideration::DistanceToBall {
                        weight: 0.8,
                        curve: lin,
                    },
                    Consideration::Stamina {
                        weight: 0.3,
                        curve: log,
                    },
                ],
            },
            PlayerAction {
                intent: Intent::Movement(MovementIntent::SupportRun),
                considerations: vec![],
            },
        ],
        hysteresis: 0.1,
    }
}

fn world_with_player() -> (World, Entity) {
    let mut world = World::new();
    let player = world
        .spawn((
            Player {
                team_id: TeamId(0),
                intent: None,
            },
            Stamina(0.9),
            Skill(0.7),
            RoleComponent(Role::Striker),
            TeamIdComponent(TeamId(0)),
            perception(),
            brain(),
        ))
        .id();
    world.insert_resource(DecisionEvaluationCount::default());
    world.insert_resource(MatchClock {
        elapsed_ticks: 0,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    (world, player)
}

/// Drive one full cadence window (every player evaluated once).
fn run_window(world: &mut World, gate_on: bool) {
    let mut schedule = Schedule::default();
    schedule.add_systems(player_decision_system);
    let config = TelemetryConfig::default();
    let mut gate = TraceGate::default();
    for tick in 0..crate::DECISION_CADENCE_TICKS {
        world.resource_mut::<MatchClock>().elapsed_ticks = tick;
        if gate_on {
            gate.advance(tick, &config, crate::DECISION_CADENCE_TICKS);
            world.insert_resource(gate);
        }
        schedule.run(world);
    }
}

fn names<'a>(events: &'a [Value], ph: &str) -> Vec<&'a str> {
    events
        .iter()
        .filter(|e| e["ph"] == ph)
        .filter_map(|e| e["name"].as_str())
        .collect()
}

#[test]
fn trace_lists_every_action_and_consideration_with_the_winner_marked() {
    let (mut world, player) = world_with_player();
    let events = capture_trace(|| run_window(&mut world, true));

    let begins = names(&events, "B");
    assert_eq!(
        begins
            .iter()
            .filter(|n| n.starts_with("decision @ tick "))
            .count(),
        1
    );
    assert_eq!(&begins[1..], ["HoldPosition", "ChaseBall", "SupportRun"]);

    let considerations: Vec<&str> = names(&events, "i");
    assert_eq!(considerations, ["stamina", "distance_to_ball", "stamina"]);

    let chosen: Vec<&str> = events
        .iter()
        .filter(|e| e["ph"] == "B" && e["args"]["chosen"] == true)
        .filter_map(|e| e["name"].as_str())
        .collect();
    let decided = world
        .entity(player)
        .get::<Player>()
        .and_then(|p| p.intent)
        .expect("decided");
    assert_eq!(chosen, [intent_kind(&decided)]);

    let process = events
        .iter()
        .find(|e| e["name"] == "process_name")
        .expect("process_name");
    assert_eq!(
        process["args"]["name"],
        format!("Home {} — Striker", player.index())
    );
}

#[test]
fn traced_values_match_an_independent_recomputation() {
    let (mut world, _player) = world_with_player();
    let events = capture_trace(|| run_window(&mut world, true));
    let snapshot = perception();
    let brain = brain();
    let chase = &brain.actions[1];
    let ctx_intent = chase.intent;
    let ctx = ConsiderationContext {
        perception: &snapshot,
        intent: &ctx_intent,
        stamina: 0.9,
        skill: 0.7,
        grid: None,
    };
    let expected: Vec<(f32, f32, f32)> = chase
        .considerations
        .iter()
        .map(|c| {
            let raw = c.raw_input(&ctx);
            (raw, c.weight(), c.curve().evaluate(raw).raw())
        })
        .collect();
    let scores: Vec<f32> = expected.iter().map(|e| e.2).collect();
    let aggregate = geometric_mean(&scores);

    let dist = events
        .iter()
        .find(|e| e["name"] == "distance_to_ball")
        .expect("event");
    let (raw, weight, score) = expected[0];
    assert_eq!(dist["args"]["raw"].as_f64(), Some(f64::from(raw)));
    assert_eq!(dist["args"]["weight"].as_f64(), Some(f64::from(weight)));
    assert_eq!(dist["args"]["score"].as_f64(), Some(f64::from(score)));
    assert_eq!(dist["args"]["curve"], "linear");

    let span = events
        .iter()
        .find(|e| e["ph"] == "B" && e["name"] == "ChaseBall")
        .expect("span");
    assert_eq!(
        span["args"]["aggregate_score"].as_f64(),
        Some(f64::from(aggregate))
    );
    let baseline = events
        .iter()
        .find(|e| e["ph"] == "B" && e["name"] == "SupportRun")
        .expect("span");
    assert_eq!(
        baseline["args"]["aggregate_score"].as_f64(),
        Some(0.6_f32.into())
    );
}

#[test]
fn gate_off_emits_nothing() {
    let (mut world, _player) = world_with_player();
    let events = capture_trace(|| run_window(&mut world, false));
    assert!(events.is_empty(), "no gate resource ⇒ no trace: {events:?}");
}

#[test]
fn tracing_does_not_change_decisions_or_evaluation_counts() {
    let (mut traced, traced_player) = world_with_player();
    let (mut plain, plain_player) = world_with_player();
    let _ = capture_trace(|| run_window(&mut traced, true));
    run_window(&mut plain, false);
    let intent = |w: &World, e: Entity| w.entity(e).get::<Player>().and_then(|p| p.intent);
    assert_eq!(intent(&traced, traced_player), intent(&plain, plain_player));
    assert_eq!(
        traced.resource::<DecisionEvaluationCount>().get(),
        plain.resource::<DecisionEvaluationCount>().get()
    );
}

#[test]
fn hysteresis_is_included_only_for_the_current_intent() {
    let (mut world, player) = world_with_player();
    world
        .entity_mut(player)
        .get_mut::<Player>()
        .expect("player")
        .intent = Some(Intent::Movement(MovementIntent::HoldPosition));
    let events = capture_trace(|| run_window(&mut world, true));

    let snapshot = perception();
    let brain = brain();
    let plain = |action: &PlayerAction| {
        let ctx = ConsiderationContext {
            perception: &snapshot,
            intent: &action.intent,
            stamina: 0.9,
            skill: 0.7,
            grid: None,
        };
        let scores: Vec<f32> = action
            .considerations
            .iter()
            .map(|c| c.curve().evaluate(c.raw_input(&ctx)).raw())
            .collect();
        geometric_mean(&scores)
    };
    let traced = |name: &str| {
        events
            .iter()
            .find(|e| e["ph"] == "B" && e["name"] == name)
            .and_then(|e| e["args"]["aggregate_score"].as_f64())
            .expect("traced aggregate")
    };
    let hold = plain(&brain.actions[0]) + brain.hysteresis;
    assert_eq!(traced("HoldPosition"), f64::from(hold));
    assert_eq!(traced("ChaseBall"), f64::from(plain(&brain.actions[1])));
}
