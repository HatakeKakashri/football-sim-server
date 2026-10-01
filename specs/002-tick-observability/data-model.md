# Data Model: Tick Observability

**Date**: 2026-09-27

## New Types (crate: `sim-telemetry`)

### TelemetryConfig

Recording configuration, inserted as a Bevy ECS resource each tick the same way `CurrentTick` is today.

| Field | Type | Description |
|-------|------|-------------|
| interval_ticks | `u64` | Coarse recording cadence. A snapshot is recorded every `interval_ticks` ticks. Default `600` (~10s at fixed 60Hz timestep; was `60` until 2026-10-01, see FR-002). MUST be `> 0`. |
| full_range | `Option<(u64, u64)>` | Inclusive tick range `(start, end)` recorded at full resolution (every tick), independent of `interval_ticks`. Clamped to `0..total_ticks` if out of bounds. |

**Validation Rules**:
- `interval_ticks == 0` is rejected at CLI parse time.
- `full_range` with `start > end` is rejected at CLI parse time.

### should_record(tick, config) -> bool

Pure function: `tick % interval_ticks == 0 || full_range.map_or(false, |(s,e)| tick >= s && tick <= e)`. Checked once per tick by `sim-core::Simulation::tick()` to decide whether to enter the (otherwise skipped) emission path for that tick. When `--trace-out` was never supplied, no subscriber is installed and this check's result is moot — nothing listens regardless.

## Trace Event Shapes (emitted via `tracing`, not persisted as Rust structs)

These are not new components — they're `tracing::event!`/`span!` call sites added inline at points where the relevant data already exists in scope (see `contracts/trace-schema.md` for the exact field names and Perfetto process/thread mapping).

### Position Snapshot (per player, per recorded tick)

Sourced from existing `Position`, `Velocity`, `Stamina` components — no new fields added to `Player`.

| Field | Source | Description |
|-------|--------|-------------|
| x, y | `Position` | Pitch coordinates |
| vx, vy | `Velocity` | Velocity components |
| stamina | `Stamina` | 0.0–1.0 |

### Ball Snapshot (per recorded tick)

Sourced from the existing `Ball` resource (note: `Ball` is a Bevy `Resource`, not a `Component`, in the current codebase).

| Field | Source | Description |
|-------|--------|-------------|
| x, y | `Ball.position` | Pitch coordinates |
| vx, vy | `Ball.velocity` | Velocity components |
| spin | `Ball.spin` | Affects trajectory |
| state | `Ball.state` | `Free \| Possessed \| InFlight \| Dead` |
| possessor | `Ball.possessor` | `Option<Entity>`, rendered as player id or absent |

### Decision Trace (per player, per recorded tick)

Sourced from the values already computed transiently inside `player_decision_system`'s scoring loop, at `sim-ai-player/src/decision.rs`. Note: `Consideration` and `ResponseCurve` currently lack `name()` methods; these will be added during implementation to produce the snake_case identifiers required by `contracts/trace-schema.md`.

- One `tracing` span per player per recorded tick (`player_decision`).
- One nested span per action evaluated that tick (named by `intent_kind()`: `MoveToPosition`, `ChaseBall`, `PassTo`, `ShootAtGoal`, `Tackle`, `MarkOpponent`, `Press`, `SupportRun`, `HoldPosition`), carrying a `chosen: bool` field.
- Within each action span, one field set per consideration evaluated:

| Field | Description |
|-------|-------------|
| consideration | Name of the consideration (e.g. `distance_to_goal`, `teammate_openness`) |
| raw | Raw input value before curve/weight applied |
| curve | Response curve identifier applied (from `sim-ai-core`) |
| weight | Weight applied to this consideration |
| score | Resulting score contributed to the action's aggregate |

### Referee Snapshot (per recorded tick, only when a `Referee` component exists)

Sourced from the existing (currently never-instantiated) `Referee` component in `sim-components`. Emission is gated on `Query<&Referee>` returning a result; absence is not an error.

| Field | Source | Description |
|-------|--------|-------------|
| cards | `Referee.cards` | List of `(player, card_color, tick_issued)` |
| stoppage_events | `Referee.stoppage_events` | List of stoppage occurrences |

## Perfetto Process/Thread/Track Mapping

| Perfetto concept | Maps to |
|---|---|
| Process `"<Team> <entity index> — <Role>"` | One per player (no squad number exists in the codebase) |
| ↳ Thread `Position` | Counter tracks: x, y, vx, vy, stamina |
| ↳ Thread `Decision` | Span per recorded tick → nested span per action → args per consideration |
| Process `"Ball"` | Counter tracks (position, velocity, spin) + instant events (state/possession changes) |
| Process `"Referee"` (absent if no `Referee` component) | Instant events (cards, stoppages) |

This mapping is deliberately a *fourth* snapshot concept, independent of:
- `specs/001-football-sim-engine/contracts/state-snapshot.md` (persistence/recovery contract)
- `sim-replay::RecordedSnapshot` (replay divergence testing)
- `sim-core::MatchSnapshot` / `get_state()` (live client-facing query)

None of those three carry decision-trace or referee data, and their design goals (compactness, versioning, checksums) actively conflict with this feature's goal of maximum, undropped detail. No shared struct is introduced between them.
