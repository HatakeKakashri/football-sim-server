//! Out-of-bounds detection (Law 9) and restart placement.
//!
//! `out_of_bounds_system` detects a ball that has settled at the pitch
//! boundary and emits a [`RuleEvent::OutOfBounds`] with the correct
//! restart type. `calculate_oob_restart_position` then computes the
//! placement spot for the restart (consumed by `goals::restart_system`).

use bevy_ecs::prelude::*;
use sim_components::{Ball, BallOutOfBoundsType, BallMarker, BallState, Position, RuleEvent, Velocity};
use sim_math::Vec2;

use crate::constants::{
    GOAL_Y_MAX, GOAL_Y_MIN, PITCH_LENGTH, PITCH_WIDTH,
};

/// Phase 3: Out-of-bounds detection system (Law 9).
///
/// Detects when ball has gone out of pitch boundary and needs a restart.
///
/// NOTE: Physics (`ball_physics_system`) clamps positions into [0, 105]×[0, 68] and
/// bounces balls that would cross. After physics runs, a ball that was "going out"
/// will be at the boundary with reduced (possibly near-zero) velocity.
///
/// This system detects OOB by checking if a ball is:
///   1. At/near the boundary (within `OOB_TOLERANCE`)
///   2. Has low velocity (not actively moving toward play)
///
/// This correctly handles: ball kicked out → physics bounces it back → OOB detects
/// it settled at the boundary → classifies restart type.
///
/// Does NOT reset ball position — that belongs to `restart_system`.
pub fn out_of_bounds_system(
    mut commands: Commands,
    ball_query: Query<(Entity, &Position, &Velocity), With<BallMarker>>,
    _current_tick: Res<crate::CurrentTick>,
    mut ball: ResMut<Ball>,
) {
    // Tolerance for "at boundary" detection (physics clamps to boundary)
    const OOB_TOLERANCE: f32 = 0.15;
    // Velocity threshold below which ball is considered "settled"
    const VELOCITY_THRESHOLD: f32 = 0.5;

    let Ok((ball_entity, position, velocity)) = ball_query.get_single() else {
        return;
    };

    // Skip balls already dead or in flight (unless settling from flight)
    if ball.state == BallState::Dead {
        return;
    }

    let (x, y) = (position.0.x, position.0.y);
    let speed = velocity.0.length();

    // Only detect OOB for settled balls
    if speed >= VELOCITY_THRESHOLD {
        return;
    }

    // Check if ball is at/near any boundary
    let at_left = x <= OOB_TOLERANCE;
    let at_right = x >= PITCH_LENGTH - OOB_TOLERANCE;
    let at_bottom = y <= OOB_TOLERANCE;
    let at_top = y >= PITCH_WIDTH - OOB_TOLERANCE;
    let at_goal_line = at_left || at_right;
    let at_touchline = at_bottom || at_top;

    if !at_goal_line && !at_touchline {
        return; // Not at any boundary
    }

    let oob_type = if at_goal_line && at_touchline {
        // Corner: ball at intersection of goal line and touchline.
        // Check goal area first → GoalKick; otherwise → Corner.
        if (GOAL_Y_MIN..=GOAL_Y_MAX).contains(&y) {
            BallOutOfBoundsType::GoalKick
        } else {
            BallOutOfBoundsType::Corner
        }
    } else if at_goal_line {
        // Goal line (x=0 or x=105): goal area → GoalKick, outside → Corner
        if (GOAL_Y_MIN..=GOAL_Y_MAX).contains(&y) {
            BallOutOfBoundsType::GoalKick
        } else {
            BallOutOfBoundsType::Corner
        }
    } else {
        // Touchline (y=0 or y=68): always ThrowIn
        BallOutOfBoundsType::ThrowIn
    };

    // Apply state changes
    ball.state = BallState::Dead;
    commands
        .entity(ball_entity)
        .insert(RuleEvent::OutOfBounds(oob_type));
}

/// Calculate restart position for out-of-bounds based on event type and ball position.
///
/// Coordinate system:
/// - x-axis (0 to `PITCH_LENGTH=105)`: goal line axis
/// - y-axis (0 to `PITCH_WIDTH=68)`: touchline axis
///
/// Law 9:
/// - Crossing touchline (y<0 or y>68) → Throw-in at point where ball crossed
/// - Crossing goal line in goal area (y in [26.68, 41.32]) → `GoalKick`
/// - Crossing goal line outside goal area → Corner from nearest corner arc
pub fn calculate_oob_restart_position(pos: Vec2, oob_type: BallOutOfBoundsType) -> Vec2 {
    match oob_type {
        BallOutOfBoundsType::ThrowIn => {
            // Throw-in: ball crossed touchline (y<0 or y>68)
            // Place at the touchline where ball went out (same y, x where it crossed)
            // Since physics clamped, ball is now at y=0 or y=68
            if pos.y < PITCH_WIDTH / 2.0 {
                // Bottom touchline (y=0 side)
                Vec2::new(pos.x.clamp(0.0, PITCH_LENGTH), 0.0)
            } else {
                // Top touchline (y=68 side)
                Vec2::new(pos.x.clamp(0.0, PITCH_LENGTH), PITCH_WIDTH)
            }
        }
        BallOutOfBoundsType::GoalKick => {
            // Goal kick: ball crossed goal line in goal area
            // Place at penalty spot (11m from goal line for away, 94m for home)
            if pos.x < PITCH_LENGTH / 2.0 {
                // Left goal - away team goal kick
                Vec2::new(11.0, 34.0)
            } else {
                // Right goal - home team goal kick
                Vec2::new(94.0, 34.0)
            }
        }
        BallOutOfBoundsType::Corner => {
            // Corner: ball crossed goal line outside goal area
            // Place near the appropriate corner arc
            let corner_offset = 1.0; // 1m from corner flag

            if pos.x < PITCH_LENGTH / 2.0 {
                // Left side - check if top or bottom
                if pos.y < PITCH_WIDTH / 2.0 {
                    Vec2::new(corner_offset, corner_offset)
                } else {
                    Vec2::new(corner_offset, PITCH_WIDTH - corner_offset)
                }
            } else {
                // Right side - check if top or bottom
                if pos.y < PITCH_WIDTH / 2.0 {
                    Vec2::new(PITCH_LENGTH - corner_offset, corner_offset)
                } else {
                    Vec2::new(PITCH_LENGTH - corner_offset, PITCH_WIDTH - corner_offset)
                }
            }
        }
    }
}