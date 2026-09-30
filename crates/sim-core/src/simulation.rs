//! `Simulation` and `SimulationSet`: the public surface of `sim-core`.

mod brain_default;
mod commands;
mod construct;
mod formation;
pub mod lifecycle;
mod snapshot;
pub mod state_hash;
mod tick;

pub use lifecycle::lifecycle_system;

use bevy_ecs::prelude::*;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SimulationSet {
    Perception,
    Decision,
    Execution,
    Physics,
    Possession,
    Rules,
    MatchAdmin,
}

pub struct Simulation {
    pub world: World,
    pub schedule: Schedule,
    pub original_seed: u64,
    pub tick: u64,
    pub match_entity: Entity,
    pub ball_entity: Entity,
}

// `Simulation`'s implementation is split across the sibling submodules:
// - `construct` — `new`, `create_match` (world + match setup)
// - `tick` — `tick`, `tick_clock` (per-tick driver)
// - `commands` — `validate_command`, `apply_validated_command` (F5 split API)
// - `snapshot` — `get_state`
// - `state_hash` — `get_state_hash` and discriminant helpers
// `register_systems` (schedule wiring) lives in `construct` as a private
// helper used by `Simulation::new`.