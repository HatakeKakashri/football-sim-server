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
///   1. At/near any boundary (within `OOB_TOLERANCE`), OR
///   2. Settled inside one of the four corner zones (within `CORNER_TOLERANCE`
///      of both a goal line and a touchline).
///
/// And has low velocity (not actively moving toward play).
///
/// This correctly handles: ball kicked out → physics bounces it back → OOB detects
/// it settled at the boundary → classifies restart type.
///
/// Phase 0 fix: a ball stuck inside a corner (e.g. settled at (104, 1) after
/// multiple bounces) does not satisfy `OOB_TOLERANCE` against either boundary
/// (it sits 1 m inside the goal line and 1 m above the touchline). Without the
/// corner-zone branch below, such balls oscillate at the corner for hundreds
/// of ticks and stall the match. The corner-zone branch uses a wider tolerance
/// and emits the appropriate `Corner` / `GoalKick` so play restarts.
///
/// Does NOT reset ball position — that belongs to `restart_system`.
///
/// Records the full last-touch record in the [`PendingOutOfBounds`]
/// resource so `restart_system` can resolve the OOB restart-taker
/// (awarded to the *opposite* team of the toucher per Law 9) in the
/// same tick. A resource is used instead of a `Commands` component
/// insert because deferred commands are not visible to later systems
/// until a sync point runs.
#[allow(
    clippy::similar_names,
    reason = "the four `in_corner_tl/tr/bl/br` locals are intentionally parallel (corner-quadrant suffix)"
)]
pub fn out_of_bounds_system(
    ball_query: Query<(&Position, &Velocity), With<BallMarker>>,
    current_tick: Res<crate::CurrentTick>,
    mut ball: ResMut<Ball>,
    mut pending_oob: ResMut<PendingOutOfBounds>,
) {
    // Tolerance for "at boundary" detection (physics clamps to boundary)
    const OOB_TOLERANCE: f32 = 0.15;
    // Wider tolerance for the corner-zone detection. A ball can settle up
    // to ~1 m inside the corner after the physics bounce (damping +
    // half-velocity wall bounce leaves the ball a few ticks' worth
    // inside the pitch). `OOB_TOLERANCE = 0.15` would miss this;
    // `CORNER_TOLERANCE = 2.0` catches the stuck-corner case without
    // false-positiveing legitimate play (which is filtered out by the
    // velocity check + the `Possessed` skip below).
    const CORNER_TOLERANCE: f32 = 2.0;
    // Velocity threshold below which ball is considered "settled"
    const VELOCITY_THRESHOLD: f32 = 0.5;

    let Ok((position, velocity)) = ball_query.get_single() else {
        return;
    };

    // Skip balls already dead, in flight, or possessed by a player.
    // Possessed = a player is actively carrying / dribbling; that is
    // in-play, not out of play, regardless of where on the pitch.
    if ball.state == BallState::Dead || ball.state == BallState::Possessed {
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

    // Corner-zone check: settled ball near one of the four corners.
    let in_corner_tl = x <= CORNER_TOLERANCE && y <= CORNER_TOLERANCE;
    let in_corner_tr =
        x >= PITCH_LENGTH - CORNER_TOLERANCE && y <= CORNER_TOLERANCE;
    let in_corner_bl =
        x <= CORNER_TOLERANCE && y >= PITCH_WIDTH - CORNER_TOLERANCE;
    let in_corner_br =
        x >= PITCH_LENGTH - CORNER_TOLERANCE && y >= PITCH_WIDTH - CORNER_TOLERANCE;
    let at_corner_zone = in_corner_tl || in_corner_tr || in_corner_bl || in_corner_br;

    if !at_goal_line && !at_touchline && !at_corner_zone {
        return; // Not at any boundary and not in any corner zone
    }

    let oob_type = if at_corner_zone {
        // Corner zone: classify by y against the goal area.
        if (GOAL_Y_MIN..=GOAL_Y_MAX).contains(&y) {
            BallOutOfBoundsType::GoalKick
        } else {
            BallOutOfBoundsType::Corner
        }
    } else if at_goal_line && at_touchline {
        // Goal line + touchline (within OOB_TOLERANCE of both): classify
        // by y against the goal area.
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
