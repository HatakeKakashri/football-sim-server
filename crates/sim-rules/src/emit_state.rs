//! Emit signals for goal / kickoff-restart / shot events (ticket 10).
//!
//! `sim-rules` cannot depend on `sim-core` (cycle: `sim-core` already
//! depends on `sim-rules`), so rule systems cannot append to the
//! `EventLog` directly. Instead they stash the resolved data on these
//! resources and the `sim-core` emitters (`event_emit`) drain them in
//! the same `Rules` set, after the writers have run.
//!
//! The pattern mirrors `OobRestartForEmit` (ticket 09): one resource
//! per signal, `None` when nothing awaits emission.

use bevy_ecs::prelude::*;
use sim_components::{LastTouch, TeamId};
use sim_math::Vec2;

/// Resolved goal data, written by `goal_detection_system` after it has
/// credited the score and captured the touch window (PRD §8.5).
#[derive(Debug, Clone, Copy)]
pub struct GoalResolved {
    /// Team credited by goal position (per-half attack direction).
    pub scorer_team: TeamId,
    /// Running score after this goal.
    pub score: (u8, u8),
    /// `Ball.last_touch` at the goal tick (the scorer candidate).
    pub last_touch: Option<LastTouch>,
    /// `Ball.touch_history[0]` at the goal tick (the assist candidate).
    pub prev_touch: Option<LastTouch>,
    /// `Ball.touch_history[1]` at the goal tick (the deeper assist
    /// candidate when the previous touch is a restart placement).
    pub older_touch: Option<LastTouch>,
    /// Global tick at which the goal was detected.
    pub goal_tick: u64,
    /// Ball position when the goal was detected.
    pub ball_pos: Vec2,
}

/// Goal awaiting emission as a `goal` event, if any.
///
/// Drained by the `sim-core` goal emitter (which also resolves the
/// in-flight pass and the `possession_change` with cause `goal`).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct GoalForEmit(pub Option<GoalResolved>);

/// Resolved post-goal kickoff data, written by `restart_system` when
/// it places the ball at the center spot after the 60-tick delay
/// (Law 8 / Law 10: taker is the team that conceded).
#[derive(Debug, Clone, Copy)]
pub struct KickoffResolved {
    /// Team taking the kickoff (the conceding team).
    pub taker_team: TeamId,
    /// Player taking the kickoff, or `Entity::PLACEHOLDER` in sparse
    /// test worlds with no player on the taker team.
    pub taker: Entity,
    /// Kickoff placement (the center spot).
    pub at: Vec2,
}

/// Post-goal kickoff awaiting emission as a `kickoff` event
/// (`reason = after_goal`), if any.
///
/// Drained by the `sim-core` kickoff emitter. Lifecycle kickoffs
/// (`match_start`, `second_half`) are emitted directly by
/// `apply_transition`, which already owns the world.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct KickoffForEmit(pub Option<KickoffResolved>);

/// An in-flight shot awaiting resolution (PRD §9.3, ticket 10).
///
/// Written by `kick_execution_system` whenever a possessing player
/// executes a `ShootAtGoal` kick. The resolver (acquisition, OOB, or
/// goal) reads it, emits the `shot` event (plus a `save` event when a
/// goalkeeper makes the stop), and clears the resource. Any non-shot
/// kick cancels a stale pending shot so a later event is never
/// attributed to an earlier kick.
#[derive(Debug, Clone, Copy)]
pub struct PendingShotState {
    /// Player who shot. The receiving `shot` event stamps this onto
    /// the `shooter` field.
    pub shooter: Entity,
    /// Team of the shooter at the kick.
    pub shooter_team: TeamId,
    /// Global tick at which the kick happened.
    pub kick_tick: u64,
    /// Ball position at the kick.
    pub kick_pos: Vec2,
}

/// Shot awaiting resolution, if any.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct PendingShot(pub Option<PendingShotState>);
