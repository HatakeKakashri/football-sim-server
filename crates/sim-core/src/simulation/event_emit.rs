//! Event emission systems (PRD §9).
//!
//! These systems read the rule-engine state (`Ball`, `PendingPass`,
//! `MatchClock`, the OOB/goal signals) and append events to the
//! `EventLog` resource. They are the only writers to the log; rule
//! crates (`sim-rules`) keep producing their existing
//! `RuleEvent`-style component signals, and these systems translate
//! them into the structured event log.
//!
//! ## Why here, not in `sim-rules`?
//!
//! `sim-rules` cannot depend on `sim-core` (cycle: sim-core already
//! depends on sim-rules). The event types live in `sim-core` (this
//! crate), so the writers live here too. `sim-rules` exposes the
//! bare signals (resource state changes) and this module is the
//! single funnel that turns them into wire events.
//!
//! ## Writers
//!
//! - `resolve_pass_events`: emit `pass` events from `PendingPass`
//!   whenever the ball is acquired (possession resolution) or
//!   goes OOB. Goal-driven resolution lands in a follow-up
//!   ticket (10). Clears `PendingPass` after emission.
//! - `emit_possession_change_on_acquisition`: emit
//!   `possession_change` whenever the team that owns possession
//!   changes, with cause `interception` (the only acquisition
//!   cause in scope for issue 08).
//! - `emit_possession_change_on_oob`: emit `possession_change`
//!   with cause `out_of_bounds` when the ball crosses the pitch
//!   boundary. (Goal-driven `possession_change` is in ticket 10.)

use bevy_ecs::prelude::*;
use sim_components::{Ball, BallMarker, BallState, MatchClock, Position, TeamId};
use sim_math::Vec2;
use sim_rules::{PendingOutOfBounds, PendingPass};

use super::SimulationSet;
use super::events::{
    EventLog, PossessionChangeCause, build_pass_event, build_possession_change_event,
};

/// `pass` event resolution: whenever the ball becomes
/// `Possessed` after a free state and a `PendingPass` is active,
/// resolve it. Also resolves when OOB fires.
///
/// `kick_tick` is preserved across the in-flight window; `start`
/// is the ball position at the kick; `end` is the ball position
/// at the resolution (recorded at the resolver, not at the
/// acquisition, so the coords are stable).
///
/// Runs in the `Rules` set, after `out_of_bounds_system` and
/// `possession_resolution_system` have updated the live state.
/// That order is important: possession must have been resolved
/// (so we know whether the pass succeeded) before we emit the
/// event. The OOB arm is symmetric: the OOB detector sets
/// `PendingOutOfBounds` first, and we drain it after.
///
/// `success` is `true` when the new possessor is on the same
/// team as the kicker (`Ball.possessor`'s team equals
/// `PendingPass.from_team`). For the OOB arm the pass is always
/// `success = false` and `to = None` (the ball went astray).
#[allow(
    clippy::too_many_arguments,
    reason = "Bevy system param set; splitting into multiple systems would scatter the OOB + acquisition resolution logic"
)]
pub fn resolve_pass_events(
    mut event_log: ResMut<EventLog>,
    mut pending_pass: ResMut<PendingPass>,
    pending_oob: Option<Res<PendingOutOfBounds>>,
    ball: Res<Ball>,
    ball_query: Query<&Position, With<BallMarker>>,
    clock: Res<MatchClock>,
    current_tick: Option<Res<sim_rules::CurrentTick>>,
    team_query: Query<(Entity, &sim_components::TeamIdComponent)>,
) {
    // The ball's current position is the `end` of any pass we
    // resolve here. If the entity is missing (test fixture),
    // `Vec2::zero()` is a defensive placeholder.
    let ball_pos = ball_query.iter().next().map_or(Vec2::zero(), |p| p.0);
    let tick = current_tick.map_or(0, |c| c.0);

    // OOB arm: the OOB detector set `PendingOutOfBounds` on this
    // tick. Resolve the pending pass with `to = None` and
    // `success = false`, then drain the OOB signal.
    if let Some(oob) = pending_oob
        && oob.get().is_some()
        && let Some(pass) = pending_pass.take()
    {
        let event = build_pass_event(
            event_log.next_seq,
            tick,
            &clock,
            pass.from_team,
            pass.from,
            None,
            false,
            pass.kick_tick,
            pass.kick_position,
            ball_pos,
        );
        event_log.append(event);
        // `PendingOutOfBounds` is drained by `restart_system` later
        // in the same set. Leaving the signal in place is fine.
    }

    // Acquisition arm: the ball is now Possessed and a pass was
    // in flight. Resolve it. The possessor's team is read from
    // the live `Ball.possessor` via the player query (the
    // `TeamIdComponent` is the canonical place).
    if ball.state == BallState::Possessed
        && let Some(possessor) = ball.possessor
        && let Some(pass) = pending_pass.take()
    {
        let possessor_team = team_query
            .iter()
            .find(|(e, _)| *e == possessor)
            .map_or(pass.from_team, |(_, t)| t.0);
        let success = possessor_team == pass.from_team;
        // PRD §9.3: `to` is the intended receiver, null if the
        // kick was astray. We record `intended_to` as-is. The
        // wire event will use it; if the pass was astray (caught
        // by the opponent) the field still points at the original
        // target. PRD §9.3 explicitly allows `to: nullable if
        // astray` — the "astray" interpretation is "the kick
        // didn't reach its target". We treat an interception as
        // astray (to=None) per the rule.
        let to = if success { pass.intended_to } else { None };
        let event = build_pass_event(
            event_log.next_seq,
            tick,
            &clock,
            pass.from_team,
            pass.from,
            to,
            success,
            pass.kick_tick,
            pass.kick_position,
            ball_pos,
        );
        event_log.append(event);
    }
}

/// `possession_change` emission on acquisition.
///
/// `possession_team` is the cached "team currently in possession"
/// field on the `EventLog`. On every acquisition we compare the
/// new possessor's team to the cache:
/// - first acquisition of the match: cache is `None`, no event
///   (the first pass is the kickoff placement, which is a
///   `Restart` touch, not a team change in the
///   team-vs-team sense).
/// - same team as cache: no event.
/// - different team: emit `possession_change` with cause
///   `interception` (the only acquisition cause in scope for
///   issue 08).
pub fn emit_possession_change_on_acquisition(
    mut event_log: ResMut<EventLog>,
    ball: Res<Ball>,
    team_query: Query<(Entity, &sim_components::TeamIdComponent)>,
    current_tick: Option<Res<sim_rules::CurrentTick>>,
    clock: Res<MatchClock>,
) {
    if ball.state != BallState::Possessed {
        return;
    }
    let Some(possessor) = ball.possessor else {
        return;
    };
    let new_team = team_query
        .iter()
        .find(|(e, _)| *e == possessor)
        .map(|(_, t)| t.0);
    let Some(new_team) = new_team else {
        return;
    };
    if event_log.possession_team == Some(new_team) {
        return;
    }
    // Don't emit on the first transition from None. The first
    // acquisition after a kickoff is the team already
    // established as "owning" the play via the kickoff
    // placement, not a fresh change. PRD §9.3 requires
    // possession_change to fire on team changes, so the
    // transition from "no team" → "first team" is skipped here
    // (no team change); the change from team A → team B is the
    // first event.
    let Some(from_team) = event_log.possession_team else {
        event_log.possession_team = Some(new_team);
        return;
    };
    let tick = current_tick.map_or(0, |c| c.0);
    let event = build_possession_change_event(
        event_log.next_seq,
        tick,
        &clock,
        Some(from_team),
        new_team,
        PossessionChangeCause::Interception,
        ball.last_touch,
    );
    event_log.append(event);
    event_log.possession_team = Some(new_team);
}

/// `possession_change` emission on out-of-bounds.
///
/// When the OOB detector fires on this tick, the ball's last
/// `Play` touch is the kicker (preserved per the touched-record
/// refactor). The new possessing team is the opponent of the
/// last-toucher (Law 9 restart-taker team); we emit
/// `possession_change` with cause `out_of_bounds`.
///
/// Goal-driven `possession_change` is in ticket 10; this function
/// only fires for the OOB family.
pub fn emit_possession_change_on_oob(
    mut event_log: ResMut<EventLog>,
    pending_oob: Option<Res<PendingOutOfBounds>>,
    ball: Res<Ball>,
    current_tick: Option<Res<sim_rules::CurrentTick>>,
    clock: Res<MatchClock>,
) {
    let Some(oob) = pending_oob else {
        return;
    };
    let Some(_oob_event) = oob.get() else {
        return;
    };
    let Some(last_touch) = ball.last_touch else {
        // Without a last_touch we cannot attribute the OOB; the
        // `out_of_bounds_system` debug-asserts in this case. The
        // possession_change is skipped to avoid fabricating a
        // team identity.
        return;
    };
    let from_team = last_touch.team;
    // The restart taker is the opposite of the last-toucher
    // (Law 9). Encoded as a possession_change to that team.
    let to_team = if from_team == TeamId(0) {
        TeamId(1)
    } else {
        TeamId(0)
    };
    if event_log.possession_team == Some(to_team) {
        // Defensive: if the cache already says the new team, the
        // acquisition arm should have fired. Skip.
        return;
    }
    let tick = current_tick.map_or(0, |c| c.0);
    let event = build_possession_change_event(
        event_log.next_seq,
        tick,
        &clock,
        Some(from_team),
        to_team,
        PossessionChangeCause::OutOfBounds,
        Some(last_touch),
    );
    event_log.append(event);
    event_log.possession_team = Some(to_team);
}

/// Register the event-emission systems into the simulation
/// schedule. Called from `construct::register_systems`.
pub fn register_event_systems(schedule: &mut Schedule) {
    // Pass-event resolution and possession_change emission both
    // run in the `Rules` set, after the rule writers have settled
    // their signals. The order is:
    // 1. `out_of_bounds_system` (writes `PendingOutOfBounds`)
    // 2. `possession_resolution_system` (sets `Ball.possessor`)
    // 3. `emit_possession_change_on_oob` (reads OOB signal)
    // 4. `emit_possession_change_on_acquisition` (reads
    //    `Ball.possessor`)
    // 5. `resolve_pass_events` (reads both signals to resolve
    //    the pending pass).
    schedule.add_systems(
        (
            emit_possession_change_on_oob,
            emit_possession_change_on_acquisition,
            resolve_pass_events,
        )
            .chain()
            .in_set(SimulationSet::Rules),
    );
}
