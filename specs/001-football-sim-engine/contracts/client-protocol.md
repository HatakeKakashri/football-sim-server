# Client Protocol Contract

**Interface**: Server ↔ Client

> **PROVISIONAL — pending Phase 7 design.** The authoritative specification (spec/football-sim-server-spec.md §8, §13 decision #20, §14) defers the network/wire protocol entirely until the core simulation (Phases 0–6) exists. Every message shape, field, transmission rule, and security choice in this document is a placeholder and may change without notice. Do not build clients against this document.

## Purpose
Defines the message protocol between the server-authoritative simulation and rendering clients. Clients receive snapshots and send validated commands.

## Message Types

### Server → Client Messages

#### `MatchSnapshot`
Periodic state snapshot transmitted to clients for rendering.

```rust
struct MatchSnapshot {
    tick: u64,
    match_state: MatchStateView,
    ball: BallView,
    players: Vec<PlayerView>,
    score: (u8, u8),
    clock: ClockView,
}
```

**Transmission Frequency**: Every tick (60 Hz) or configurable tick rate.

**Fields**:
- `tick`: Simulation tick number
- `match_state`: Current match phase (Kickoff, InPlay, etc.)
- `ball`: Ball position, velocity, state
- `players`: Array of all 22 players with positions, teams, roles
- `score`: Current score
- `clock`: Elapsed time, half, added time

#### `GoalScored`
Event message when a goal is scored.

```rust
struct GoalScored {
    team: TeamId,
    scorer: EntityId,
    assist: Option<EntityId>,
    tick: u64,
    time: f32,
}
```

#### `MatchEvent`
Generic event message for significant match occurrences.

```rust
enum MatchEvent {
    Goal(GoalScored),
    Foul(FoulEvent),
    Card(CardEvent),
    Substitution(SubstitutionEvent),
    HalfTime { score: (u8, u8) },
    FullTime { score: (u8, u8) },
}
```

### Client → Server Messages

#### `ManagerCommand`
Validated command from client manager interface.

```rust
enum ManagerCommand {
    ChangeFormation(Formation),
    Substitute { out: EntityId, in: EntityId },
    ChangeMentality(Mentality),
    SetTactic(Tactic),
}
```

**Validation**:
- Command must be valid for current match state
- Player must be available for substitution
- Formation must be valid

#### `ClientAck`
Acknowledgment of received snapshot.

```rust
struct ClientAck {
    last_tick_received: u64,
}
```

## Protocol Details

### Transmission Order
1. Server sends `MatchSnapshot` at each tick (or configured rate)
2. Client processes snapshot and renders state
3. Client sends `ManagerCommand` if needed
4. Server validates and queues command for next tick boundary
5. Server sends event messages as they occur

### State Synchronization
- Server is authoritative; client never corrects server
- Client maintains local state for interpolation between snapshots
- If client detects missing ticks, it requests snapshot from last known tick

### Error Handling
- Invalid commands are silently rejected (no error response to prevent information leakage)
- Malformed messages are dropped
- Client disconnection does not affect simulation

## Security Considerations

1. **Command Validation**: All commands validated server-side before execution
2. **No State Queries**: Client cannot query arbitrary state; only receives snapshots
3. **Rate Limiting**: Commands rate-limited per client to prevent flooding
4. **Encryption**: TLS required for all client-server communication
