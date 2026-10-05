//! Goal detection (Law 10) and restart-after-goal.
//!
//! `goal_detection_system` scores a goal when the ball fully crosses the
//! goal line within the goal area. `restart_system` then places the ball
//! at the correct spot after either a goal (delayed kickoff) or an
//! out-of-bounds restart.

use bevy_ecs::prelude::*;
use sim_components::{
    Ball, BallMarker, BallState, Match, Player, Position, RuleEvent, TeamId, TeamIdComponent,
    Velocity,
};
use sim_math::Vec2;
use sim_telemetry::TelemetryConfig;

use crate::constants::{CENTER_SPOT, GOAL_Y_MAX, GOAL_Y_MIN, PITCH_LENGTH};
use crate::out_of_bounds::calculate_oob_restart_position;
use crate::{CurrentTick, PendingRestart};

/// Buffered goal event data collected in `goal_detection_system`'s first
/// pass (avoiding a borrow on `Match` / `Ball` while iterating queries).
struct GoalEvent {
    ball_entity: Entity,
    scorer_team: TeamId,
    final_score: (u8, u8),
    last_touch_team: Option<TeamId>,
}

/// Phase 3: Goal detection system (Law 10).
///
/// Detects when ball fully crosses goal line within goal area,
/// increments score, emits `RuleEvent::Goal`, sets `BallState` to Dead,
/// schedules `KickoffRestart` after 60 ticks.
///
/// BUG FIX: Guards against repeat scoring by checking `BallState::Dead` first.
///
/// Scorer attribution uses **ball position** (x ≤ 0 → away, x ≥
/// `PITCH_LENGTH` → home). The last toucher's team is recorded in the
/// event payload for completeness but does **not** drive scorer
/// attribution.
pub fn goal_detection_system(
    mut commands: Commands,
    ball_query: Query<(Entity, &Position), With<BallMarker>>,
    player_query: Query<(Entity, &TeamIdComponent), With<Player>>,
    mut match_res: ResMut<Match>,
    mut ball: ResMut<Ball>,
    current_tick: Option<Res<CurrentTick>>,
    telemetry: Option<Res<TelemetryConfig>>,
) {
    // First pass: collect goal events (avoid borrow conflicts).
    // Tuple: (ball_entity, scorer_team, final_score, last_touch_team).
    let mut goal_events: Vec<GoalEvent> = Vec::new();

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
                goal_events.push(GoalEvent {
                    ball_entity,
                    scorer_team: TeamId(1),
                    final_score,
                    last_touch_team: ball.last_touch.as_ref().and_then(|t| {
                        player_query.get(t.entity).ok().map(|(_, team)| team.0)
                    }),
                });
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
                goal_events.push(GoalEvent {
                    ball_entity,
                    scorer_team: TeamId(0),
                    final_score,
                    last_touch_team: ball.last_touch.as_ref().and_then(|t| {
                        player_query.get(t.entity).ok().map(|(_, team)| team.0)
                    }),
                });
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
    for event in goal_events {
        ball.state = BallState::Dead;
        // Clear the last touch — the delayed kickoff will re-register
        // it as `kind = Restart` when the ball is placed at the
        // center spot.
        ball.last_touch = None;
        commands.entity(event.ball_entity).insert(RuleEvent::Goal {
            scorer_team: event.scorer_team,
            score: event.final_score,
            last_touch_team: event.last_touch_team,
        });
        commands.insert_resource(PendingRestart {
            event: Some(RuleEvent::KickoffRestart),
            restart_tick: current_tick.as_deref().map_or(0, |t| t.0)
                + crate::constants::KICKOFF_RESTART_TICKS,
            // Law 8 / Law 10: the post-goal kickoff is taken by the
            // team that *conceded* the goal. The conceded team is
            // the opposite of `scorer_team`. Restart consumers read
            // this field to pick the kickoff taker correctly.
            scorer_team: Some(event.scorer_team),
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
///
/// Registers the restart as a `TouchKind::Restart` touch on the ball
/// (Law 9 / Law 10 restart placement; `offside_detection_system` ignores
/// it). The `restart_taker` is resolved from the event payload — for
/// OOB it is the team opposite of the last toucher, for kickoff-after-
/// goal it is the team that conceded the goal.
#[allow(
    clippy::too_many_arguments,
    reason = "Bevy system param set: 8 standard params is the project norm for restart-time systems (construct.rs:37 establishes the ceiling)"
)]
pub fn restart_system(
    mut commands: Commands,
    mut ball_query: Query<(Entity, &mut Position, &mut Velocity), With<BallMarker>>,
    rule_event_query: Query<(Entity, &RuleEvent)>,
    player_query: Query<(Entity, &TeamIdComponent), With<Player>>,
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
            // Law 8 / Law 10: the kickoff is taken by the team that
            // *conceded* the goal — the opposite of `scorer_team`.
            // Goal detection writes `scorer_team` onto
            // `PendingRestart`; if it's missing (defensive), fall
            // back to home (the canonical default until half-time
            // swap lands).
            let conceded_team = pr.scorer_team.map_or(TeamId(0), |scorer| {
                if scorer == TeamId(0) {
                    TeamId(1)
                } else {
                    TeamId(0)
                }
            });
            let taker = first_player_on_team(&player_query, conceded_team);
            ball.last_touch = Some(sim_components::LastTouch {
                entity: taker,
                kind: sim_components::TouchKind::Restart,
            });
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
            && let RuleEvent::OutOfBounds {
                oob_type,
                last_touch_team,
            } = rule_event
        {
            // Determine restart position based on OOB type and ball position
            let restart_pos = calculate_oob_restart_position(pos.0, *oob_type);
            pos.0 = restart_pos;
            vel.0 = Vec2::zero();
            ball.state = BallState::Free;
            // Drop any pending kick velocity — see goal-restart arm above.
            ball.kick_velocity = None;
            // Law 9: the restart taker is the team opposite of the
            // last toucher. Find any player on that team to record
            // as the taker identity.
            //
            // `last_touch_team` may be `None` only when the toucher
            // entity was stale by the time `out_of_bounds_system`
            // resolved the team — a real desync, not a normal
            // state. We propagate the `None` to the Restart touch
            // entity too (via `Entity::PLACEHOLDER`) rather than
            // silently defaulting to a team.
            let taker_entity = last_touch_team.as_ref().map_or(Entity::PLACEHOLDER, |last| {
                // Opposite team: 0 ↔ 1.
                let taker_team = if *last == TeamId(0) {
                    TeamId(1)
                } else {
                    TeamId(0)
                };
                first_player_on_team(&player_query, taker_team)
            });
            ball.last_touch = Some(sim_components::LastTouch {
                entity: taker_entity,
                kind: sim_components::TouchKind::Restart,
            });
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

/// Find the first player (by entity id order) on `team_id`.
/// Returns `Entity::PLACEHOLDER` if the team has no players in the
/// query (e.g. a test world with no `TeamIdComponent` matches).
fn first_player_on_team(
    player_query: &Query<(Entity, &TeamIdComponent), With<Player>>,
    team_id: TeamId,
) -> Entity {
    let mut best: Option<Entity> = None;
    for (e, t) in player_query.iter() {
        if t.0 == team_id
            && best.is_none_or(|b| e.to_bits() < b.to_bits())
        {
            best = Some(e);
        }
    }
    best.unwrap_or(Entity::PLACEHOLDER)
}