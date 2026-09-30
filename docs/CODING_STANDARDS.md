# Coding Standards

> **Status**: New (Phase F, PR F0). Effective immediately on merge. Existing
> `#[allow(...)]` annotations stay; new code is expected to follow this doc
> unless an existing pattern in the touched file already establishes a
> different convention (then match the file).

This document is the authoritative coding standard for the `football-sim-server`
workspace. It pairs with the codebase review at `docs/reviews/CODEBASE_REVIEW.md`
and the phased evolution plan at `docs/reviews/CODEBASE_REVIEW.md` §7.

---

## 1. Scope and authority

- Applies to every Rust crate in `crates/`, the `sim-server` CLI, and any
  future workspace member.
- `cargo clippy --workspace --all-targets -- -D warnings` is the **floor**;
  PRs that add new clippy warnings fail CI.
- Specific clippy lints may be denied workspace-wide via `#![deny(...)]` in
  `crates/<crate>/src/lib.rs`; existing denies are listed in §2.
- Where this document contradicts code in the repo, the **code is wrong**
  until a PR fixes it (or this doc is amended).

## 2. Clippy policy

- New code is written against `clippy::pedantic`. Where `pedantic` flags a
  pattern that doesn't apply here, prefer `#[expect(...)]` (forward-compat
  with rustc) over `#[allow(...)]`.
- Every `#[allow(...)]` / `#[expect(...)]` **must** carry a
  `reason = "..."` argument that explains why the lint doesn't apply at
  this site.
- Cluster `#[allow(...)]` attributes when more than one lint applies to
  the same item (one attribute, comma-separated lints).
- The following lints are denied **workspace-wide** (set in each crate's
  `lib.rs` top-of-file; do not locally override without a reason):
  - `clippy::unwrap_used` — use `expect("invariant: …")` instead, or
    restructure to `match` / `?`.
  - `clippy::expect_used` is **not** denied (legitimate at startup paths).
- New `unsafe` requires an inline `// SAFETY:` comment with the same
  evidence as the `clippy::missing_safety_doc` lint expects.

## 3. Naming

- **Types** are `CamelCase`. **Functions and variables** are `snake_case`.
  **Constants and statics** are `SCREAMING_SNAKE_CASE`. **Modules** are
  `snake_case`.
- Names reveal intent. If a name needs a comment to explain what it does,
  the name is wrong (`Mysterious Name` smell).
- A type cluster follows the convention `Foo` / `FooMarker` / `FooView` /
  `FooSnapshot`:
  - `Foo`: the canonical ECS Component or Resource.
  - `FooMarker`: a zero-sized tag component that lets systems find the
    single entity carrying the data (e.g. `BallMarker`).
  - `FooView`: a read-only projection handed out to consumers that
    shouldn't mutate the underlying data.
  - `FooSnapshot`: a serializable / cloneable record of `Foo`'s state at a
    point in time (used by replay / persistence).
- Crate names are `sim-<role>` (`sim-math`, `sim-core`, `sim-ai-player`,
  …). Binary crates in the workspace are `<role>-bin`.
- Avoid stuttering: `sim_components::Match` is fine; `sim_components::MatchComponent` is not.

## 4. Module organization

- **File-size budget: 500 LoC for `lib.rs`.** When a `lib.rs` exceeds 500
  LoC, the next PR must split it into focused submodules. See §11 for the
  concrete submodule layout for the three current offenders.
- Use `mod foo;` (file `foo.rs` next to `lib.rs`), **not** `mod foo;` in
  a `mod.rs`. The repo uses Rust 2018+ module conventions consistently;
  do not introduce `mod.rs` files.
- A `lib.rs` contains:
  1. Crate-level `#![deny(...)]` if any.
  2. `pub use` re-exports (the public surface).
  3. The primary type(s) of the crate (e.g. `Simulation`).
  4. Nothing else. Helpers, systems, default constructors, and tests go
     into submodules.
- A submodule named `time` / `math` / `geometry` for a focused utility
  collection is preferred over an inline `impl` block in `lib.rs`.

## 5. `pub` visibility policy

- **Default to `pub(crate)`** for items that exist only to support the
  crate's `lib.rs` API. Only widen to `pub` at the public boundary.
- A `pub` field on a `pub` struct is the strongest possible API
  commitment (it becomes part of every consumer's pattern-matching).
  Prefer a `pub(crate)` field plus a `pub fn accessor(&self) -> T` so the
  invariant (if any) is preserved. Example from
  `sim-ai-player::DecisionEvaluationCount` (post-F0).
- A `pub` type that is only used by **tests** in the same crate is fine;
  a `pub` field on such a type is not — tests should use the accessor
  pattern.
- `pub fn from_<other_crate>(x: OtherType)` adapters that exist only to
  work around a `pub(crate)` boundary are an anti-pattern; widen the
  underlying item's visibility instead.

## 6. `#[allow]` / `#[expect]` discipline

- Every allow / expect **must** have `reason = "..."`. A bare
  `#[allow(clippy::foo)]` fails review.
- Prefer `#[expect(clippy::foo, reason = "...")]` over
  `#[allow(...)]` when the lint is expected to fire today but might stop
  firing in a future rustc / clippy. `expect` surfaces when the
  expectation is wrong.
- File-level `#![allow(...)]` is allowed **only** for crate-wide
  decisions (e.g. `#![allow(clippy::needless_pass_by_value)]` for a
  crate that exposes a generic builder API). A file-level allow without
  a reason fails review.
- One-off inline allows must be on the **narrowest item that the lint
  applies to** (a single function, a single struct), not on the whole
  module.

## 7. `main.rs` vs `lib.rs`

- A crate with a binary entry point has **both** a `lib.rs` and a
  `main.rs`. All logic lives in `lib.rs` (and its submodules);
  `main.rs` is a thin CLI dispatcher that:
  - parses CLI args (`clap` is the workspace convention),
  - constructs the `lib.rs` API surface,
  - prints / writes output,
  - returns `Result<(), Box<dyn Error>>` (or a typed alias).
- A `main.rs` larger than ~250 LoC is a smell — it means CLI plumbing
  has crept into the entry point. Move dispatch into a
  `cli::dispatch()` fn in `lib.rs` and call it from `main.rs`.
- Tests for the CLI live in `lib.rs` (`#[cfg(test)] mod tests`); do
  **not** test `main.rs` directly.

## 8. Error handling

- Fallible operations return `Result<T, E>` with a typed `E`. `String`
  errors are allowed only at the **crate boundary** (the top-level
  public API of `sim-core` / `sim-server`).
- `sim-components` owns **shared error types** (e.g. `CommandError`).
  `sim-core` reuses them rather than defining its own parallel
  `String`-error variant.
- `.unwrap()` is forbidden (see §2). `.expect("invariant: ...")` is
  legitimate at startup / test paths where the failure mode is a
  programmer error and the message is the fix.
- `?` propagation is preferred over nested `match` for error handling.
- Logging an error and continuing is **never** the default. If a
  recovery path exists, it must be documented at the call site; if it
  doesn't, propagate with `?`.

## 9. ECS shape

- **Singletons** (`Match`, `Ball`, `MatchClock`) are `Resource`s after
  Phase C. Do not regress to `Component` on the match entity.
- **Per-entity state** is `Component`. Per-entity state that is queried
  by many systems benefits from being a `Component` even when the value
  could be a field on `Player` (Phase B eliminated the struct-mirror
  pattern).
- **Tag components** are zero-sized (`pub struct FooMarker;`) and
  `#[derive(Component, Debug, Clone, Copy)]`. They exist so systems can
  find a specific entity via `Query<&FooMarker>` without scanning
  `world.iter_entities()`.
- **Systems** live in submodules (e.g.
  `sim-ai-player::perception::perception_system`). They are `pub fn`
  when registered externally, `fn` when registered by the local
  `register_systems` helper.
- A system's `SystemParam` set is taken as a single tuple of the
  standard Bevy parameter types — no `World` reach-in (the legacy
  `world.iter_entities()` pattern is banned; see §12).

## 10. Domain constraints

- **Determinism** is a hard requirement. The following are forbidden
  in simulation systems:
  - `HashMap` / `HashSet` iteration (iteration order is randomized per
    process). Use a `BTreeMap`, an index array, or a stable ordering
    by `Entity::to_bits()`.
  - `f32` ordering assumptions across processes (use `f32::total_cmp`
    where stable ordering matters, not `<` / `>`).
  - `Instant::now()` / `SystemTime` (wall-clock is non-deterministic).
    Time is `MatchClock.elapsed_ticks` (60 Hz).
  - `rand` / `getrandom` (use `sim-physics::SimRng`, an LCG seeded
    deterministically from `Match.seed`).
- **RNG state** is part of the deterministic state contract. A future
  PR will fold the live `SimRng` state into `get_state_hash`; new
  systems that consume RNG should be testable for "same seed →
  same RNG draws" without re-reading the full world state.
- **Tick time**: 60 Hz. All in-simulation durations are expressed in
  ticks. Conversions to minutes / seconds go through
  `sim-components::time::*` (which is the single source for the
  constants).
- **Snapshot contracts**: `RecordedSnapshot` (in `sim-replay`) and
  `MatchSnapshot` (in `sim-core`) are distinct types and remain so;
  do not merge them (spec §9).

## 11. File-size budget — current offenders

The 500-LoC budget (§4) means the following files must be split as part
of Phase F. The proposed submodule layouts are listed for review but
can be amended in the per-PR review.

### `crates/sim-core/src/lib.rs` (1807 LoC) — split into:

- `lib.rs` — re-exports, `Simulation`, `SimulationSet`, view types,
  `register_systems`.
- `simulation/new.rs` — `Simulation::new`, `Simulation::tick`,
  `Simulation::get_state`, `Simulation::create_match`.
- `simulation/lifecycle.rs` — `lifecycle_system` (108 LoC),
  `apply_player_slot`.
- `simulation/formation.rs` — `reset_formation_to_4_4_2`.
- `simulation/brain_default.rs` — `default_utility_brain` (185 LoC).
- `simulation/state_hash.rs` — `get_state_hash` (110 LoC),
  `match_state_discriminant`, `ball_state_discriminant`,
  `role_discriminant`.
- `tests.rs` — all `#[cfg(test)] mod tests` content.

### `crates/sim-ai-player/src/lib.rs` (1632 LoC) — split into:

- `lib.rs` — re-exports, `DecisionEvaluationCount`, constants
  (`DECISION_CADENCE_TICKS`, `player_stagger_slot`), `Consideration`,
  `ConsiderationContext`.
- `brain.rs` — `UtilityBrain`, `PlayerAction`, `intent_kind`,
  `default_consideration_score`.
- `perception.rs` — `PerceptionSnapshot`, `NearbyEntity`,
  `perception_system`.
- `decision.rs` — `player_decision_system`,
  `consideration_scoring_system`.
- `execution.rs` — `player_action_execution_system`,
  `kick_execution_system`, `steer`, `nearest_teammate_position`, kick
  constants.
- `tests.rs` — test modules.

### `crates/sim-rules/src/lib.rs` (1357 LoC) — split into:

- `lib.rs` — re-exports, `CurrentTick`, `PendingRestart`,
  `AttackingDirection`.
- `out_of_bounds.rs` — `out_of_bounds_system`,
  `calculate_oob_restart_position`.
- `goals.rs` — `goal_detection_system`, `restart_system`.
- `possession.rs` — `possession_resolution_system`.
- `referee.rs` — `offside_detection_system`, `foul_detection_system`,
  `minimum_player_count_system`.
- `clock.rs` — `added_time_calculation_system`,
  `match_duration_enforcement_system`.
- `tests.rs` — test modules.

### `crates/sim-server/src/main.rs` (245 LoC) — already under
budget; no split planned in Phase F.

## 12. Refactor signals

| Smell | Threshold | Action |
|-------|-----------|--------|
| Repeated match arms (≥3 arms, same shape) | 3+ | Extract a macro (`consideration_accessors!`-style) |
| `min_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap_or(Equal))` | 3+ sites | Extract `closest_by_distance(&[NearbyEntity])` helper |
| Per-tick `Vec<T>` allocation in a system | 1+ | Convert to `Local<ScratchBuffer<T>>` resource |
| Three or more fields travelling together | 3+ sites | Extract a struct |
| Magic float / int constants | 3+ sites, no name | Promote to a named `pub const` in the relevant module |
| `apply_command`-style validation duplicated across crates | 1+ duplicate | Move validation into `sim-core`; downstream crates delegate (see PR F5) |
| `Intent`-style `match` repeated across ≥3 sites | 3+ sites | Add a method on the enum (e.g. `Intent::steer_target(&self, ...)`); the sites call the method |

The thresholds are deliberately low. A smell found at threshold-1 is a
todo; a smell found at threshold+2 is a blocker.

## 13. Tests

- New behavior lands with a test that fails before the change and passes
  after. The existing `cargo test --workspace --lib` must stay green.
- Tests that are minutes-long (full-match simulations) get
  `#[ignore = "..."]` with a comment that says how to run them
  explicitly.
- Test setup boilerplate (`world.spawn(...).insert(...)` chains of ≥5
  lines) repeated in ≥3 tests is a refactor target: extract a
  `TestWorld::new_with_players()` helper in `sim-components` or a
  future `sim-test-support` crate.

## 14. Review checklist

When reviewing a PR, check the following in order:

1. `cargo build --workspace --all-targets` is green.
2. `cargo clippy --workspace --all-targets -- -D warnings` is green.
3. `cargo test --workspace --lib` is green (ignored tests skipped).
4. No new `pub` items without a `pub(crate)`-vs-`pub` justification.
5. No new `#[allow(...)]` without `reason = "..."`.
6. No new file exceeds the 500-LoC budget.
7. No new `unwrap()` / `expect()` outside startup / test paths.
8. No new `HashMap` / `HashSet` in simulation hot paths.
9. New public types and methods have a one-line `///` doc comment.
10. Tests cover the new behavior at the unit level (no test that
    "happens to pass").

---

**Maintenance.** This document is amended by PR. Each amendment cites
the review finding that motivates it (e.g. *"closes CODEBASE_REVIEW §2.6
named-lookup"*).
