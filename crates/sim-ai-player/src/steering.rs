//! Pure steering math: arrival slow-down and acceleration capping.
//!
//! These functions have no ECS dependencies and are used by
//! `player_action_execution_system` to convert resolved intents into
//! desired velocities.

use sim_components::{Intent, MovementIntent, PerceptionSnapshot};
use sim_math::Vec2;
use sim_physics::PLAYER_ARRIVAL_DECEL;

/// Resolve an intent to a target point and an action-specific speed,
/// then return the unit-direction × speed vector.
///
/// Phase F (F8): the per-variant match moved to
/// `Intent::steer_target(&self, &PerceptionSnapshot)` in
/// `sim_components::intent_dispatch`. This function is now a thin wrapper
/// that consumes the `(target, speed)` pair and converts it to a velocity
/// vector. The `> 0.1` distance deadband is preserved.
///
/// MVP fix (steering): arrival slow-down. Near the target, speed is capped
/// at `sqrt(2 * arrival_decel * distance)` so the player decelerates into
/// the target instead of leaping past it and oscillating on subsequent
/// ticks.
pub fn steer(perception: &PerceptionSnapshot, intent: &Intent) -> Vec2 {
    let Some((target, speed)) = intent.steer_target(perception) else {
        return Vec2::zero();
    };
    let decel = match intent {
        Intent::Movement(
            MovementIntent::ChaseBall | MovementIntent::Intercept,
        ) => f32::INFINITY,
        _ => PLAYER_ARRIVAL_DECEL,
    };
    steer_desired_velocity(target, perception.self_position, speed, decel)
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
pub fn steer_desired_velocity(target: Vec2, position: Vec2, speed: f32, decel: f32) -> Vec2 {
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
pub fn apply_acceleration_limit(prev: Vec2, desired: Vec2, max_accel_per_tick: f32) -> Vec2 {
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
