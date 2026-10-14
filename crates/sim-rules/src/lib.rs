//! `sim-rules`: football-law systems (goals, out-of-bounds, possession,
//! referee decisions, match clock).
//!
//! Public surface: the seven `pub fn` systems and the small support
//! types (`CurrentTick`, `PendingRestart`, `PendingOutOfBounds`).

pub(crate) mod clock;
#[cfg(test)]
mod clock_tests;
pub(crate) mod goals;
#[cfg(test)]
mod goals_tests;
pub(crate) mod out_of_bounds;
#[cfg(test)]
mod out_of_bounds_tests;
pub(crate) mod possession;
#[cfg(test)]
mod possession_tests;
pub(crate) mod referee;
#[cfg(test)]
mod referee_tests;
#[cfg(test)]
mod tests;
pub(crate) mod touch;
#[cfg(test)]
mod touch_registration_tests;

pub use clock::{added_time_calculation_system, match_duration_enforcement_system};
pub use goals::{goal_detection_system, restart_system};
pub use out_of_bounds::out_of_bounds_system;
pub use possession::possession_resolution_system;
pub use referee::{foul_detection_system, minimum_player_count_system};
pub use touch::apply_ball_touched_system;

use bevy_ecs::prelude::*;
use sim_components::{BallOutOfBoundsType, LastTouch, PlayerId, RuleEvent, Team, TeamId};
use sim_math::Vec2;

/// Shared constants used by every rule system. Kept in one place so the
/// pitch geometry has a single source of truth.
pub(crate) mod constants {
    pub const PITCH_LENGTH: f32 = 105.0;
    pub const PITCH_WIDTH: f32 = 68.0;
    pub const GOAL_WIDTH: f32 = 7.32;
    pub const GOAL_Y_CENTER: f32 = PITCH_WIDTH / 2.0;
    pub const GOAL_Y_MIN: f32 = GOAL_Y_CENTER - GOAL_WIDTH / 2.0; // 26.68
    pub const GOAL_Y_MAX: f32 = GOAL_Y_CENTER + GOAL_WIDTH / 2.0; // 41.32
    /// Center spot of the pitch
    pub const CENTER_SPOT: (f32, f32) = (52.5, 34.0);
    pub const KICKOFF_RESTART_TICKS: u64 = 60;

    pub const SKILL_TOLERANCE: f32 = 0.1;
}

/// Phase 3: Tick counter resource for referee systems.
/// Inserted by sim-core before each schedule run.
#[derive(Resource, Debug, Clone, Copy)]
pub struct CurrentTick(pub u64);

/// Tracks pending restart state: (event, `tick_when_restart`).
///
/// `scorer_team` carries the team that scored the goal that triggered
/// a `KickoffRestart` (Law 8 / Law 10: the kickoff is taken by the
/// team that *conceded* the goal). `restart_system` resolves the
/// conceded team as the opposite of `scorer_team`. Only set for
/// `KickoffRestart` events; `None` for OOB-driven restarts (which
/// derive the taker team from `PendingOutOfBounds`'s `last_touch`
/// instead).
#[derive(Resource, Debug, Clone, Default)]
pub struct PendingRestart {
    pub event: Option<RuleEvent>,
    pub restart_tick: u64,
    pub scorer_team: Option<sim_components::TeamId>,
}

/// Out-of-bounds signal handed from `out_of_bounds_system` to
/// `restart_system`.
///
/// Carried by a [`PendingOutOfBounds`] resource rather than a deferred
/// `Commands` component insert, so the restarter sees it in the same
/// system run without relying on an `apply_deferred` sync point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutOfBoundsEvent {
    /// Which restart the ball's resting position earns (positional).
    pub oob_type: BallOutOfBoundsType,
    /// The last-touch record when the ball went out. The restart taker
    /// is the team opposite `last_touch.team` (Law 9).
    pub last_touch: Option<LastTouch>,
}

/// The out-of-bounds restart awaiting resolution, if any.
///
/// Must be present in any world that runs `out_of_bounds_system` or
/// `restart_system` (`Simulation::create_match` inserts it).
/// `out_of_bounds_system` writes it via [`Self::set`]; `restart_system`
/// drains it via [`Self::take`].
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PendingOutOfBounds(Option<OutOfBoundsEvent>);

impl PendingOutOfBounds {
    /// Record an OOB event for the restarter to resolve on the same tick.
    pub const fn set(&mut self, event: OutOfBoundsEvent) {
        self.0 = Some(event);
    }

    /// Take the pending OOB event, leaving `None` for next time.
    #[must_use]
    pub const fn take(&mut self) -> Option<OutOfBoundsEvent> {
        self.0.take()
    }

    /// Peek at the pending OOB event without consuming it.
    #[must_use]
    pub const fn get(&self) -> Option<&OutOfBoundsEvent> {
        self.0.as_ref()
    }
}

/// Team that attacks the goal at the given end of the pitch in the
/// current half, read from each team's per-half `attack_direction`
/// (`+1` attacks `x = PITCH_LENGTH`, `-1` attacks `x = 0`).
///
/// Worlds with no `Team` component pointing that way (unit fixtures that
/// never spawn teams) fall back to the construction-time convention:
/// home (team 0) attacks +x, away (team 1) attacks -x. A live
/// `Simulation` always has both teams, so the fallback never fires there.
pub(crate) fn team_attacking_goal<'a>(
    teams: impl IntoIterator<Item = &'a Team>,
    goal_at_positive_x: bool,
) -> TeamId {
    teams
        .into_iter()
        .find(|team| (team.attack_direction() > 0) == goal_at_positive_x)
        .map_or_else(
            || TeamId(u8::from(!goal_at_positive_x)),
            |team| team.id,
        )
}

pub const fn referee_advantage_system(
    ball_res: Res<sim_components::Ball>,
    match_res: Res<sim_components::Match>,
) {
    // Phase N: real advantage window logic
    let _ = (ball_res, match_res);
}

/// In-flight pass awaiting resolution (PRD §9.3).
///
/// A `pass` event is emitted at the resolution tick — the ball's
/// next acquisition, OOB, or goal. The kick site (e.g.
/// `kick_execution_system`) writes this resource with the kicker
/// identity, the intended receiver, the kick tick, and the
/// ball position at the kick. The resolver (acquisition, OOB,
/// goal) reads it, emits the `pass` event, and clears the
/// resource. If the ball transitions out of `InFlight` (e.g. the
/// physics system resets it) without any of those triggers
/// firing, the resource must be cleared by `cancel_pending_pass`
/// so stale state doesn't leak into a later event.
#[derive(Resource, Debug, Clone, Copy)]
pub struct PendingPass {
    /// `true` when a pass is in flight and the resource holds
    /// meaningful data.
    pub active: bool,
    /// Kicker (last `Play` touch at the kick). The receiving
    /// `pass` event stamps this onto the `from` field.
    pub from: PlayerId,
    /// Team of the kicker at the kick. Used for `success` (true
    /// when the receiver is on the same team) and `to` (the
    /// intended receiver identity).
    pub from_team: TeamId,
    /// Intended receiver. `None` when the kick was directed at
    /// "space" or a generic teammate slot, or when the
    /// AI brain emitted `PassTo` without a real teammate to
    /// target.
    pub intended_to: Option<PlayerId>,
    /// Tick at which the kick happened (PRD §9.2 `kick_tick`).
    pub kick_tick: u64,
    /// Ball position at the kick (PRD §9.2 `start`).
    pub kick_position: Vec2,
}

impl PendingPass {
    /// `true` when there is an in-flight pass awaiting resolution.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// Resource default: inactive, with the inner fields zeroed
    /// via the same primitives `Bevy` resource storage uses.
    /// `Vec2::zero` and `TeamId(0)` and `PlayerId(0)` are valid
    /// placeholder values for an inactive pending pass.
    #[must_use]
    pub const fn default_inactive() -> Self {
        Self {
            active: false,
            from: PlayerId::from_bits(0),
            from_team: TeamId(0),
            intended_to: None,
            kick_tick: 0,
            kick_position: Vec2::zero(),
        }
    }

    /// Mark the resource as having a pending pass. Writers fill
    /// the rest of the fields directly; this method only flips
    /// `active` so a partially-constructed value is never
    /// reported as active.
    pub const fn set(&mut self) {
        self.active = true;
    }

    /// Clear the resource, leaving it inactive. Returns the
    /// previously-active state so callers can branch on whether
    /// a pass actually fired.
    #[must_use]
    pub const fn cancel(&mut self) -> bool {
        let was_active = self.active;
        self.active = false;
        was_active
    }

    /// Take the pending pass (clears it). Used by resolvers that
    /// need the value to emit a `pass` event.
    #[must_use]
    pub const fn take(&mut self) -> Option<Self> {
        if self.active {
            Some(Self {
                active: false,
                from: self.from,
                from_team: self.from_team,
                intended_to: self.intended_to,
                kick_tick: self.kick_tick,
                kick_position: self.kick_position,
            })
        } else {
            None
        }
    }
}

impl Default for PendingPass {
    fn default() -> Self {
        Self::default_inactive()
    }
}
