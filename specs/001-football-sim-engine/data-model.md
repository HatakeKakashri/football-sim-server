# Data Model: Football Match Simulation Engine

**Date**: 2026-09-19

## Entities

### Match

Represents a football match with teams, score, time, and state.

| Field | Type | Description |
|-------|------|-------------|
| id | `u64` | Unique match identifier |
| home_team | `Entity` | Home team entity reference |
| away_team | `Entity` | Away team entity reference |
| score | `(u8, u8)` | (home_goals, away_goals) |
| clock | `MatchClock` | Current match time |
| state | `MatchState` | Current match phase |
| seed | `u64` | Deterministic PRNG seed |

**Relationships**: Contains two Team entities, one Ball entity, one Referee entity.

**State Transitions**: `PreMatch → Kickoff → InPlay → Stoppage → HalfTime → Kickoff → InPlay → Stoppage → FullTime`

### MatchClock

Tracks simulation time independently of wall-clock time.

| Field | Type | Description |
|-------|------|-------------|
| elapsed | `f32` | Seconds elapsed in current half |
| half | `u8` | Current half (1 or 2) |
| added_time | `f32` | Referee-determined added time |
| is_running | `bool` | Whether clock is advancing |

**Validation Rules**: 
- `elapsed` never exceeds 45 + added_time per half
- `half` is always 1 or 2
- `is_running` is false during stoppages

### Player

Individual player with position, velocity, stamina, role, and decision-making capability.

| Field | Type | Description |
|-------|------|-------------|
| id | `Entity` | Unique player entity |
| team_id | `TeamId` | Team membership |
| position | `Vec2` | Current position on pitch |
| velocity | `Vec2` | Current velocity |
| stamina | `f32` | 0.0 (exhausted) to 1.0 (full) |
| role | `Role` | Positional role |
| skill | `f32` | Overall skill rating 0.0-1.0 (v1 simplification added to support FR-018 / product resolution of authoritative Open item #17; may be replaced by per-consideration skill factors later) |
| perception | `PerceptionSnapshot` | Local spatial perception |
| intent | `Option<Intent>` | Current decision output |
| active_action | `Option<ActiveAction>` | Currently executing action |

**Relationships**: Belongs to one Team. Interacts with Ball.

### Ball

Physics entity with position, velocity, spin, and state.

| Field | Type | Description |
|-------|------|-------------|
| position | `Vec2` | Current position on pitch |
| velocity | `Vec2` | Current velocity |
| spin | `f32` | Ball spin (affects trajectory) |
| state | `BallState` | Free, Possessed, InFlight, OutOfPlay, Dead |
| possessor | `Option<Entity>` | Player possessing the ball (if any) |

**State Transitions**: 
- `Free → Possessed` (player picks up ball)
- `Possessed → InFlight` (player kicks/passes)
- `InFlight → Free` (ball lands)
- `* → OutOfPlay` (ball leaves pitch)
- `* → Dead` (foul, offside, goal)

### Team

Group of players with tactics, formation, and manager.

| Field | Type | Description |
|-------|------|-------------|
| id | `TeamId` | Team identifier |
| name | `String` | Team name |
| formation | `Formation` | Current tactical formation |
| mentality | `Mentality` | Attack/Balance/Defend |
| manager | `Entity` | Manager entity reference |
| players | `Vec<Entity>` | Player entities on pitch |
| substitutes | `Vec<Entity>` | Available substitutes |
| tactics | `TacticalSettings` | Team-level tactical configuration |

**Relationships**: Contains Manager entity. Has many Player entities.

### Manager

Strategic decision-maker with weighted decision table.

| Field | Type | Description |
|-------|------|-------------|
| decision_table | `WeightedDecisionTable` | Factor weights for decisions |
| last_decision_tick | `u64` | Tick of last strategic decision |
| decision_cooldown | `u64` | Minimum ticks between decisions |

**Decision Factors**:
- Score difference
- Time remaining
- Stamina levels
- Momentum
- Tactical matchup

### Simulation

Core engine running deterministic fixed-timestep updates.

| Field | Type | Description |
|-------|------|-------------|
| world | `World` | bevy_ecs World |
| schedule | `Schedule` | Ordered system execution |
| rng | `DeterministicRng` | Seeded PRNG |
| tick | `u64` | Current tick number |
| accumulator | `f32` | Time accumulator for fixed step |

### Referee

Controls match flow, added time, and enforces Laws of the Game.

| Field | Type | Description |
|-------|------|-------------|
| stoppage_events | `Vec<StoppageEvent>` | Events causing added time |
| advantage | `Option<Advantage>` | Active advantage decision |
| cards | `Vec<Card>` | Cards shown this match |

**Decision Output**: Advantage Decision (play on or foul).

### DeterministicRng

Seeded pseudo-random number generator for deterministic simulation.

| Field | Type | Description |
|-------|------|-------------|
| state | `u32` | Internal PRNG state |
| seed | `u64` | Original seed for replay |

**Methods**:
- `next_u32() -> u32` - Next random u32
- `next_f32() -> f32` - Random f32 in [0.0, 1.0)
- `clone_for_entity(entity_id) -> DeterministicRng` - Fork for entity-specific randomness

## Spatial Data

### PitchDimensions

| Field | Type | Description |
|-------|------|-------------|
| width | `f32` | Pitch width in meters (105m standard) |
| length | `f32` | Pitch length in meters (68m standard) |
| penalty_area | `Rect` | Penalty area bounds |
| goal_area | `Rect` | 6-yard box bounds |
| center_circle | `Circle` | Center circle |

### PitchControlGrid

Coarse 2D grid over the pitch providing per-cell attacker/defender time-to-reach values and a sigmoid dominance score, used to evaluate which team controls each area of the pitch.

| Field | Type | Description |
|-------|------|-------------|
| width | `usize` | Grid column count (illustrative: 16) |
| height | `usize` | Grid row count (illustrative: 12) |
| cell_size | `Vec2` | World-space size of one cell, derived from pitch dimensions |
| t_att | `Vec<f32>` | Per-cell attacker time-to-reach (flattened row-major) |
| t_def | `Vec<f32>` | Per-cell defender time-to-reach (flattened row-major) |
| p_control | `Vec<f32>` | Per-cell dominance score, recomputed from `t_att`/`t_def` (flattened row-major) |
| k | `f32` | Sigmoid steepness for dominance score |
| last_recompute_tick | `u64` | Tick of most recent recompute (decision-cadence, not physics tick) |
| version | `u64` | Monotonic version, incremented each recompute |

**Sigmoid dominance score** (per cell, where `x` is the cell index):

```
P_control(x) = 1 / (1 + exp(-k * (t_def(x) - t_att(x))))
```

**Recompute cadence**: on the decoupled decision cadence — never every 60 Hz physics tick. Both teams evaluate against the same freshly-computed grid in the same tick window; the grid is **never** team-staggered (no "team A evaluates on ticks 0/6/12, team B on ticks 3/9/15" pattern).

**Grid resolution status**: Provisional — illustrative `16 × 12` cells; the exact cell count is subject to Phase-5(-equivalent) profiling.

### PerceptionSnapshot

Local spatial perception built once per decision cycle.

| Field | Type | Description |
|-------|------|-------------|
| nearby_teammates | `SmallVec<[NearbyEntity; 8]>` | Closest teammates |
| nearby_opponents | `SmallVec<[NearbyEntity; 8]>` | Closest opponents |
| ball_position | `Vec2` | Ball position relative to player |
| goal_position | `Vec2` | Target goal position |
| pitch_bounds | `PitchBounds` | Distance to pitch edges |

## Enums

### MatchState
```rust
enum MatchState {
    PreMatch,
    Kickoff,
    InPlay,
    Stoppage,
    HalfTime,
    FullTime,
}
```

### BallState
```rust
enum BallState {
    Free,
    Possessed,
    InFlight,
    OutOfPlay,
    Dead,
}
```

### Role
```rust
enum Role {
    Goalkeeper,
    CenterBack,
    FullBack,
    DefensiveMidfielder,
    CentralMidfielder,
    AttackingMidfielder,
    Winger,
    Striker,
}
```

### Mentality
```rust
enum Mentality {
    Attack,
    Balance,
    Defend,
}
```

### Intent
```rust
enum Intent {
    MoveTo(Vec2),
    PassTo(Entity),
    Shoot(Vec2),
    Tackle(Entity),
    Intercept,
    Press,
    HoldPosition,
    SupportAttack,
    TrackBack,
}
```

### Formation
```rust
enum Formation {
    FourFourTwo,
    FourThreeThree,
    ThreeFiveTwo,
    FourTwoThreeOne,
    FiveThreeTwo,
}
```

## Validation Rules

1. **Player Count**: Maximum 11 players per team on pitch (Law 3)
2. **Substitutions**: Maximum 5 substitutions per team (modern rules)
3. **Stamina**: Must be clamped to [0.0, 1.0]
4. **Position**: Must be within pitch bounds (except goalkeeper in penalty area)
5. **Velocity**: Must be clamped to maximum sprint speed (~10 m/s for players, ~30 m/s for ball)
6. **Skill**: Must be in range [0.0, 1.0]
7. **Added Time**: Cannot exceed 10 minutes per half (practical limit)
8. **Manager Commands**: Must validate against match state and available resources
