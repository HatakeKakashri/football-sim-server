# Implementation Plan: Tick Observability (Post-Match Decision Trace)

**Branch**: `002-tick-observability` | **Date**: 2026-09-27 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/002-tick-observability/spec.md`

## Summary

Add post-hoc, file-based observability of match ticks: player positions and full Utility AI decision traces (every action considered, every consideration's raw/weight/curve/score, and the chosen action), ball state, and referee state (when present). Recorded at a configurable coarse interval (default ~1/sec), with an optional forced full-resolution tick range for deep-dives, all into a single Chrome JSON Trace Format file viewable directly in Perfetto UI. No live/streaming component, no custom viewer, no changes to production snapshot formats.

## Technical Context

**Language/Version**: Rust (edition 2024), consistent with the rest of the workspace

**New Dependencies**: `tracing`, `tracing-chrome`. No other workspace dependency changes.

**New Crate**: `sim-telemetry` — a leaf crate (no dependency on `sim-core`, to avoid a cycle: `sim-core` depends on `sim-ai-player`, and `sim-ai-player` needs `sim-telemetry`, so `sim-telemetry` must sit below both, alongside `sim-components`/`sim-math`).

**Storage**: A single file, path supplied via `--trace-out`, containing Chrome JSON Trace Format events.

**Testing**: `cargo test` — unit tests for `should_record()` boundary conditions; an integration test running a short simulation with tracing enabled and asserting the output parses as well-formed JSON with the expected event count.

**Target Platform**: Same as existing (Linux server, CLI tool) — no new platform surface.

**Project Type**: Library + CLI, unchanged. This feature only adds a crate and modifies existing crates' internals; no new binaries.

**Performance Goals**: Zero measurable overhead when `--trace-out` is not supplied (no subscriber installed at all). No hard numeric target set for the enabled path — consistent with this track's stated priority of a working simulation over performance work; a follow-up benchmarking task can establish one later if needed.

**Constraints**:
- MUST NOT affect simulation determinism — the state hash of a run must be identical with or without tracing enabled (tracing is a pure side channel, reading state to emit events, never feeding back into simulation state or influencing scheduling/ordering).
- MUST NOT introduce a dependency cycle between `sim-telemetry`, `sim-core`, and `sim-ai-player`.
- MUST NOT change the public shape of `sim-core::MatchSnapshot`, `sim-replay::MatchSnapshot`, or the `state-snapshot.md` persistence contract — this is a deliberately separate, fourth snapshot concept (see `data-model.md`).

**Scale/Scope**: One new crate; targeted changes to four existing crates (`sim-core`, `sim-ai-player`, `sim-rules`, `sim-server`); removal of one existing debug block in `sim-server`'s `main.rs`.

## Constitution Check

*GATE: Must pass before implementation begins.*

- **Determinism preserved**: Yes — telemetry emission reads already-computed state to produce trace events; it does not write back into any component, resource, or RNG stream that affects simulation outcome. Verified via SC-004 (state hash equality with/without `--trace-out`).
- **No hash-map iteration / no unseeded RNG introduced in hot paths**: `tracing`'s field recording does not introduce either; span/event field ordering does not affect simulation state, only trace output ordering, which is not covered by the determinism guarantee (only sim state is).
- **Crate boundary discipline**: `sim-telemetry` is a leaf crate; this preserves the acyclic dependency graph established in `001-football-sim-engine`.
- **No unmotivated implementation leakage into spec.md**: The Chrome Trace Format / Perfetto choice is itself a product decision from this session's clarifications, not incidental — see the note in `spec.md`'s Success Criteria section, mirroring how `001`'s checklist treats Rust/bevy_ecs/Mulberry32/60Hz as product requirements rather than leakage.

No violations requiring justification.

## Architecture

```
sim-telemetry (leaf crate: TelemetryConfig, should_record(), tracing-chrome subscriber setup + guard)
     ^                    ^                         ^                        ^
     |                    |                         |                       |
 sim-core            sim-ai-player               sim-rules              sim-server
 (inserts             (checks should_record       (checks should_record  (CLI flags,
  TelemetryConfig      via resource; emits         via resource; emits    installs
  as a per-tick        nested tick→action→         referee card/         subscriber +
  resource, same       consideration spans          stoppage events       guard, removes
  pattern as           inline in                    when Referee          old debug
  CurrentTick)          player_decision_system,      component present)   println! block)
                        plus position/ball
                        snapshot events)
```

Per-tick sequence:
1. `Simulation::tick()` computes `should_record(current_tick, config)` once and inserts/updates a resource read by downstream systems (mirrors existing `CurrentTick` resource insertion).
2. `player_decision_system` reads that resource. When true, it wraps its existing per-action, per-consideration scoring loop in `tracing::span!`/`tracing::event!` calls at the point where those values are already computed — no change to the loop's return type or control flow when tracing is disabled or the tick isn't selected.
3. A ball-state system and a referee system (new, small; referee gated on `Query<&Referee>` being non-empty) emit their own events on the same recorded ticks.
4. `sim-server` installs the `tracing-chrome` subscriber only when `--trace-out` is supplied, before the simulation loop starts, and holds the flush guard until the process exits.

## Phase Breakdown

- **Phase 0 (Research)**: Confirm `tracing-chrome`'s API for the flush-guard pattern and nested span → Chrome "duration event" conversion behavior; confirm `tracing::enabled!()` short-circuit cost is negligible when no subscriber is installed. No open unknowns expected to block design; this is a well-trodden pattern in the Rust ecosystem.
- **Phase 1 (Design)**: This plan + `data-model.md` + `contracts/trace-schema.md`.
- **Phase 2 (Tasks)**: See `tasks.md`.

## Complexity Tracking

No entries — this feature does not require deviating from existing project constraints.
