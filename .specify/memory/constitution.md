# Football Match Simulation Engine Constitution

## Core Principles

### I. Server Authority
The server is the single source of truth. Clients never execute match logic; they render interpolated snapshots. All match state, decisions, and outcomes are determined exclusively by the server.

### II. Fixed Timestep Determinism
The simulation uses a fixed timestep (60 Hz default), decoupled from any render or poll rate. The determinism target is same-build, same-machine reproducibility, not cross-platform bit-exactness.

### III. Strict Pipeline Separation
The tick pipeline follows a strict order: perception → decision → intent → execution/physics. Same-tick read/write separation ensures no agent's decision this tick can observe another agent's not-yet-applied decision from the same tick.

### IV. Purpose-Driven AI
Utility AI is used only where a genuine multi-factor, continuous trade-off exists. Everything else—ball motion, offside, out-of-bounds, goals—is physics or a rule engine. The server ships the smallest deterministic slice first; AI is added only after the substrate is proven reproducible.

### V. Determinism Discipline
Hash-map iteration order, transcendental functions in hot paths, wall-clock time, unseeded RNG, and unordered query iteration for RNG-consuming systems are prohibited. All order-sensitive systems must be explicitly chained; query iteration for RNG-consuming systems must use stable EntityId sorting.

## Technology Stack

- **Language**: Rust (edition 2024)
- **ECS Framework**: `bevy_ecs` used as a library only (default rendering features off)
- **Scheduling**: `Schedule` with every order-sensitive system explicitly chained; never rely on default parallel inference for order-sensitive systems
- **Numeric Representation**: Plain `f32`; transcendentals avoided in the hot path (except logistic curve using `f32::exp()` directly)
- **PRNG**: Mulberry32 seeded deterministic RNG
- **Workspace Structure**: 11-crate workspace with clear dependency boundaries

## Development Workflow

- **Roadmap**: Determinism substrate proven first (Phase 0), before any AI/rules content
- **Testing**: Per-tick state hash/checksum verification; same seed + same inputs → identical result
- **Performance**: Functional completeness outranks performance work; profiling targets are unvalidated projections until Phase 5
- **Documentation**: Single canonical design doc per project convention

## Governance

This constitution supersedes all other development practices for the Football Match Simulation Engine. All pull requests and code reviews must verify compliance with the principles above. Complexity must be justified against these principles. Amendments require documentation, approval, and a migration plan.

**Version**: 1.0.0 | **Ratified**: 2026-09-18 | **Last Amended**: 2026-09-18