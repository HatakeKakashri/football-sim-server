# Football Match Simulation Engine — Codebase Review

*Rust · ECS · Utility AI · Server-Authoritative*

**Scope.** A working review of the current codebase across all 10 crates
(`sim-math`, `sim-components`, `sim-physics`, `sim-ai-core`, `sim-ai-player`,
`sim-ai-manager`, `sim-rules`, `sim-core`, `sim-replay`, `sim-server`),
informed by the consolidated design spec at `spec/football-sim-server-spec.md`
and the feature specs at `specs/001-…` and `specs/002-…`.

**Construction notes.**

- The previous review (2026-09-28, archived at
  `docs/reviews/archive/CODEBASE_REVIEW_2026-09-28.md`) identified 12
  anti-patterns and 9 architecture improvements. The team has since
  executed nearly all of Phase A + C + D + E + F. This review is a
  fresh assessment of the **current** codebase.
- The codebase builds clean against `cargo check` and `cargo clippy`
  (pedantic+nursery+all lints enabled at workspace level) — review
  notes are forward-looking, not blocking.
- All 100 workspace tests pass (2 ignored: full-match end-to-end tests).

**Current architecture (verified 2026-09-30).**

```
Perception  →  Decision  →  Execution  →  Physics  →  Possession  →  Rules  →  MatchAdmin
```

- `Match` and `Ball` are `Resource`s (not Components).
- `MatchClock` is dual-form (`Resource` + `Component`) — the Component
  form is retained for backwards compat.
- `PerceptionSnapshot` is a `Component` on each player entity.
- `Consideration` is a 17-variant enum with exhaustive dispatch.
- `Intent` is split into `Movement(MovementIntent)` / `Action(ActionIntent)`.
- `ResponseCurve::evaluate` returns a `Score` newtype.
- Decision cadence: `DECISION_CADENCE_TICKS = 6` with `player_stagger_slot`.
- `SimRng` is a deterministic LCG; RNG draw order is stabilised by
  sorting entities by ID in `player_action_execution_system`.
- All `println!` calls have been replaced with `tracing`.

---

## 1. System Understanding

### 1.1 What the system is

A server-authoritative, deterministic, fixed-timestep football match
simulation engine. The server is the single source of truth for match
state; clients would render interpolated snapshots off a future
network protocol (deferred — §13 decision #20). One process owns one match.

### 1.2 Crate boundaries

```
                        ┌──────────────────────────┐
                        │       sim-math           │  Vec2, PitchDimensions, no logic
                        └──────────────────────────┘
                          ▲            ▲      ▲     ▲
                          │            │      │     │
              ┌──────┐ ┌─────┐  ┌────┐ ┌─────┐ ┌─────┐ ┌────────┐
              │sim-  │ │sim- │  │sim-│ │sim- │ │sim- │ │  sim-  │
              │server│ │replay│ │rules│ │physics│ │ai-...│ │ core   │
              └──────┘ └─────┘  └────┘ └─────┘ └─────┘ └────────┘
                  ▲        ▲       ▲      ▲        ▲        ▲
                  └────────┴───────┴──────┴────────┴────────┘
                              sim-components
                  (pure-data components & resources)
```

The dependency graph is clean and matches the spec:

- `sim-server` depends on `sim-core` only (plus `sim-replay` for
  snapshot persistence) — no `sim-rules`, `sim-physics`, or `sim-ai-*`.
- `sim-ai-manager` does not depend on `sim-ai-core`.
- `sim-core` is the composition root, depending on all 7 upstream crates.

### 1.3 Data and control flow

End-to-end on one tick:

1. `Simulation::tick()` inserts `CurrentTick` resource → `schedule.run()` →
   `lifecycle_system` → `tick_clock` → `self.tick += 1`.
2. `SimulationSet::Perception`:
   - `pitch_control_system` (in `sim-physics`) rebuilds the per-cell
     pitch-control grid using typed `Query`s.
   - `perception_system` (in `sim-ai-player`) builds a
     `PerceptionSnapshot` component for each player.
3. `SimulationSet::Decision`: `player_decision_system` evaluates every
   player's `UtilityBrain` against its freshly-built snapshot (cadence-gated)
   and writes `player.intent`.
4. `SimulationSet::Execution`:
   - `player_action_execution_system` steers each player toward the target
     derived from their intent. RNG draws (tackle success, pass completion,
     shot accuracy) feed back into player velocity.
   - `kick_execution_system` (only if a `Ball.possessor` is set) writes
     `ball.kick_velocity` and clears possessor.
5. `SimulationSet::Physics`: `apply_kick_velocity_system` →
   `ball_physics_system` (integrate, clamp, bounce) →
   `player_movement_system` (clamp + integrate).
6. `SimulationSet::Possession`: `possession_resolution_system` updates
   `Ball.possessor` and `Ball.last_touched_by` based on proximity.
7. `SimulationSet::Rules`: OOB → goal → offside → foul → restart.
8. `SimulationSet::MatchAdmin`: added-time calc, minimum-player warning,
   substitution, mentality shift.
9. `lifecycle_system` (outside the schedule): match state machine
   (`decide_transition` + `apply_transition` loop).

### 1.4 Design constraints confirmed

- **Server-authoritative**: clients don't execute any match logic.
- **Single-tick read/write separation**: perception/decision systems only
  write to `Intent`-shaped output and play on the previous tick's resolved
  state.
- **Determinism bar**: same-build, same-machine reproducibility; `f32`
  IEEE-754 operators are stable enough; `transcendentals` (only `exp()`
  in the logistic curve and pitch-control sigmoid) are an accepted named
  exception.
- **Schedule ordering discipline**: every order-sensitive system is
  pinned via `SimulationSet` chaining and `.after(...)`.
- **One RNG** (`SimRng` resource) drives every stochastic game-event
  outcome. RNG draw order is stabilised by sorting entities by ID.
- **`unsafe_code = "forbid"`** at the workspace level. Confirmed.

---

## 2. Anti-Patterns and Design Issues

### 2.1 Unused `rand` crate dependency in three crates

**Location.**
- `crates/sim-physics/Cargo.toml` — `rand` in `[dependencies]`
- `crates/sim-ai-player/Cargo.toml` — `rand` in `[dependencies]`
- `crates/sim-core/Cargo.toml` — `rand` in `[dependencies]`

**What it does.** All three crates declare `rand` as a dependency but
never import it. `SimRng` uses a hand-rolled LCG; no code path touches
`rand::SmallRng` or any other `rand` type.

**Why it is problematic.**

1. **Misleading signal.** A new contributor sees `rand` in `Cargo.toml`
   and assumes the codebase uses it. They may reach for `rand` types
   instead of `SimRng`, breaking determinism.
2. **Unnecessary compile time.** `rand` and its transitive deps
   (`rand_core`, `rand_chacha`) are compiled for every build.
3. **Workspace `Cargo.toml` also lists `rand`** — the workspace-level
   dependency declaration should be removed if no crate uses it.

**Concrete improvement.** Remove `rand` from all three `Cargo.toml`s
and from the workspace `[workspace.dependencies]`.

**Trade-offs.** None. Pure dead-dependency removal.

---

### 2.2 No-op stub systems retained in the schedule

**Location.**
- `crates/sim-ai-player/src/decision.rs:16` — `consideration_scoring_system`
  is a complete no-op, retained "for schedule compatibility".
- `crates/sim-rules/src/lib.rs` — `referee_advantage_system` is a no-op
  stub (`let _ = (ball_res, match_res);`).
- `crates/sim-ai-manager/src/lib.rs:47` — `formation_change_system`
  only logs, has no logic.

**What it does.** These systems are registered in the schedule but
perform no meaningful work. `consideration_scoring_system` is a `const fn`
that takes a `Query` and does nothing. `referee_advantage_system` takes
two resources and discards them. `formation_change_system` iterates teams
and logs the formation.

**Why it is problematic.**

1. **Schedule noise.** Empty systems in the schedule make it harder to
   reason about what actually runs each tick.
2. **False impression of functionality.** A reader sees
   `referee_advantage_system` in the Rules set and assumes advantage
   rule is implemented.
3. **API surface.** All three are `pub` and exported from their crate
   roots, suggesting they're part of the public contract.

**Concrete improvement.**

- `consideration_scoring_system`: delete it and remove from the schedule.
  The real scoring lives inside `player_decision_system`.
- `referee_advantage_system`: either implement it or remove it from the
  schedule and mark it `#[cfg(test)]` or move to a `todo!()` stub.
- `formation_change_system`: either implement formation validation or
  remove it.

**Trade-offs.** None for `consideration_scoring_system`. The other two
need a decision on whether the feature is in scope.

---

### 2.3 Dead code and unused types

**Location.** Multiple crates.

**What it does.** The following types and functions are defined but
never used in production code paths:

| Item | Location | Status |
|------|----------|--------|
| `closest_by_distance` | `sim-ai-player/src/execution.rs:25` | `#[allow(dead_code)]`, only used by unit tests |
| `WorldWrapper` | `sim-core/src/lib.rs:68` | No consumers anywhere |
| `ScheduleWrapper` | `sim-core/src/lib.rs:87` | No consumers anywhere |
| `MatchStateComponent` | `sim-components/src/lib.rs:212` | Never inserted or read |
| `TeamSide` | `sim-components/src/lib.rs:384` | Never used |
| `Referee`, `Card`, `StoppageEvent`, `CardColor` | `sim-components/src/lib.rs:358-380` | Never populated |
| `FoulType::ProfessionalFoul`, `Handball`, `DenyingGoalScoringOpportunity` | `sim-rules/src/lib.rs` | Never constructed |
| `CommandError::NoSubstitutesRemaining`, `PlayerNotOnPitch`, `FormationInvalid`, `CommandCooldownActive` | `sim-components/src/lib.rs:410-414` | Never constructed |
| `Tactic` enum | `sim-components/src/lib.rs:417` | Never stored or read |
| `MatchEvent` enum | `sim-replay/src/lib.rs:146` | `event_log` always empty |
| `SnapshotConfig` | `sim-replay/src/lib.rs:23` | Never used in production |
| `ReplaySession` | `sim-replay/src/lib.rs:15` | Never used in production |
| `PitchDimensions::penalty_area`, `goal_area` | `sim-math/src/lib.rs` | Always zeroed |

**Why it is problematic.**

1. **Cognitive load.** A new contributor must determine what's live and
   what's vestigial. The `_`-prefixed unused variables in
   `perception_system` (see §2.4) are a symptom.
2. **False API surface.** `pub` types like `ReplaySession` and
   `SnapshotConfig` suggest they're part of the replay API, but the
   actual replay path (`replay_with_ticks`) doesn't use them.
3. **Dead `CommandError` variants** suggest validation rules that don't
   exist, misleading callers about what can fail.

**Concrete improvement.**

- Delete `WorldWrapper`, `ScheduleWrapper`, `closest_by_distance`
  (move to `#[cfg(test)]`), `MatchStateComponent`, `TeamSide`,
  `Tactic`, `SnapshotConfig`, `ReplaySession`.
- Either populate `Referee`/`Card`/`StoppageEvent` or remove them.
- Remove unused `CommandError` variants or implement the validation.
- Either populate `PitchDimensions::penalty_area`/`goal_area` or remove
  the fields.

**Trade-offs.** Low-risk deletions. The `Referee` types may be needed
for Phase 5 — if so, add a `// Phase 5: populated by referee system`
comment to signal intent.

---

### 2.4 Unused computed values in `perception_system`

**Location.** `crates/sim-ai-player/src/perception.rs:58-67`

```rust
let (_score_diff, _time_remaining_secs, _home_id, _away_id) = {
    let diff = i16::from(match_res.score.0) - i16::from(match_res.score.1);
    let time_remaining_secs = sim_components::time::match_time_remaining_secs(&clock);
    (diff, time_remaining_secs, sim_components::TeamId(0), sim_components::TeamId(1))
};
```

**What it does.** Computes four values, prefixes them with `_`, and
never uses them. The `match_res` and `clock` reads are still needed for
other fields in the snapshot, but these four computations are dead.

**Why it is problematic.** Wasted cycles per tick, and the `_` prefix
signals the code was meant to be wired up but never was.

**Concrete improvement.** Delete the block. If score differential and
time remaining are needed in `PerceptionSnapshot`, add fields and populate
them properly.

**Trade-offs.** None.

---

### 2.5 Dual-form `MatchClock` (Resource + Component)

**Location.** `crates/sim-components/src/lib.rs:224` — `MatchClock`
derives both `Resource` and `Component`.

**What it does.** The same data is stored twice: once as a `Resource`
and once as a `Component` on the match entity. Systems read from the
Resource form; the Component form is kept "for backwards compatibility
with tests and external code."

**Why it is problematic.**

1. **Two writes for every state change.** `apply_transition` in
   `lifecycle.rs:159-166` writes to both forms.
2. **Drift risk.** Any future write that forgets one form creates
   divergence with no compile-time detection.
3. **Not load-bearing.** All systems now read `Res<MatchClock>`. The
   Component form is only read by tests.

**Concrete improvement.** Remove the `Component` derive from
`MatchClock`. Update the handful of tests that insert it as a Component
to use `world.insert_resource()` instead.

**Trade-offs.** A small test refactor. Behaviour-identical for
production code.

---

### 2.6 `apply_validated_command` is home-team only; substitution and tactic are no-ops

**Location.** `crates/sim-core/src/simulation/commands.rs:64-99`

**What it does.**

- The `_match_id` parameter is ignored; commands always apply to the
  home team.
- `ManagerCommand::Substitute` only logs — no actual substitution
  happens.
- `ManagerCommand::SetTactic` only logs — no tactic is stored.

**Why it is problematic.**

1. **Silent no-op.** A caller who enqueues a `Substitute` command
   gets `Ok(())` but nothing changes. This is worse than an error.
2. **API lie.** The `ManagerCommand` enum suggests these actions are
   supported.
3. **The `substitution_system` in `sim-ai-manager`** does perform
   substitutions (based on stamina), but it's a separate system —
   the manager command path is disconnected from it.

**Concrete improvement.**

- Either implement `Substitute` and `SetTactic` in
  `apply_validated_command`, or return `Err(CommandError::NotImplemented)`
  for these variants.
- Remove the `_match_id` parameter or use it for side-aware application.

**Trade-offs.** Implementing substitution requires deciding which team
and validating the substitute is on the bench. A `NotImplemented` error
is the honest short-term fix.

---

### 2.7 `get_state` takes an ignored `match_id` parameter

**Location.** `crates/sim-core/src/simulation/snapshot.rs` —
`Simulation::get_state(&self, _match_id: Entity) -> Result<MatchSnapshot, String>`

**What it does.** The `_match_id` parameter is prefixed with `_` and
never used. `Match` is a `Resource`, so the lookup doesn't need an
entity ID.

**Why it is problematic.** API noise. Callers must pass an entity they
already have, suggesting the parameter matters when it doesn't.

**Concrete improvement.** Remove the parameter. Update all callers
(`sim-server`, `sim-replay`).

**Trade-offs.** Breaking API change, but mechanical.

---

### 2.8 `Simulation` fields are all `pub`

**Location.** `crates/sim-core/src/simulation.rs:27-34`

```rust
pub struct Simulation {
    pub world: World,
    pub schedule: Schedule,
    pub original_seed: u64,
    pub tick: u64,
    pub match_entity: Entity,
    pub ball_entity: Entity,
}
```

**What it does.** All fields are public, allowing any consumer to
mutate `world`, `schedule`, or `tick` directly, bypassing the
`tick()` pipeline.

**Why it is problematic.**

1. **Bypasses invariants.** A consumer can set `tick = 1000` without
   running the simulation, or mutate `world` between ticks.
2. **Breaks determinism contract.** External mutation of `world`
   isn't captured by the state hash.
3. **`ServerSimulation.simulation` is also `pub`**, compounding the
   issue — a server consumer can reach through and mutate the world.

**Concrete improvement.**

- Make fields private with accessor methods: `world()`, `schedule()`,
  `tick()`, `match_entity()`, `ball_entity()`.
- Keep `original_seed` `pub` or provide a getter.
- Make `ServerSimulation.simulation` private.

**Trade-offs.** Some test code may need updating to use accessors.

---

### 2.9 `offside_detection_system` is a stub

**Location.** `crates/sim-rules/src/referee.rs:35-97`

**What it does.** Detects offside position and logs at most once every
300 ticks. Does not penalise, does not emit a `RuleEvent`, does not
trigger a restart.

**Why it is problematic.** Acknowledged as a Phase-1 stub. The spec
promises a real implementation in Phase 5. The current stub is fine
for now but should be tracked.

**Concrete improvement.** No action needed until Phase 5. Consider
adding a `// TODO(Phase 5): penalise offside` comment.

**Trade-offs.** None.

---

### 2.10 `foul_detection_system` has dead code

**Location.** `crates/sim-rules/src/referee.rs:164-174`

```rust
for (_entity, _player, _pos, vel) in player_query.iter() {
    let speed = vel.0.length();
    if speed < 0.5 {
        // Could be professional foul - player holding ball deliberately
        // Phase 1: just log, don't penalize yet
    }
}
```

**What it does.** Iterates all players, computes speed, checks if it's
< 0.5, and does nothing.

**Why it is problematic.** Dead loop that wastes cycles every tick and
signals unfinished work.

**Concrete improvement.** Delete the loop. If professional foul
detection is planned, add a `// TODO(Phase 5)` comment.

**Trade-offs.** None.

---

### 2.11 `PitchDimensions::standard()` returns zeroed penalty/goal areas

**Location.** `crates/sim-math/src/lib.rs` — `PitchDimensions::standard()`

**What it does.** Returns a `PitchDimensions` with `penalty_area` and
`goal_area` set to `Rect { min: Vec2::new(0,0), max: Vec2::new(0,0) }`.

**Why it is problematic.** These fields are never populated with real
values. If any future code tries to use them for goal-line or
penalty-area checks, it will silently get wrong results.

**Concrete improvement.** Either populate them with real values
(standard pitch: penalty area is 16.5m from goal line, 40.32m wide;
goal area is 5.5m from goal line, 18.32m wide) or remove the fields.

**Trade-offs.** Populating them is trivial and future-proofs the type.

---

### 2.12 `sim-server/tests/determinism_test.rs` duplicates `sim-core` tests

**Location.** `crates/sim-server/tests/determinism_test.rs`

**What it does.** Re-implements the same determinism tests that exist
in `sim-core/src/tests/determinism_tests.rs`.

**Why it is problematic.** Duplication. If the determinism contract
changes, both files need updating.

**Concrete improvement.** Either delete the file (the `sim-core` tests
are authoritative) or make it a `#[path = "..."]` reference.

**Trade-offs.** None.

---

## 3. Rust Idiomaticity Review

### 3.1 Ownership & borrowing

The codebase is clean. `ParamSet` is used correctly in
`perception_system` to avoid query conflicts. The `Vec` snapshot
pattern (collect entity data, release borrow, then mutate) is
idiomatic.

### 3.2 Error handling

- `CommandError` is a proper enum with `#[derive(Debug, Clone, PartialEq, Eq)]`.
- `apply_validated_command` returns `()` — infallible after validation.
- `get_state` returns `Result<MatchSnapshot, String>` — the `String`
  error is a weak point but acceptable for a simulation engine.
- `replay_with_ticks` returns `Result<ReplayResult, String>` — same.

### 3.3 Trait and generic design

- `Consideration` enum with `consideration_accessors!` macro is a
  clean pattern — adding a variant forces a compile error until the
  macro is updated.
- `Score` newtype is well-designed with `ZERO`/`ONE` constants and
  `new()` that clamps.
- `ResponseCurve::evaluate` returning `Score` is the right shape.

### 3.4 Enums and state modelling

- `MatchState` is small and exhaustive.
- `BallState` has 4 variants, all used.
- `Intent` split into `Movement`/`Action` is a clear improvement.
- `CommandError` has 4 unused variants (see §2.3).

### 3.5 Lifetimes

- `ConsiderationContext<'a>` uses a lifetime to borrow `PerceptionSnapshot`
  and `Intent` — correct and zero-copy.
- Most types are `Copy` or `Clone`, so lifetime annotations are rare.

### 3.6 Concurrency and async

- None. The simulation is single-threaded and deterministic.
- Bevy's parallel scheduling is available but not relied upon for
  order-sensitive code.

### 3.7 API design

- `ServerSimulation` is a thin wrapper — good.
- `CommandQueue` is a public type with public fields — should be
  private with `enqueue`/`dequeue` methods.
- `Simulation` fields all `pub` — should be private with accessors.

### 3.8 Allocation and cloning

- `perception_system` allocates `Vec`s for entity data — necessary
  for the borrow-release pattern.
- `player_decision_system` allocates a `Vec<f32>` per player per
  evaluation for consideration scores — small and cadence-gated.
- `PitchControlGrid` uses `Vec<Vec<f32>>` — 16×12 = 192 floats,
  negligible.

### 3.9 Unsafe code

- `unsafe_code = "forbid"` at workspace level. No `unsafe` blocks.
  **Excellent.**

### 3.10 Module and dependency boundaries

- The dependency graph is clean and matches the spec.
- `sim-core` is the composition root with a well-structured submodule
  tree (`construct`, `lifecycle`, `tick`, `commands`, `snapshot`,
  `state_hash`, `formation`, `brain_default`).
- `sim-rules` is split into per-concern files (`clock`, `out_of_bounds`,
  `goals`, `referee`, `possession`).

### 3.11 Testing and observability

- 100 tests across 10 crates, 2 ignored (full-match end-to-end).
- `sim-components/time.rs` has 18 dedicated tests for clock semantics.
- `sim-rules` has 13 tests across OOB, goals, fouls, possession.
- `sim-core` has 22 tests including determinism and lifecycle.
- `sim-replay` has 8 tests including round-trip serialization.
- `tracing` is used throughout for observability.

### 3.12 Appropriate use of Rust-specific design patterns

- Newtype for `TeamId`, `Score`, `Stamina`, `Skill`, etc. — good.
- `SmallVec` in `PerceptionSnapshot` — right pattern.
- `#[expect(...)]` attributes for clippy lints — forward-compatible.
- `const fn` for `player_stagger_slot` and `consideration_accessors!` —
  compile-time evaluation where possible.

---

## 4. Architecture Improvement Opportunities

### 4.1 Remove unused `rand` dependency

See §2.1. Three `Cargo.toml` edits + workspace `Cargo.toml` edit.

### 4.2 Delete dead code and unused types

See §2.3. A single PR removing `WorldWrapper`, `ScheduleWrapper`,
`closest_by_distance`, `MatchStateComponent`, `TeamSide`, `Tactic`,
`SnapshotConfig`, `ReplaySession`, unused `CommandError` variants,
unused `FoulType` variants, and the `Referee`/`Card`/`StoppageEvent`
types (or populate them in Phase 5).

### 4.3 Make `Simulation` fields private

See §2.8. Add accessor methods, update `ServerSimulation` to hold
`Simulation` privately.

### 4.4 Remove dual-form `MatchClock`

See §2.5. Remove the `Component` derive, update tests.

### 4.5 Implement or reject `Substitute` and `SetTactic` commands

See §2.6. Either wire them up or return `NotImplemented`.

### 4.6 Populate `PitchDimensions` penalty/goal areas

See §2.11. Trivial data fix.

### 4.7 Delete `sim-server/tests/determinism_test.rs`

See §2.12. The `sim-core` tests are authoritative.

---

## 5. Prioritised Recommendations

### 5.1 High impact / low effort

1. **Remove unused `rand` dependency** (§2.1). Four `Cargo.toml` edits.
2. **Delete dead code** (§2.3). One PR, no behaviour change.
3. **Remove unused computed values in `perception_system`** (§2.4).
   Delete 10 lines.
4. **Delete `sim-server/tests/determinism_test.rs`** (§2.12).
5. **Populate `PitchDimensions` penalty/goal areas** (§2.11).
6. **Delete dead loop in `foul_detection_system`** (§2.10).

### 5.2 High impact / high effort

1. **Make `Simulation` fields private** (§2.8). API change, requires
   updating `ServerSimulation` and test code.
2. **Remove dual-form `MatchClock`** (§2.5). Requires test updates.
3. **Implement or reject `Substitute`/`SetTactic`** (§2.6). Needs a
   product decision.

### 5.3 Lower-priority improvements

1. **Remove no-op stub systems** (§2.2). Needs a decision on whether
   `referee_advantage_system` and `formation_change_system` are in
   scope.
2. **Remove ignored `match_id` parameter from `get_state`** (§2.7).
   Breaking API change.
3. **Add `// TODO(Phase 5)` comments to offside/foul stubs** (§2.9,
   §2.10).

---

## 6. Suggested Evolution Path

### Phase G: Dead code cleanup (one PR)

**Goal:** Remove all unused dependencies, types, and dead code paths.

1. Remove `rand` from all `Cargo.toml`s.
2. Delete `WorldWrapper`, `ScheduleWrapper`, `closest_by_distance`,
   `MatchStateComponent`, `TeamSide`, `Tactic`, `SnapshotConfig`,
   `ReplaySession`.
3. Remove unused `CommandError` and `FoulType` variants.
4. Delete dead loop in `foul_detection_system`.
5. Delete unused computed values in `perception_system`.
6. Delete `sim-server/tests/determinism_test.rs`.
7. Populate `PitchDimensions` penalty/goal areas.

### Phase H: API hardening (one PR)

**Goal:** Make `Simulation` and `ServerSimulation` fields private.

1. Add accessor methods to `Simulation`.
2. Make `ServerSimulation.simulation` private.
3. Make `CommandQueue.commands` private with `enqueue`/`dequeue`.
4. Remove `_match_id` parameter from `get_state`.

### Phase I: Command completion (one PR)

**Goal:** Either implement or explicitly reject `Substitute` and
`SetTactic`.

1. Decide: are these in scope for the current phase?
2. If yes: implement in `apply_validated_command`.
3. If no: return `Err(CommandError::NotImplemented)`.

### Phase J: MatchClock Component removal (one PR)

**Goal:** Single-form `MatchClock` as Resource only.

1. Remove `Component` derive.
2. Update tests to use `world.insert_resource()`.
3. Remove the dual-write in `apply_transition`.

---

## Appendix A: Evidence index

- **Unused `rand`**: `sim-physics/Cargo.toml`, `sim-ai-player/Cargo.toml`,
  `sim-core/Cargo.toml` — all declare `rand` but never `use` it.
- **No-op stubs**: `sim-ai-player/src/decision.rs:16`,
  `sim-rules/src/lib.rs` (`referee_advantage_system`),
  `sim-ai-manager/src/lib.rs:47`.
- **Dead code**: `sim-core/src/lib.rs:68,87` (`WorldWrapper`,
  `ScheduleWrapper`); `sim-ai-player/src/execution.rs:25`
  (`closest_by_distance`); `sim-components/src/lib.rs:212,384,417`.
- **Unused perception values**: `sim-ai-player/src/perception.rs:58-67`.
- **Dual-form MatchClock**: `sim-components/src/lib.rs:224`;
  `sim-core/src/simulation/lifecycle.rs:159-166`.
- **Home-team-only commands**: `sim-core/src/simulation/commands.rs:64-99`.
- **Ignored `match_id`**: `sim-core/src/simulation/snapshot.rs`.
- **Public `Simulation` fields**: `sim-core/src/simulation.rs:27-34`.
- **Offside stub**: `sim-rules/src/referee.rs:35-97`.
- **Dead foul loop**: `sim-rules/src/referee.rs:164-174`.
- **Zeroed pitch areas**: `sim-math/src/lib.rs` (`PitchDimensions::standard()`).
- **Duplicated tests**: `sim-server/tests/determinism_test.rs`.

## Appendix B: Out of scope for this review

- The wire protocol (spec §13 #20) — deferred.
- Phase 5+ unimplemented rules (advantage, cards, real offside penalty).
- Networking / persistence layer.
- Manager AI's deeper behaviour (momentum factor, §5.1).
- The bincode snapshot format in `sim-replay` — works; no critique.
