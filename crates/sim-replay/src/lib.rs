use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use sim_components::{
    BallState, CardColor, ManagerCommand, Match, MatchClock, Player, Position, Role, TeamId,
};
use sim_core::Simulation;

#[derive(Debug, Clone)]
pub struct TimedCommand {
    pub tick: u64,
    pub command: ManagerCommand,
}

/// Replay a match from a seed and a list of timed manager commands.
///
/// # Errors
///
/// Returns an error string if the underlying `Simulation::get_state` call
/// fails (no `Match` resource registered) or the snapshot cannot be
/// constructed.
pub fn replay(seed: u64, commands: Vec<TimedCommand>) -> Result<ReplayResult, String> {
    replay_with_ticks(seed, commands, 324_000)
}

/// Replay a match for up to `total_ticks` simulation ticks.
///
/// # Errors
///
/// Returns an error string if the underlying `Simulation::get_state` call
/// fails (no `Match` resource registered) or the snapshot cannot be
/// constructed.
pub fn replay_with_ticks(
    seed: u64,
    commands: Vec<TimedCommand>,
    total_ticks: u64,
) -> Result<ReplayResult, String> {
    // Create a new simulation with the given seed. `Simulation::new` already
    // calls `create_match` internally (spawns ball + 22 players + two teams +
    // the match entity and inserts the `Ball` and `Match` resources). Calling
    // `create_match` a second time would orphan the first set of entities and
    // overwrite the live resources, so we use the match entity the
    // constructor already gave us.
    let mut simulation = Simulation::new(seed);
    let match_entity = simulation.match_entity();

    // Track state hash history
    let mut state_hash_history = Vec::new();
    let event_log = Vec::new();
    let mut divergence_point = None;

    for tick in 0..total_ticks {
        // Check if there's a command for this tick
        for timed_command in &commands {
            if timed_command.tick == tick {
                // Apply command. Phase F §F5 split: replay runs against
                // a bare `Simulation` with no command queue, so it applies
                // directly via `apply_validated_command`. Pre-F5 the
                // function returned `Result<(), String>`; the split
                // removes the error path here since replay's pre-F5 callers
                // always discarded the result.
                if let Err(e) = simulation.apply_validated_command(
                    match_entity,
                    timed_command.command.clone(),
                ) {
                    tracing::warn!("command at tick {tick} failed to apply: {e:?}");
                }
            }
        }

        // Run simulation tick
        simulation.tick();

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
    let final_state = simulation.get_state()?;

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
    Goal {
        team: TeamId,
        scorer: Entity,
        tick: u64,
    },
    Foul {
        player: Entity,
        tick: u64,
    },
    Card {
        player: Entity,
        color: CardColor,
        tick: u64,
    },
    Substitution {
        team: TeamId,
        out: Entity,
        substitute: Entity,
        tick: u64,
    },
    HalfTime {
        score: (u8, u8),
        tick: u64,
    },
    FullTime {
        score: (u8, u8),
        tick: u64,
    },
}

/// A match snapshot recorded for replay purposes.
/// Distinct from `sim_core::MatchSnapshot` which is the live state view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedSnapshot {
    pub tick: u64,
    pub state_hash: u64,
    pub ball_position: [f32; 2],
    pub player_positions: Vec<([f32; 2], TeamId)>,
    pub score: (u8, u8),
    pub clock: MatchClock,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BallSnapshot {
    pub position: [f32; 2],
    pub velocity: [f32; 2],
    pub spin: f32,
    pub state: BallState,
    pub possessor: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerSnapshot {
    pub entity_id: u64,
    pub team_id: TeamId,
    pub position: [f32; 2],
    pub velocity: [f32; 2],
    pub stamina: f32,
    pub role: Role,
    pub skill: f32,
}

/// Serialize a `RecordedSnapshot` to disk via `bincode`.
///
/// # Errors
///
/// Returns the bincode serialization error if the snapshot cannot be
/// encoded, or the I/O error if the destination file cannot be written.
pub fn save_snapshot(snapshot: &RecordedSnapshot, path: &str) -> Result<(), String> {
    let encoded = bincode::serialize(snapshot).map_err(|e| e.to_string())?;
    std::fs::write(path, encoded).map_err(|e| e.to_string())
}

/// Read and deserialize a `RecordedSnapshot` from disk via `bincode`.
///
/// # Errors
///
/// Returns the I/O error if the file cannot be read, or the bincode
/// deserialization error if its bytes do not match the
/// `RecordedSnapshot` schema.
pub fn load_snapshot(path: &str) -> Result<RecordedSnapshot, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    bincode::deserialize(&data).map_err(|e| e.to_string())
}

/// Capture the current `Simulation` world as a serializable
/// `RecordedSnapshot`.
///
/// # Errors
///
/// Returns an error string if the `Match` resource is not registered
/// (no match has been created yet).
pub fn create_snapshot_from_simulation(
    simulation: &Simulation,
) -> Result<RecordedSnapshot, String> {
    let match_component = simulation.world().resource::<Match>().clone();

    let mut ball_position = [0.0f32; 2];
    // Phase C §4.3: ball entity is identified by `BallMarker`. There is
    // exactly one such entity per match; iterate to find it.
    for entity in simulation.world().iter_entities() {
        if entity.get::<sim_components::BallMarker>().is_some()
            && let Some(pos) = entity.get::<Position>()
        {
            ball_position = [pos.0.x, pos.0.y];
            break;
        }
    }

    let mut player_positions = Vec::new();
    for entity in simulation.world().iter_entities() {
        if let Some(player) = entity.get::<Player>()
            && let Some(pos) = entity.get::<Position>()
        {
            player_positions.push(([pos.0.x, pos.0.y], player.team_id));
        }
    }

    // Get clock from MatchClock component
    let clock = simulation
        .world()
        .iter_entities()
        .find_map(|e| e.get::<MatchClock>())
        .cloned()
        .unwrap_or(MatchClock {
            elapsed_ticks: 0,
            half: 1,
            added_time_ticks: 0,
            is_running: false,
        });

    Ok(RecordedSnapshot {
        tick: simulation.current_tick(),
        state_hash: simulation.get_state_hash(),
        ball_position,
        player_positions,
        score: match_component.score,
        clock,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

        assert_eq!(
            result1.state_hash_history.len(),
            result2.state_hash_history.len()
        );
        for (i, (hash1, hash2)) in result1
            .state_hash_history
            .iter()
            .zip(result2.state_hash_history.iter())
            .enumerate()
        {
            assert_eq!(hash1, hash2, "State hash mismatch at tick {i}");
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

        let commands = vec![TimedCommand {
            tick: 1000,
            command: ManagerCommand::ChangeFormation(sim_components::Formation::FourThreeThree),
        }];
        let result2 = replay_with_ticks(seed, commands, 5000).unwrap();

        assert_eq!(
            result1.state_hash_history, result2.state_hash_history,
            "state-hash histories diverged despite same seed and formation-only command"
        );
    }

    #[test]
    fn test_snapshot_serialization() {
        let snapshot = RecordedSnapshot {
            tick: 1000,
            state_hash: 123_456_789,
            ball_position: [52.5, 34.0],
            player_positions: vec![([10.0, 20.0], TeamId(0))],
            score: (2, 1),
            clock: MatchClock {
                elapsed_ticks: 45 * 60 * 60,
                half: 1,
                added_time_ticks: 3 * 60,
                is_running: false,
            },
        };

        let encoded = bincode::serialize(&snapshot).unwrap();
        let decoded: RecordedSnapshot = bincode::deserialize(&encoded).unwrap();

        assert_eq!(decoded.tick, snapshot.tick);
        assert_eq!(decoded.state_hash, snapshot.state_hash);
        assert_eq!(decoded.score, snapshot.score);
    }

    /// Round-trip: serialize a `RecordedSnapshot` to bytes and deserialize
    /// back; the result must equal the original. This is the contract that
    /// `save_snapshot` / `load_snapshot` rely on.
    #[test]
    fn test_recorded_snapshot_round_trip() {
        let snapshot = RecordedSnapshot {
            tick: 1000,
            state_hash: 123_456_789,
            ball_position: [52.5, 34.0],
            player_positions: vec![
                ([10.0, 20.0], TeamId(0)),
                ([50.0, 30.0], TeamId(1)),
            ],
            score: (2, 1),
            clock: MatchClock {
                elapsed_ticks: 45 * 60 * 60,
                half: 1,
                added_time_ticks: 3 * 60,
                is_running: false,
            },
        };

        let encoded = bincode::serialize(&snapshot).unwrap();
        let decoded: RecordedSnapshot = bincode::deserialize(&encoded).unwrap();

        assert_eq!(decoded, snapshot);
    }

    /// Round-trip via the file-based save/load API.
    #[test]
    fn test_snapshot_save_load_round_trip() {
        let snapshot = RecordedSnapshot {
            tick: 500,
            state_hash: 987_654_321,
            ball_position: [30.0, 40.0],
            player_positions: vec![([5.0, 10.0], TeamId(0))],
            score: (0, 0),
            clock: MatchClock {
                elapsed_ticks: 100,
                half: 1,
                added_time_ticks: 0,
                is_running: true,
            },
        };

        let path = "/tmp/test_snapshot_round_trip.bin";
        save_snapshot(&snapshot, path).unwrap();
        let loaded = load_snapshot(path).unwrap();

        assert_eq!(loaded, snapshot);

        // Clean up
        std::fs::remove_file(path).ok();
    }

    /// Round-trip for `BallSnapshot`.
    #[test]
    fn test_ball_snapshot_round_trip() {
        let snapshot = BallSnapshot {
            position: [52.5, 34.0],
            velocity: [1.0, 2.0],
            spin: 0.5,
            state: BallState::Free,
            possessor: Some(42),
        };

        let encoded = bincode::serialize(&snapshot).unwrap();
        let decoded: BallSnapshot = bincode::deserialize(&encoded).unwrap();

        assert_eq!(decoded, snapshot);
    }

    /// Round-trip for `PlayerSnapshot`.
    #[test]
    fn test_player_snapshot_round_trip() {
        let snapshot = PlayerSnapshot {
            entity_id: 7,
            team_id: TeamId(1),
            position: [10.0, 20.0],
            velocity: [0.5, 0.5],
            stamina: 0.8,
            role: Role::Striker,
            skill: 0.75,
        };

        let encoded = bincode::serialize(&snapshot).unwrap();
        let decoded: PlayerSnapshot = bincode::deserialize(&encoded).unwrap();

        assert_eq!(decoded, snapshot);
    }

    /// End-to-end: capture a snapshot from a live simulation, save it,
    /// load it back, and verify it matches.
    #[test]
    fn test_create_snapshot_from_simulation_round_trip() {
        let mut sim = Simulation::new(42);

        // Advance a few ticks so the snapshot has non-trivial state.
        for _ in 0..10 {
            sim.tick();
        }

        let snapshot = create_snapshot_from_simulation(&sim).unwrap();

        let path = "/tmp/test_e2e_snapshot_round_trip.bin";
        save_snapshot(&snapshot, path).unwrap();
        let loaded = load_snapshot(path).unwrap();

        assert_eq!(loaded, snapshot);

        // Clean up
        std::fs::remove_file(path).ok();
    }
}
