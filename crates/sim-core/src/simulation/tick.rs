//! `Simulation::tick` and `tick_clock`: the per-tick driver.

use bevy_ecs::system::RunSystemOnce;
use sim_components::MatchClock;
use sim_rules::apply_ball_touched_system;

use super::Simulation;
use super::lifecycle_system;

impl Simulation {
    /// Advance the simulation by one fixed timestep (1/60 second).
    pub fn tick(&mut self) {
        // Phase 3: Insert current tick as a resource so referee systems can access it.
        self.world
            .insert_resource(sim_rules::CurrentTick(self.tick));

        // Feature 002: recompute what this tick records (no-op without telemetry).
        super::trace_emit::advance_trace_gate(&mut self.world);

        self.schedule.run(&mut self.world);

        // lifecycle_system takes the current tick counter.
        lifecycle_system(
            self.match_entity,
            self.ball_entity,
            self.tick,
            &mut self.world,
        );

        // Apply any touches registered by lifecycle_system (kickoff placement)
        // which runs after the main schedule. This ensures the buffer is drained
        // and Ball.last_touch is updated before the tick ends.
        self.world.run_system_once(apply_ball_touched_system);

        // Advance match clock (integer ticks, no floating point).
        self.tick_clock();

        self.tick += 1;
    }

    /// Advance the match clock by one tick if the clock is running.
    ///
    /// Phase F follow-up: `MatchClock` is a Resource (spec §3); the
    /// Component form on the match entity is kept in sync for backwards
    /// compat but the Resource is the source of truth.
    fn tick_clock(&mut self) {
        let elapsed_before = self
            .world
            .get_resource::<MatchClock>()
            .map_or(0, |c| c.elapsed_ticks);
        let is_running = self
            .world
            .get_resource::<MatchClock>()
            .is_some_and(|c| c.is_running);

        if is_running {
            // Update the Resource form (source of truth).
            if let Some(mut clock) = self.world.get_resource_mut::<MatchClock>() {
                clock.elapsed_ticks = elapsed_before + 1;
            }
            // Mirror to the Component form for backwards compat.
            if let Some(mut clock) = self
                .world
                .entity_mut(self.match_entity)
                .get_mut::<MatchClock>()
            {
                clock.elapsed_ticks = elapsed_before + 1;
            }
        }
    }
}
