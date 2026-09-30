//! `sim-rules`: football-law systems (goals, out-of-bounds, possession,
//! referee decisions, match clock).
//!
//! Public surface: the seven `pub fn` systems and the small support
//! types (`CurrentTick`, `PendingRestart`, `AttackingDirection`).

pub(crate) mod clock;
pub(crate) mod goals;
pub(crate) mod out_of_bounds;
pub(crate) mod possession;
pub(crate) mod referee;
#[cfg(test)]
mod clock_tests;
#[cfg(test)]
mod goals_tests;
#[cfg(test)]
mod out_of_bounds_tests;
#[cfg(test)]
mod possession_tests;
#[cfg(test)]
mod referee_tests;
#[cfg(test)]
mod tests;

pub use clock::{added_time_calculation_system, match_duration_enforcement_system};
pub use goals::{goal_detection_system, restart_system};
pub use out_of_bounds::out_of_bounds_system;
pub use possession::possession_resolution_system;
pub use referee::{
    foul_detection_system, minimum_player_count_system, offside_detection_system,
};

use bevy_ecs::prelude::*;
use sim_components::{RuleEvent, TeamId, TeamIdComponent};

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

/// Tracks pending restart state: (event, `tick_when_restart`)
#[derive(Resource, Debug, Clone, Default)]
pub struct PendingRestart {
    pub event: Option<RuleEvent>,
    pub restart_tick: u64,
}

/// Attack polarity for a given team. Home (team 0) attacks toward +x
/// (right goal, x = `PITCH_LENGTH`); away (team 1) attacks toward -x
/// (left goal, x = 0).
#[derive(Debug, Clone, Copy)]
pub struct AttackingDirection {
    /// True if this team attacks in the positive-x direction.
    pub(crate) attacks_positive_x: bool,
}

impl AttackingDirection {
    /// Returns true if `player_x` is in the opponent's half.
    pub(crate) fn in_opponent_half(self, player_x: f32) -> bool {
        if self.attacks_positive_x {
            player_x > crate::constants::PITCH_LENGTH / 2.0
        } else {
            player_x < crate::constants::PITCH_LENGTH / 2.0
        }
    }

    /// Returns true if `player_x` is nearer to the attacking goal line
    /// than `reference_x`.  For a team attacking +x, nearer means larger x.
    pub(crate) fn closer_than(self, player_x: f32, reference_x: f32) -> bool {
        if self.attacks_positive_x {
            player_x > reference_x
        } else {
            player_x < reference_x
        }
    }
}

/// Returns the `AttackingDirection` for the given team.
/// Home (team 0) → attacks +x; away (team 1) → attacks -x.
pub(crate) const fn attacking_direction(team: TeamId) -> AttackingDirection {
    AttackingDirection {
        attacks_positive_x: team.0 == 0,
    }
}

/// Extracts the raw `u8` team-id from a `TeamIdComponent`.
pub(crate) const fn team_id_u8(team: &TeamIdComponent) -> u8 {
    team.0.0
}

pub const fn referee_advantage_system(ball_res: Res<sim_components::Ball>, match_res: Res<sim_components::Match>) {
    // Phase N: real advantage window logic
    let _ = (ball_res, match_res);
}