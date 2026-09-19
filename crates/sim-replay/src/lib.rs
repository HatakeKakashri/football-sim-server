use bevy_ecs::prelude::*;
use sim_components::{Ball, BallState, Match, MatchClock, Player, Role, ManagerCommand, TeamId, CardColor};
use sim_core::Simulation;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone)]
pub struct TimedCommand {
    pub tick: u64,
    pub command: ManagerCommand,
}

#[derive(Debug, Clone, Resource)]
pub struct ReplaySession {
    pub seed: u64,
    pub commands: Vec<TimedCommand>,
    pub current_tick: u64,
    pub state_hash_history: Vec<u64>,
}

#[derive(Debug, Clone, Resource)]
pub struct SnapshotConfig {
    pub snapshot_interval: u64,
    pub last_snapshot_tick: u64,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            snapshot_interval: 100,
            last_snapshot_tick: 0,
        }
    }
}

impl ReplaySession {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            commands: Vec::new(),
            current_tick: 0,
            state_hash_history: Vec::new(),
        }
    }

    pub fn add_command(&mut self, command: TimedCommand) {
        self.commands.push(command);
    }

    pub fn get_state_hash(&self) -> u64 {
        // Placeholder for state hash computation
        0
    }
}

pub fn replay(seed: u64, commands: Vec<TimedCommand>) -> Result<ReplayResult, String> {
    // Create a new simulation with the given seed
    let mut simulation = Simulation::new(seed);
    
    // Create a match
    let match_entity = simulation.create_match(seed);
    
    // Track state hash history
    let mut state_hash_history = Vec::new();
    let mut event_log = Vec::new();
    let mut divergence_point = None;
    
    // Get total ticks to run (run for 90 minutes at 60 Hz = 324000 ticks)
    let total_ticks = 324000;
    
    for tick in 0..total_ticks {
        // Check if there's a command for this tick
        for timed_command in &commands {
            if timed_command.tick == tick {
                // Apply command
                let _ = simulation.apply_command(match_entity, timed_command.command.clone());
            }
        }
        
        // Run simulation tick
        simulation.tick(sim_core::FIXED_TIMESTEP);
        
        // Get state hash
        let state_hash = simulation.get_state_hash();
        state_hash_history.push(state_hash);
        
        // Check for divergence (compare with previous hash if available)
        if state_hash_history.len() > 1 {
            let previous_hash = state_hash_history[state_hash_history.len() - 2];
            if state_hash != previous_hash {
                // Divergence detected
                if divergence_point.is_none() {
                    divergence_point = Some(tick);
                }
            }
        }
    }
    
    // Get final state
    let final_state = simulation.get_state(match_entity)?;
    
    Ok(ReplayResult {
        final_state,
        event_log,
        divergence_point,
        state_hash_history,
    })
}

#[derive(Debug, Clone)]
pub struct ReplayResult {
    pub final_state: sim_core::MatchSnapshot,
    pub event_log: Vec<MatchEvent>,
    pub divergence_point: Option<u64>,
    pub state_hash_history: Vec<u64>,
}

#[derive(Debug, Clone)]
pub enum MatchEvent {
    Goal { team: TeamId, scorer: Entity, tick: u64 },
    Foul { player: Entity, tick: u64 },
    Card { player: Entity, color: CardColor, tick: u64 },
    Substitution { team: TeamId, out: Entity, substitute: Entity, tick: u64 },
    HalfTime { score: (u8, u8), tick: u64 },
    FullTime { score: (u8, u8), tick: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchSnapshot {
    pub tick: u64,
    pub state_hash: u64,
    pub ball_position: [f32; 2],
    pub player_positions: Vec<([f32; 2], TeamId)>,
    pub score: (u8, u8),
    pub clock: MatchClock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BallSnapshot {
    pub position: [f32; 2],
    pub velocity: [f32; 2],
    pub spin: f32,
    pub state: BallState,
    pub possessor: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSnapshot {
    pub entity_id: u64,
    pub team_id: TeamId,
    pub position: [f32; 2],
    pub velocity: [f32; 2],
    pub stamina: f32,
    pub role: Role,
    pub skill: f32,
}

pub fn event_recording_system(
    match_query: Query<&Match>,
    ball_query: Query<&Ball>,
    player_query: Query<&Player>,
) {
    // Placeholder for event recording
}

pub fn divergence_detection_system(
    replay_session: Res<ReplaySession>,
) {
    // Placeholder for divergence detection
}

pub fn debug_logging_system(
    query: Query<&Player>,
) {
    // Placeholder for debug logging
}

pub fn save_snapshot(snapshot: &MatchSnapshot, path: &str) -> Result<(), String> {
    let encoded = bincode::serialize(snapshot).map_err(|e| e.to_string())?;
    std::fs::write(path, encoded).map_err(|e| e.to_string())
}

pub fn load_snapshot(path: &str) -> Result<MatchSnapshot, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    bincode::deserialize(&data).map_err(|e| e.to_string())
}

pub fn create_snapshot_from_simulation(simulation: &Simulation, match_entity: Entity) -> Result<MatchSnapshot, String> {
    let match_component = simulation.world.entity(match_entity).get::<Match>().ok_or("Match not found")?;
    
    let mut ball_position = [0.0f32; 2];
    for entity in simulation.world.iter_entities() {
        if let Some(ball) = entity.get::<Ball>() {
            ball_position = [ball.position.x, ball.position.y];
            break;
        }
    }
    
    let mut player_positions = Vec::new();
    for entity in simulation.world.iter_entities() {
        if let Some(player) = entity.get::<Player>() {
            player_positions.push(([player.position.x, player.position.y], player.team_id));
        }
    }
    
    Ok(MatchSnapshot {
        tick: simulation.tick,
        state_hash: simulation.get_state_hash(),
        ball_position,
        player_positions,
        score: match_component.score,
        clock: match_component.clock.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replay_session() {
        let session = ReplaySession::new(12345);
        assert_eq!(session.seed, 12345);
        assert_eq!(session.current_tick, 0);
    }

    #[test]
    fn test_replay_determinism() {
        let seed = 12345;
        let commands = Vec::new();
        
        let result1 = replay(seed, commands.clone()).unwrap();
        let result2 = replay(seed, commands).unwrap();
        
        assert_eq!(result1.state_hash_history.len(), result2.state_hash_history.len());
        for (i, (hash1, hash2)) in result1.state_hash_history.iter().zip(result2.state_hash_history.iter()).enumerate() {
            assert_eq!(hash1, hash2, "State hash mismatch at tick {}", i);
        }
        
        assert_eq!(result1.final_state.tick, result2.final_state.tick);
        assert_eq!(result1.final_state.score, result2.final_state.score);
    }

    #[test]
    fn test_divergence_detection() {
        let seed = 12345;
        
        let _result1 = replay(seed, Vec::new()).unwrap();
        
        let mut commands = Vec::new();
        commands.push(TimedCommand {
            tick: 1000,
            command: ManagerCommand::ChangeFormation(sim_components::Formation::FourThreeThree),
        });
        let result2 = replay(seed, commands).unwrap();
        
        assert!(result2.divergence_point.is_some());
        let divergence_tick = result2.divergence_point.unwrap();
        println!("Divergence detected at tick {}", divergence_tick);
    }

    #[test]
    fn test_snapshot_serialization() {
        let snapshot = MatchSnapshot {
            tick: 1000,
            state_hash: 123456789,
            ball_position: [52.5, 34.0],
            player_positions: vec![([10.0, 20.0], TeamId(0))],
            score: (2, 1),
            clock: MatchClock {
                elapsed: 45.0,
                half: 1,
                added_time: 3.0,
                is_running: false,
            },
        };
        
        let encoded = bincode::serialize(&snapshot).unwrap();
        let decoded: MatchSnapshot = bincode::deserialize(&encoded).unwrap();
        
        assert_eq!(decoded.tick, snapshot.tick);
        assert_eq!(decoded.state_hash, snapshot.state_hash);
        assert_eq!(decoded.score, snapshot.score);
    }

    #[test]
    fn test_snapshot_config() {
        let config = SnapshotConfig::default();
        assert_eq!(config.snapshot_interval, 100);
        assert_eq!(config.last_snapshot_tick, 0);
    }
}