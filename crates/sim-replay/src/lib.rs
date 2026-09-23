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
    replay_with_ticks(seed, commands, 324000)
}

pub fn replay_with_ticks(seed: u64, commands: Vec<TimedCommand>, total_ticks: u64) -> Result<ReplayResult, String> {
    // Create a new simulation with the given seed
    let mut simulation = Simulation::new(seed);

    // Create a match
    let (match_entity, _home_team_entity) = Simulation::create_match(&mut simulation.world, seed);

    // Track state hash history
    let mut state_hash_history = Vec::new();
    let event_log = Vec::new();
    let mut divergence_point = None;
    
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
    _match_query: Query<&Match>,
    _ball_query: Query<&Ball>,
    _player_query: Query<&Player>,
) {
    // Placeholder for event recording
}

pub fn divergence_detection_system(
    _replay_session: Res<ReplaySession>,
) {
    // Placeholder for divergence detection
}

pub fn debug_logging_system(
    _query: Query<&Player>,
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
        // Smoke-test determinism: 1000 ticks is sufficient to exercise the
        // full pipeline (physics, perception, decisions, lifecycle transitions)
        // without paying the 324k-tick cost of a full 90-min match. The
        // sim-core `test_full_match_reaches_full_time` test already validates
        // the full-length path separately.
        let seed = 12345;
        let commands = Vec::new();

        let result1 = replay_with_ticks(seed, commands.clone(), 1000).unwrap();
        let result2 = replay_with_ticks(seed, commands, 1000).unwrap();

        assert_eq!(result1.state_hash_history.len(), result2.state_hash_history.len());
        for (i, (hash1, hash2)) in result1.state_hash_history.iter().zip(result2.state_hash_history.iter()).enumerate() {
            assert_eq!(hash1, hash2, "State hash mismatch at tick {}", i);
        }

        assert_eq!(result1.final_state.tick, result2.final_state.tick);
        assert_eq!(result1.final_state.score, result2.final_state.score);
    }

    #[test]
    fn test_divergence_detection() {
        // Under the new Phase-0 world-state hash, `ChangeFormation` modifies
        // `Team.formation`, which is intentionally NOT in the hash (the spec
        // only includes `Team.id`). So replaying with vs without a formation
        // command should produce identical state-hash histories. The previous
        // placeholder hash (`rng.state + tick`) happened to differ per tick
        // and made the old assertion vacuously true; the real test of
        // divergence detection now lives in `sim-core`'s
        // `test_hash_detects_state_change`. This test now verifies the
        // cross-replay determinism contract: same seed + same commands ⇒
        // identical state hashes per tick, regardless of formation tweaks.
        let seed = 12345;

        let result1 = replay_with_ticks(seed, Vec::new(), 5000).unwrap();

        let mut commands = Vec::new();
        commands.push(TimedCommand {
            tick: 1000,
            command: ManagerCommand::ChangeFormation(sim_components::Formation::FourThreeThree),
        });
        let result2 = replay_with_ticks(seed, commands, 5000).unwrap();

        assert_eq!(
            result1.state_hash_history, result2.state_hash_history,
            "state-hash histories diverged despite same seed and formation-only command"
        );
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