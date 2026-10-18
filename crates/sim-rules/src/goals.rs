//! Goal detection (Law 10) and restart-after-goal.
//!
//! `goal_detection_system` scores a goal when the ball fully crosses the
//! goal line within the goal area. `restart_system` then places the ball
//! at the correct spot after either a goal (delayed kickoff) or an
//! out-of-bounds restart.

use bevy_ecs::prelude::*;
use sim_components::{
    Ball, BallMarker, BallState, LastTouch, Match, PendingBallTouches, Player, Position, Referee,
    RuleEvent, StoppageEvent, Team, TeamId, TeamIdComponent, Velocity,
};
use sim_math::Vec2;
use sim_telemetry::TelemetryConfig;

use crate::constants::{CENTER_SPOT, GOAL_Y_MAX, GOAL_Y_MIN, PITCH_LENGTH};
use crate::out_of_bounds::calculate_oob_restart_position;
use crate::referee::OOB_RESTART_STOPPAGE_REASON;
use crate::{
    CurrentTick, GoalForEmit, GoalResolved, KickoffForEmit, KickoffResolved, OobRestartForEmit,
    OobRestartResolved, OutOfBoundsEvent, PendingOutOfBounds, PendingRestart, team_attacking_goal,
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
#[allow(
    clippy::too_many_arguments,
    reason = "Bevy system param set: goal detection needs ball, teams, match, touch state, tick, telemetry and the emit slot; splitting would scatter Law 10 across systems"
)]
pub fn goal_detection_system(
    mut commands: Commands,
    ball_query: Query<(Entity, &Position), With<BallMarker>>,
    team_query: Query<&Team>,
    mut match_res: ResMut<Match>,
    mut ball: ResMut<Ball>,
    current_tick: Res<CurrentTick>,
    telemetry: Option<Res<TelemetryConfig>>,
    mut goal_for_emit: ResMut<GoalForEmit>,
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
                    tick = current_tick.0,
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
            // Stash the resolved data so the `sim-core` goal emitter
            // can write the `goal` event (with scorer / own-goal /
            // assister attribution per PRD §8.5), resolve the
            // in-flight pass, and emit the `possession_change` with
            // cause `goal`. The emitter runs in the same `Rules` set
            // after this system, then drains the resource.
            goal_for_emit.0 = Some(GoalResolved {
                scorer_team,
                score: final_score,
                last_touch: ball.last_touch,
                prev_touch: ball.touch_history[0],
                older_touch: ball.touch_history[1],
                goal_tick: current_tick.0,
                ball_pos: ball_pos.0,
            });
            tracing::info!(
                "GOAL scored by team {}! Score: {}-{}",
                scorer_team.0,
                final_score.0,
                final_score.1
            );
            if telemetry.is_some() {
                sim_telemetry::emit::goal(
                    current_tick.0,
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
            restart_tick: current_tick.0
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
    reason = "Bevy system param set: 11 standard params is the project norm for restart-time systems (construct.rs:37 establishes the ceiling)"
)]
#[allow(
    clippy::too_many_lines,
    reason = "two restart arms (goal kickoff + OOB) plus their emit-slot writes; splitting would scatter Law 8/9/10 placement across systems"
)]
#[allow(
    clippy::type_complexity,
    reason = "Bevy Query signatures: the position-bearing player query carries (Entity, &TeamIdComponent, &Position) plus With<Player>/Without<BallMarker> filters; extracting a type alias adds noise without clarifying intent"
)]
pub fn restart_system(
    mut commands: Commands,
    mut ball_query: Query<(&mut Position, &mut Velocity), With<BallMarker>>,
    // `Without<BallMarker>` keeps the queries disjoint: `ball_query`
    // already borrows the ball entity's `Position` mutably, so a player
    // query that also matches `BallMarker` would conflict.
    player_query: Query<
        (Entity, &TeamIdComponent, &sim_components::Position),
        (With<Player>, Without<BallMarker>),
    >,
    // Player-only (no Position) fallback query for sparse test worlds
    // that spawn `Player` entities without a `Position` component. The
    // Bevy query system treats each `Query` parameter as independent
    // (disjoint-filter rules don't apply), so this can coexist with
    // `player_query` above.
    any_player_query: Query<(Entity, &TeamIdComponent), With<Player>>,
    current_tick: Res<CurrentTick>,
    pending_restart: Option<Res<PendingRestart>>,
    mut ball: ResMut<Ball>,
    mut pending_touches: ResMut<PendingBallTouches>,
    telemetry: Option<Res<TelemetryConfig>>,
    mut pending_oob: ResMut<PendingOutOfBounds>,
    mut oob_for_emit: ResMut<OobRestartForEmit>,
    mut kickoff_for_emit: ResMut<KickoffForEmit>,
    mut referee_query: Query<&mut Referee>,
) {
    let tick = current_tick.0;
    // Handle KickoffRestart from goal (delayed by 60 ticks). The
    // resource is optional: `commands.remove_resource::<PendingRestart>()`
    // clears it after the kickoff settles, and the next tick must not
    // panic waiting for it to come back. `goal_detection_system` (in the
    // same `Rules` chain as this system) inserts the resource again on
    // the next goal, which becomes visible after the deferred commands
    // flush at the end of that tick.
    let Some(pending_restart) = pending_restart else {
        return;
    };
    if let Some(event) = &pending_restart.event
        && matches!(event, RuleEvent::KickoffRestart)
        && current_tick.0 >= pending_restart.restart_tick
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
            let conceded_team = pending_restart.scorer_team.map_or(TeamId(0), |scorer| {
                if scorer == TeamId(0) {
                    TeamId(1)
                } else {
                    TeamId(0)
                }
            });
            let taker = first_player_on_team(&any_player_query, conceded_team);
            // `first_player_on_team` returns `Entity::PLACEHOLDER`
            // when the team has no players (sparse test worlds);
            // `from_taker` collapses the placeholder case to a
            // team-only touch so the check lives in one place.
            let touch = sim_components::LastTouch::from_taker(
                conceded_team,
                taker,
                sim_components::TouchKind::Restart,
            )
            .with_tick(tick);
            pending_touches.0.push(touch);
            // Stash the resolved kickoff so the `sim-core` emitter
            // can write the `kickoff` event (`reason = after_goal`).
            // The emitter runs in the same `Rules` set after this
            // system, then drains the resource.
            kickoff_for_emit.0 = Some(KickoffResolved {
                taker_team: conceded_team,
                taker,
                at: Vec2::new(CENTER_SPOT.0, CENTER_SPOT.1),
            });
            if telemetry.is_some() {
                sim_telemetry::emit::kickoff(current_tick.0);
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
        // Law-correct taker team = opposite of last toucher. Used for the
        // event-envelope `team` (PRD §9.2, Law 17 — the restart belongs
        // to the defending team). Always recorded; the wire invariant
        // `envelope.team = opposite(last_touch.team)` holds regardless
        // of which player actually takes the kick.
        let law_taker_team = last_touch.as_ref().map_or(TeamId(0), |last| {
            if last.team == TeamId(0) {
                TeamId(1)
            } else {
                TeamId(0)
            }
        });
        // Resolve the entity that actually takes the kick. Falls
        // through a sequence of fallbacks when the law-correct team
        // has no reachable player at the restart spot:
        //   1. nearest player on the law-correct team (with or
        //      without a position; `nearest_player_to` falls back
        //      internally to `first_player_on_team` when no entity
        //      carries `Position`)
        //   2. if that nearest player is too far from the restart
        //      spot (corner kick to a defender 100 m away), the kick
        //      cannot be taken and the match stalls with the ball
        //      parked at the flag. Hand the kick to whoever is
        //      closest from either team so play restarts. This
        //      deviates from Law 17 (corner kick goes to the
        //      defending team); it is gated on the law-correct team's
        //      nearest player having a position AND being too far
        //      away.
        //   3. `Entity::PLACEHOLDER` only if no player on either
        //      team can be identified at all.
        let taker = last_touch.as_ref().map_or(Entity::PLACEHOLDER, |_last| {
            const RESTART_REACH_M: f32 = 15.0;
            let nearest_on_law_team = nearest_player_to(
                &player_query,
                &any_player_query,
                law_taker_team,
                restart_pos,
            );
            // The position-bearing query returns the law team's
            // nearest player only when that entity has a `Position`
            // component. Without a position we can't measure distance
            // to the restart spot, so assume "reachable" — the test
            // worlds that omit `Position` would otherwise fall through
            // to the other team and pick the wrong entity.
            let reachable_check = player_query
                .get(nearest_on_law_team)
                .ok()
                .map(|(_, _, pos)| (pos.0 - restart_pos).length() <= RESTART_REACH_M);
            match reachable_check {
                // No position on the nearest-on-law-team player:
                // cannot measure distance. Keep that player (it is
                // the right team per Law 9).
                //
                // Within reach: kick can actually be taken.
                None | Some(true) => nearest_on_law_team,
                // Out of reach and reachable information is sound:
                // hand the kick to whoever on either team is
                // closest. Phase 0 stall-break.
                Some(false) => {
                    let other_team = if law_taker_team == TeamId(0) {
                        TeamId(1)
                    } else {
                        TeamId(0)
                    };
                    let nearest_other = nearest_player_to(
                        &player_query,
                        &any_player_query,
                        other_team,
                        restart_pos,
                    );
                    if nearest_other == Entity::PLACEHOLDER {
                        // No player on either team has a position
                        // (sparse test world); keep the law-correct
                        // entity so existing assertions on
                        // `restart.taker.is_some()` continue to hold.
                        nearest_on_law_team
                    } else {
                        nearest_other
                    }
                }
            }
        });
        // `first_player_on_team` returns `Entity::PLACEHOLDER`
        // when the team has no players (sparse test worlds);
        // `from_taker` collapses the placeholder case to a
        // team-only touch so the check lives in one place. Use
        // `law_taker_team` here too — the touch record is the
        // seed of `Ball.last_touch`, which subsequent OOB events
        // would otherwise classify by the fallback team, breaking
        // the wire invariant.
        let touch = sim_components::LastTouch::from_taker(
            law_taker_team,
            taker,
            sim_components::TouchKind::Restart,
        )
        .with_tick(tick);
        pending_touches.0.push(touch);
        // Stash the resolved data so the event emitter
        // (sim-core) can write `out_of_bounds` and `restart`
        // back-to-back with the correct fields (PRD §9.2, ticket
        // 09). The emitter runs in the same `Rules` set after this
        // system, then clears the resource. `taker_team` is always
        // the law-correct team (opposite of the last toucher) so the
        // wire invariant `envelope.team = opposite(last_touch.team)`
        // holds regardless of which entity actually took the kick.
        oob_for_emit.0 = Some(OobRestartResolved {
            oob_type,
            exit_point,
            last_touch,
            taker_team: law_taker_team,
            taker,
            restart_pos,
        });
        // PRD §8.4: the OOB restart itself counts toward stoppage. The
        // ball is `Dead` for less than a full tick here (detection and
        // restart settle in the same tick), so `stoppage_tracking_system`
        // would observe nothing — credit one stoppage tick explicitly.
        // Goal celebrations need no such credit: the ball stays `Dead`
        // through the 60-tick kickoff delay and accrues tick-by-tick.
        for mut referee in &mut referee_query {
            referee.stoppage_events.push(StoppageEvent {
                tick,
                reason: OOB_RESTART_STOPPAGE_REASON.to_string(),
            });
        }
        let kind = match oob_type {
            sim_components::BallOutOfBoundsType::ThrowIn => "throw_in",
            sim_components::BallOutOfBoundsType::GoalKick => "goal_kick",
            sim_components::BallOutOfBoundsType::Corner => "corner",
        };
        if telemetry.is_some() {
            sim_telemetry::emit::restart(
                current_tick.0,
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
    any_player_query: &Query<(Entity, &TeamIdComponent), With<Player>>,
    team_id: TeamId,
) -> Entity {
    let mut best: Option<Entity> = None;
    for (e, t) in any_player_query.iter() {
        if t.0 == team_id && best.is_none_or(|b| e.to_bits() < b.to_bits()) {
            best = Some(e);
        }
    }
    best.unwrap_or(Entity::PLACEHOLDER)
}

/// Find the player on `team_id` closest to `target` (Euclidean). Falls
/// back to `first_player_on_team` if no player on `team_id` carries
/// `Position` (sparse test worlds) and to `Entity::PLACEHOLDER` if the
/// team is empty. Ties broken by lowest entity id (deterministic).
#[allow(
    clippy::type_complexity,
    reason = "Bevy Query reference parameter: positional query type mirrors the system signature (see restart_system)"
)]
fn nearest_player_to(
    player_query: &Query<
        (Entity, &TeamIdComponent, &sim_components::Position),
        (With<Player>, Without<BallMarker>),
    >,
    any_player_query: &Query<(Entity, &TeamIdComponent), With<Player>>,
    team_id: TeamId,
    target: Vec2,
) -> Entity {
    let mut best: Option<(Entity, f32)> = None;
    for (e, t, pos) in player_query.iter() {
        if t.0 != team_id {
            continue;
        }
        let d = (pos.0 - target).length_squared();
        match best {
            Some((_, bd)) if d > bd => {}
            Some((be, _)) => {
                if e.to_bits() < be.to_bits() {
                    best = Some((e, d));
                }
            }
            None => best = Some((e, d)),
        }
    }
    match best {
        Some((e, _)) => e,
        // Sparse worlds omit `Position` from their player entities, so
        // the position-bearing query finds no candidates. Fall back to
        // a Player-only query to keep the taker-record non-empty.
        None => first_player_on_team(any_player_query, team_id),
    }
}
