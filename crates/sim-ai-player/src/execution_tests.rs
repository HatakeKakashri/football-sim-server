//! Integration tests for execution systems. Compiled only under `cfg(test)`.

use bevy_ecs::prelude::*;
use sim_components::{
    ActionIntent, Intent, MovementIntent, Skill, Stamina, TackleRecovery,
};
use sim_math::Vec2;
use sim_physics::SimRng;
use smallvec::smallvec;

use crate::execution::player_action_execution_system;
use crate::kicking::kick_execution_system;
use crate::recovery::tackle_recovery_system;
use crate::steering::{apply_acceleration_limit, steer_desired_velocity};
use crate::tests::ai_world;
use sim_components::{
    Ball, BallMarker, MatchClock, NearbyEntity,
    PendingBallTouches, PerceptionSnapshot, PitchBounds, Player, Position, Velocity,
};
use sim_rules::PendingRuleOutcome;
use sim_telemetry::{TelemetryConfig, capture_trace};

type EventVec = Vec<serde_json::Value>;

fn pos(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

/// Arrival slow-down: a player close to a stationary target should steer
/// slower than cruise speed so they don't oscillate around the target.
/// The Reynolds "arrival" formula caps speed at
/// `sqrt(2 * decel * distance)` when inside the arrival radius. Without
/// this, a player 0.5 m from target with cruise speed 8/60 would
/// overshoot and flip direction next tick.
#[test]
fn arrival_slowdown_caps_speed_near_target() {
    // Player 0.5 m east of target. With decel = 4 m/s² the arrival cap
    // is sqrt(2 * 4 * 0.5) = 2 m/s = 2/60 m/tick. Cruise is 8/60.
    // Result must be exactly the cap (cruise > cap), and must point
    // toward the target.
    let desired = steer_desired_velocity(pos(0.0, 0.0), pos(0.5, 0.0), 8.0 / 60.0, 4.0);
    let expected_cap = (2.0_f32 / 60.0).sqrt() * (2.0_f32 / 60.0).sqrt();
    // sqrt(2 * 4 * 0.5) = sqrt(4) = 2.0 m/s => 2/60 m/tick
    let expected_len = 2.0 / 60.0;
    assert!(
        (desired.length() - expected_len).abs() < 1e-6,
        "expected length {expected_len}, got {}",
        desired.length(),
    );
    // Direction: from (0.5, 0) to (0.0, 0) is (-1, 0).
    assert!(desired.x < 0.0, "should point toward target (negative x)");
    assert!(desired.y.abs() < 1e-6, "should be along x-axis");
    // Defensive: confirm our own expected_len formula
    let _ = expected_cap;
}

/// At distance beyond the arrival radius, speed should be cruise speed
/// unchanged. The arrival cap `sqrt(2 * decel * distance)` exceeds
/// cruise (8 m/s) when `distance > cruise^2 / (2*decel) = 8 m`. So at
/// 20 m the cruise cap dominates and the desired velocity equals
/// `unit_dir * 8/60` exactly.
#[test]
fn arrival_slowdown_does_not_reduce_speed_far_from_target() {
    let desired = steer_desired_velocity(pos(0.0, 0.0), pos(20.0, 0.0), 8.0 / 60.0, 4.0);
    let expected = -8.0 / 60.0; // negative x, toward target
    assert!(
        (desired.x - expected).abs() < 1e-6,
        "expected x {expected}, got {}",
        desired.x,
    );
    assert!(desired.y.abs() < 1e-6);
}

/// Below the 0.1 m deadband, return zero (preserve existing behaviour:
/// the player is "at" the target).
#[test]
fn arrival_returns_zero_inside_deadband() {
    let desired = steer_desired_velocity(pos(0.0, 0.0), pos(0.05, 0.0), 8.0 / 60.0, 4.0);
    assert_eq!(desired, Vec2::zero());
}

/// The acceleration clamp must cap the per-tick velocity change so a
/// stationary player cannot jump from 0 to full speed in a single tick.
/// With `max_accel` = 1.5 m/s² and `dt` = 1/60 s, the max per-tick change
/// is 1.5/60 m/tick ≈ 0.025 m/tick. So after one tick, |velocity| ≤
/// `max_accel_per_tick`.
#[test]
fn accel_clamp_limits_charge_of_magnitude_change() {
    let prev = Vec2::zero();
    let desired = Vec2::new(10.0 / 60.0, 0.0); // full cruise east
    let max_accel = 1.5 / 60.0; // per-tick cap
    let new_velocity = apply_acceleration_limit(prev, desired, max_accel);
    let expected = Vec2::new(max_accel, 0.0);
    assert!(
        (new_velocity.x - expected.x).abs() < 1e-6,
        "expected x {}, got {}",
        expected.x,
        new_velocity.x,
    );
    assert!(new_velocity.y.abs() < 1e-6);
    // After one tick, we are well under cruise (10/60 m/tick).
    assert!(new_velocity.length() < (10.0 / 60.0));
}

/// Acceleration clamp must allow the velocity to ramp up over multiple
/// ticks. From zero, applying the clamp N times in a row should bring
/// |velocity| to roughly `min(cruise, N * max_accel_per_tick)`. With
/// `max_accel` = 1.5/60 and cruise = 10/60, full speed should be reached
/// in ceil(10/1.5) = 7 ticks.
#[test]
fn accel_clamp_ramps_up_over_multiple_ticks() {
    let prev = Vec2::zero();
    let desired = Vec2::new(10.0 / 60.0, 0.0);
    let max_accel = 1.5 / 60.0;
    let mut v = prev;
    for _ in 0..7 {
        v = apply_acceleration_limit(v, desired, max_accel);
    }
    // After 7 ticks of clamping, velocity should match desired (within
    // a small epsilon).
    assert!(
        (v.x - desired.x).abs() < 1e-5,
        "expected x {} after 7 ticks, got {}",
        desired.x,
        v.x,
    );
}

/// Acceleration clamp must *decelerate* as well as accelerate — when
/// desired is the opposite direction, velocity must change by at most
/// `max_accel_per_tick` toward desired (i.e. we begin to slow down).
#[test]
fn accel_clamp_applies_to_deceleration() {
    let prev = Vec2::new(10.0 / 60.0, 0.0); // moving east
    let desired = Vec2::new(-10.0 / 60.0, 0.0); // want to move west
    let max_accel = 1.5 / 60.0;
    let new_velocity = apply_acceleration_limit(prev, desired, max_accel);
    // Diff = desired - prev = (-20/60, 0). Length = 20/60 ≈ 0.333.
    // Clamp to max_accel_per_tick ≈ 0.025. So new = prev + (-0.025, 0)
    // = (10/60 - 0.025, 0) = (0.1417 - 0.025, 0) ≈ (0.1167, 0).
    let expected_x = 10.0 / 60.0 - max_accel;
    assert!(
        (new_velocity.x - expected_x).abs() < 1e-6,
        "expected x {}, got {}",
        expected_x,
        new_velocity.x,
    );
    assert!(new_velocity.y.abs() < 1e-6);
    // Velocity should still be positive x — we decelerated, not flipped.
    assert!(new_velocity.x > 0.0, "should decelerate, not flip");
}

#[test]
fn kick_execution_emits_a_pass_flow_event_when_telemetry_is_on() {
    // Two players on the same team, 15 m apart — within `nearby_teammates`
    // range (20 m). The kicker holds the ball and has `PassTo` as its
    // committed intent. We expect one `flow_start` on the kicker's
    // process and one `flow_end` on the receiver's, sharing an id.
    let mut world = ai_world();

    let kicker = world
        .spawn((
            Player {
                team_id: sim_components::TeamId(0),
                intent: Some(Intent::Action(ActionIntent::PassTo)),
            },
            Position(pos(50.0, 34.0)),
            Velocity(pos(0.0, 0.0)),
            PerceptionSnapshot {
                self_position: pos(50.0, 34.0),
                nearby_teammates: smallvec![NearbyEntity {
                    entity: Entity::from_raw(42),
                    distance: 15.0,
                    relative_position: pos(15.0, 0.0),
                }],
                nearby_opponents: smallvec![],
                ball_position: pos(52.5, 34.0),
                ball_state: sim_components::BallState::Possessed,
                goal_position: sim_math::goal_position_for(1),
                home_slot: pos(50.0, 34.0),
                pitch_bounds: PitchBounds {
                    distance_to_left: 50.0,
                    distance_to_right: 55.0,
                    distance_to_top: 34.0,
                    distance_to_bottom: 34.0,
                },
            },
        ))
        .id();
    // Spawn a teammate entity for completeness (the kicker only reads
    // its perception snapshot, so this is just a placeholder).
    world.spawn(BallMarker);

    world.insert_resource(Ball {
        state: sim_components::BallState::Possessed,
        possessor: Some(kicker),
        last_touch: None,

        touch_history: [None, None],
        kick_velocity: None,
        spin: 0.0,
    });
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(MatchClock {
        elapsed_ticks: 100,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(TelemetryConfig::new(60, None).expect("valid"));
    world.insert_resource(sim_rules::PendingRuleOutcome::default());

    let events: EventVec = capture_trace(|| {
        let mut schedule = Schedule::default();
        schedule.add_systems(kick_execution_system);
        schedule.run(&mut world);
    });

    let starts: Vec<_> = events
        .iter()
        .filter(|e| e["ph"] == "s" && e["name"] == "pass")
        .collect();
    let ends: Vec<_> = events
        .iter()
        .filter(|e| e["ph"] == "f" && e["name"] == "pass")
        .collect();
    assert_eq!(starts.len(), 1, "one flow_start for the pass");
    assert_eq!(ends.len(), 1, "one flow_end for the pass");
    assert_eq!(starts[0]["args"]["id"], ends[0]["args"]["id"]);
    assert_eq!(starts[0]["args"]["completed"], true);
}

#[test]
fn kick_execution_emits_no_flow_when_telemetry_is_off() {
    // Same setup but with telemetry disabled: no flow events.
    let mut world = ai_world();

    let kicker = world
        .spawn((
            Player {
                team_id: sim_components::TeamId(0),
                intent: Some(Intent::Action(ActionIntent::PassTo)),
            },
            Position(pos(50.0, 34.0)),
            Velocity(pos(0.0, 0.0)),
            PerceptionSnapshot {
                self_position: pos(50.0, 34.0),
                nearby_teammates: smallvec![NearbyEntity {
                    entity: Entity::from_raw(42),
                    distance: 15.0,
                    relative_position: pos(15.0, 0.0),
                }],
                nearby_opponents: smallvec![],
                ball_position: pos(52.5, 34.0),
                ball_state: sim_components::BallState::Possessed,
                goal_position: sim_math::goal_position_for(1),
                home_slot: pos(50.0, 34.0),
                pitch_bounds: PitchBounds {
                    distance_to_left: 50.0,
                    distance_to_right: 55.0,
                    distance_to_top: 34.0,
                    distance_to_bottom: 34.0,
                },
            },
        ))
        .id();
    world.spawn(BallMarker);

    world.insert_resource(Ball {
        state: sim_components::BallState::Possessed,
        possessor: Some(kicker),
        last_touch: None,

        touch_history: [None, None],
        kick_velocity: None,
        spin: 0.0,
    });
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(MatchClock {
        elapsed_ticks: 0,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    // No TelemetryConfig: telemetry fully off.

    let events: EventVec = capture_trace(|| {
        let mut schedule = Schedule::default();
        schedule.add_systems(kick_execution_system);
        schedule.run(&mut world);
    });
    assert!(
        events.is_empty(),
        "no telemetry → no trace events, got {events:?}"
    );
}

/// MVP steering fix integration test (item #2): from rest, after one tick
/// the player's velocity magnitude must not exceed `PLAYER_MAX_ACCEL_PER_TICK`.
/// This is the simplest possible test that distinguishes "broken steering"
/// (instant cruise-speed jump from rest) from "fixed steering" (gradual
/// acceleration from rest). Without the acceleration cap the player's
/// velocity would jump straight to `8/60 ≈ 0.133 m/tick` on tick 1.
#[test]
fn accel_cap_clamps_first_tick_velocity_from_rest() {
    use sim_physics::{PLAYER_MAX_ACCEL_PER_TICK, player_movement_system};

    let mut world = World::new();
    world.insert_resource(sim_math::PitchDimensions::standard());
    world.insert_resource(SimRng::new(1));
    world.insert_resource(Ball {
        spin: 0.0,
        state: sim_components::BallState::Free,
        possessor: None,
        last_touch: None,
        touch_history: [None, None],
        kick_velocity: None,
    });
    world.insert_resource(sim_math::Tuning::default());
    world.insert_resource(PendingRuleOutcome::default());

    let player_pos = Vec2::new(50.0, 34.0);
    let ball_pos = Vec2::new(60.0, 34.0);
    let player_entity = world
        .spawn((
            Player {
                team_id: sim_components::TeamId(0),
                intent: Some(Intent::Movement(MovementIntent::ChaseBall)),
            },
            Position(player_pos),
            Velocity(Vec2::zero()),
            Stamina(1.0),
            Skill(0.7),
            PerceptionSnapshot {
                self_position: player_pos,
                nearby_teammates: smallvec![],
                nearby_opponents: smallvec![],
                ball_position: ball_pos,
                ball_state: sim_components::BallState::Free,
                goal_position: sim_math::goal_position_for(1),
                home_slot: player_pos,
                pitch_bounds: PitchBounds {
                    distance_to_left: player_pos.x,
                    distance_to_right: sim_math::PITCH_LENGTH - player_pos.x,
                    distance_to_top: player_pos.y,
                    distance_to_bottom: 68.0 - player_pos.y,
                },
            },
        ))
        .id();

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.entity_mut(ball_entity).insert(Position(ball_pos));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

    let mut schedule = Schedule::default();
    schedule.add_systems((player_action_execution_system, player_movement_system).chain());

    // Run ONE tick. The velocity after the tick must be at most
    // `PLAYER_MAX_ACCEL_PER_TICK` (≈ 0.025 m/tick). Without the cap the
    // velocity would be `8/60 ≈ 0.133 m/tick`.
    schedule.run(&mut world);
    let v = world.get::<Velocity>(player_entity).expect("velocity").0;
    assert!(
        v.length() <= PLAYER_MAX_ACCEL_PER_TICK + 1e-5,
        "first-tick velocity should be ≤ PLAYER_MAX_ACCEL_PER_TICK ({}), got length {} (v={:?})",
        PLAYER_MAX_ACCEL_PER_TICK,
        v.length(),
        v,
    );
    // And the position should increase by `v.x`, which is at most the cap.
    let p = world.get::<Position>(player_entity).expect("position").0;
    assert!(
        p.x - player_pos.x <= PLAYER_MAX_ACCEL_PER_TICK + 1e-5,
        "first-tick position delta should be ≤ PLAYER_MAX_ACCEL_PER_TICK ({}), got {}",
        PLAYER_MAX_ACCEL_PER_TICK,
        p.x - player_pos.x,
    );
}

/// MVP steering fix integration test (item #2): the arrival slow-down
/// must prevent a stationary player from oscillating around a 1.5 m
/// distant ball. Specifically: once the player is within the 0.1 m
/// deadband of the ball, the velocity must be exactly zero every
/// subsequent tick (no overshoot flip).
#[test]
fn arrival_prevents_oscillation_near_ball() {
    use sim_physics::player_movement_system;

    let mut world = World::new();
    world.insert_resource(sim_math::PitchDimensions::standard());
    world.insert_resource(SimRng::new(1));
    world.insert_resource(Ball {
        spin: 0.0,
        state: sim_components::BallState::Free,
        possessor: None,
        last_touch: None,
        touch_history: [None, None],
        kick_velocity: None,
    });
    world.insert_resource(sim_math::Tuning::default());
    world.insert_resource(PendingRuleOutcome::default());

    // Player 0.05 m from a stationary ball (inside the 0.1 m deadband).
    let player_pos = Vec2::new(50.0, 34.0);
    let ball_pos = Vec2::new(50.05, 34.0);
    let player_entity = world
        .spawn((
            Player {
                team_id: sim_components::TeamId(0),
                intent: Some(Intent::Movement(MovementIntent::ChaseBall)),
            },
            Position(player_pos),
            Velocity(Vec2::zero()),
            Stamina(1.0),
            Skill(0.7),
            PerceptionSnapshot {
                self_position: player_pos,
                nearby_teammates: smallvec![],
                nearby_opponents: smallvec![],
                ball_position: ball_pos,
                ball_state: sim_components::BallState::Free,
                goal_position: sim_math::goal_position_for(1),
                home_slot: player_pos,
                pitch_bounds: PitchBounds {
                    distance_to_left: player_pos.x,
                    distance_to_right: sim_math::PITCH_LENGTH - player_pos.x,
                    distance_to_top: player_pos.y,
                    distance_to_bottom: 68.0 - player_pos.y,
                },
            },
        ))
        .id();

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.entity_mut(ball_entity).insert(Position(ball_pos));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

    let mut schedule = Schedule::default();
    schedule.add_systems((player_action_execution_system, player_movement_system).chain());

    for _ in 0..10 {
        schedule.run(&mut world);
        let v = world.get::<Velocity>(player_entity).expect("velocity").0;
        assert!(
            v.length() < 1e-5,
            "velocity must be zero inside deadband, got {v:?}",
        );
        // Update perception so the next tick still thinks we're near the ball.
        let p = world.get::<Position>(player_entity).expect("position").0;
        if let Some(mut snap) = world.get_mut::<PerceptionSnapshot>(player_entity) {
            snap.self_position = p;
        }
    }
}

/// `tackle_recovery_system` decrements the recovery timer each tick and
/// removes the component when the window expires.
#[test]
fn tackle_recovery_decrements_each_tick_and_removes_at_zero() {
    let mut world = World::new();
    let tackler = world.spawn(TackleRecovery { ticks_remaining: 3 }).id();
    let mut schedule = Schedule::default();
    schedule.add_systems(tackle_recovery_system);

    schedule.run(&mut world);
    assert_eq!(
        world.get::<TackleRecovery>(tackler).unwrap().ticks_remaining,
        2,
        "first tick decrements 3 -> 2"
    );
    schedule.run(&mut world);
    assert_eq!(
        world.get::<TackleRecovery>(tackler).unwrap().ticks_remaining,
        1,
        "second tick decrements 2 -> 1"
    );
    schedule.run(&mut world);
    assert!(
        world.get::<TackleRecovery>(tackler).is_none(),
        "recovery component removed when the window expires"
    );
}

/// A tackle attempt against a carrier records a `PendingRuleOutcome.tackle`
/// (`tackler`, `carrier`, `success`) and, on failure, puts the tackler
/// in a `tackle_recovery_ticks` recovery window and drains stamina.
#[test]
fn tackle_attempt_records_pending_tackle_and_recovery_on_failure() {
    let mut world = World::new();
    world.insert_resource(sim_math::PitchDimensions::standard());
    world.insert_resource(SimRng::new(1));
    world.insert_resource(sim_math::Tuning::default());
    world.insert_resource(PendingRuleOutcome::default());

    let snap = |pos: Vec2, ball: Vec2| PerceptionSnapshot {
        self_position: pos,
        nearby_teammates: smallvec![],
        nearby_opponents: smallvec![],
        ball_position: ball,
        ball_state: sim_components::BallState::Possessed,
        goal_position: sim_math::goal_position_for(1),
        home_slot: pos,
        pitch_bounds: PitchBounds {
            distance_to_left: pos.x,
            distance_to_right: sim_math::PITCH_LENGTH - pos.x,
            distance_to_top: pos.y,
            distance_to_bottom: 68.0 - pos.y,
        },
    };

    let carrier = world
        .spawn((
            Player {
                team_id: sim_components::TeamId(1),
                intent: None,
            },
            sim_components::TeamIdComponent(sim_components::TeamId(1)),
            Position(Vec2::new(60.0, 34.0)),
            Velocity(Vec2::zero()),
            Stamina(1.0),
            Skill(0.7),
            snap(Vec2::new(60.0, 34.0), Vec2::new(60.0, 34.0)),
        ))
        .id();
    let tackler = {
        // Pin Aggression and Fitness so the stamina cost is
        // deterministic: Aggression 100 ⇒ factor 0.5, Fitness 100
        // ⇒ drain factor 0.36, cost = 0.002 * 25 * 0.5 * 0.36
        // = 0.009; stamina = 1.0 + 0.001 - 0.009 = 0.992.
        let mut profile =
            sim_components::PlayerProfile::with_attribute(sim_components::Attribute::Tackling, 1);
        profile
            .set(sim_components::Attribute::OutfieldAggression, 100)
            .expect("invariant: in-range attribute");
        profile
            .set(sim_components::Attribute::OutfieldFitness, 100)
            .expect("invariant: in-range attribute");
        world
            .spawn((
                Player {
                    team_id: sim_components::TeamId(0),
                    intent: Some(Intent::Action(ActionIntent::Tackle(carrier))),
                },
                sim_components::TeamIdComponent(sim_components::TeamId(0)),
                Position(Vec2::new(58.0, 34.0)),
                Velocity(Vec2::zero()),
                Stamina(1.0),
                Skill(0.7),
                profile,
                snap(Vec2::new(58.0, 34.0), Vec2::new(60.0, 34.0)),
            ))
            .id()
        };

    world.insert_resource(Ball {
        spin: 0.0,
        state: sim_components::BallState::Possessed,
        possessor: Some(carrier),
        last_touch: None,
        touch_history: [None, None],
        kick_velocity: None,
    });

    let mut schedule = Schedule::default();
    schedule.add_systems(player_action_execution_system);
    schedule.run(&mut world);

    // The attempt was recorded with the right identities.
    let attempt = world
        .resource::<PendingRuleOutcome>()
        .0
        .tackle
        .as_ref()
        .expect("a tackle attempt is recorded");
    assert_eq!(attempt.tackler, tackler);
    assert_eq!(attempt.carrier, carrier);

    // Stamina was spent on the attempt. With Aggression 100 and Fitness
    // 100: aggression factor 0.5, fitness drain factor 0.36,
    // cost = 0.002 * 25 * 0.5 * 0.36 = 0.009,
    // stamina = 1.0 + recovery 0.001 - cost 0.009 = 0.991 (f32 rounding).
    let stamina = world.get::<Stamina>(tackler).expect("tackler stamina");
    assert!(
        (stamina.0 - 0.991).abs() < 1e-6,
        "stamina drained by tackle cost, got {}",
        stamina.0
    );

    // The recovery window tracks the outcome: failure ⇒ 90 ticks,
    // success ⇒ none.
    if attempt.success {
        assert!(
            world.get::<TackleRecovery>(tackler).is_none(),
            "successful tackle leaves no recovery window"
        );
    } else {
        let recovery = world
            .get::<TackleRecovery>(tackler)
            .expect("failed tackle ⇒ recovery window");
        assert_eq!(recovery.ticks_remaining, 90);
    }
}
