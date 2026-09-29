use sim_components::ManagerCommand;
use sim_core::{MatchSnapshot, Simulation};

#[derive(Debug, Clone)]
pub enum CommandError {
    InvalidForState {
        current_state: sim_components::MatchState,
        required_state: sim_components::MatchState,
    },
    NoSubstitutesRemaining,
    PlayerNotOnPitch,
    FormationInvalid,
    CommandCooldownActive,
}

pub struct ServerSimulation {
    pub simulation: Simulation,
    pub command_queue: CommandQueue,
    /// The team side of the active manager (controls which team receives commands)
    pub active_manager_side: sim_components::TeamSide,
}

impl ServerSimulation {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            simulation: Simulation::new(seed),
            command_queue: CommandQueue::new(),
            active_manager_side: sim_components::TeamSide::Home,
        }
    }

    pub fn tick(&mut self) {
        // Apply queued commands at tick boundaries
        let commands = self.command_queue.apply_at_tick(self.simulation.tick);
        for command in commands {
            // Apply command (validation already done when queueing)
            let _ = self.apply_command_immediately(command);
        }

        // Advance simulation
        self.simulation.tick();
    }

    pub fn apply_command(&mut self, command: ManagerCommand) -> Result<(), CommandError> {
        // Phase C §4.3: Match is a Resource, not a Component.
        let match_component = self
            .simulation
            .world
            .resource::<sim_components::Match>()
            .clone();

        // Validate command based on current state
        match &command {
            ManagerCommand::ChangeFormation(_) => {
                // Validate formation change is allowed in current state
                if match_component.state != sim_components::MatchState::InPlay
                    && match_component.state != sim_components::MatchState::Stoppage
                {
                    return Err(CommandError::InvalidForState {
                        current_state: match_component.state,
                        required_state: sim_components::MatchState::InPlay,
                    });
                }

                // Validate formation is valid
                // For now, all formations are considered valid
            }
            ManagerCommand::Substitute {
                out: _,
                substitute: _,
            } => {
                // Validate substitution is allowed in current state
                if match_component.state != sim_components::MatchState::Stoppage
                    && match_component.state != sim_components::MatchState::HalfTime
                {
                    return Err(CommandError::InvalidForState {
                        current_state: match_component.state,
                        required_state: sim_components::MatchState::Stoppage,
                    });
                }

                // Validate players exist and are in correct positions
                // For now, skip detailed validation
            }
            ManagerCommand::ChangeMentality(_) => {
                // Validate mentality change is allowed in current state
                if match_component.state != sim_components::MatchState::InPlay
                    && match_component.state != sim_components::MatchState::Stoppage
                {
                    return Err(CommandError::InvalidForState {
                        current_state: match_component.state,
                        required_state: sim_components::MatchState::InPlay,
                    });
                }
            }
            ManagerCommand::SetTactic(_) => {
                // Validate tactic change is allowed in current state
                if match_component.state != sim_components::MatchState::InPlay
                    && match_component.state != sim_components::MatchState::Stoppage
                {
                    return Err(CommandError::InvalidForState {
                        current_state: match_component.state,
                        required_state: sim_components::MatchState::InPlay,
                    });
                }
            }
        }

        // Queue command for application at next tick boundary
        self.command_queue
            .enqueue(command, self.simulation.tick + 1);
        Ok(())
    }

    fn apply_command_immediately(&mut self, command: ManagerCommand) -> Result<(), CommandError> {
        let match_component = self
            .simulation
            .world
            .resource::<sim_components::Match>()
            .clone();

        // Determine which team to update based on active_manager_side
        let team_to_update = match self.active_manager_side {
            sim_components::TeamSide::Home => match_component.home_team,
            sim_components::TeamSide::Away => match_component.away_team,
        };

        match command {
            ManagerCommand::ChangeFormation(formation) => {
                if let Some(mut team) = self
                    .simulation
                    .world
                    .entity_mut(team_to_update)
                    .get_mut::<sim_components::Team>()
                {
                    team.formation = formation;
                }
            }
            ManagerCommand::Substitute { out, substitute } => {
                // Implement substitution logic
                // For now, just log it
                tracing::info!("Substitution: {out:?} -> {substitute:?}");
            }
            ManagerCommand::ChangeMentality(mentality) => {
                if let Some(mut team) = self
                    .simulation
                    .world
                    .entity_mut(team_to_update)
                    .get_mut::<sim_components::Team>()
                {
                    team.mentality = mentality;
                }
            }
            ManagerCommand::SetTactic(tactic) => {
                // Store tactic somewhere (for now, just log)
                tracing::info!("Tactic set: {tactic:?}");
            }
        }

        Ok(())
    }

    /// Phase C §4.3: returns the match entity whose `MatchClock` component
    /// is the source of truth for the clock. Kept as a method to preserve
    /// the test API; with `Match` as a Resource, callers should generally
    /// read `simulation.world.resource::<Match>()` directly.
    #[allow(dead_code, reason = "kept for test API backwards compatibility")]
    fn get_match_entity(&self) -> Result<bevy_ecs::prelude::Entity, CommandError> {
        Ok(self.simulation.match_entity)
    }

    pub fn get_state(&self) -> MatchSnapshot {
        // Phase C §4.3: Match is a Resource, so the match-entity argument
        // is now informational. sim-core's get_state reads the resource.
        self.simulation
            .get_state(self.simulation.match_entity)
            .unwrap()
    }
}

pub struct CommandQueue {
    pub commands: Vec<QueuedCommand>,
}

pub struct QueuedCommand {
    pub command: ManagerCommand,
    pub tick: u64,
}

impl Default for CommandQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandQueue {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn enqueue(&mut self, command: ManagerCommand, tick: u64) {
        self.commands.push(QueuedCommand { command, tick });
    }

    pub fn apply_at_tick(&mut self, tick: u64) -> Vec<ManagerCommand> {
        self.commands
            .drain(..)
            .filter(|cmd| cmd.tick <= tick)
            .map(|cmd| cmd.command)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_simulation() {
        let server = ServerSimulation::new(12345);
        assert_eq!(server.simulation.tick, 0);
    }

    #[test]
    fn test_command_queue() {
        let mut queue = CommandQueue::new();
        queue.enqueue(
            ManagerCommand::ChangeFormation(sim_components::Formation::FourFourTwo),
            100,
        );
        let commands = queue.apply_at_tick(100);
        assert_eq!(commands.len(), 1);
    }

    #[test]
    fn test_validation_invalid_command_rejected() {
        let mut server = ServerSimulation::new(12345);

        // Try to change formation during PreMatch state (should fail)
        let result = server.apply_command(ManagerCommand::ChangeFormation(
            sim_components::Formation::FourThreeThree,
        ));
        assert!(result.is_err());

        match result {
            Err(CommandError::InvalidForState {
                current_state,
                required_state,
            }) => {
                assert_eq!(current_state, sim_components::MatchState::PreMatch);
                assert_eq!(required_state, sim_components::MatchState::InPlay);
            }
            _ => panic!("Expected InvalidForState error"),
        }
    }

    #[test]
    fn test_validation_valid_command_accepted() {
        let mut server = ServerSimulation::new(12345);
        let match_entity = server.simulation.match_entity;

        // Set match state to InPlay (Phase C §4.3: Match is now a Resource).
        server
            .simulation
            .world
            .resource_mut::<sim_components::Match>()
            .state = sim_components::MatchState::InPlay;

        // Try to change formation during InPlay state (should succeed)
        let result = server.apply_command(ManagerCommand::ChangeFormation(
            sim_components::Formation::FourThreeThree,
        ));
        assert!(result.is_ok());
    }

    #[test]
    fn test_state_query_all_state_from_server() {
        let server = ServerSimulation::new(12345);

        // Get state snapshot
        let state = server.get_state();

        // Verify state contains expected fields
        assert_eq!(state.tick, 0);
        assert!(state.match_state.state == sim_components::MatchState::PreMatch);
        assert_eq!(state.score, (0, 0));
        assert!(state.clock.is_running);
        assert!(state.state_hash > 0);

        // Verify players are present (22 players total)
        assert_eq!(state.players.len(), 22);
    }

    #[test]
    fn test_end_to_end_full_match_simulation() {
        let mut server = ServerSimulation::new(42);

        // Phase C §4.3: Match is a Resource.
        server
            .simulation
            .world
            .resource_mut::<sim_components::Match>()
            .state = sim_components::MatchState::Kickoff;

        let full_match_ticks: u64 = 324000;
        for _ in 0..full_match_ticks {
            server.tick();
        }

        let state = server.get_state();
        assert_eq!(state.tick, full_match_ticks);
        assert_eq!(state.players.len(), 22);
        assert!(state.state_hash > 0);
    }

    #[test]
    fn test_determinism_two_seeds_differ() {
        let mut s1 = ServerSimulation::new(111);
        for _ in 0..1000 {
            s1.tick();
        }
        let h1 = s1.get_state().state_hash;

        let mut s2 = ServerSimulation::new(222);
        for _ in 0..1000 {
            s2.tick();
        }
        let h2 = s2.get_state().state_hash;

        assert_ne!(h1, h2);
    }
}
