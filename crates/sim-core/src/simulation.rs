//! `Simulation` and `SimulationSet`: the public surface of `sim-core`.

pub mod brain_default;
mod commands;
mod construct;
mod formation;
pub mod lifecycle;
mod snapshot;
pub mod state_hash;
mod tick;
pub mod trace_emit;

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
    world: World,
    schedule: Schedule,
    original_seed: u64,
    tick: u64,
    match_entity: Entity,
    ball_entity: Entity,
}

impl Simulation {
    /// Read-only access to the Bevy `World`.
    #[must_use]
    pub const fn world(&self) -> &World {
        &self.world
    }

    /// Mutable access to the Bevy `World`.
    ///
    /// Callers must not bypass the tick pipeline or mutate state in ways
    /// that break determinism. Intended for test setup and internal
    /// initialization only.
    #[allow(
        clippy::missing_const_for_fn,
        reason = "no const-evaluation context uses this; const adds noise without benefit"
    )]
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// The current simulation tick.
    #[must_use]
    pub const fn current_tick(&self) -> u64 {
        self.tick
    }

    /// The entity representing the match.
    #[must_use]
    pub const fn match_entity(&self) -> Entity {
        self.match_entity
    }

    /// The entity representing the ball.
    #[must_use]
    pub const fn ball_entity(&self) -> Entity {
        self.ball_entity
    }
}

// `Simulation`'s implementation is split across the sibling submodules:
// - `construct` — `new`, `create_match` (world + match setup)
// - `tick` — `tick`, `tick_clock` (per-tick driver)
// - `commands` — `validate_command`, `apply_validated_command` (F5 split API)
// - `snapshot` — `get_state`
// - `state_hash` — `get_state_hash` and discriminant helpers
// `register_systems` (schedule wiring) lives in `construct` as a private
// helper used by `Simulation::new`.