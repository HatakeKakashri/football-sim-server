//! `sim-core`: the deterministic, fixed-timestep simulation driver.
//!
//! Public surface:
//! - [`Simulation`]: owns the `World`, `Schedule`, and a per-tick `tick()`.
//! - [`SimulationSet`]: the system set labels for ordering.
//! - [`MatchSnapshot`], [`BallView`], [`PlayerView`], [`ClockView`],
//!   [`MatchStateView`]: read-only views returned by `Simulation::get_state`.
//! - `validate_command` / `apply_validated_command` / `get_state_hash` / `tick`.

mod simulation;
#[cfg(test)]
mod tests;

pub use simulation::{Simulation, SimulationSet, lifecycle_system};

pub use sim_components::time;

/// Read-only snapshot of a match's state at one instant. Returned by
/// `Simulation::get_state`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MatchSnapshot {
    pub tick: u64,
    pub match_state: MatchStateView,
    pub ball: BallView,
    pub players: Vec<PlayerView>,
    pub score: (u8, u8),
    pub clock: ClockView,
    pub state_hash: u64,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct MatchStateView {
    pub state: sim_components::MatchState,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct BallView {
    pub position: [f32; 2],
    pub velocity: [f32; 2],
    pub spin: f32,
    pub state: sim_components::BallState,
    pub possessor: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayerView {
    pub entity_id: u64,
    pub team_id: sim_components::TeamId,
    pub position: [f32; 2],
    pub velocity: [f32; 2],
    pub stamina: f32,
    pub role: sim_components::Role,
    pub skill: f32,
    /// Phase 2: ball position as observed by this player's perception
    /// snapshot. Used by external observers / replays to verify the
    /// perception system is populating per-player ball position correctly.
    pub perception_ball_position: [f32; 2],
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct ClockView {
    pub elapsed_ticks: u64,
    pub half: u8,
    pub added_time_ticks: u64,
    pub is_running: bool,
}

