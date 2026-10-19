//! Manager command types.

use serde::{Deserialize, Serialize};

use super::{Formation, Mentality, Tactic};

/// Manager command sent to a match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ManagerCommand {
    ChangeFormation(Formation),
    Substitute { out: u64, substitute: u64 },
    ChangeMentality(Mentality),
    SetTactic(Tactic),
}

/// Error returned by `sim_core::Simulation::apply_command` and the
/// `sim_server` wrappers.
///
/// `sim_components` owns the type because the same variants are
/// meaningful at both the simulation and the network boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// The command is not legal in the match's current state
    /// (e.g. formation change during `PreMatch`).
    InvalidForState {
        current_state: crate::MatchState,
        required_state: crate::MatchState,
    },
    /// The command is recognized but not yet implemented.
    NotImplemented,
}
