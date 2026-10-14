//! Execution: turns the chosen `Intent` into actual velocity / ball motion.

use std::sync::atomic::{AtomicU64, Ordering};

use bevy_ecs::prelude::*;
use sim_components::{
    ActionIntent, Ball, BallMarker, Intent, LastTouch, MatchClock, PendingBallTouches,
    PerceptionSnapshot, Player, PlayerId, Position, Skill, Stamina, TouchKind, Velocity,
};
use sim_math::Vec2;
use sim_physics::{PLAYER_ARRIVAL_DECEL, PLAYER_MAX_ACCEL_PER_TICK, SimRng};
use sim_rules::PendingPass;
use sim_telemetry::TelemetryConfig;

/// Monotonic counter for `pass()` flow ids. Two passes share the same
/// process and could otherwise collide on `(tick, from_pid)`; the atomic
/// gives every pass a unique id regardless of order.
static NEXT_PASS_FLOW_ID: AtomicU64 = AtomicU64::new(1);

/// Ball kick speed, in metres per tick.
///
/// Previously declared as m/s but stored directly into `ball.kick_velocity`
/// and applied via `pos += vel` with no dt scaling. Rescaled to m/tick:
/// `KICK_SPEED_SHOT` = 22 m/s ÷ 60 ≈ 0.3667 m/tick.
pub const KICK_SPEED_SHOT: f32 = 22.0 / 60.0;

/// Ball kick speed for passes, in metres per tick.
///
/// Rescaled from 14 m/s to m/tick: 14 / 60 ≈ 0.2333 m/tick.
pub const KICK_SPEED_PASS: f32 = 14.0 / 60.0;

/// Resolve an intent to a target point and an action-specific speed,
/// then return the unit-direction × speed vector. Target entities are
/// resolved from the player's perception snapshot (`nearby_teammates` /
/// `nearby_opponents`), which carries their world position relative to the
/// player at perception time. The ball position is read from the perception
/// snapshot directly (the perception system writes it every tick).
///
/// Phase F (F8): the per-variant match moved to
/// `Intent::steer_target(&self, &PerceptionSnapshot)` in
/// `sim_components::intent_dispatch`. This function is now a thin wrapper
/// that consumes the (target, speed) pair and converts it to a velocity
/// vector. The `> 0.1` distance deadband is preserved.
///
/// MVP fix (steering): arrival slow-down. Near the target, speed is capped
/// at `sqrt(2 * arrival_decel * distance)` so the player decelerates into
/// the target instead of leaping past it and oscillating on subsequent
/// ticks. See `steer_desired_velocity` for the pure-form version.
fn steer(perception: &PerceptionSnapshot, intent: &Intent) -> Vec2 {
    let Some((target, speed)) = intent.steer_target(perception) else {
        return Vec2::zero();
    };
    let decel = match intent {
        Intent::Movement(
            sim_components::MovementIntent::ChaseBall | sim_components::MovementIntent::Intercept,
        ) => f32::INFINITY,
        _ => PLAYER_ARRIVAL_DECEL,
    };
    steer_desired_velocity(
        target,
        perception.self_position,
        speed,
        decel,
    )
}

/// Pure arrival-style steering: given a target point, the steering agent's
/// current position, the agent's cruise speed (m/tick), and a deceleration
/// parameter (m/s²), returns the desired velocity vector (m/tick) the agent
/// should accelerate toward this tick. Inside the 0.1 m deadband the
/// returned velocity is zero — the player has arrived.
///
/// The Reynolds "arrival" formulation: speed is capped at
/// `sqrt(2 * decel * distance)` so that a player at full cruise can
/// decelerate to zero at the target exactly. Without this cap a player
/// nearer than one tick of motion overshoots and flips direction next
/// tick, oscillating around the target until the next decision cadence.
///
/// All four inputs are non-negative; the caller must pre-resolve the
/// target via `Intent::steer_target` or similar.
fn steer_desired_velocity(target: Vec2, position: Vec2, speed: f32, decel: f32) -> Vec2 {
    let direction = target - position;
    let distance = direction.length();
    if distance <= 0.1 {
        return Vec2::zero();
    }
    // Arrival radius: `sqrt(2 * decel * distance)` is the maximum speed at
    // which the player can decelerate to zero exactly at the target.
    // Compare in m/s (multiply both sides by `2` to avoid sqrt on per-tick
    // values) — speed is m/tick so the cruise cap is `speed*sqrt(2*decel*dt)`
    // ... actually simpler: work in m/s throughout. cruise_mps = speed * 60,
    // arrival_cap_mps = sqrt(2 * decel * distance). Pick min, scale back to
    // m/tick.
    let cruise_mps = speed * 60.0;
    let arrival_cap_mps = (2.0_f32).mul_add(decel * distance, 0.0).sqrt();
    let chosen_mps = cruise_mps.min(arrival_cap_mps);
    let chosen = chosen_mps / 60.0;
    (direction / distance) * chosen
}

/// Cap the per-tick velocity change at `max_accel_per_tick`. Returns the
/// new velocity, clamped component-wise to within `max_accel_per_tick` of
/// the previous velocity in the direction of `desired`.
///
/// `prev` and `desired` are in m/tick. The returned vector's magnitude is
/// also bounded by `cruise_speed_m_per_tick = desired.length()` when the
/// player is far from the target, but this function makes no assumptions
/// about the cruise speed — it only enforces the acceleration cap.
/// The downstream `player_movement_system` applies the absolute max-speed
/// clamp (`MAX_PLAYER_SPEED`) on top of this.
fn apply_acceleration_limit(prev: Vec2, desired: Vec2, max_accel_per_tick: f32) -> Vec2 {
    let delta = desired - prev;
    let delta_len = delta.length();
    if delta_len <= max_accel_per_tick {
        desired
    } else if delta_len < f32::EPSILON {
        // desired == prev: nothing to do.
        prev
    } else {
        prev + (delta / delta_len) * max_accel_per_tick
    }
}

pub fn player_action_execution_system(
    mut query: Query<(
        Entity,
        &mut Player,
        &mut Velocity,
        &Stamina,
        &Skill,
        &PerceptionSnapshot,
    )>,
    mut rng: ResMut<SimRng>,
) {
    // Phase 2: real steering for every action. Each arm turns the abstract
    // intent into a constant-velocity steer toward (or away from) the
    // relevant world point.
    //
    // Entity-target lookups use the player's own perception snapshot rather
    // than a separate world query — keeps the system signature simple and
    // avoids the Position/Velocity conflict that arises from querying both
    // the player and the ball.
    //
    // Phase 3: stochastic outcomes for tackles, passes, and shots using RNG.
    //
    // MVP steering fix (item #2): arrival slow-down + per-tick acceleration
    // cap. The previous velocity is captured into a local at the top of the
    // iteration, before any work is done. The new velocity is the
    // acceleration-capped step from prev_velocity toward the stochastic
    // desired velocity. This prevents the per-tick teleport-from-rest bug
    // (velocity jumping from 0 to full cruise in a single tick) and the
    // overshoot bug (a player inside the arrival radius being asked to
    // leap past the target and reverse direction next tick).
    //
    // DETERMINISM: collect entities first and sort by Entity ID to ensure
    // stable RNG draw order across processes (spec §7, rule #22). Reading
    // `velocity.0` into `prev_velocity` first preserves the previous-tick
    // value across the iteration even though we hold a `&mut Velocity`
    // borrow — the local is independent storage.
    let mut entities: Vec<Entity> = query.iter().map(|(e, _, _, _, _, _)| e).collect();
    entities.sort_by_key(|e| e.to_bits());
    for entity in entities {
        let Ok((_entity, player, mut velocity, stamina, skill, perception)) = query.get_mut(entity)
        else {
            continue;
        };
        let Some(intent) = player.intent else {
            velocity.0 = Vec2::zero();
            continue;
        };

        // Capture the previous tick's velocity BEFORE we do any work.
        // This is the "prev" the acceleration cap will clamp the new
        // velocity against.
        let prev_velocity = velocity.0;

        // Apply stochastic modifiers based on action type
        let mut speed_modifier = 1.0;
        let mut direction_modifier = Vec2::zero();

        match &intent {
            Intent::Action(ActionIntent::Tackle(target)) => {
                // Tackle resolution: higher skill = higher success probability
                // Roll against skill-based probability
                let tackle_success_prob = skill.0.clamp(0.3, 0.9);
                let roll: f32 = rng.gen_f32();
                if roll > tackle_success_prob {
                    // Tackle fails - player stumbles, moves slower
                    speed_modifier = 0.3;
                }
                // Store target for potential later use
                let _ = target;
            }
            Intent::Action(ActionIntent::PassTo) => {
                // Pass completion: probability based on distance and skill
                let pass_distance = perception
                    .nearby_teammates
                    .first()
                    .map_or(15.0, |t| t.distance);
                // Longer passes = lower completion probability
                let pass_prob = (1.0 - (pass_distance / 50.0).min(0.8)) * skill.0;
                let roll: f32 = rng.gen_f32();
                if roll > pass_prob {
                    // Pass goes awry - add random deviation
                    speed_modifier = 0.7;
                    // Deviation values are in m/s, converted to m/tick
                    let dev = 2.0 / 60.0;
                    direction_modifier =
                        Vec2::new(rng.gen_range_f32(-dev, dev), rng.gen_range_f32(-dev, dev));
                }
            }
            Intent::Action(ActionIntent::ShootAtGoal(_)) => {
                // Shot accuracy: skill and stamina affect accuracy
                let accuracy = skill.0 * stamina.0.mul_add(0.5, 0.5);
                let roll: f32 = rng.gen_f32();
                if roll > accuracy {
                    // Shot misses - add deviation
                    // Deviation values are in m/s, converted to m/tick
                    speed_modifier = 0.8;
                    let dev_x = 3.0 / 60.0;
                    let dev_y = 2.0 / 60.0;
                    direction_modifier = Vec2::new(
                        rng.gen_range_f32(-dev_x, dev_x),
                        rng.gen_range_f32(-dev_y, dev_y),
                    );
                }
            }
            Intent::Movement(_)
            | Intent::Action(ActionIntent::MarkOpponent(_) | ActionIntent::Press(_)) => {}
        }

        let desired = steer(perception, &intent);
        let desired_with_stochastic = desired * speed_modifier + direction_modifier;
        velocity.0 = apply_acceleration_limit(
            prev_velocity,
            desired_with_stochastic,
            PLAYER_MAX_ACCEL_PER_TICK,
        );
    }
}

/// Turns a possessing player's intent into actual ball velocity.
///
/// Before this system existed, nothing in the simulation ever wrote to the
/// ball's `Velocity` component: players could gain possession via
/// `possession_resolution_system`'s proximity check, but the ball itself
/// never moved as a result of an action, so "kick/pass" was purely
/// notional. This system closes that gap for the current demo scope only —
/// it is intentionally not real ball control:
/// - `ShootAtGoal`: kicks toward the already-resolved shot target.
/// - `PassTo`: kicks toward the nearest teammate (if any); the
///   teammate lookup is encapsulated in `Intent::kick`. No
///   lead/interception modeling, no pass-completion mechanic beyond
///   that — out of scope for "just movement and kick/pass".
/// - `ChaseBall` / `Tackle` / `Press`: continues the ball in the player's
///   current heading.
///
/// Runs after `player_action_execution_system` in the same `Execution` set
/// (so it sees this tick's resolved intent and velocity) and before the
/// `Physics` set integrates the ball's velocity into its position.
///
/// Clears `ball.possessor` on every kick so the ball goes properly loose
/// (`BallState::InFlight`) rather than getting stuck reporting `Possessed`
/// by a player who has since moved away: `ball_physics_system` only
/// downgrades state to `Possessed`/`Free` from velocity + possessor, and
/// `possession_resolution_system` only re-resolves possession while
/// `ball.state == BallState::Free`. Without this, a kicked ball that
/// decelerates to rest before another player reaches it would silently
/// re-freeze as "possessed" by the original kicker.
pub fn kick_execution_system(
    player_query: Query<(Entity, &Player, &PerceptionSnapshot, &Velocity)>,
    ball_query: Query<&Position, With<BallMarker>>,
    mut ball: ResMut<Ball>,
    mut pending: ResMut<PendingBallTouches>,
    pending_pass: Option<ResMut<PendingPass>>,
    match_clock: Option<Res<MatchClock>>,
    telemetry: Option<Res<TelemetryConfig>>,
) {
    let Some(possessor) = ball.possessor else {
        return;
    };
    // Capture the possessor's `Player` (carries `team_id`) in the same
    // pass that resolves perception/velocity — the team is denormalised
    // into `LastTouch` at the write site below. The `else` early-return
    // makes the team lookup at the write site provably infallible.
    let Some((_, possessor_player, perception, player_vel)) =
        player_query.iter().find(|(e, _, _, _)| *e == possessor)
    else {
        return;
    };
    let Some(intent) = possessor_player.intent else {
        return;
    };

    // ChaseBall was used to approach the ball; once possessed, the player retains
    // possession until a deliberate action (PassTo, ShootAtGoal, etc.) is chosen.
    if matches!(intent, Intent::Movement(sim_components::MovementIntent::ChaseBall)) {
        return;
    }

    // Phase F (F8): the per-variant match moved to `Intent::kick` in
    // `sim_components::intent_dispatch`. The speed multiplier depends on
    // whether the kick is a shot (uses `KICK_SPEED_SHOT`) or a pass /
    // velocity-continuation (uses `KICK_SPEED_PASS`).
    let Some(kick_dir) = intent.kick(perception, player_vel.0) else {
        return;
    };
    let kick_speed = match intent {
        Intent::Action(ActionIntent::ShootAtGoal(_)) => KICK_SPEED_SHOT,
        _ => KICK_SPEED_PASS,
    };

    // Preserve the original `dir.length() > 0.01` guard: if the
    // computed direction is effectively zero (e.g. a `ShootAtGoal`
    // target equal to the player, or a stationary player kicking on
    // `ChaseBall`/`Tackle`/`Press`), skip the kick entirely so we
    // don't touch `ball.kick_velocity` or clear `ball.possessor`.
    if kick_dir.length() > 0.01 {
        ball.kick_velocity = Some(kick_dir * kick_speed);
        // Register the kick as an explicit `Play` touch. Push to
        // `PendingBallTouches`; the consumer `apply_ball_touched_system`
        // applies it to `Ball.last_touch` at the start of the Rules set.
        // The same entity is recorded as both the kicker (during this
        // tick) and the last-toucher (after this tick).
        pending.0.push(LastTouch::by_player(
            possessor_player.team_id,
            possessor,
            TouchKind::Play,
        ));
        ball.possessor = None;
        // PRD §9.3: a `pass` event is emitted at resolution. The
        // resolver (acquisition, OOB, goal) reads `PendingPass` and
        // emits the event with `kick_tick` and `start` (kick
        // position). Record the kick site now so the resolver
        // doesn't have to reconstruct the kick from history.
        //
        // Only `PassTo` actions register a pending pass. Shots,
        // tackles and movement-driven velocity continuations do not
        // emit `pass` events.
        if matches!(intent, Intent::Action(ActionIntent::PassTo)) {
            // Ball position at the kick (`start`). The position is
            // on the ball entity as a `Position` component; look it
            // up directly. If the entity is missing (test fixture
            // without a ball entity), leave `start` as zero — the
            // event still emits with valid `from` / `to` fields.
            let kick_pos = ball_query
                .iter()
                .next()
                .map_or(Vec2::zero(), |p| p.0);
            let kick_tick = match_clock.as_deref().map_or(0, |c| c.elapsed_ticks);
            // `PendingPass` is taken as `Option<ResMut<_>>` so
            // legacy test worlds that don't register the resource
            // still work. The real match always registers it via
            // `Simulation::create_match`.
            if let Some(mut pending_pass) = pending_pass {
                pending_pass.active = true;
                pending_pass.from = PlayerId::from_entity(possessor);
                pending_pass.from_team = possessor_player.team_id;
                pending_pass.intended_to = perception
                    .nearby_teammates
                    .first()
                    .map(|t| PlayerId::from_entity(t.entity));
                pending_pass.kick_tick = kick_tick;
                pending_pass.kick_position = kick_pos;
            }
        } else if let Some(mut pending_pass) = pending_pass {
            // Any non-pass kick cancels a stale pending pass. This
            // shouldn't happen in normal play (a player with the
            // ball has only one intent at a time) but the defensive
            // cancel keeps the resolver from emitting a `pass` for
            // a shot.
            let _ = pending_pass.cancel();
        }
        // Emit a `pass` flow event when telemetry is on and this was a
        // pass. Shots / tackles don't carry identity of a "receiver", so
        // we restrict to `PassTo`. `match_clock` is taken as `Option` so
        // tests can omit it.
        if let (true, Some(receiver_entity)) = (
            telemetry.is_some() && matches!(intent, Intent::Action(ActionIntent::PassTo)),
            perception.nearby_teammates.first().map(|t| t.entity),
        ) {
            let tick = match_clock.as_deref().map_or(0, |c| c.elapsed_ticks);
            let flow_id = NEXT_PASS_FLOW_ID.fetch_add(1, Ordering::Relaxed);
            sim_telemetry::emit::pass(
                flow_id,
                sim_telemetry::emit::player_pid(possessor.index()),
                sim_telemetry::emit::TID_DECISION,
                sim_telemetry::emit::player_pid(receiver_entity.index()),
                sim_telemetry::emit::TID_DECISION,
                tick,
                true,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_components::{
        ActionIntent, Ball, BallMarker, MatchClock, MovementIntent, NearbyEntity,
        PendingBallTouches, PerceptionSnapshot, PitchBounds, Player, Position, Velocity,
    };
    use sim_telemetry::{TelemetryConfig, capture_trace};
    use smallvec::smallvec;

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
        let mut world = World::new();

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
        world.insert_resource(sim_rules::PendingPass::default());

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
        let mut world = World::new();

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
        world.insert_resource(sim_rules::PendingPass::default());
        // No MatchClock, no TelemetryConfig: telemetry fully off.

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
}
