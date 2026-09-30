//! `Simulation::tick` and `tick_clock`: the per-tick driver.

use sim_components::MatchClock;

use super::lifecycle_system;
use super::Simulation;

impl Simulation {
    /// Advance the simulation by one fixed timestep (1/60 second).
    pub fn tick(&mut self) {
        // Phase 3: Insert current tick as a resource so referee systems can access it.
        self.world
            .insert_resource(sim_rules::CurrentTick(self.tick));

        self.schedule.run(&mut self.world);

        // lifecycle_system takes the current tick counter.
        lifecycle_system(
            self.match_entity,
            self.ball_entity,
            self.tick,
            &mut self.world,
        );

        // Advance match clock (integer ticks, no floating point).
        self.tick_clock();

        self.tick += 1;
    }

    /// Advance the match clock by one tick if the clock is running.
    fn tick_clock(&mut self) {
        if let Some(mut clock) = self
            .world
            .entity_mut(self.match_entity)
            .get_mut::<MatchClock>()
            && clock.is_running
        {
            clock.elapsed_ticks += 1;
        }
    }
}