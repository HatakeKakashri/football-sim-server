//! Event emission systems (PRD §9).
//!
//! These systems read the rule-engine state (`Ball`, `PendingRuleOutcome`,
//! `MatchClock`) and append events to the `EventLog` resource. They are
//! the only writers to the log; rule crates (`sim-rules`) keep producing
//! their existing `RuleEvent`-style component signals, and these systems
//! translate them into the structured event log.
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
//! - `resolve_pass_events`: emit `pass` events from `RuleOutcome.pass`
//!   whenever the ball is acquired (possession resolution), goes OOB,
//!   or scores a goal. Drains `RuleOutcome.pass` and clears
//!   `RuleOutcome.goal` last (it is the final reader in the chain).
//! - `resolve_shot_events`: emit `shot` events from `RuleOutcome.shot`
//!   on the same three resolutions, plus a companion `save` event
//!   when an opposing goalkeeper makes the stop.
//! - `emit_goal_events`: emit `goal` (scorer / own-goal / assister
//!   per PRD §8.5) plus `possession_change` with cause `goal`.
//! - `emit_kickoff_restart_events`: emit `kickoff` with
//!   `reason = after_goal` for post-goal restarts.
//! - `emit_possession_change_on_acquisition` /
//!   `..._on_oob`: emit `possession_change` with cause
//!   `interception` / `out_of_bounds`.
//! - `emit_oob_and_restart_events`: emit the `out_of_bounds` +
//!   `restart` pair.
//!
//! Lifecycle transitions (`kickoff` on half starts, `half_time`,
//! `full_time`) are emitted directly by `apply_transition`, which
//! already owns the world.

use bevy_ecs::prelude::*;
use sim_components::{
    Attribute, Ball, BallMarker, BallOutOfBoundsType, BallState, KickoffReason, MatchClock,
    PlayerId, PlayerProfile, Position, Role, RoleComponent, SaveKind, ShotOutcome, TeamId,
    TeamIdComponent, Velocity,
};
use sim_math::{Tuning, Vec2};
use sim_physics::SimRng;
use sim_rules::{PendingOutOfBounds, PendingRuleOutcome};
use sim_components::shot_pipeline::shot_accuracy;

use super::SimulationSet;
use super::events::{
    CrossOutcome, EventLog, HeaderKind, HeaderOutcome, PossessionChangeCause, RestartKind,
    attribute_goal, build_cross_event, build_goal_event, build_header_event,
    build_kickoff_event, build_out_of_bounds_event, build_pass_event,
    build_possession_change_event, build_restart_event, build_save_event, build_shot_event,
    build_tackle_event, opponent_of,
};

/// Ball speed at or below which a goalkeeper catches (rather than
/// parries) a resolved shot, in m/tick.
///
/// Placeholder for the Phase 4 goalkeeper mechanics (ticket 20),
/// which replace this single threshold with the Reflexes / Aerial
/// Reach / Concentration model. Faster balls are parried; punching
/// is reserved for cross clearances (aerial play, also Phase 4).
const SAVE_CATCH_MAX_SPEED: f32 = 0.25;

/// A resolved cross awaiting event emission (PRD §9.2, ticket 23).
///
/// Written by the cross-resolution logic and drained by
/// [`emit_cross_header_events`] in the same tick, so the emitter sees
/// it without relying on a deferred-commands sync point (mirrors the
/// `PendingOutOfBounds` contract). Inserted as an empty default by
/// `Simulation::create_match`; no cross is emitted until a cross is
/// resolved (the live cross kick site is the natural consumer).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct PendingCross(pub Option<CrossResolved>);

impl PendingCross {
    /// Take the pending cross, leaving `None` for next time.
    #[must_use]
    pub const fn take(&mut self) -> Option<CrossResolved> {
        self.0.take()
    }
}

/// One resolved cross, ready to be emitted as `cross` (plus optional
/// `header`) events.
#[derive(Debug, Clone, Copy)]
pub struct CrossResolved {
    /// Stable player identity for the wire payload.
    pub player_id: PlayerId,
    /// The crosser's team (credited on the envelope).
    pub team: TeamId,
    /// Target point the cross was aimed at.
    pub target: Vec2,
    /// How the cross resolved (`completed` / `missed` / `intercepted`
    /// / `punched`).
    pub outcome: CrossOutcome,
    /// The header that followed the cross, if the attacker won the
    /// aerial duel at the target point. `None` when the cross was
    /// missed, intercepted, or punched (no clean header).
    pub header: Option<HeaderResolved>,
}

/// One resolved header awaiting emission.
#[derive(Debug, Clone, Copy)]
pub struct HeaderResolved {
    /// Player who headed the ball.
    pub player: PlayerId,
    /// Intent of the header (pass / shot / clearance).
    pub kind: HeaderKind,
    /// How the header resolved.
    pub outcome: HeaderOutcome,
}

/// Whether a shot is on target, per the Phase 4 pipeline: the shooter's
/// `Shooting` attribute feeds [`shot_accuracy`] (response curve plus fit
/// multiplier) and is compared against an RNG roll.
///
/// Returns `None` when the shooter has no profile (or no `Shooting`
/// attribute), in which case the caller keeps the placeholder behaviour
/// (on-target).
#[must_use]
fn shot_on_target(
    shooter: Entity,
    team_query: &Query<(
        Entity,
        &TeamIdComponent,
        Option<&RoleComponent>,
        Option<&PlayerProfile>,
    )>,
    tuning: &Tuning,
    rng: &mut SimRng,
) -> Option<bool> {
    let (_, _, _, profile) = team_query.iter().find(|(e, _, _, _)| *e == shooter)?;
    let shooting = profile.and_then(|profile| profile.get(Attribute::Shooting))?;
    let accuracy = shot_accuracy(shooting, 1.0, tuning);
    let roll = rng.gen_f32();
    Some(roll < accuracy)
}

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
    mut outcome: ResMut<PendingRuleOutcome>,
    pending_oob: Res<PendingOutOfBounds>,
    ball: Res<Ball>,
    ball_query: Query<&Position, With<BallMarker>>,
    clock: Res<MatchClock>,
    current_tick: Res<sim_rules::CurrentTick>,
    team_query: Query<(Entity, &sim_components::TeamIdComponent)>,
) {
    // The ball's current position is the `end` of any pass we
    // resolve here. If the entity is missing (test fixture),
    // `Vec2::zero()` is a defensive placeholder.
    let ball_pos = ball_query.iter().next().map_or(Vec2::zero(), |p| p.0);
    let tick = current_tick.0;

    // OOB arm: the OOB detector set `PendingOutOfBounds` on this
    // tick. Resolve the pending pass with `to = None` and
    // `success = false`, then drain the OOB signal.
    if pending_oob.get().is_some()
        && let Some(pass) = outcome.0.pass.take()
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

    // Goal arm (ticket 10): the ball crossed the line while a pass
    // was in flight. Resolve with `to = None` and `success = false`
    // — the pass never reached a teammate.
    if outcome.0.goal.is_some()
        && let Some(pass) = outcome.0.pass.take()
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
    }
    // Drain the goal signal. This system is the last reader in the
    // chain (`emit_goal_events` and `resolve_shot_events` ran
    // earlier), so clearing here keeps a stale signal from re-firing
    // on the next tick.
    outcome.0.goal = None;

    // Acquisition arm: the ball is now Possessed and a pass was
    // in flight. Resolve it. The possessor's team is read from
    // the live `Ball.possessor` via the player query (the
    // `TeamIdComponent` is the canonical place).
    if ball.state == BallState::Possessed
        && let Some(possessor) = ball.possessor
        && let Some(pass) = outcome.0.pass.take()
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
    current_tick: Res<sim_rules::CurrentTick>,
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
    let tick = current_tick.0;
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
    pending_oob: Res<PendingOutOfBounds>,
    ball: Res<Ball>,
    current_tick: Res<sim_rules::CurrentTick>,
    clock: Res<MatchClock>,
) {
    let Some(_oob_event) = pending_oob.get() else {
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
    let tick = current_tick.0;
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

/// `goal` event + goal-driven `possession_change` (PRD §9.2, ticket 10).
///
/// Runs in the `Rules` set after `goal_detection_system` has stashed
/// the resolved data on `GoalForEmit`. Emits the `goal` event first
/// (scorer / own-goal / assister per PRD §8.5 via [`attribute_goal`]),
/// then the `possession_change` with cause `goal` to the conceding
/// team (which takes the next kickoff, Law 10). The cache update keeps
/// the later kickoff acquisition from double-emitting.
///
/// Does **not** drain `RuleOutcome.goal`: the pass and shot resolvers read
/// the same signal later in the chain, and `resolve_pass_events`
/// (last) owns the drain.
pub fn emit_goal_events(
    mut event_log: ResMut<EventLog>,
    outcome: Res<PendingRuleOutcome>,
    current_tick: Res<sim_rules::CurrentTick>,
    clock: Res<MatchClock>,
) {
    let Some(resolved) = outcome.0.goal.as_ref() else {
        return;
    };
    let tick = current_tick.0;
    let attribution = attribute_goal(
        resolved.scorer_team,
        [
            resolved.last_touch,
            resolved.prev_touch,
            resolved.older_touch,
        ],
        resolved.goal_tick,
    );
    let goal_event = build_goal_event(
        event_log.next_seq,
        tick,
        &clock,
        resolved.scorer_team,
        attribution,
        resolved.score,
    );
    event_log.append(goal_event);
    // The conceding team takes the next kickoff, so possession moves
    // to them. Skip the no-op case (e.g. an own goal while the cache
    // already credits the conceding team).
    let to_team = opponent_of(resolved.scorer_team);
    if event_log.possession_team != Some(to_team) {
        let from_team = event_log.possession_team;
        let change = build_possession_change_event(
            event_log.next_seq,
            tick,
            &clock,
            from_team,
            to_team,
            PossessionChangeCause::Goal,
            resolved.last_touch,
        );
        event_log.append(change);
        event_log.possession_team = Some(to_team);
    }
}

/// `kickoff` event for post-goal kickoffs (PRD §9.2, ticket 10).
///
/// Runs in the `Rules` set after `restart_system` has stashed the
/// resolved taker on `KickoffForEmit`. Emits with
/// `reason = after_goal` and no attack directions (ends don't change
/// after a goal). Lifecycle kickoffs (`match_start`, `second_half`)
/// are emitted directly by `apply_transition`, which already owns the
/// world.
pub fn emit_kickoff_restart_events(
    mut event_log: ResMut<EventLog>,
    mut outcome: ResMut<PendingRuleOutcome>,
    current_tick: Res<sim_rules::CurrentTick>,
    clock: Res<MatchClock>,
) {
    let Some(resolved) = outcome.0.kickoff.take() else {
        return;
    };
    let tick = current_tick.0;
    // Map the (possibly placeholder) entity to a PlayerId. Real
    // matches always have a player on the taker team so this
    // resolves to `Some`; sparse test worlds collapse to `None`.
    let taker = if resolved.taker == Entity::PLACEHOLDER {
        None
    } else {
        Some(PlayerId::from_entity(resolved.taker))
    };
    let event = build_kickoff_event(
        event_log.next_seq,
        tick,
        &clock,
        resolved.taker_team,
        taker,
        KickoffReason::AfterGoal,
        None,
        None,
    );
    event_log.append(event);
}

/// `shot` (plus `save`) resolution (PRD §9.2, ticket 10).
///
/// Runs in the `Rules` set while `GoalForEmit` and
/// `OobRestartForEmit` are still populated (before the OOB emitter
/// drains its slot), so all three resolution arms see the same-tick
/// signals:
///
/// - goal fired on this tick: the pending shot scored
///   (`on_target`, `goal`). Sound because the ball is single: a still
///   pending shot with no intervening acquisition means the shot went
///   straight in.
/// - OOB resolved on this tick: a corner means the shot was stopped on
///   the line (`on_target`, `blocked`); a throw-in or goal kick means
///   it missed (`off_target`).
/// - acquisition on this tick: an opposing goalkeeper means a save
///   (`on_target`, `saved`, plus a same-tick `save` event); anyone
///   else means the shot was blocked or gathered (`blocked`), except
///   the shooter's own keeper gathering it (`off_target`).
///
/// The mapping is a documented placeholder: Phase 4 mechanics
/// (ticket 20) refine the `outcome` distribution and the catch/parry
/// rule. The `post` outcome has no producer yet (no post geometry).
#[allow(
    clippy::too_many_arguments,
    reason = "Bevy system param set: standard Rules-set emitters all carry similar param lists; splitting would scatter shot/save resolution across systems"
)]
pub fn resolve_shot_events(
    mut event_log: ResMut<EventLog>,
    mut outcome: ResMut<PendingRuleOutcome>,
    ball: Res<Ball>,
    ball_vel: Query<&Velocity, With<BallMarker>>,
    team_query: Query<(
        Entity,
        &TeamIdComponent,
        Option<&RoleComponent>,
        Option<&PlayerProfile>,
    )>,
    tuning: Res<Tuning>,
    mut rng: ResMut<SimRng>,
    current_tick: Res<sim_rules::CurrentTick>,
    clock: Res<MatchClock>,
) {
    // Peek (don't consume) — the shot may need multiple ticks to resolve
    // (goal / OOB / possession). Only clear when an arm fires.
    let Some(shot_state) = outcome.0.shot.as_ref().copied() else {
        return;
    };
    let tick = current_tick.0;
    let shooter_id = PlayerId::from_entity(shot_state.shooter);

    // Goal arm: the shot went straight in.
    if outcome.0.goal.is_some() {
        outcome.0.shot = None;
        let event = build_shot_event(
            event_log.next_seq,
            tick,
            &clock,
            shot_state.shooter_team,
            shooter_id,
            true,
            ShotOutcome::Goal,
        );
        event_log.append(event);
        return;
    }

    // OOB arm: resolved restart kind decides on-target vs off-target.
    if let Some(resolved) = outcome.0.oob_restart.as_ref().copied() {
        outcome.0.shot = None;
        let (on_target, shot_outcome) = match resolved.oob_type {
            BallOutOfBoundsType::Corner => (true, ShotOutcome::Blocked),
            BallOutOfBoundsType::ThrowIn | BallOutOfBoundsType::GoalKick => {
                (false, ShotOutcome::OffTarget)
            }
        };
        let event = build_shot_event(
            event_log.next_seq,
            tick,
            &clock,
            shot_state.shooter_team,
            shooter_id,
            on_target,
            shot_outcome,
        );
        event_log.append(event);
        return;
    }

    // Acquisition arm: whoever holds the ball now resolved the shot.
    if ball.state == BallState::Possessed
        && let Some(possessor) = ball.possessor
        && let Some((_, team, role, _)) = team_query.iter().find(|(e, _, _, _)| *e == possessor)
    {
        outcome.0.shot = None;
        let possessor_team = team.0;
        let is_keeper = role.is_some_and(|r| r.0 == Role::Goalkeeper);
        // `team` and `role` are copied out above, ending their borrows of
        // `team_query` so `shot_on_target` can re-borrow it below.
        let (on_target, shot_outcome) = if possessor_team == shot_state.shooter_team {
            // Own side gathered it: the shooter's keeper collecting a
            // back-heeled effort (off target); a teammate gathering a
            // rebound (blocked in spirit — it never reached the goal).
            if is_keeper {
                (false, ShotOutcome::OffTarget)
            } else {
                (true, ShotOutcome::Blocked)
            }
        } else if is_keeper {
            // Opposing keeper stopped the shot. Shooting accuracy (the
            // shooter's `Shooting` attribute through the response curve and
            // fit multiplier) decides whether it was on target; the keeper
            // holding the ball means it did not become a goal, so the
            // outcome is `Saved` (physical ground truth). `Reflexes` /
            // `Finishing` shape the save model via the pipeline pure
            // functions, which are unit-tested in
            // `sim_components::shot_pipeline`.
            let on_target = shot_on_target(
                shot_state.shooter,
                &team_query,
                &tuning,
                &mut rng,
            )
            .unwrap_or(true);
            (on_target, ShotOutcome::Saved)
        } else {
            (true, ShotOutcome::Blocked)
        };
        let shot_event = build_shot_event(
            event_log.next_seq,
            tick,
            &clock,
            shot_state.shooter_team,
            shooter_id,
            on_target,
            shot_outcome,
        );
        event_log.append(shot_event);
        // A goalkeeper stop earns the companion `save` event on the
        // next `seq` (same tick).
        if shot_outcome == ShotOutcome::Saved {
            let ball_speed = ball_vel.iter().next().map_or(0.0, |v| v.0.length());
            let kind = if ball_speed <= SAVE_CATCH_MAX_SPEED {
                SaveKind::Catch
            } else {
                SaveKind::Parry
            };
            let save = build_save_event(
                event_log.next_seq,
                tick,
                &clock,
                possessor_team,
                PlayerId::from_entity(possessor),
                shooter_id,
                kind,
            );
            event_log.append(save);
        }
    }
}

/// `tackle` event emission (PRD §9.2, ticket 21).
///
/// Runs in the `Rules` set after `player_action_execution_system` has
/// stashed the resolved tackle attempt on `PendingTackle`. Emits one
/// `tackle` event per attempt with `tackler`, `carrier`, `success`.
pub fn emit_tackle_events(
    mut event_log: ResMut<EventLog>,
    mut outcome: ResMut<PendingRuleOutcome>,
    current_tick: Res<sim_rules::CurrentTick>,
    clock: Res<MatchClock>,
    team_query: Query<(Entity, &sim_components::TeamIdComponent)>,
) {
    let Some(attempt) = outcome.0.tackle.take() else {
        return;
    };
    let tick = current_tick.0;
    let tackler_team = team_query
        .iter()
        .find(|(e, _)| *e == attempt.tackler)
        .map_or(TeamId(0), |(_, t)| t.0);
    let event = build_tackle_event(
        event_log.next_seq,
        tick,
        &clock,
        tackler_team,
        PlayerId::from_entity(attempt.tackler),
        PlayerId::from_entity(attempt.carrier),
        attempt.success,
    );
    event_log.append(event);
}

/// `cross` (plus optional `header`) resolution (PRD §9.2, ticket 23).
///
/// Runs in the `Rules` set and drains `PendingCross`, emitting the
/// `cross` event and, when the attacker won the aerial duel at the
/// target point, the companion `header` event on the next `seq` (same
/// tick). Clears `PendingCross` so it does not re-fire.
#[allow(
    clippy::too_many_arguments,
    reason = "Bevy system param set: standard Rules-set emitters all carry similar param lists"
)]
pub fn emit_cross_header_events(
    mut event_log: ResMut<EventLog>,
    mut pending_cross: ResMut<PendingCross>,
    current_tick: Res<sim_rules::CurrentTick>,
    clock: Res<MatchClock>,
) {
    let Some(resolved) = pending_cross.take() else {
        return;
    };
    let tick = current_tick.0;
    let cross = build_cross_event(
        event_log.next_seq,
        tick,
        &clock,
        resolved.team,
        resolved.player_id,
        resolved.target,
        resolved.outcome,
    );
    event_log.append(cross);
    if let Some(header) = resolved.header {
        let header_event = build_header_event(
            event_log.next_seq,
            tick,
            &clock,
            resolved.team,
            header.player,
            header.kind,
            header.outcome,
        );
        event_log.append(header_event);
    }
}

/// Emit the `out_of_bounds` and `restart` event pair (PRD §9.2,
/// ticket 09).
///
/// Runs in the `Rules` set after `restart_system` has resolved the
/// taker and computed the restart placement. Both events share the
/// same tick (PRD §9.3 / ticket 09 ordering invariant), and the
/// `out_of_bounds` event precedes its restart event by exactly one
/// `seq`.
///
/// Data flow:
///
/// 1. `out_of_bounds_system` writes `PendingOutOfBounds` with
///    `oob_type` + `last_touch`.
/// 2. `restart_system` reads the signal, picks the taker team via
///    Law 9 (opposite of last-touch team), computes `restart_pos`,
///    places the ball, and stashes the resolved data on
///    `OobRestartForEmit`.
/// 3. THIS system reads `OobRestartForEmit`, emits the
///    `out_of_bounds` event, then the `restart` event, then clears
///    the resource. Two `event_log.append` calls in immediate
///    succession guarantee the seq-adjacency invariant.
///
/// The `taker` `PlayerId` is `None` when `restart_system` found no
/// resolvable player on the taker team (`Entity::PLACEHOLDER`
/// collapse). The wire payload accepts the `null` per PRD §9.2's
/// "nullable where it doesn't apply" rule (pass events already
/// follow this convention).
#[allow(
    clippy::too_many_arguments,
    reason = "Bevy system param set: standard Rules-set emitters all carry similar param lists; splitting would scatter OOB+restart emission across systems"
)]
pub fn emit_oob_and_restart_events(
    mut event_log: ResMut<EventLog>,
    mut outcome: ResMut<PendingRuleOutcome>,
    current_tick: Res<sim_rules::CurrentTick>,
    clock: Res<MatchClock>,
) {
    let Some(resolved) = outcome.0.oob_restart.take() else {
        return;
    };
    let tick = current_tick.0;
    let kind = RestartKind::from_oob(resolved.oob_type);
    // The OOB event's `team` is the last-touch team (the team whose
    // touch sent the ball out). `last_touch` carries the full record
    // including `team`, so the duplication is consistent with the
    // possession_change envelope convention.
    let last_touch_team = resolved
        .last_touch
        .as_ref()
        .map_or(resolved.taker_team, |lt| lt.team);
    let oob_event = build_out_of_bounds_event(
        event_log.next_seq,
        tick,
        &clock,
        last_touch_team,
        resolved.last_touch,
        resolved.exit_point,
        kind,
    );
    event_log.append(oob_event);
    // Map the (possibly placeholder) entity to a PlayerId. Real
    // matches always have a player on the taker team so this
    // resolves to `Some`; sparse test worlds collapse to `None`.
    let taker_player_id = if resolved.taker == bevy_ecs::prelude::Entity::PLACEHOLDER {
        None
    } else {
        Some(PlayerId::from_entity(resolved.taker))
    };
    let restart_event = build_restart_event(
        event_log.next_seq,
        tick,
        &clock,
        resolved.taker_team,
        taker_player_id,
        resolved.restart_pos,
        kind,
    );
    event_log.append(restart_event);
    // Clear so the next emitter run is a no-op until the next OOB
    // restart. Defensive: leave the resource cleared if the run
    // was somehow re-entered (Bevy schedules a single run per tick).
}

/// Register the event-emission systems into the simulation
/// schedule. Called from `construct::register_systems`.
pub fn register_event_systems(schedule: &mut Schedule) {
    // Pass-event resolution and possession_change emission both
    // run in the `Rules` set, after the rule writers have settled
    // their signals. The order is:
    // 1. `out_of_bounds_system` (writes `PendingOutOfBounds`)
    // 2. `possession_resolution_system` (sets `Ball.possessor`)
    // 3. `restart_system` (consumes OOB, stashes `RuleOutcome.oob_restart`;
    //    consumes post-goal kickoff, stashes `RuleOutcome.kickoff`)
    // 4. `goal_detection_system` (credits the score, stashes
    //    `RuleOutcome.goal`)
    // 5. `resolve_shot_events` (reads `RuleOutcome.goal` +
    //    `RuleOutcome.oob_restart` + `Ball`; emits `shot`, then `save`)
    // 6. `emit_goal_events` (reads `RuleOutcome.goal`; emits `goal`, then
    //    `possession_change` with cause `goal`)
    // 7. `emit_kickoff_restart_events` (drains `RuleOutcome.kickoff`;
    //    emits `kickoff` with `reason = after_goal`)
    // 8. `emit_oob_and_restart_events` (drains `RuleOutcome.oob_restart`)
    // 9. `emit_possession_change_on_oob` / `..._on_acquisition`
    // 10. `resolve_pass_events` (reads `RuleOutcome.pass` and
    //    `RuleOutcome.goal`; drains `RuleOutcome.pass` and clears
    //    `RuleOutcome.goal` last).
    //
    // Shot/goal ordering is causal: the resolving `shot` (+ `save`)
    // precedes the `goal` it produced. The OOB+restart pair is
    // emitted back-to-back (one step produces `out_of_bounds` at seq
    // N, then `restart` at seq N+1) to satisfy the ticket 09 ordering
    // invariant. Possession_change follows wherever it falls — its
    // placement is irrelevant to that invariant.
    schedule.add_systems(
        (
            resolve_shot_events,
            emit_goal_events,
            emit_kickoff_restart_events,
            emit_tackle_events,
            emit_cross_header_events,
            emit_oob_and_restart_events,
            emit_possession_change_on_oob,
            emit_possession_change_on_acquisition,
            resolve_pass_events,
        )
            .chain()
            .in_set(SimulationSet::Rules),
    );
}
