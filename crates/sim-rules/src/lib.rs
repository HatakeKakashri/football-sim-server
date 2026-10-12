//! `sim-rules`: football-law systems (goals, out-of-bounds, possession,
//! referee decisions, match clock).
//!
//! Public surface: the seven `pub fn` systems and the small support
//! types (`CurrentTick`, `PendingRestart`).

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
use sim_components::{RuleEvent, Team, TeamId};

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
/// derive the taker team from `RuleEvent::OutOfBounds.last_touch`
/// instead).
#[derive(Resource, Debug, Clone, Default)]
pub struct PendingRestart {
    pub event: Option<RuleEvent>,
    pub restart_tick: u64,
    pub scorer_team: Option<sim_components::TeamId>,
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
