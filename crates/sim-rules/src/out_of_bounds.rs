//! Out-of-bounds detection (Law 9) and restart placement.
//!
//! `out_of_bounds_system` detects a ball that has settled at the pitch
//! boundary and records the correct restart type in the
//! [`PendingOutOfBounds`] resource. `calculate_oob_restart_position`
//! then computes the placement spot for the restart (consumed by
//! `goals::restart_system`).

use bevy_ecs::prelude::*;
use sim_components::{Ball, BallMarker, BallOutOfBoundsType, BallState, Position, Velocity};
use sim_math::Vec2;

use crate::constants::{GOAL_Y_MAX, GOAL_Y_MIN, PITCH_LENGTH, PITCH_WIDTH};
use crate::{OutOfBoundsEvent, PendingOutOfBounds};

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
///
/// Records the full last-touch record in the [`PendingOutOfBounds`]
/// resource so `restart_system` can resolve the OOB restart-taker
/// (awarded to the *opposite* team of the toucher per Law 9) in the
/// same tick. A resource is used instead of a `Commands` component
/// insert because deferred commands are not visible to later systems
/// until a sync point runs.
pub fn out_of_bounds_system(
    ball_query: Query<(&Position, &Velocity), With<BallMarker>>,
    current_tick: Res<crate::CurrentTick>,
    mut ball: ResMut<Ball>,
    mut pending_oob: ResMut<PendingOutOfBounds>,
) {
    // Tolerance for "at boundary" detection (physics clamps to boundary)
    const OOB_TOLERANCE: f32 = 0.15;
    // Velocity threshold below which ball is considered "settled"
    const VELOCITY_THRESHOLD: f32 = 0.5;

    let Ok((position, velocity)) = ball_query.get_single() else {
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

    // Law 9 note on classification: throw-in / corner / goal-kick is
    // **purely positional** — determined by where the ball crossed
    // the boundary (goal-line vs touchline, inside vs outside the
    // goal area). The kicker's team determines *who takes* the
    // restart (the opposite team of the last toucher), not *which
    // type* of restart is awarded. We therefore do not consult
    // `last_touch` for the classification itself; we only carry
    // it forward in the signal payload so downstream restart
    // consumers can resolve the taker team.
    //
    // Capture the full last-touch record into the event payload.
    // `Ball.last_touch` itself is preserved (M8): the record stays on
    // the ball for replay data and is overwritten by the next writer
    // (kick / possession acquisition / restart placement).
    let last_touch = ball.last_touch;
    debug_assert!(
        ball.last_touch.is_some(),
        "out_of_bounds_system fired with no `last_touch` — \
         touch-registration wiring gap"
    );
    if ball.last_touch.is_none() {
        tracing::error!(
            tick = current_tick.0,
            ball_pos = ?position.0,
            "out_of_bounds_system fired with no `last_touch` (desync); \
             skipping OOB event"
        );
        return;
    }

    // Apply state changes
    ball.state = BallState::Dead;
    pending_oob.set(OutOfBoundsEvent {
        oob_type,
        last_touch,
    });
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
