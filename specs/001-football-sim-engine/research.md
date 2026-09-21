# Research: Football Match Simulation Engine

**Date**: 2026-09-19

## Resolved Unknowns

### 1. bevy_ecs Usage Pattern

**Decision**: Use `bevy_ecs` as a standalone library with default rendering features disabled.

**Rationale**: The spec mandates bevy_ecs for its mature ECS implementation, query system, and resource management. Using it as a library avoids pulling in rendering dependencies while leveraging battle-tested scheduling and component management.

**Alternatives Considered**:
- Custom ECS: More control but significantly higher implementation effort; bevy_ecs is proven at scale
- `hecs`/`legion`: Less mature scheduling; bevy_ecs has better documentation and community support

### 2. Deterministic PRNG Strategy

**Decision**: Implement Mulberry32 as the seeded deterministic PRNG.

**Rationale**: Mulberry32 is a simple, fast 32-bit PRNG that produces statistically sound results for game simulations. It's well-understood, easy to audit for determinism, and avoids platform-specific floating-point issues by using integer operations internally.

**Alternatives Considered**:
- `rand` crate with `StdRng`: Not guaranteed deterministic across platforms
- `rand_xorshift`: Acceptable but Mulberry32 has better distribution properties for this use case

### 3. Fixed Timestep Implementation

**Decision**: Implement a fixed 60 Hz timestep with accumulator pattern, decoupled from wall-clock time.

**Rationale**: The constitution requires fixed timestep decoupled from render/poll rate. An accumulator pattern is the standard approach: accumulate elapsed time, consume in fixed chunks. The simulation never sees variable dt values.

**Alternatives Considered**:
- Variable timestep: Rejected by constitution (determinism violation)
- Semi-fixed timestep: Adds complexity without benefit for server-only simulation

### 4. State Persistence for Recovery

**Decision**: Serialize simulation state snapshots at configurable intervals (default: every 100 ticks / ~1.67 seconds).

**Rationale**: Crash-recovery SLA (5 seconds) formally adopted by the product team as a product addition beyond the authoritative spec's replay-only scope. At 60 Hz, 300 ticks = 5 seconds. Saving every 100 ticks provides ~3.3 second granularity, well within the 5-second requirement while keeping serialization overhead manageable.

**Alternatives Considered**:
- Save every tick: High I/O overhead for minimal gain
- Event-sourced persistence: Over-engineered for current scope; can be added later

### 5. Manager AI Architecture

**Decision**: Use a weighted decision table rather than full Utility AI for manager decisions.

**Rationale**: The spec explicitly states manager AI uses weighted decision table, not full Utility AI machinery. Manager decisions are discrete (formation change, substitution, mentality shift) and don't require the continuous evaluation that Utility AI provides.

**Alternatives Considered**:
- Full Utility AI: Over-engineered for discrete tactical decisions
- Rule-based system: Lacks the nuance needed for multi-factor tactical decisions

### 6. Possession Resolution in Contested Situations

**Decision**: First contact wins; higher skill wins unless difference is within tolerance threshold.

**Rationale**: Spec clarifies that possession is determined by first contact. For simultaneous claims, skill difference determines winner unless within tolerance (within ~0.1 skill points). This avoids deterministic ambiguity in edge cases. *(Product decision resolving authoritative-spec Open item #17 — 50/50 physical contention — made by the product team: deterministic geometry + skill scalar with 0.1 tolerance. Not derived from the authoritative specification.)*

**Alternatives Considered**:
- Pure first-contact: Too random; skill should matter
- Pure skill-based: First contact provides spatial realism

### 7. FIFA/IFAB Laws of the Game Compliance

**Decision**: Implement 2026/27 FIFA/IFAB Laws as the source of truth for match rules.

**Rationale**: Spec explicitly states FIFA/IFAB Laws 2026/27 are the source of truth. Key rules: Law 3 (minimum 7 players), Law 7 (90 minutes + added time), Law 11 (offside), Law 12 (fouls/advantage).

**Alternatives Considered**:
- Custom ruleset: Defeats the purpose of realistic simulation
- Older FIFA laws: Should use current season for accuracy

### 8. Tolerance Threshold for Skill-Based Outcomes

**Decision**: 0.1 skill point tolerance (on a 0.0-1.0 scale).

**Rationale**: Within 0.1 skill points, the outcome is considered effectively equal and first-contact determines possession. Above 0.1 difference, the higher-skill player wins contested situations. This threshold is configurable for tuning. *(Product decision resolving authoritative-spec Open item #17 — 50/50 physical contention — made by the product team: deterministic geometry + skill scalar with 0.1 tolerance. Not derived from the authoritative specification.)*

**Alternatives Considered**:
- 0.05 tolerance: Too narrow; skill differences too granular
- 0.15 tolerance: Too wide; reduces impact of skill differences

### 9. Match Duration and Added Time

**Decision**: 90 minutes simulation time + referee-determined added time based on FIFA/IFAB Law 7.

**Rationale**: Law 7 specifies added time for substitutions, injuries, time-wasting, and other delays. The referee system tracks stoppage events and calculates appropriate added time (typically 1-5 minutes per half).

**Alternatives Considered**:
- Fixed added time: Less realistic
- No added time: Violates FIFA/IFAB compliance

### 10. Cross-Platform Determinism Scope

**Decision**: Same-build, same-machine determinism (not cross-platform bit-exact).

**Rationale**: The spec explicitly states cross-platform bit-exact determinism is not required. This avoids the complexity of fixed-point math while still enabling deterministic replay for debugging on the same machine.

**Alternatives Considered**:
- Cross-platform bit-exact: Requires fixed-point math, significant complexity
- No determinism: Defeats the core requirement

## Best Practices Research

### Rust ECS with bevy_ecs

- Use `SystemSet` for explicit ordering of order-sensitive systems
- Prefer `Query` with `Without<>` filters over manual entity checking
- Use `Resource` trait for global state (MatchClock, Score, DeterministicRng)
- Avoid `World::get_resource_mut()` in systems; use `ResMut<T>` parameter

### Deterministic Simulation Patterns

- All random numbers from a single `DeterministicRng` resource
- No floating-point operations that depend on evaluation order
- Hash-map iteration must use sorted keys or stable iteration order
- State hash should include all simulation-relevant data, omit timing

### Fixed Timestep Best Practices

- Accumulator pattern with fixed dt = 1/60 second
- Cap maximum accumulated time to prevent spiral of death
- Store remainder time for smooth interpolation (though server-only doesn't need interpolation)

### Testing Deterministic Systems

- State hash comparison across runs with same seed
- Per-tick hash logging for divergence detection
- Adversarial input tests for physics velocity clamping
- Unit tests for AI scoring functions as pure transformations
