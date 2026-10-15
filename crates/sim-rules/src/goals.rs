//! Goal detection (Law 10) and restart-after-goal.
//!
//! `goal_detection_system` scores a goal when the ball fully crosses the
//! goal line within the goal area. `restart_system` then places the ball
//! at the correct spot after either a goal (delayed kickoff) or an
//! out-of-bounds restart.

use bevy_ecs::prelude::*;
use sim_components::{
    Ball, BallMarker, BallState, LastTouch, Match, PendingBallTouches, Player, Position, RuleEvent,
    Team, TeamId, TeamIdComponent, Velocity,
};
use sim_math::Vec2;
use sim_telemetry::TelemetryConfig;

use crate::constants::{CENTER_SPOT, GOAL_Y_MAX, GOAL_Y_MIN, PITCH_LENGTH};
use crate::out_of_bounds::calculate_oob_restart_position;
use crate::{
    CurrentTick, OobRestartForEmit, OobRestartResolved, OutOfBoundsEvent, PendingOutOfBounds,
    PendingRestart, team_attacking_goal,
};

/// Buffered goal event data collected in `goal_detection_system`'s first
/// pass (avoiding a borrow on `Match` / `Ball` while iterating queries).
struct GoalEvent {
    ball_entity: Entity,
    scorer_team: TeamId,
    final_score: (u8, u8),
    last_touch: Option<LastTouch>,
}

/// Phase 3: Goal detection system (Law 10).
///
/// Detects when ball fully crosses goal line within goal area,
/// increments score, emits `RuleEvent::Goal`, sets `BallState` to Dead,
/// schedules `KickoffRestart` after 60 ticks.
///
/// BUG FIX: Guards against repeat scoring by checking `BallState::Dead` first.
///
/// Scorer attribution uses **ball position and per-half attack
/// direction**: the team credited is the one whose current
/// `Team.attack_direction` points at the goal end the ball crossed
/// (see `team_attacking_goal`), so it stays correct after the
/// half-time end swap. The full last-touch record is captured in the
/// event payload for completeness but does **not** drive scorer
/// attribution.
pub fn goal_detection_system(
    mut commands: Commands,
    ball_query: Query<(Entity, &Position), With<BallMarker>>,
    team_query: Query<&Team>,
    mut match_res: ResMut<Match>,
    mut ball: ResMut<Ball>,
    current_tick: Option<Res<CurrentTick>>,
    telemetry: Option<Res<TelemetryConfig>>,
) {
    // First pass: collect goal events (avoid borrow conflicts).
    // Tuple: (ball_entity, scorer_team, final_score, last_touch).
    let mut goal_events: Vec<GoalEvent> = Vec::new();

    let Ok((ball_entity, ball_pos)) = ball_query.get_single() else {
        return;
    };

    // BUG FIX: Do NOT score if ball is already Dead
    if ball.state != BallState::Dead {
        let in_goal_y = ball_pos.0.y >= GOAL_Y_MIN && ball_pos.0.y <= GOAL_Y_MAX;
        // A goal requires the ball to cross the goal line inside the
        // goal mouth. Gate the pre-touch guard on that condition so a
        // ball merely sitting in the goal-mouth y-band (e.g. the
        // kickoff placement at the center spot, y = 34, where
        // `last_touch` is legitimately `None` before the first
        // lifecycle placement) does not trip it.
        let crosses_goal_line = in_goal_y && (ball_pos.0.x <= 0.0 || ball_pos.0.x >= PITCH_LENGTH);

        if crosses_goal_line {
            // Pre-touch guard (spec §E): a goal is only detected after
            // a touch has been registered — kickoff placement always
            // writes one before play begins, and OOB/goal preserve the
            // record (M8). `None` here means a wiring gap; bail
            // without scoring rather than emit a goal with no touch
            // attribution (debug builds assert).
            debug_assert!(
                ball.last_touch.is_some(),
                "goal_detection_system fired with no `last_touch` — \
                 touch-registration wiring gap"
            );
            if ball.last_touch.is_none() {
                tracing::error!(
                    tick = current_tick.as_deref().map_or(0, |t| t.0),
                    ball_pos = ?ball_pos.0,
                    "goal_detection_system fired with no `last_touch` (desync); \
                     skipping goal"
                );
                return;
            }
            // The ball crossed the left goal (x <= 0) or the right goal
            // (x >= PITCH_LENGTH); the scorer is whichever team is
            // attacking that end in the current half.
            let scorer_team = team_attacking_goal(&team_query, ball_pos.0.x >= PITCH_LENGTH);
            if scorer_team == TeamId(0) {
                match_res.score.0 += 1;
            } else {
                match_res.score.1 += 1;
            }
            let final_score = match_res.score;
            goal_events.push(GoalEvent {
                ball_entity,
                scorer_team,
                final_score,
                // Capture the full record into the event payload
                // (`Ball.last_touch` itself is preserved; M8).
                last_touch: ball.last_touch,
            });
            tracing::info!(
                "GOAL scored by team {}! Score: {}-{}",
                scorer_team.0,
                final_score.0,
                final_score.1
            );
            if telemetry.is_some() {
                sim_telemetry::emit::goal(
                    current_tick.as_deref().map_or(0, |t| t.0),
                    scorer_team.0,
                    final_score.0,
                    final_score.1,
                );
            }
        }
    }

    // Second pass: apply state changes and emit events
    for event in goal_events {
        ball.state = BallState::Dead;
        // `Ball.last_touch` is preserved (M8): the record stays on
        // the ball for replay data and is overwritten by the next
        // writer (kick / possession acquisition / restart placement).
        commands.entity(event.ball_entity).insert(RuleEvent::Goal {
            scorer_team: event.scorer_team,
            score: event.final_score,
            last_touch: event.last_touch,
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
/// - out-of-bounds: place at correct restart spot immediately
///
/// Takes the out-of-bounds signal from the [`PendingOutOfBounds`]
/// resource written by `out_of_bounds_system` earlier in the same tick.
/// `PendingRestart` resource tracks the delayed kickoff restart timing.
///
/// Registers the restart as a `TouchKind::Restart` touch (pushed to
/// `PendingBallTouches`, applied to `Ball.last_touch` by the consumer
/// at the start of the next Rules set; Law 9 / Law 10 restart
/// placement). The
/// `restart_taker` is resolved from the event payload — for OOB it is
/// the team opposite of the last toucher, for kickoff-after-goal it is
/// the team that conceded the goal.
#[allow(
    clippy::too_many_arguments,
    reason = "Bevy system param set: 9 standard params is the project norm for restart-time systems (construct.rs:37 establishes the ceiling)"
)]
pub fn restart_system(
    mut commands: Commands,
    mut ball_query: Query<(&mut Position, &mut Velocity), With<BallMarker>>,
    player_query: Query<(Entity, &TeamIdComponent), With<Player>>,
    current_tick: Option<Res<CurrentTick>>,
    pending_restart: Option<Res<PendingRestart>>,
    mut ball: ResMut<Ball>,
    mut pending_touches: ResMut<PendingBallTouches>,
    telemetry: Option<Res<TelemetryConfig>>,
    mut pending_oob: ResMut<PendingOutOfBounds>,
    mut oob_for_emit: Option<ResMut<OobRestartForEmit>>,
) {
    // Handle KickoffRestart from goal (delayed by 60 ticks)
    if let Some(pr) = pending_restart
        && let Some(event) = &pr.event
        && matches!(event, RuleEvent::KickoffRestart)
        && current_tick.as_deref().map_or(0, |t| t.0) >= pr.restart_tick
    {
        // Place ball at center spot
        if let Ok((mut pos, mut vel)) = ball_query.get_single_mut() {
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
            // `first_player_on_team` returns `Entity::PLACEHOLDER`
            // when the team has no players (sparse test worlds);
            // `from_taker` collapses the placeholder case to a
            // team-only touch so the check lives in one place.
            let touch = sim_components::LastTouch::from_taker(
                conceded_team,
                taker,
                sim_components::TouchKind::Restart,
            );
            pending_touches.0.push(touch);
            if telemetry.is_some() {
                sim_telemetry::emit::kickoff(current_tick.as_deref().map_or(0, |t| t.0));
            }
        }
        // Clear pending restart
        commands.remove_resource::<PendingRestart>();
    }

    // Handle out-of-bounds restarts from the same-tick signal. Only
    // Dead balls are restarted; the signal is taken (cleared) once used.
    if ball.state == BallState::Dead
        && let Some(OutOfBoundsEvent {
            oob_type,
            last_touch,
        }) = pending_oob.take()
        && let Ok((mut pos, mut vel)) = ball_query.get_single_mut()
    {
        // The ball's current position IS the exit point: the OOB
        // detector fired after physics clamped the ball to the
        // boundary, so `pos.0` is the clamped "where the ball
        // crossed" coordinate. Capture it before moving the ball.
        let exit_point = pos.0;
        // Determine restart position based on OOB type and ball position
        let restart_pos = calculate_oob_restart_position(pos.0, oob_type);
        pos.0 = restart_pos;
        vel.0 = Vec2::zero();
        ball.state = BallState::Free;
        // Drop any pending kick velocity — see goal-restart arm above.
        ball.kick_velocity = None;
        // Law 9: the restart taker is the team opposite of the
        // last toucher. Find any player on that team to record
        // as the taker identity.
        //
        // `last_touch` may be `None` only when the ball went out
        // without ever being touched since the last restart — a
        // wiring gap that `out_of_bounds_system` asserts against
        // in debug builds. In that case the taker team is
        // unknown: the record falls back to `TeamId(0)` with
        // `player: None` rather than inventing a taker, and the
        // desync is flagged via `tracing::error!` in release
        // builds too.
        let (taker_team, taker) = last_touch.as_ref().map_or_else(
            || {
                tracing::error!(
                    tick = current_tick.as_deref().map_or(0, |t| t.0),
                    "restart_system OOB arm: last_touch is None (touch \
                     entity desync); defaulting to home"
                );
                (TeamId(0), Entity::PLACEHOLDER)
            },
            |last| {
                // Opposite team: 0 ↔ 1.
                let team = if last.team == TeamId(0) {
                    TeamId(1)
                } else {
                    TeamId(0)
                };
                (team, first_player_on_team(&player_query, team))
            },
        );
        // `first_player_on_team` returns `Entity::PLACEHOLDER`
        // when the team has no players (sparse test worlds);
        // `from_taker` collapses the placeholder case to a
        // team-only touch so the check lives in one place.
        let touch = sim_components::LastTouch::from_taker(
            taker_team,
            taker,
            sim_components::TouchKind::Restart,
        );
        pending_touches.0.push(touch);
        // Stash the resolved data so the event emitter
        // (sim-core) can write `out_of_bounds` and `restart`
        // back-to-back with the correct fields (PRD §9.2, ticket
        // 09). The emitter runs in the same `Rules` set after this
        // system, then clears the resource.
        if let Some(ref mut emit) = oob_for_emit {
            emit.0 = Some(OobRestartResolved {
                oob_type,
                exit_point,
                last_touch,
                taker_team,
                taker,
                restart_pos,
            });
        }
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
        if t.0 == team_id && best.is_none_or(|b| e.to_bits() < b.to_bits()) {
            best = Some(e);
        }
    }
    best.unwrap_or(Entity::PLACEHOLDER)
}
