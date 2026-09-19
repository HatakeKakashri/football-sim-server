use sim_core::Simulation;
use sim_components::{Match, MatchClock, MatchState, TeamId};

#[derive(Debug, Clone)]
pub enum ManagerCommand {
    ChangeFormation,
    Substitute,
    ChangeMentality,
    SetTactic,
}

#[derive(Debug, Clone)]
pub enum CommandError {
    InvalidForState,
    NoSubstitutesRemaining,
    InvalidPlayerId,
    InvalidFormation,
}

pub struct ServerSimulation {
    pub simulation: Simulation,
}

impl ServerSimulation {
    pub fn new(seed: u64) -> Self {
        Self {
            simulation: Simulation::new(seed),
        }
    }

    pub fn apply_command(&mut self, command: ManagerCommand) -> Result<(), CommandError> {
        // Placeholder for command validation and application
        Ok(())
    }

    pub fn get_state(&self) -> MatchSnapshot {
        // Placeholder for state snapshot
        MatchSnapshot {
            tick: self.simulation.tick,
            state_hash: self.simulation.get_state_hash(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MatchSnapshot {
    pub tick: u64,
    pub state_hash: u64,
}

pub struct CommandQueue {
    pub commands: Vec<QueuedCommand>,
}

struct QueuedCommand {
    command: ManagerCommand,
    tick: u64,
}

impl CommandQueue {
    pub fn new() -> Self {
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
        queue.enqueue(ManagerCommand::ChangeFormation, 100);
        let commands = queue.apply_at_tick(100);
        assert_eq!(commands.len(), 1);
    }
}