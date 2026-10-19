//! `RuleOutcome`: one transactional record of everything the Rules phase
//! produced this tick, drained by the event-emission systems in
//! `sim-core::simulation::event_emit`.
//!
//! All fields are `None` when nothing awaits emission on this tick.
//! Writers (rule systems in `sim-rules`, execution systems in
//! `sim-ai-player`) set fields; emitter systems in `sim-core` drain them
//! in a fixed order registered by `register_event_systems`.
//!
//! Note: `PendingCross` (defined in `sim-core`) is deliberately excluded
//! because `CrossResolved` references `sim_core::simulation::events`
//! types, and `sim-rules` cannot depend on `sim-core` (cycle).
//! `PendingCross` remains a separate `sim-core` resource installed by
//! `WorldInstaller`.

use bevy_ecs::prelude::*;
use sim_components::{BallOutOfBoundsType, LastTouch, PlayerId, TeamId};
use sim_math::Vec2;

/// The complete emit-related output of one tick's Rules phase.
///
/// All fields are `None` when nothing awaits emission on this tick.
/// Writers (rule systems in `sim-rules`, execution systems in
/// `sim-ai-player`) set fields; emitter systems in `sim-core` drain them
/// in a fixed order.
#[derive(Debug, Default)]
pub struct RuleOutcome {
    /// A goal scored this tick (Law 10).
    pub goal: Option<GoalResolved>,
    /// A post-goal kickoff scheduled this tick (delayed 60 ticks).
    pub kickoff: Option<KickoffResolved>,
    /// An out-of-bounds restart resolved this tick (Law 9).
    pub oob_restart: Option<OobRestartResolved>,
    /// A tackle attempt this tick (Law 12).
    pub tackle: Option<TackleAttempt>,
    /// An in-flight shot awaiting resolution this tick.
    pub shot: Option<PendingShotState>,
    /// An in-flight pass awaiting resolution this tick.
    pub pass: Option<PendingPassState>,
}

/// Resource wrapper for `RuleOutcome`.
///
/// Inserted by [`install_rules_resources`]. Every system that runs the
/// Rules phase takes `Res<PendingRuleOutcome>` or
/// `ResMut<PendingRuleOutcome>` to read or drain fields.
#[derive(Resource, Debug, Default)]
pub struct PendingRuleOutcome(pub RuleOutcome);

impl PendingRuleOutcome {
    /// Take a field, leaving `None` for next time.
    #[must_use]
    pub const fn take_goal(&mut self) -> Option<GoalResolved> {
        self.0.goal.take()
    }

    #[must_use]
    pub const fn take_kickoff(&mut self) -> Option<KickoffResolved> {
        self.0.kickoff.take()
    }

    #[must_use]
    pub const fn take_oob_restart(&mut self) -> Option<OobRestartResolved> {
        self.0.oob_restart.take()
    }

    #[must_use]
    pub const fn take_tackle(&mut self) -> Option<TackleAttempt> {
        self.0.tackle.take()
    }

    #[must_use]
    pub const fn take_shot(&mut self) -> Option<PendingShotState> {
        self.0.shot.take()
    }

    #[must_use]
    pub const fn take_pass(&mut self) -> Option<PendingPassState> {
        self.0.pass.take()
    }
}

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

/// Snapshot of an OOB restart's resolved data.
#[derive(Debug, Clone, Copy)]
pub struct OobRestartResolved {
    /// Concrete restart kind (positional: throw-in / goal-kick /
    /// corner).
    pub oob_type: BallOutOfBoundsType,
    /// Ball position when the OOB detector fired — the clamped
    /// boundary position (the "exit point" on the wire).
    pub exit_point: Vec2,
    /// Full `LastTouch` at the moment of the OOB.
    pub last_touch: Option<LastTouch>,
    /// Team that takes the restart (Law 9: opposite of the last
    /// toucher).
    pub taker_team: TeamId,
    /// Player who takes the restart. `Entity::PLACEHOLDER` for test
    /// worlds without players on the taker team.
    pub taker: Entity,
    /// Restart placement position.
    pub restart_pos: Vec2,
}

/// One resolved tackle attempt.
#[derive(Debug, Clone, Copy)]
pub struct TackleAttempt {
    /// The player attempting the tackle.
    pub tackler: Entity,
    /// The ball carrier being challenged.
    pub carrier: Entity,
    /// `true` when the tackle won the ball.
    pub success: bool,
}

/// An in-flight shot awaiting resolution (PRD §9.3).
#[derive(Debug, Clone, Copy)]
pub struct PendingShotState {
    /// Player who shot.
    pub shooter: Entity,
    /// Team of the shooter at the kick.
    pub shooter_team: TeamId,
    /// Global tick at which the kick happened.
    pub kick_tick: u64,
    /// Ball position at the kick.
    pub kick_pos: Vec2,
}

/// In-flight pass data (no `active` flag — `None` replaces it).
#[derive(Debug, Clone, Copy)]
pub struct PendingPassState {
    /// Kicker (last `Play` touch at the kick).
    pub from: PlayerId,
    /// Team of the kicker at the kick.
    pub from_team: TeamId,
    /// Intended receiver.
    pub intended_to: Option<PlayerId>,
    /// Tick at which the kick happened (PRD §9.2 `kick_tick`).
    pub kick_tick: u64,
    /// Ball position at the kick (PRD §9.2 `start`).
    pub kick_position: Vec2,
}
