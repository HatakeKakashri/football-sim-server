# 002 Tick Observability — Implementation Design

Status: approved by user decisions 1–6 (2026-10-01). Supersedes the `tracing-chrome`
assumption in `specs/002-tick-observability/{plan,tasks,contracts/trace-schema}.md`.

## Approved decisions
1. **Custom `tracing_subscriber::Layer`** in `sim-telemetry` writes Chrome JSON. `tracing-chrome` is NOT added
   (it cannot emit `C` events, per-player `pid`, typed args or sim-time `ts`; verified against 0.7.2).
2. **Decision window**: decisions are traced for every tick in `[t, t+5]` for each recorded tick `t`, keyed on
   `MatchClock.elapsed_ticks`. `DECISION_CADENCE_TICKS = 6` slots, so each player is evaluated exactly once per
   window. SC-002 is amended: *every player evaluated in a window is fully traced*; snapshot count is
   `total_ticks / interval_ticks` (±1) in clock ticks.
3. **Weight** is traced as configured; the scorer ignores it (`geometric_mean` of curve outputs). Scoring is unchanged.
4. **Names**: action = `intent_kind()`; process = `"<Team> <entity index> — <Role>"`; `aggregate_score` is post-hysteresis.
5. **Dependencies**: `tracing-subscriber` (registry+std only) and `serde_json` in `sim-telemetry`; `tracing` in `sim-ai-player`.
6. **Fix `TeamIdComponent` not inserted by `create_match`** as a separate first change (changes state hashes; approved).
7. **Default interval 600** (post-review, 2026-10-01): FR-002 amended from 60 because a full match at 60 was ~660 MB.

## Architecture
`sim-telemetry` (leaf crate; deps: `bevy_ecs`, `tracing`, `tracing-subscriber`, `serde_json`)
- `config`: `TelemetryConfig`, `TelemetryError`, `should_record`, `should_record_decisions`, `parse_full_range`, `clamp_to`.
- `gate`: `TraceGate` resource `{ snapshot: bool, decisions: bool }`, set once per tick by `Simulation::tick`.
- `emit`: call-site helpers + the field-name contract (`trace.pid/tid/name/process/thread`, `tick`) and constants.
- `chrome`: `ChromeTraceLayer<W>`, `TraceGuard` (writes the closing `]` + flushes on drop), `install(path, config)`.

Timestamps are **sim time**: `ts = tick * 1_000_000 / 60 + seq` µs (`seq` = per-tick event counter). No wall clock,
so traces are deterministic and nesting order is guaranteed.

Disabled path: no `TelemetryConfig` resource ⇒ `TraceGate` stays all-false ⇒ no scratch collection, no `tracing`
macro reached, no subscriber installed. `Simulation::enable_telemetry(config)` is the only switch.

### Deviations from task text (forced by the approved decisions; called out for review)
- **T007–T009**: `chosen` is only known after the action loop, so spans cannot be emitted inline without producing
  overlapping (non-nested) siblings. The loop copies `(kind, raw, curve, weight, score)` into a reusable `Local`
  scratch (no change to scoring or return types) and spans/events are emitted after the winner is known.
- **T002/T003**: `tracing-chrome` replaced by the custom layer (decision 1).
- **T004**: `sim-telemetry` added to `sim-core`, `sim-ai-player`, `sim-server` only; `sim-rules` has no emission site
  (referee/ball/player emission lives in `sim-core::simulation::trace_emit`), so a dependency there would be unused.
- **FR-005** (player position/velocity/stamina counters) has no task; covered by the T016 emission system.
- T006 cites FR-011 for the `--trace-full-range` check; the requirement actually being enforced is FR-001/FR-013 semantics.
- `--full-match`: `clamp_to` is skipped (length unknown up front); out-of-range ticks simply never occur.
- Change events (`state_change`, `possession_change`) compare against the last *emitted* value, so a change between
  recorded ticks surfaces at the next recorded tick.
- While the clock is paused, `elapsed_ticks` repeats: snapshot emission **and decision spans** are de-duplicated per clock tick (`TraceGate`), and the per-tick timestamp sequence saturates inside the tick's slot (post-review fix; originally decisions repeated).

## Testing
Unit: config/gate/layer. Integration: real `Simulation` runs to a temp file, parsed as JSON. Determinism:
`get_state_hash` equal with/without telemetry (T019). Every production change is preceded by a failing test.
