//! Goal detection (Law 10) and restart-after-goal.
//!
//! `goal_detection_system` scores a goal when the ball fully crosses the
//! goal line within the goal area. `restart_system` then places the ball
//! at the correct spot after either a goal (delayed kickoff) or an
//! out-of-bounds restart.

use bevy_ecs::prelude::*;
use sim_components::{
    Ball, BallMarker, BallState, Match, Position, RuleEvent, Velocity,
};
use sim_math::Vec2;
use sim_telemetry::TelemetryConfig;

use crate::constants::{CENTER_SPOT, GOAL_Y_MAX, GOAL_Y_MIN, PITCH_LENGTH};
use crate::out_of_bounds::calculate_oob_restart_position;
use crate::{CurrentTick, PendingRestart};

/// Phase 3: Goal detection system (Law 10).
///
/// Detects when ball fully crosses goal line within goal area,
/// increments score, emits `RuleEvent::Goal`, sets `BallState` to Dead,
/// schedules `KickoffRestart` after 60 ticks.
///
/// BUG FIX: Guards against repeat scoring by checking `BallState::Dead` first.
pub fn goal_detection_system(
    mut commands: Commands,
    ball_query: Query<(Entity, &Position), With<BallMarker>>,
    mut match_res: ResMut<Match>,
    mut ball: ResMut<Ball>,
    current_tick: Option<Res<CurrentTick>>,
    telemetry: Option<Res<TelemetryConfig>>,
) {
    // First pass: collect goal events (avoid borrow conflicts)
    let mut goal_events: Vec<(Entity, sim_components::TeamId, (u8, u8))> = Vec::new();

    let Ok((ball_entity, ball_pos)) = ball_query.get_single() else {
        return;
    };

    // BUG FIX: Do NOT score if ball is already Dead
    if ball.state != BallState::Dead {
        let in_goal_y = ball_pos.0.y >= GOAL_Y_MIN && ball_pos.0.y <= GOAL_Y_MAX;

        if in_goal_y {
            // Away goal: ball at x <= 0 (left goal)
            if ball_pos.0.x <= 0.0 {
                match_res.score.1 += 1;
                let final_score = match_res.score;
                goal_events.push((ball_entity, sim_components::TeamId(1), final_score));
                tracing::info!(
                    "GOAL scored by away team! Score: {}-{}",
                    final_score.0,
                    final_score.1
                );
                if telemetry.is_some() {
                    sim_telemetry::emit::goal(
                        current_tick.as_deref().map_or(0, |t| t.0),
                        1,
                        final_score.0,
                        final_score.1,
                    );
                }
            }
            // Home goal: ball at x >= PITCH_LENGTH (right goal)
            else if ball_pos.0.x >= PITCH_LENGTH {
                match_res.score.0 += 1;
                let final_score = match_res.score;
                goal_events.push((ball_entity, sim_components::TeamId(0), final_score));
                tracing::info!(
                    "GOAL scored by home team! Score: {}-{}",
                    final_score.0,
                    final_score.1
                );
                if telemetry.is_some() {
                    sim_telemetry::emit::goal(
                        current_tick.as_deref().map_or(0, |t| t.0),
                        0,
                        final_score.0,
                        final_score.1,
                    );
                }
            }
        }
    }

    // Second pass: apply state changes and emit events
    for (ball_entity, scorer_team, final_score) in goal_events {
        ball.state = BallState::Dead;
        commands.entity(ball_entity).insert(RuleEvent::Goal {
            scorer_team,
            score: final_score,
        });
        commands.insert_resource(PendingRestart {
            event: Some(RuleEvent::KickoffRestart),
            restart_tick: current_tick.as_deref().map_or(0, |t| t.0)
                + crate::constants::KICKOFF_RESTART_TICKS,
        });
    }
}

/// Phase 3: Restart system.
///
/// Handles ball placement after out-of-bounds and goal events.
/// - `KickoffRestart` (from goal): place at center spot after 60-tick delay
/// - `OutOfBounds`: place at correct restart spot immediately
///
/// Reads `RuleEvent` directly from entity to determine restart type.
/// `PendingRestart` resource tracks the delayed kickoff restart timing.
pub fn restart_system(
    mut commands: Commands,
    mut ball_query: Query<(Entity, &mut Position, &mut Velocity), With<BallMarker>>,
    rule_event_query: Query<(Entity, &RuleEvent)>,
    current_tick: Option<Res<CurrentTick>>,
    pending_restart: Option<Res<PendingRestart>>,
    mut ball: ResMut<Ball>,
    telemetry: Option<Res<TelemetryConfig>>,
) {
    // Handle KickoffRestart from goal (delayed by 60 ticks)
    if let Some(pr) = pending_restart
        && let Some(event) = &pr.event
        && matches!(event, RuleEvent::KickoffRestart)
        && current_tick.as_deref().map_or(0, |t| t.0) >= pr.restart_tick
    {
        // Place ball at center spot
        if let Ok((_, mut pos, mut vel)) = ball_query.get_single_mut() {
            pos.0 = Vec2::new(CENTER_SPOT.0, CENTER_SPOT.1);
            vel.0 = Vec2::zero();
            ball.state = BallState::Free;
            // Drop any pending kick velocity from a kick that was
            // decided on the same tick as the goal — carrying it
            // into the restart would inject a phantom impulse on
            // the next tick's Physics set.
            ball.kick_velocity = None;
            if telemetry.is_some() {
                sim_telemetry::emit::kickoff(current_tick.as_deref().map_or(0, |t| t.0));
            }
        }
        // Clear pending restart
        commands.remove_resource::<PendingRestart>();
    }

    // Handle OutOfBounds restarts by reading RuleEvent from entity
    let mut processed_entities: Vec<Entity> = Vec::new();

    if let Ok((ball_entity, mut pos, mut vel)) = ball_query.get_single_mut() {
        // Only process Dead balls
        if ball.state == BallState::Dead
            && let Ok((_, rule_event)) = rule_event_query.get(ball_entity)
            && let RuleEvent::OutOfBounds(oob_type) = rule_event
        {
            // Determine restart position based on OOB type and ball position
            let restart_pos = calculate_oob_restart_position(pos.0, *oob_type);
            pos.0 = restart_pos;
            vel.0 = Vec2::zero();
            ball.state = BallState::Free;
            // Drop any pending kick velocity — see goal-restart arm above.
            ball.kick_velocity = None;
            let kind = match oob_type {
                sim_components::BallOutOfBoundsType::ThrowIn => "throw_in",
                sim_components::BallOutOfBoundsType::GoalKick => "goal_kick",
                sim_components::BallOutOfBoundsType::Corner => "corner",
            };
            if telemetry.is_some() {
                sim_telemetry::emit::restart(
                    current_tick.as_deref().map_or(0, |t| t.0),
                    kind,
                    restart_pos.x,
                    restart_pos.y,
                );
            }
            processed_entities.push(ball_entity);
        }
    }

    // Remove RuleEvent component from processed entities
    for entity in processed_entities {
        commands.entity(entity).remove::<RuleEvent>();
    }
}