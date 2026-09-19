# Implementation Plan: Football Match Simulation Engine

**Branch**: `001-football-sim-engine` | **Date**: 2026-09-19 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-football-sim-engine/spec.md`

## Summary

Build a server-authoritative football match simulation engine using Rust with bevy_ecs (as a library). The engine runs a fixed-timestep (60 Hz) deterministic simulation where identical seeds and inputs produce identical results. The architecture follows a strict pipeline: perception → decision → intent → execution/physics. The server is the single source of truth; clients only render snapshots.

## Technical Context

**Language/Version**: Rust (edition 2024)

**Primary Dependencies**: `bevy_ecs` (as library, default rendering features off), `smallvec`

**Storage**: N/A (state persistence via serialized snapshots for recovery)

**Testing**: `cargo test` (Rust standard framework)

**Target Platform**: Linux server (cross-platform determinism for same-build/same-machine)

**Project Type**: library + CLI tool (simulation core as library, CLI for running matches)

**Performance Goals**: 60 Hz fixed timestep, real-time match simulation

**Constraints**: Determinism requires prohibiting hash-map iteration, transcendental functions in hot paths (except `f32::exp()` in logistic curve), wall-clock time, unseeded RNG in simulation core

**Scale/Scope**: 11-crate workspace, ~20 ECS components, 5-stage tick pipeline

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Server Authority | ✅ PASS | Architecture enforces server-only logic execution |
| II. Fixed Timestep Determinism | ✅ PASS | 60 Hz fixed timestep, same-build/same-machine target |
| III. Strict Pipeline Separation | ✅ PASS | 5-stage pipeline with explicit ordering |
| IV. Purpose-Driven AI | ✅ PASS | Utility AI only for multi-factor trade-offs; physics/rules for the rest |
| V. Determinism Discipline | ✅ PASS | PRNG: Mulberry32; stable EntityId sorting for RNG-consuming queries |

**Technology Stack Compliance**:
- ✅ Rust edition 2024
- ✅ bevy_ecs as library (rendering features off)
- ✅ Schedule with explicit system chaining
- ✅ f32 numeric representation
- ✅ Mulberry32 seeded PRNG
- ✅ 11-crate workspace structure

**GATE RESULT**: PASS - No violations detected. Ready for Phase 0 research.

## Project Structure

### Documentation (this feature)

```text
specs/001-football-sim-engine/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit-tasks command)
```

### Source Code (repository root)

```text
football-sim/                    (Cargo workspace)
├── Cargo.toml                   (workspace root)
├── crates/
│   ├── sim-math/                fixed-step-safe math: Vec2 ops, deterministic PRNG (Mulberry32)
│   ├── sim-components/          Position, Velocity, TeamId, Role, Stamina, BallState...
│   ├── sim-physics/             ball + player movement integration, collision response
│   ├── sim-ai-core/             Consideration, ResponseCurve, geometric-mean aggregator
│   ├── sim-ai-player/           concrete player considerations/actions
│   ├── sim-ai-manager/          concrete manager considerations/actions (weighted table)
│   ├── sim-rules/               deterministic rule engine: offside, out-of-play, goals
│   ├── sim-referee/             wraps sim-rules + advantage Decision
│   ├── sim-core/                World, fixed-step scheduler, explicit system ordering
│   ├── sim-replay/              deterministic replay/record harness, state-hash checks
│   └── sim-server/              server-authoritative loop, network boundary
└── tests/
    ├── integration/             end-to-end match tests
    └── determinism/             seed-replay hash verification tests
```

**Structure Decision**: 11-crate workspace with clear dependency boundaries. `sim-server` depends on `sim-core` only, never on `sim-ai-*` internals. `sim-ai-core` has zero knowledge of football.

## Complexity Tracking

No violations to justify.
