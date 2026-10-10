# State Snapshot Contract

**Interface**: Persistence & Replay

## Purpose
Defines the serialized snapshot format for deterministic replay and state recovery.

## Snapshot Format

### `MatchSnapshot`
Complete match state for persistence.

```rust
struct MatchSnapshot {
    version: u32,
    seed: u64,
    tick: u64,
    match_clock: MatchClock,
    score: (u8, u8),
    ball: BallSnapshot,
    players: Vec<PlayerSnapshot>,
    teams: Vec<TeamSnapshot>,
    referee: RefereeSnapshot,
    state_hash: u64,
}
```

**Version**: Schema version for forward compatibility.

### `BallSnapshot`
Ball state at snapshot time.

```rust
struct BallSnapshot {
    position: [f32; 2],
    velocity: [f32; 2],
    spin: f32,
    state: BallState,
    possessor: Option<u64>,
}
```

### `PlayerSnapshot`
Player state at snapshot time.

```rust
struct PlayerSnapshot {
    entity_id: u64,
    team_id: u8,
    position: [f32; 2],
    velocity: [f32; 2],
    stamina: f32,
    role: Role,
    skill: f32,
    active_action: Option<ActiveAction>,
}
```

### `TeamSnapshot`
Team state at snapshot time.

```rust
struct TeamSnapshot {
    team_id: u8,
    formation: Formation,
    mentality: Mentality,
    substitutions_used: u8,
}
```

### `RefereeSnapshot`
Referee state at snapshot time.

```rust
struct RefereeSnapshot {
    advantage: Option<Advantage>,
    cards_count: (u8, u8),
}
```

## Persistence Strategy

### Snapshot Interval
- **Default**: Every 100 ticks (~1.67 seconds at 60 Hz)
- **Configurable**: Via `snapshot_interval` in simulation config
- **Minimum**: Every tick (for debugging)
- **Maximum**: Every 300 ticks (5 seconds, meets recovery requirement)

### Storage Format
- **Binary**: `bincode` serialization for compact representation
- **Compression**: Optional LZ4 compression for large snapshots
- **Checksum**: SHA-256 hash of serialized data for integrity verification

### File Naming
```
matches/{match_id}/snapshots/tick_{tick:08d}.bin
matches/{match_id}/snapshots/latest.bin
matches/{match_id}/seed.bin
```

## Replay Contract

### `ReplaySession`
Manages deterministic replay of a match.

```rust
struct ReplaySession {
    seed: u64,
    commands: Vec<TimedCommand>,
    current_tick: u64,
    state_hash_history: Vec<u64>,
}
```

### `TimedCommand`
Command with its execution tick.

```rust
struct TimedCommand {
    tick: u64,
    command: ManagerCommand,
}
```

### Replay Flow
1. Load seed from `seed.bin`
2. Initialize simulation with seed
3. For each tick up to target:
   a. Check if command scheduled for this tick
   b. Apply command if present
   c. Advance simulation
   d. Compare state hash with recorded hash
4. Report divergence point if hashes mismatch

### Divergence Detection
- State hash comparison at each tick
- If divergence detected, log:
  - Tick number
  - Expected vs actual hash
  - Last matching state snapshot
  - Commands applied since last match

## Recovery Contract

### Recovery Flow
1. Find latest snapshot file
2. Deserialize snapshot
3. Verify state hash matches
4. Re-initialize simulation from snapshot state
5. Continue from snapshot tick

### Recovery Time Guarantee
- Maximum recovery time: 5 seconds (300 ticks)
- Snapshot interval ensures ≤ 100 ticks since last save
- Deserialization + re-initialization must complete in < 1 second

## Serialization Format

### Binary Schema
```rust
#[derive(Serialize, Deserialize)]
struct SnapshotHeader {
    magic: [u8; 4],  // "FSIM"
    version: u32,
    size: u32,
    checksum: [u8; 32],
}

#[derive(Serialize, Deserialize)]
struct SnapshotData {
    header: SnapshotHeader,
    snapshot: MatchSnapshot,
}
```

### Endianness
- Little-endian for all numeric fields
- Network byte order for cross-platform compatibility
