# Simulation API Contract

**Interface**: `sim-server` ↔ `sim-core`

## Purpose
Defines how the server-authoritative loop interacts with the simulation engine.

## API Surface

### `Simulation::new(seed: u64) -> Simulation`
Creates a new simulation instance with a deterministic seed.

**Returns**: Initialized simulation with empty world, seeded RNG, and default match state.

### `Simulation::create_match(home_team: TeamConfig, away_team: TeamConfig) -> MatchId`
Creates a new match with two teams.

**Parameters**:
- `home_team`: Team configuration (name, formation, player roster)
- `away_team`: Team configuration

**Returns**: Match identifier for subsequent operations.

### `Simulation::tick() -> TickResult`
Advances the simulation by one fixed timestep (1/60 second).

**Returns**: `TickResult` containing:
- `state_hash: u64` - Hash of simulation state for determinism verification
- `events: Vec<MatchEvent>` - Events that occurred this tick (goals, fouls, etc.)
- `snapshot: Option<MatchSnapshot>` - Periodic state snapshot (if configured)

**Guarantees**:
- Deterministic given same seed and inputs
- No I/O or wall-clock dependencies
- All systems execute in explicit order

### `Simulation::apply_command(match_id: MatchId, command: ManagerCommand) -> Result<(), CommandError>`
Applies a validated manager command at the next tick boundary.

**Parameters**:
- `match_id`: Target match
- `command`: Manager command (formation change, substitution, mentality shift)

**Returns**: Ok(()) if command is valid and queued, Err(CommandError) if invalid.

**Validation Rules**:
- Command must be valid for current match state
- Available resources must exist (e.g., substitutes remaining)
- Commands are applied at tick boundaries, not immediately

### `Simulation::get_state(match_id: MatchId) -> MatchState`
Returns current match state snapshot.

**Returns**: Read-only snapshot of match state for client transmission.

### `Simulation::get_state_hash(match_id: MatchId) -> u64`
Returns hash of current state for determinism verification.

**Returns**: u64 hash of all simulation-relevant state.

### `Simulation::replay(seed: u64, commands: Vec<TimedCommand>) -> ReplayResult`
Replays a match from seed with recorded commands for debugging.

**Parameters**:
- `seed`: Original match seed
- `commands`: Commands with their tick timestamps

**Returns**: `ReplayResult` containing:
- `final_state: MatchState` - Final match state
- `event_log: Vec<MatchEvent>` - All events in chronological order
- `divergence_point: Option<u64>` - Tick where replay diverged (if any)

## Error Types

### `CommandError`
```rust
enum CommandError {
    InvalidForState { current_state: MatchState, required_state: MatchState },
    NoSubstitutesRemaining,
    PlayerNotOnPitch,
    FormationInvalid,
    CommandCooldownActive,
}
```

## Determinism Contract

1. `tick()` with same seed and inputs produces identical `state_hash`
2. `apply_command()` does not affect determinism; commands are part of input
3. `replay()` must reproduce identical event sequence
4. State hash includes: positions, velocities, stamina, ball state, score, clock
5. State hash excludes: timing, I/O, external state
