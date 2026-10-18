//! `WorldInstaller`: the single place that decides which resources a
//! simulation world holds.
//!
//! Production (`Simulation::new`) and tests both build worlds through
//! this installer, and every system takes the resources it needs as
//! required `Res`/`ResMut` params. A world that is missing one fails
//! loudly at the first system run instead of silently defaulting.
//!
//! The installer composes one installer per feature crate
//! (`sim_rules`, `sim_physics`, `sim_ai_player`) with the resources owned
//! by `sim-core` itself. Crates below `sim-core` cannot depend on it, so
//! their own tests use their crate-level installer directly.
//!
//! Deliberately not installed, because their absence is meaningful:
//! `TelemetryConfig` / `TraceGate` (telemetry is opt-in via
//! `Simulation::enable_telemetry`) and `PendingRestart` (created on
//! demand when a goal is scored).

use bevy_ecs::prelude::*;
use sim_components::{Ball, BallAirborne, BallState, PendingBallTouches};
use sim_math::Tuning;

use super::event_emit::PendingCross;
use super::lifecycle::{FirstHalfAddedTime, HalfTimeEntryTick};

/// Builds the resource set for one simulation world.
#[derive(Debug, Clone, Copy)]
pub struct WorldInstaller {
    seed: u64,
}

impl WorldInstaller {
    /// An installer for a world whose RNG is seeded with `seed`.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { seed }
    }

    /// Insert every resource the registered systems require, at its
    /// initial value. Spawns no entities. `Match` and `MatchClock` are
    /// installed by `Simulation::create_match`, since `Match` refers to
    /// the team entities it spawns.
    pub fn install_resources(&self, world: &mut World) {
        sim_physics::install_physics_resources(world, self.seed);
        sim_ai_player::install_ai_player_resources(world);
        sim_rules::install_rules_resources(world);

        // Phase C §4.3: `Ball` is the single source of truth for
        // spin/state/possessor/last_touch/kick_velocity.
        world.insert_resource(Ball {
            spin: 0.0,
            state: BallState::Free,
            possessor: None,
            last_touch: None,
            touch_history: [None, None],
            kick_velocity: None,
        });
        world.insert_resource(PendingBallTouches::default());
        world.insert_resource(BallAirborne::default());
        world.insert_resource(PendingCross::default());
        world.insert_resource(HalfTimeEntryTick { tick: None });
        world.insert_resource(FirstHalfAddedTime(0));
        world.insert_resource(crate::EventLog::new());
        // Tuning is the single edit surface for mechanic calibration
        // (PRD §11); every shot/save consumer reads through it.
        world.insert_resource(Tuning::default());
    }
}
