# Phase E API Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land five small PRs (Phase E) that polish public API surfaces and remove dead code, per `docs/superpowers/specs/2026-09-29-phase-e-api-polish-design.md`.

**Architecture:** Each PR is independent, ships separately, and is verified against `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings`. Order of landing: PR 1 → PR 2 → PR 3 → PR 4 → PR 5. PRs 3 and 4 each have an independent-feature branch off `main` so they can be reviewed in parallel.

**Tech Stack:** Rust workspace, Bevy 0.14 ECS, serde, serde_json (dev-only for tests), bincode (already used by sim-replay), tracing.

**Spec:** `docs/superpowers/specs/2026-09-29-phase-e-api-polish-design.md`

## Global Constraints

- Workspace `unsafe_code = "forbid"`. No `unsafe` blocks ever.
- All workspace tests must pass at every step. Baseline: 72 passing tests.
- `cargo clippy --workspace --all-targets -- -D warnings` must remain clean.
- Use `#[must_use]` on accessors and pure constructors where appropriate.
- Constructors and helpers that wrap invariants go through `#[expect(clippy::..., reason = "...")]` only when the lint is wrong for this context.
- Phase 0 determinism contract — single-seed reproducer per process. Don't break it.
- Commit messages for Phase E follow the prefix `refactor(sim):` per Phase A–D convention.
- No `as` casts on `f32` widths; use the existing `count_to_f32` helper or add an analogous one if a new cast is needed.
- `cargo fmt --all` before every commit.

## File Structure Map (Phase E changes)

| File | PR | Action | Responsibility |
|------|----|----|----------------|
| `crates/sim-components/src/lib.rs` | 1, 4 | Modify | Delete `BallStateComponent` (PR1); remove `OutOfPlay` from `BallState` (PR1); replace flat `Intent` with split shape (PR4) |
| `crates/sim-core/src/lib.rs` | 1, 3, 4 | Modify | Renumber `ball_state_discriminant` arms (PR1); replace all `PlayerConsideration` constructions with `sim_ai_core::Consideration` enum variants (PR3); wrap brain-template intents in `Intent::Movement`/`Intent::Action` (PR4) |
| `crates/sim-core/Cargo.toml` | 5 | Modify | Add `serde_json` as `[dev-dependencies]` |
| `crates/sim-ai-core/src/lib.rs` | 2, 3 | Modify | Add `Score` newtype (PR2); replace string-dispatch `Consideration` with exhaustive enum + `raw_input` (PR3); fix `ResponseCurve::Linear` precedence (PR2) |
| `crates/sim-ai-player/src/lib.rs` | 2, 3, 4 | Modify | Update all `ResponseCurve::evaluate` callers to handle `Score` (PR2); delete `PlayerConsideration` struct (PR3); update `intent_kind`/`player_action_execution_system`/`kick_execution_system`/`steer` to use split `Intent` (PR4) |
| `crates/sim-replay/src/lib.rs` | 5 | Modify | Add `PartialEq` to `RecordedSnapshot`; delete dead `BallSnapshot`/`PlayerSnapshot`; extend `test_snapshot_serialization` |
| `crates/sim-replay/Cargo.toml` | 5 | Modify | Add `serde_json` as `[dev-dependencies]` |
| `crates/sim-core/tests/snapshot_round_trip.rs` | 5 | Create | New round-trip test file |
| `spec/football-sim-server-spec.md` | 1 | Modify | Remove `OutOfPlay` from both enum blocks |
| `specs/001-football-sim-engine/data-model.md` | 1 | Modify | Remove `OutOfPlay` from column listing + transition note + enum block |
| `specs/002-tick-observability/data-model.md` | 1 | Modify | Remove `OutOfPlay` from state listing |
| `specs/002-tick-observability/contracts/trace-schema.md` | 1 | Modify | Remove `OutOfPlay` from `state_change` event schema |

---

## PR 1: `BallStateComponent` removal + `BallState::OutOfPlay` cleanup

**Branch:** `refactor/phase-e-1-ballstate-cleanup`
**Goal:** Delete the dead `BallStateComponent` wrapper, remove the never-constructed `BallState::OutOfPlay` variant, update the 4 spec docs that mention it.

**Pre-flight check:**
```bash
cargo test --workspace 2>&1 | tail -1   # expect "test result: ok."
grep -rn "BallStateComponent" crates/   # expect zero matches
grep -rn "BallState::OutOfPlay" crates/ # expect 1 match (sim-core discriminant)
```

### Task 1.1: Remove `BallStateComponent` from `sim-components`

**Files:**
- Modify: `crates/sim-components/src/lib.rs:150-151`

- [ ] **Step 1: Verify zero usages**

```bash
grep -rn "BallStateComponent" crates/ docs/
```

Expected: zero matches outside the definition itself.

- [ ] **Step 2: Delete the struct**

In `crates/sim-components/src/lib.rs`, delete lines 150-151:

```rust
#[derive(Component, Debug, Clone)]
pub struct BallStateComponent(pub BallState);
```

- [ ] **Step 3: Run tests**

```bash
cargo test --workspace
```

Expected: 72 passed, no failures.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/sim-components/src/lib.rs
git commit -m "refactor(sim): Phase E PR 1a - remove dead BallStateComponent wrapper"
```

### Task 1.2: Remove `BallState::OutOfPlay` variant

**Files:**
- Modify: `crates/sim-components/src/lib.rs:63-70`
- Modify: `crates/sim-core/src/lib.rs:804-813`

- [ ] **Step 1: Remove the variant**

In `crates/sim-components/src/lib.rs`, edit the `BallState` enum (lines 63-70):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BallState {
    Free,
    Possessed,
    InFlight,
    OutOfPlay,
    Dead,
}
```

Change to:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BallState {
    Free,
    Possessed,
    InFlight,
    Dead,
}
```

- [ ] **Step 2: Renumber `ball_state_discriminant`**

In `crates/sim-core/src/lib.rs:804-813`, edit:

```rust
const fn ball_state_discriminant(s: sim_components::BallState) -> u64 {
    use sim_components::BallState as B;
    match s {
        B::Free => 0,
        B::Possessed => 1,
        B::InFlight => 2,
        B::OutOfPlay => 3,
        B::Dead => 4,
    }
}
```

Change to:

```rust
const fn ball_state_discriminant(s: sim_components::BallState) -> u64 {
    use sim_components::BallState as B;
    match s {
        B::Free => 0,
        B::Possessed => 1,
        B::InFlight => 2,
        B::Dead => 3,
    }
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test --workspace
```

Expected: 72 passed. Note: state hash values change for the same simulation; this is acceptable (per spec, the hash is internal and cross-process determinism is not yet guaranteed).

- [ ] **Step 4: Run clippy**

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/sim-components/src/lib.rs crates/sim-core/src/lib.rs
git commit -m "refactor(sim): Phase E PR 1b - remove unused BallState::OutOfPlay variant"
```

### Task 1.3: Update spec docs

**Files:**
- Modify: `spec/football-sim-server-spec.md` (lines 279, 613)
- Modify: `specs/001-football-sim-engine/data-model.md` (lines 69, 76, 221)
- Modify: `specs/002-tick-observability/data-model.md` (line 47)
- Modify: `specs/002-tick-observability/contracts/trace-schema.md` (line 65)

- [ ] **Step 1: Update `spec/football-sim-server-spec.md`**

In both the line 279 and line 613 enum blocks, delete the `OutOfPlay,` line. Replace the transition note implicit in the surrounding text with no change (the variant is just gone).

- [ ] **Step 2: Update `specs/001-football-sim-engine/data-model.md`**

Line 69: change `Free, Possessed, InFlight, OutOfPlay, Dead` to `Free, Possessed, InFlight, Dead`.

Line 76: change `* → OutOfPlay (ball leaves pitch)` to `* → Dead (ball leaves pitch or play is stopped)`.

Line 221: delete `OutOfPlay,` from the enum block.

- [ ] **Step 3: Update `specs/002-tick-observability/data-model.md`**

Line 47: change `Free \| Possessed \| InFlight \| OutOfPlay \| Dead` to `Free \| Possessed \| InFlight \| Dead`.

- [ ] **Step 4: Update `specs/002-tick-observability/contracts/trace-schema.md`**

Line 65: change the `from`/`to` type from `Free \| Possessed \| InFlight \| OutOfPlay \| Dead` to `Free \| Possessed \| InFlight \| Dead`.

- [ ] **Step 5: Verify no remaining references**

```bash
grep -rn "OutOfPlay" spec/ specs/ crates/
```

Expected: zero matches.

- [ ] **Step 6: Commit**

```bash
git add spec/ specs/
git commit -m "docs(spec): Phase E PR 1c - remove BallState::OutOfPlay from all spec docs"
```

### Task 1.4: Final verification for PR 1

- [ ] **Step 1: Run full check + clippy + tests**

```bash
cargo fmt --all
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: all clean; 72+ passed.

- [ ] **Step 2: Push branch and open PR**

```bash
git push origin refactor/phase-e-1-ballstate-cleanup
gh pr create --base main --title "Phase E PR 1: BallState cleanup" \
  --body "Removes dead BallStateComponent wrapper and never-constructed BallState::OutOfPlay variant. Per docs/superpowers/specs/2026-09-29-phase-e-api-polish-design.md PR 1."
```

After approval and merge, continue with PR 2.

---

## PR 2: `Score` newtype + `ResponseCurve::Linear` precedence fix

**Branch:** `refactor/phase-e-2-score-newtype`
**Goal:** Add a `Score(f32)` newtype with `[0.0, 1.0]` invariant, change `ResponseCurve::evaluate` to return it, fix the Linear-arm operator-precedence bug.

**Pre-flight check:**
```bash
cargo test --workspace 2>&1 | tail -1   # expect "test result: ok."
git checkout main && git pull           # sync latest
git checkout -b refactor/phase-e-2-score-newtype
```

### Task 2.1: Add `Score` newtype with failing tests

**Files:**
- Modify: `crates/sim-ai-core/src/lib.rs` (add Score, add tests)
- Modify: `crates/sim-ai-core/src/lib.rs` (modify `ResponseCurve::evaluate` return type and impl)

- [ ] **Step 1: Write failing tests for `Score`**

Append to `crates/sim-ai-core/src/lib.rs` `tests` module (after the existing `test_geometric_mean`):

```rust
#[test]
fn test_score_new_clamps_to_unit_interval() {
    let below = Score::new(-0.5);
    let above = Score::new(1.5);
    let inside = Score::new(0.5);
    assert_eq!(below, Score::ZERO);
    assert_eq!(above, Score::ONE);
    assert_eq!(inside.raw(), 0.5);
}

#[test]
fn test_score_constants() {
    assert_eq!(Score::ZERO.raw(), 0.0);
    assert_eq!(Score::ONE.raw(), 1.0);
}

#[test]
fn test_score_partial_eq() {
    assert_eq!(Score::new(0.5), Score::new(0.5));
    assert_ne!(Score::new(0.5), Score::new(0.4));
}
```

- [ ] **Step 2: Run tests to verify they fail to compile**

```bash
cargo test -p sim-ai-core
```

Expected: compile error `cannot find type Score`.

- [ ] **Step 3: Add the `Score` newtype**

In `crates/sim-ai-core/src/lib.rs`, immediately after the existing imports (or at top of file), add:

```rust
/// A utility-AI score in the closed unit interval `[0.0, 1.0]`.
///
/// Constructed via `Score::new(input)` which clamps the input into range.
/// Use `.raw()` only when arithmetic genuinely requires raw `f32` (e.g.
/// log-space geometric mean combiners); otherwise propagate the `Score` so
/// the type system enforces the invariant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score(f32);

impl Score {
    pub const ZERO: Score = Score(0.0);
    pub const ONE: Score = Score(1.0);

    /// Construct a Score, clamping the input into `[0.0, 1.0]`.
    #[must_use]
    pub fn new(value: f32) -> Self {
        if value.is_nan() {
            return Score(0.0);
        }
        Score(value.clamp(0.0, 1.0))
    }

    /// Lossy accessor returning the underlying `f32`. Use only when
    /// downstream arithmetic requires raw float values.
    #[must_use]
    pub fn raw(self) -> f32 {
        self.0
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p sim-ai-core
```

Expected: 5 passed (was 5; the 3 new tests pass).

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/sim-ai-core/src/lib.rs
git commit -m "refactor(sim): Phase E PR 2a - add Score newtype with [0,1] invariant"
```

### Task 2.2: Change `ResponseCurve::evaluate` return type

**Files:**
- Modify: `crates/sim-ai-core/src/lib.rs:24-49` (`ResponseCurve::evaluate`)
- Modify: `crates/sim-ai-player/src/lib.rs` (all callers)

- [ ] **Step 1: Update `ResponseCurve::evaluate` signature + impl**

In `crates/sim-ai-core/src/lib.rs:24-49`, edit:

```rust
impl ResponseCurve {
    #[must_use]
    pub fn evaluate(&self, input: f32) -> f32 {
        match self {
            Self::Linear { min, max } => (input - min) / (max - min).clamp(0.0, 1.0),
            Self::Logistic {
                midpoint,
                steepness,
            } => {
                let x = steepness * (input - midpoint);
                1.0 / (1.0 + (-x).exp())
            }
            Self::Step {
                threshold,
                below,
                above,
            } => {
                if input < *threshold {
                    *below
                } else {
                    *above
                }
            }
        }
    }
}
```

Change to:

```rust
impl ResponseCurve {
    /// Evaluate the curve at the given input, returning a `Score` clamped
    /// to `[0.0, 1.0]`.
    ///
    /// `Linear` divides by `(max - min).max(f32::EPSILON)` so the
    /// `max <= min` degenerate case yields `Score::ZERO` instead of NaN.
    /// The previous implementation incorrectly clamped the *denominator*
    /// due to operator precedence — fixed in Phase E PR 2.
    #[must_use]
    pub fn evaluate(&self, input: f32) -> Score {
        match self {
            Self::Linear { min, max } => {
                let denom = (max - min).max(f32::EPSILON);
                let raw = (input - min) / denom;
                Score::new(raw)
            }
            Self::Logistic {
                midpoint,
                steepness,
            } => {
                let x = steepness * (input - midpoint);
                Score::new(1.0 / (1.0 + (-x).exp()))
            }
            Self::Step {
                threshold,
                below,
                above,
            } => Score::new(if input < *threshold { *below } else { *above }),
        }
    }
}
```

- [ ] **Step 2: Locate callers in `sim-ai-player`**

```bash
grep -n "ResponseCurve::evaluate\|\\.evaluate(" crates/sim-ai-player/src/lib.rs
```

Expected: 1 call site at line 301 (inside `player_decision_system`).

- [ ] **Step 3: Update the caller**

In `crates/sim-ai-player/src/lib.rs:301`, the call is inside a loop that feeds `Score` values into a geometric-mean combiner. The downstream code calls `geometric_mean(&[f32])` (sim-ai-core) which takes `&[f32]`. Update so the per-consideration `Score` value gets `.raw()` before being pushed into the `Vec<f32>` passed to `geometric_mean`.

Find the block (around line 295-310 — read 295-315 first to confirm exact shape):

```rust
                let raw = compute_consideration_input(
                    &consideration.name,
                    perception,
                    intent,
                    stamina.0,
                    skill.0,
                    grid,
                );
                weighted_scores.push(consideration.weight * raw);
```

After updating `ResponseCurve::evaluate` to return `Score`, the `raw` here is now a `Score`. Change to:

```rust
                let raw = compute_consideration_input(
                    &consideration.name,
                    perception,
                    intent,
                    stamina.0,
                    skill.0,
                    grid,
                );
                weighted_scores.push(consideration.weight * raw.raw());
```

Note: `consideration.name`/`weight`/`curve` remain on `PlayerConsideration` (still a struct in this PR; the enum conversion is PR 3). So we change only the `.raw()` extraction.

- [ ] **Step 4: Run tests**

```bash
cargo test --workspace
```

Expected: 75 passed (was 72; the 3 new Score tests added in Task 2.1). All existing tests still pass.

- [ ] **Step 5: Run clippy**

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/sim-ai-core/src/lib.rs crates/sim-ai-player/src/lib.rs
git commit -m "refactor(sim): Phase E PR 2b - Score newtype in ResponseCurve::evaluate"
```

### Task 2.3: Add a regression test for the Linear-curve precedence fix

**Files:**
- Modify: `crates/sim-ai-core/src/lib.rs` (test module)

- [ ] **Step 1: Add the test**

Append to the test module in `crates/sim-ai-core/src/lib.rs`:

```rust
#[test]
fn test_linear_response_handles_max_eq_min() {
    // Phase E PR 2 regression: before the fix, the Linear arm clamped
    // `(max - min)` (the wrong operand) so `max == min` produced NaN/inf.
    // After the fix, the denominator is `(max - min).max(f32::EPSILON)`
    // and the result is clamped via `Score::new`, so a degenerate
    // `max == min` linear curve returns `Score::ZERO` instead of NaN.
    let degenerate = ResponseCurve::Linear { min: 0.5, max: 0.5 };
    let score = degenerate.evaluate(0.5);
    assert!(!score.raw().is_nan(), "Linear with max==min returned NaN");
    assert_eq!(score, Score::ZERO);
}

#[test]
fn test_linear_response_clamped_at_unit_interval() {
    // Linear curve that overshoots its bounds should still return a
    // valid Score in [0,1] rather than raw f32.
    let curve = ResponseCurve::Linear { min: 0.0, max: 1.0 };
    assert_eq!(curve.evaluate(2.0), Score::ONE);
    assert_eq!(curve.evaluate(-2.0), Score::ZERO);
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p sim-ai-core
```

Expected: 7 passed (was 5; +2 new).

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add crates/sim-ai-core/src/lib.rs
git commit -m "refactor(sim): Phase E PR 2c - Linear-curve precedence regression test"
```

### Task 2.4: Final verification for PR 2

- [ ] **Step 1: Full check**

```bash
cargo fmt --all
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: 75+ passed; clippy clean.

- [ ] **Step 2: Push and open PR**

```bash
git push origin refactor/phase-e-2-score-newtype
gh pr create --base main --title "Phase E PR 2: Score newtype" \
  --body "Adds Score(f32) newtype with [0,1] invariant; fixes ResponseCurve::Linear operator-precedence bug. Per docs/superpowers/specs/2026-09-29-phase-e-api-polish-design.md PR 2."
```

After merge, continue with PR 3.

---

## PR 3: Consideration enum (no more string dispatch)

**Branch:** `refactor/phase-e-3-consideration-enum`
**Goal:** Replace `PlayerConsideration { name: String, ... }` and the 190-line string-dispatch `compute_consideration_input` with an exhaustive enum.

**Pre-flight check:**
```bash
cargo test --workspace 2>&1 | tail -1   # expect 75+ passed
git checkout main && git pull
git checkout -b refactor/phase-e-3-consideration-enum
```

### Task 3.1: Add the `Consideration` enum with failing tests

**Files:**
- Modify: `crates/sim-ai-core/src/lib.rs` (delete old `Consideration` struct; add new enum)

- [ ] **Step 1: Verify old `Consideration` is unused**

```bash
grep -rn "use sim_ai_core::Consideration\|sim_ai_core::Consideration" crates/
```

Expected: zero matches outside the definition.

- [ ] **Step 2: Delete the old struct**

In `crates/sim-ai-core/src/lib.rs:1-5`, delete:

```rust
#[derive(Debug, Clone)]
pub struct Consideration {
    pub name: String,
    pub curve: ResponseCurve,
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test --workspace
```

Expected: 75 passed; no regressions (old struct was unused).

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/sim-ai-core/src/lib.rs
git commit -m "refactor(sim): Phase E PR 3a - delete unused sim_ai_core::Consideration struct"
```

### Task 3.2: Add the `Consideration` enum + `ConsiderationContext`

**Files:**
- Modify: `crates/sim-ai-core/src/lib.rs` (add `Consideration` enum, `ConsiderationContext`, `raw_input` method)

- [ ] **Step 1: Write failing test for `raw_input`**

Append to the test module in `crates/sim-ai-core/src/lib.rs`:

```rust
#[test]
fn test_consideration_distance_to_target() {
    use sim_math::Vec2;
    let perception = sim_components::PerceptionSnapshot {
        self_position: Vec2::new(50.0, 34.0),
        nearby_teammates: smallvec::SmallVec::new(),
        nearby_opponents: smallvec::SmallVec::new(),
        ball_position: Vec2::new(52.5, 34.0),
        ball_state: sim_components::BallState::Free,
        goal_position: Vec2::new(105.0, 34.0),
        pitch_bounds: sim_components::PitchBounds {
            distance_to_left: 50.0,
            distance_to_right: 55.0,
            distance_to_top: 34.0,
            distance_to_bottom: 34.0,
        },
    };
    let intent = sim_components::Intent::HoldPosition;
    let c = Consideration::DistanceToTarget {
        weight: 1.0,
        curve: ResponseCurve::Linear { min: 0.0, max: 1.0 },
    };
    let ctx = ConsiderationContext {
        perception: &perception,
        intent: &intent,
        stamina: 1.0,
        skill: 0.7,
        grid: None,
    };
    // Distance to target is meaningless for HoldPosition intent (no
    // target), so the fallback uses distance-to-ball: 2.5m, clamped
    // to the "closeness" 30 - d = 27.5 raw input.
    let raw = c.raw_input(&ctx);
    assert!(raw > 25.0 && raw < 30.0);
}
```

- [ ] **Step 2: Run test to verify compile failure**

```bash
cargo test -p sim-ai-core test_consideration_distance_to_target
```

Expected: compile error (`Consideration`, `ConsiderationContext` not defined).

- [ ] **Step 3: Verify smallvec + sim-math are available as dev-deps**

Check `crates/sim-ai-core/Cargo.toml`. Currently it has only `sim-math` and `sim-components`. The new test uses `sim_components::PerceptionSnapshot` and `smallvec::SmallVec`. sim-components re-exports `smallvec` so we can use `sim_components::PerceptionSnapshot` but not `smallvec::SmallVec` directly.

Check by adding the import alias:

```bash
grep -n "smallvec" crates/sim-components/src/lib.rs | head -5
```

Expected: smallvec is re-exported from sim-components or accessible via re-export.

Adjust the test imports to use `sim_components::PerceptionSnapshot` only (already in scope via `use sim_components`). Replace `smallvec::SmallVec::new()` with `Default::default()` since `PerceptionSnapshot.nearby_teammates: SmallVec<...>` is `Default`.

Updated test:

```rust
#[test]
fn test_consideration_distance_to_target() {
    use sim_math::Vec2;
    let perception = sim_components::PerceptionSnapshot {
        self_position: Vec2::new(50.0, 34.0),
        nearby_teammates: Default::default(),
        nearby_opponents: Default::default(),
        ball_position: Vec2::new(52.5, 34.0),
        ball_state: sim_components::BallState::Free,
        goal_position: Vec2::new(105.0, 34.0),
        pitch_bounds: sim_components::PitchBounds {
            distance_to_left: 50.0,
            distance_to_right: 55.0,
            distance_to_top: 34.0,
            distance_to_bottom: 34.0,
        },
    };
    let intent = sim_components::Intent::HoldPosition;
    let c = Consideration::DistanceToTarget {
        weight: 1.0,
        curve: ResponseCurve::Linear { min: 0.0, max: 1.0 },
    };
    let ctx = ConsiderationContext {
        perception: &perception,
        intent: &intent,
        stamina: 1.0,
        skill: 0.7,
        grid: None,
    };
    let raw = c.raw_input(&ctx);
    assert!(raw > 25.0 && raw < 30.0);
}
```

- [ ] **Step 4: Run test to verify compile failure**

```bash
cargo test -p sim-ai-core test_consideration_distance_to_target
```

Expected: still compile error (`Consideration`, `ConsiderationContext` not defined).

- [ ] **Step 5: Add the `Consideration` enum + `ConsiderationContext`**

In `crates/sim-ai-core/src/lib.rs`, after the `Score` newtype (added in PR 2) and before the `ResponseCurve` impl, add:

```rust
use sim_components::{Intent, PerceptionSnapshot};

/// Context passed to `Consideration::raw_input`. Bundles the data the
/// dispatcher would otherwise take as five parameters.
pub struct ConsiderationContext<'a> {
    pub perception: &'a PerceptionSnapshot,
    pub intent: &'a Intent,
    pub stamina: f32,
    pub skill: f32,
    pub grid: Option<&'a sim_physics::PitchControlGrid>,
}

/// One consideration in a player's utility brain. Each variant carries
/// its own `weight` and `ResponseCurve`; the dispatcher
/// (`Consideration::raw_input`) is exhaustive, so a brain template that
/// references a typo'd consideration fails at compile time.
#[derive(Debug, Clone)]
pub enum Consideration {
    DistanceToTarget { weight: f32, curve: ResponseCurve },
    DistanceToBall { weight: f32, curve: ResponseCurve },
    Stamina { weight: f32, curve: ResponseCurve },
    PitchControlAtBall { weight: f32, curve: ResponseCurve },
    PassAngleClear { weight: f32, curve: ResponseCurve },
    TeammateDistance { weight: f32, curve: ResponseCurve },
    TeammateSpace { weight: f32, curve: ResponseCurve },
    DistanceToGoal { weight: f32, curve: ResponseCurve },
    GoalAngle { weight: f32, curve: ResponseCurve },
    DefenderPressure { weight: f32, curve: ResponseCurve },
    DistanceToOpponent { weight: f32, curve: ResponseCurve },
    SkillDiff { weight: f32, curve: ResponseCurve },
    DistanceToMarked { weight: f32, curve: ResponseCurve },
    DefensivePosition { weight: f32, curve: ResponseCurve },
    DistanceToPress { weight: f32, curve: ResponseCurve },
    SpaceAhead { weight: f32, curve: ResponseCurve },
    TeammateBall { weight: f32, curve: ResponseCurve },
    FormationDiscipline { weight: f32, curve: ResponseCurve },
}
```

Add the `sim_physics` dependency for `PitchControlGrid`:

```toml
[dependencies]
sim-math = { path = "../sim-math" }
sim-components = { path = "../sim-components" }
sim-physics = { path = "../sim-physics" }
```

Wait — adding `sim-physics` to `sim-ai-core` would create a new edge in the
dependency graph. Review the spec: the spec says "Placement: sim-ai-core
since it's data, not behaviour". But `PitchControlGrid` lives in
`sim-physics`. Two options:

- (a) Keep `Consideration` in `sim-ai-core` and accept the new dep edge.
  This is the cleanest for the type.
- (b) Move `Consideration` to `sim-ai-player` (which already depends on
  `sim-physics`) and re-export from `sim-ai-core` if needed. Keeps the
  dep graph clean.

Pick (b) — `Consideration` lives in `sim-ai-player`. Delete the code we
just added to `sim-ai-core` and re-add it in `sim-ai-player` instead.

- [ ] **Step 6: Move `Consideration` to `sim-ai-player`**

Revert the changes from Step 5. Then in `crates/sim-ai-player/src/lib.rs`,
add the `Consideration` enum + `ConsiderationContext` + `raw_input` impl
above the `PlayerAction` struct (around line 43-47). The new types are
pub so the brain templates in `sim-core` can use them.

Add to `crates/sim-ai-player/src/lib.rs` (immediately after the
`UtilityBrain` struct, before `PlayerAction`):

```rust
#[derive(Debug, Clone)]
pub enum Consideration {
    DistanceToTarget { weight: f32, curve: sim_ai_core::ResponseCurve },
    DistanceToBall { weight: f32, curve: sim_ai_core::ResponseCurve },
    Stamina { weight: f32, curve: sim_ai_core::ResponseCurve },
    PitchControlAtBall { weight: f32, curve: sim_ai_core::ResponseCurve },
    PassAngleClear { weight: f32, curve: sim_ai_core::ResponseCurve },
    TeammateDistance { weight: f32, curve: sim_ai_core::ResponseCurve },
    TeammateSpace { weight: f32, curve: sim_ai_core::ResponseCurve },
    DistanceToGoal { weight: f32, curve: sim_ai_core::ResponseCurve },
    GoalAngle { weight: f32, curve: sim_ai_core::ResponseCurve },
    DefenderPressure { weight: f32, curve: sim_ai_core::ResponseCurve },
    DistanceToOpponent { weight: f32, curve: sim_ai_core::ResponseCurve },
    SkillDiff { weight: f32, curve: sim_ai_core::ResponseCurve },
    DistanceToMarked { weight: f32, curve: sim_ai_core::ResponseCurve },
    DefensivePosition { weight: f32, curve: sim_ai_core::ResponseCurve },
    DistanceToPress { weight: f32, curve: sim_ai_core::ResponseCurve },
    SpaceAhead { weight: f32, curve: sim_ai_core::ResponseCurve },
    TeammateBall { weight: f32, curve: sim_ai_core::ResponseCurve },
    FormationDiscipline { weight: f32, curve: sim_ai_core::ResponseCurve },
}

impl Consideration {
    #[must_use]
    pub fn weight(&self) -> f32 {
        match *self {
            Self::DistanceToTarget { weight, .. }
            | Self::DistanceToBall { weight, .. }
            | Self::Stamina { weight, .. }
            | Self::PitchControlAtBall { weight, .. }
            | Self::PassAngleClear { weight, .. }
            | Self::TeammateDistance { weight, .. }
            | Self::TeammateSpace { weight, .. }
            | Self::DistanceToGoal { weight, .. }
            | Self::GoalAngle { weight, .. }
            | Self::DefenderPressure { weight, .. }
            | Self::DistanceToOpponent { weight, .. }
            | Self::SkillDiff { weight, .. }
            | Self::DistanceToMarked { weight, .. }
            | Self::DefensivePosition { weight, .. }
            | Self::DistanceToPress { weight, .. }
            | Self::SpaceAhead { weight, .. }
            | Self::TeammateBall { weight, .. }
            | Self::FormationDiscipline { weight, .. } => weight,
        }
    }

    #[must_use]
    pub fn curve(&self) -> sim_ai_core::ResponseCurve {
        match *self {
            Self::DistanceToTarget { curve, .. }
            | Self::DistanceToBall { curve, .. }
            | Self::Stamina { curve, .. }
            | Self::PitchControlAtBall { curve, .. }
            | Self::PassAngleClear { curve, .. }
            | Self::TeammateDistance { curve, .. }
            | Self::TeammateSpace { curve, .. }
            | Self::DistanceToGoal { curve, .. }
            | Self::GoalAngle { curve, .. }
            | Self::DefenderPressure { curve, .. }
            | Self::DistanceToOpponent { curve, .. }
            | Self::SkillDiff { curve, .. }
            | Self::DistanceToMarked { curve, .. }
            | Self::DefensivePosition { curve, .. }
            | Self::DistanceToPress { curve, .. }
            | Self::SpaceAhead { curve, .. }
            | Self::TeammateBall { curve, .. }
            | Self::FormationDiscipline { curve, .. } => curve,
        }
    }
}

pub struct ConsiderationContext<'a> {
    pub perception: &'a sim_components::PerceptionSnapshot,
    pub intent: &'a sim_components::Intent,
    pub stamina: f32,
    pub skill: f32,
    pub grid: Option<&'a sim_physics::PitchControlGrid>,
}
```

- [ ] **Step 7: Re-run test to verify compile failure**

```bash
cargo test -p sim-ai-core test_consideration_distance_to_target
```

Expected: still compile error (`Consideration`, `ConsiderationContext` not defined in `sim-ai-core`).

- [ ] **Step 8: Move the failing test to `sim-ai-player`**

Move the failing test from Step 3 of Task 3.2 to `crates/sim-ai-player/src/lib.rs` test module. Update imports accordingly (the test now uses `crate::Consideration`, `crate::ConsiderationContext`, etc.).

- [ ] **Step 9: Run test to verify it still fails (missing `raw_input`)**

```bash
cargo test -p sim-ai-player test_consideration_distance_to_target
```

Expected: compile error (`raw_input` method not found on `Consideration`).

- [ ] **Step 10: Commit**

```bash
cargo fmt --all
git add crates/sim-ai-player/src/lib.rs
git commit -m "refactor(sim): Phase E PR 3b - add Consideration enum + ConsiderationContext"
```

### Task 3.3: Implement `Consideration::raw_input` with exhaustive match

**Files:**
- Modify: `crates/sim-ai-player/src/lib.rs` (add `raw_input` impl; delete `compute_consideration_input`)

- [ ] **Step 1: Read the current `compute_consideration_input`**

Read `crates/sim-ai-player/src/lib.rs:386-579` in full. Note every arm and its body — these become the arms of `Consideration::raw_input`.

- [ ] **Step 2: Add the `raw_input` impl**

Add an `impl Consideration` block (continuing the existing impl from Task 3.2):

```rust
impl Consideration {
    /// Compute the raw, unnormalized input for this consideration. The
    /// returned value is fed into `self.curve()` by the caller (typically
    /// `player_decision_system`) which then multiplies by the weight.
    ///
    /// `pitch_control_grid` is `None` when the perception snapshot was
    /// built without a grid; the relevant consideration falls back to
    /// a neutral value in that case.
    #[must_use]
    pub fn raw_input(&self, ctx: &ConsiderationContext) -> f32 {
        match *self {
            Self::DistanceToTarget { .. } => {
                let d = if let sim_components::Intent::Movement(
                    sim_components::MovementIntent::MoveToPosition(target),
                ) = ctx.intent
                {
                    ctx.perception.self_position.distance(target)
                } else {
                    ctx.perception.self_position.distance(ctx.perception.ball_position)
                };
                30.0 - d.min(30.0)
            }
            Self::DistanceToBall { .. } => ctx.perception.self_position.distance(ctx.perception.ball_position),
            Self::Stamina { .. } => ctx.stamina,
            Self::PitchControlAtBall { .. } => ctx.grid.map_or(50.0, |g| {
                g.control_at(ctx.perception.ball_position.x, ctx.perception.ball_position.y) * 100.0
            }),
            Self::PassAngleClear { .. } => {
                if matches!(ctx.intent, sim_components::Intent::Action(sim_components::ActionIntent::PassTo)) {
                    let lane_clear = !ctx.perception.nearby_opponents.iter().any(|opp| opp.distance < 8.0);
                    if lane_clear { 1.0 } else { 0.4 }
                } else {
                    0.5
                }
            }
            Self::TeammateDistance { .. } => {
                if ctx.perception.nearby_teammates.is_empty() {
                    0.0
                } else {
                    let avg: f32 = ctx.perception.nearby_teammates.iter().map(|t| t.distance).sum::<f32>()
                        / ctx.perception.nearby_teammates.len() as f32;
                    30.0 - avg.min(30.0)
                }
            }
            Self::TeammateSpace { .. } => {
                let nearest_opp = ctx.perception.nearby_opponents.iter().map(|o| o.distance)
                    .fold(f32::INFINITY, f32::min);
                let space_m = if nearest_opp.is_finite() { nearest_opp } else { 20.0 };
                20.0 - space_m.min(20.0)
            }
            Self::DistanceToGoal { .. } => 35.0 - ctx.perception.self_position.distance(ctx.perception.goal_position),
            Self::GoalAngle { .. } => {
                let to_ball = ctx.perception.ball_position - ctx.perception.self_position;
                let to_goal = ctx.perception.goal_position - ctx.perception.self_position;
                let dot = to_ball.dot(to_goal);
                let mags = to_ball.length() * to_goal.length();
                if mags > 1e-3 { (dot / mags).clamp(-1.0, 1.0) } else { 0.0 }
            }
            Self::DefenderPressure { .. } => {
                ctx.perception.nearby_opponents.iter()
                    .filter(|o| o.distance <= 5.0)
                    .count() as f32
            }
            Self::DistanceToOpponent { .. } => {
                5.0 - ctx.perception.nearby_opponents.iter().map(|o| o.distance)
                    .fold(f32::INFINITY, f32::min).min(5.0)
            }
            Self::SkillDiff { .. } => (ctx.skill - 0.7).clamp(-1.0, 1.0),
            Self::DistanceToMarked { .. } => {
                10.0 - ctx.perception.nearby_opponents.iter().map(|o| o.distance)
                    .fold(f32::INFINITY, f32::min).min(10.0)
            }
            Self::DefensivePosition { .. } => {
                let own_goal_x = 0.0_f32;
                let ball_x = ctx.perception.ball_position.x;
                let player_x = ctx.perception.self_position.x;
                if player_x <= ball_x && player_x >= own_goal_x { 1.0 } else { 0.0 }
            }
            Self::DistanceToPress { .. } => {
                12.0 - ctx.perception.nearby_opponents.iter().map(|o| o.distance)
                    .fold(f32::INFINITY, f32::min).min(12.0)
            }
            Self::SpaceAhead { .. } => {
                let forward = ctx.perception.ball_position.x > ctx.perception.self_position.x;
                let opp_in_front = ctx.perception.nearby_opponents.iter()
                    .filter(|o| if forward { o.relative_position.x > 0.0 } else { o.relative_position.x < 0.0 })
                    .map(|o| o.distance)
                    .fold(f32::INFINITY, f32::min);
                let d = if opp_in_front.is_finite() { opp_in_front } else { 15.0 };
                15.0 - d.min(15.0)
            }
            Self::TeammateBall { .. } => {
                let has = ctx.perception.nearby_teammates.iter().any(|t| {
                    let dist_to_ball = (t.relative_position + ctx.perception.self_position
                        - ctx.perception.ball_position).length();
                    dist_to_ball < 2.0
                });
                if has { 1.0 } else { 0.0 }
            }
            Self::FormationDiscipline { .. } => 1.0,
        }
    }
}
```

- [ ] **Step 3: Delete `compute_consideration_input`**

In `crates/sim-ai-player/src/lib.rs`, delete the function `compute_consideration_input` (lines 386-579, now with the `Intent`/`MovementIntent`/`ActionIntent` shape from PR 4 — but PR 4 hasn't landed yet, so for this task use the *old* `Intent` shape, e.g. `Intent::PassTo`, `Intent::MoveToPosition(target)`, `Intent::ShootAtGoal(_)`. Adapt the match arms to the flat enum for now; PR 4 will adjust them again. To avoid double work, write `raw_input` using the *current* flat `Intent` enum, then in PR 4 update both the new `raw_input` and the existing `intent_kind`/`steer`/`player_action_execution_system`/`kick_execution_system` to the split shape in one commit).

In this PR 3, write the raw_input as:

```rust
            Self::DistanceToTarget { .. } => {
                let d = if let sim_components::Intent::MoveToPosition(target) = ctx.intent {
                    ctx.perception.self_position.distance(target)
                } else {
                    ctx.perception.self_position.distance(ctx.perception.ball_position)
                };
                30.0 - d.min(30.0)
            }
            ...
            Self::PassAngleClear { .. } => {
                if matches!(ctx.intent, sim_components::Intent::PassTo) {
                    let lane_clear = !ctx.perception.nearby_opponents.iter().any(|opp| opp.distance < 8.0);
                    if lane_clear { 1.0 } else { 0.4 }
                } else {
                    0.5
                }
            }
            ...
```

(Use the flat `Intent` enum variants for now. PR 4 will revise.)

- [ ] **Step 4: Update `player_decision_system` to use `Consideration::raw_input`**

In `crates/sim-ai-player/src/lib.rs:295-315`, find the per-consideration scoring block. Replace:

```rust
                let raw = compute_consideration_input(
                    &consideration.name,
                    perception,
                    intent,
                    stamina.0,
                    skill.0,
                    grid,
                );
                weighted_scores.push(consideration.weight * raw);
```

With:

```rust
                let ctx = ConsiderationContext {
                    perception,
                    intent,
                    stamina: stamina.0,
                    skill: skill.0,
                    grid,
                };
                let raw = consideration.raw_input(&ctx);
                weighted_scores.push(consideration.weight * raw);
```

Then update the rest of the function to consume `Vec<Consideration>` instead of `Vec<PlayerConsideration>`. The `action.considerations` field type changes from `Vec<PlayerConsideration>` to `Vec<Consideration>`.

- [ ] **Step 5: Delete `PlayerConsideration`**

In `crates/sim-ai-player/src/lib.rs:49-58`, delete the `PlayerConsideration` struct.

- [ ] **Step 6: Run tests**

```bash
cargo test --workspace
```

Expected: existing tests still pass; the new test from Task 3.2 (moved to sim-ai-player) passes.

If any existing test constructs a `PlayerConsideration`, fix it (see Task 3.4).

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add crates/sim-ai-player/src/lib.rs
git commit -m "refactor(sim): Phase E PR 3c - exhaustive Consideration::raw_input"
```

### Task 3.4: Update brain templates in `sim-core`

**Files:**
- Modify: `crates/sim-core/src/lib.rs:996-1180` (brain templates)
- Modify: `crates/sim-ai-player/src/lib.rs:1213, 1344` (test brains)

- [ ] **Step 1: Find all `PlayerConsideration` constructions**

```bash
grep -n "PlayerConsideration" crates/
```

Expected: matches only in brain templates (sim-core) and test brains (sim-ai-player). All go away.

- [ ] **Step 2: Read the brain template (`default_utility_brain`)**

Read `crates/sim-core/src/lib.rs:975-1180` to understand the shape of each consideration.

- [ ] **Step 3: Translate each `PlayerConsideration` construction**

For each `PlayerConsideration { name: "x".to_string(), weight, curve }`, replace with the matching enum variant:

| Old `name: String`        | New enum variant                          |
|-------------------------|-------------------------------------------|
| `"distance_to_target"`  | `sim_ai_player::Consideration::DistanceToTarget { weight, curve }` |
| `"distance_to_ball"`    | `sim_ai_player::Consideration::DistanceToBall { weight, curve }` |
| `"ball_distance"`       | `sim_ai_player::Consideration::DistanceToBall { weight, curve }` |
| `"self_to_ball"`        | `sim_ai_player::Consideration::DistanceToBall { weight, curve }` |
| `"stamina"`             | `sim_ai_player::Consideration::Stamina { weight, curve }` |
| `"pitch_control_at_ball"` | `sim_ai_player::Consideration::PitchControlAtBall { weight, curve }` |
| `"pass_angle_clear"`    | `sim_ai_player::Consideration::PassAngleClear { weight, curve }` |
| `"teammate_distance"`   | `sim_ai_player::Consideration::TeammateDistance { weight, curve }` |
| `"teammate_space"`      | `sim_ai_player::Consideration::TeammateSpace { weight, curve }` |
| `"distance_to_goal"`    | `sim_ai_player::Consideration::DistanceToGoal { weight, curve }` |
| `"goal_angle"`          | `sim_ai_player::Consideration::GoalAngle { weight, curve }` |
| `"defender_pressure"`   | `sim_ai_player::Consideration::DefenderPressure { weight, curve }` |
| `"distance_to_opponent"` | `sim_ai_player::Consideration::DistanceToOpponent { weight, curve }` |
| `"skill_diff"`          | `sim_ai_player::Consideration::SkillDiff { weight, curve }` |
| `"distance_to_marked"`  | `sim_ai_player::Consideration::DistanceToMarked { weight, curve }` |
| `"defensive_position"`  | `sim_ai_player::Consideration::DefensivePosition { weight, curve }` |
| `"distance_to_press"`   | `sim_ai_player::Consideration::DistanceToPress { weight, curve }` |
| `"space_ahead"`         | `sim_ai_player::Consideration::SpaceAhead { weight, curve }` |
| `"teammate_ball"`       | `sim_ai_player::Consideration::TeammateBall { weight, curve }` |
| `"formation_discipline"` | `sim_ai_player::Consideration::FormationDiscipline { weight, curve }` |

Note: `"ball_distance"` and `"self_to_ball"` were string aliases for the
same dispatch — they collapse to one enum variant.

Update the `use` import in `crates/sim-core/src/lib.rs`:

```rust
use sim_ai_player::{Consideration, PlayerAction};
```

- [ ] **Step 4: Update test brains in `sim-ai-player`**

In `crates/sim-ai-player/src/lib.rs:1213, 1344`, replace `PlayerConsideration { name: "formation_discipline".to_string(), weight, curve }` with `Consideration::FormationDiscipline { weight, curve }`.

- [ ] **Step 5: Run tests**

```bash
cargo test --workspace
```

Expected: 76+ passed (was 75 + 1 new test = 76). All existing tests still pass.

- [ ] **Step 6: Run clippy**

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add crates/sim-core/src/lib.rs crates/sim-ai-player/src/lib.rs
git commit -m "refactor(sim): Phase E PR 3d - migrate brain templates to Consideration enum"
```

### Task 3.5: Final verification for PR 3

- [ ] **Step 1: Full check**

```bash
cargo fmt --all
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: 76+ passed; clippy clean.

- [ ] **Step 2: Push and open PR**

```bash
git push origin refactor/phase-e-3-consideration-enum
gh pr create --base main --title "Phase E PR 3: Consideration enum" \
  --body "Replaces string-dispatch PlayerConsideration with exhaustive enum. Per docs/superpowers/specs/2026-09-29-phase-e-api-polish-design.md PR 3."
```

After merge, continue with PR 4.

---

## PR 4: `Intent` split into movement/action hierarchies

**Branch:** `refactor/phase-e-4-intent-split`
**Goal:** Replace flat 11-variant `Intent` with `Intent(MovementIntent | ActionIntent)`. Hard cutover.

**Pre-flight check:**
```bash
cargo test --workspace 2>&1 | tail -1   # expect 76+ passed
git checkout main && git pull
git checkout -b refactor/phase-e-4-intent-split
```

### Task 4.1: Replace the `Intent` enum shape AND update all call sites

The new `Intent` shape touches ~15 call sites across 4 crates. Rather than
land an intermediate "broken state" commit, do all changes in Task 4.1 and
commit once.

**Files:**
- Modify: `crates/sim-components/src/lib.rs:108-121`
- Modify: `crates/sim-ai-player/src/lib.rs:362-376` (`intent_kind`)
- Modify: `crates/sim-ai-player/src/lib.rs:621-662` (`player_action_execution_system`)
- Modify: `crates/sim-ai-player/src/lib.rs:741-754` (`kick_execution_system`)
- Modify: `crates/sim-ai-player/src/lib.rs:796-820` (`steer`)
- Modify: `crates/sim-ai-player/src/lib.rs` (PR-3 `Consideration::raw_input` arms for `DistanceToTarget`/`PassAngleClear`)
- Modify: `crates/sim-core/src/lib.rs:996-1180` (brain templates)
- Modify: `crates/sim-ai-player/src/lib.rs:1213, 1344` (test brains)

- [ ] **Step 1: Locate all `Intent` variants in use**

```bash
grep -rn "Intent::" crates/ | grep -v "/target/" | head -50
```

- [ ] **Step 2: Replace the enum in `sim-components`**

In `crates/sim-components/src/lib.rs:108-121`, replace the flat enum with:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Intent {
    Movement(MovementIntent),
    Action(ActionIntent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MovementIntent {
    MoveToPosition(Vec2),
    HoldPosition,
    ChaseBall,
    Intercept,
    SupportRun,
    TrackBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionIntent {
    PassTo,
    ShootAtGoal(Vec2),
    Tackle(Entity),
    MarkOpponent(Entity),
    Press(Entity),
}

impl Intent {
    /// Coarse two-way tag: does this intent move the player, or commit to
    /// an action? Useful for sites that don't care about the specific
    /// variant.
    #[must_use]
    pub fn kind(&self) -> IntentKind {
        match *self {
            Self::Movement(_) => IntentKind::Movement,
            Self::Action(_) => IntentKind::Action,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntentKind {
    Movement,
    Action,
}
```

- [ ] **Step 3: Update `intent_kind` in `sim-ai-player` (line 362)**

Replace the existing `intent_kind` function with the variant that matches
the split shape:

```rust
fn intent_kind(intent: &Intent) -> &'static str {
    match intent {
        Intent::Movement(MovementIntent::MoveToPosition(_)) => "MoveToPosition",
        Intent::Movement(MovementIntent::HoldPosition) => "HoldPosition",
        Intent::Movement(MovementIntent::ChaseBall) => "ChaseBall",
        Intent::Movement(MovementIntent::Intercept) => "Intercept",
        Intent::Movement(MovementIntent::SupportRun) => "SupportRun",
        Intent::Movement(MovementIntent::TrackBack) => "TrackBack",
        Intent::Action(ActionIntent::PassTo) => "PassTo",
        Intent::Action(ActionIntent::ShootAtGoal(_)) => "ShootAtGoal",
        Intent::Action(ActionIntent::Tackle(_)) => "Tackle",
        Intent::Action(ActionIntent::MarkOpponent(_)) => "MarkOpponent",
        Intent::Action(ActionIntent::Press(_)) => "Press",
    }
}
```

Note: removed `const` because `IntentKind::kind` is not const; match arms
are exhaustive so it's a normal `fn`.

- [ ] **Step 4: Update `player_action_execution_system` (line 621)**

Replace the `match &intent` block (lines 621-662):

```rust
        match &intent {
            Intent::Action(ActionIntent::Tackle(target)) => {
                let tackle_success_prob = skill.0.clamp(0.3, 0.9);
                let roll: f32 = rng.gen_f32();
                if roll > tackle_success_prob {
                    speed_modifier = 0.3;
                }
                let _ = target;
            }
            Intent::Action(ActionIntent::PassTo) => {
                let pass_distance = perception
                    .nearby_teammates
                    .first()
                    .map_or(15.0, |t| t.distance);
                let pass_prob = (1.0 - (pass_distance / 50.0).min(0.8)) * skill.0;
                let roll: f32 = rng.gen_f32();
                if roll > pass_prob {
                    speed_modifier = 0.7;
                    direction_modifier =
                        Vec2::new(rng.gen_range_f32(-2.0, 2.0), rng.gen_range_f32(-2.0, 2.0));
                }
            }
            Intent::Action(ActionIntent::ShootAtGoal(_)) => {
                let accuracy = skill.0 * stamina.0.mul_add(0.5, 0.5);
                let roll: f32 = rng.gen_f32();
                if roll > accuracy {
                    speed_modifier = 0.8;
                    direction_modifier =
                        Vec2::new(rng.gen_range_f32(-3.0, 3.0), rng.gen_range_f32(-2.0, 2.0));
                }
            }
            Intent::Movement(_) => {}
        }
```

- [ ] **Step 5: Update `kick_execution_system` (line 741)**

Replace the `match &intent` block (lines 741-754):

```rust
    let kick = match &intent {
        Intent::Action(ActionIntent::ShootAtGoal(target)) => {
            let dir = *target - perception.self_position;
            (dir.length() > 0.01).then(|| (dir.normalized(), KICK_SPEED_SHOT))
        }
        Intent::Action(ActionIntent::PassTo) => nearest_teammate_position(perception).and_then(|target| {
            let dir = target - perception.self_position;
            (dir.length() > 0.01).then(|| (dir.normalized(), KICK_SPEED_PASS))
        }),
        Intent::Movement(MovementIntent::ChaseBall)
        | Intent::Action(ActionIntent::Tackle(_))
        | Intent::Action(ActionIntent::Press(_)) => {
            (player_vel.0.length() > 0.01).then(|| (player_vel.0.normalized(), KICK_SPEED_PASS))
        }
        Intent::Movement(_) => None,
    };
```

- [ ] **Step 6: Update `steer` (line 796)**

Replace the `match intent` block (lines 796-820):

```rust
    let (target, speed): (Vec2, f32) = match intent {
        Intent::Movement(MovementIntent::MoveToPosition(target)) => (*target, 5.0),
        Intent::Movement(MovementIntent::ChaseBall) => (ball_pos, 8.0),
        Intent::Movement(MovementIntent::Intercept) => (ball_pos + ball_vel * 0.5, 9.0),
        Intent::Movement(MovementIntent::HoldPosition) => return Vec2::zero(),
        Intent::Movement(MovementIntent::SupportRun) => {
            if let Some(worst) = perception.nearby_opponents.iter().min_by(|a, b| {
                a.distance
                    .partial_cmp(&b.distance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }) {
                let away = player_pos + (player_pos - worst.relative_position);
                (away, 5.0)
            } else {
                return Vec2::zero();
            }
        }
        Intent::Movement(MovementIntent::TrackBack) => (Vec2::new(0.0, 34.0), 6.0),
        Intent::Action(ActionIntent::PassTo) => (nearest_teammate_pos().unwrap_or(ball_pos), 8.0),
        Intent::Action(ActionIntent::ShootAtGoal(target)) => (*target, 10.0),
        Intent::Action(ActionIntent::Tackle(_)) | Intent::Action(ActionIntent::Press(_)) => {
            (nearest_opponent_pos().unwrap_or(player_pos), 10.0)
        }
        Intent::Action(ActionIntent::MarkOpponent(_)) => {
            (nearest_opponent_pos().unwrap_or(player_pos), 4.0)
        }
    };
```

- [ ] **Step 7: Update the PR-3 `Consideration::raw_input` arms**

In the `impl Consideration` block added in PR 3 Task 3.3, update
`DistanceToTarget` and `PassAngleClear`:

```rust
            Self::DistanceToTarget { .. } => {
                let d = if let sim_components::Intent::Movement(
                    sim_components::MovementIntent::MoveToPosition(target),
                ) = ctx.intent
                {
                    ctx.perception.self_position.distance(target)
                } else {
                    ctx.perception.self_position.distance(ctx.perception.ball_position)
                };
                30.0 - d.min(30.0)
            }
```

```rust
            Self::PassAngleClear { .. } => {
                if matches!(ctx.intent, sim_components::Intent::Action(sim_components::ActionIntent::PassTo)) {
                    let lane_clear = !ctx.perception.nearby_opponents.iter().any(|opp| opp.distance < 8.0);
                    if lane_clear { 1.0 } else { 0.4 }
                } else {
                    0.5
                }
            }
```

- [ ] **Step 8: Update brain templates in `sim-core`**

In `crates/sim-core/src/lib.rs`, find each `intent: Intent::X(...)` in the
brain templates (lines 996-1180) and wrap per the table:

| Old                              | New                                                                  |
|----------------------------------|----------------------------------------------------------------------|
| `Intent::MoveToPosition(t)`      | `Intent::Movement(MovementIntent::MoveToPosition(t))`               |
| `Intent::PassTo`                 | `Intent::Action(ActionIntent::PassTo)`                               |
| `Intent::ShootAtGoal(t)`         | `Intent::Action(ActionIntent::ShootAtGoal(t))`                       |
| `Intent::Tackle(e)`              | `Intent::Action(ActionIntent::Tackle(e))`                            |
| `Intent::ChaseBall`              | `Intent::Movement(MovementIntent::ChaseBall)`                        |
| `Intent::MarkOpponent(e)`        | `Intent::Action(ActionIntent::MarkOpponent(e))`                      |
| `Intent::Intercept`              | `Intent::Movement(MovementIntent::Intercept)`                        |
| `Intent::Press(e)`               | `Intent::Action(ActionIntent::Press(e))`                             |
| `Intent::HoldPosition`           | `Intent::Movement(MovementIntent::HoldPosition)`                     |
| `Intent::SupportRun`             | `Intent::Movement(MovementIntent::SupportRun)`                       |
| `Intent::TrackBack`              | `Intent::Movement(MovementIntent::TrackBack)`                      |

Add to the `use` import in the brain template module:

```rust
use sim_components::{ActionIntent, Intent, MovementIntent};
```

- [ ] **Step 9: Update test brains in `sim-ai-player`**

In `crates/sim-ai-player/src/lib.rs:1213, 1344`, replace
`sim_components::Intent::HoldPosition` with
`sim_components::Intent::Movement(sim_components::MovementIntent::HoldPosition)`.

- [ ] **Step 10: Run tests**

```bash
cargo test --workspace
```

Expected: 76+ passed (no new tests added in PR 4 yet).

- [ ] **Step 11: Run clippy**

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 12: Commit**

```bash
cargo fmt --all
git add crates/sim-components/src/lib.rs crates/sim-ai-player/src/lib.rs crates/sim-core/src/lib.rs
git commit -m "refactor(sim): Phase E PR 4a - split Intent into Movement/Action hierarchies"
```
### Task 4.2: Add a test verifying `intent_kind` parity with old discriminant

**Files:**
- Modify: `crates/sim-ai-player/src/lib.rs` (test module)

- [ ] **Step 1: Add the test**

```rust
#[test]
fn test_intent_kind_string_for_all_variants() {
    // Phase E PR 4: the string labels produced by `intent_kind` must
    // match the labels the old flat enum produced, so any consumer that
    // hashes on the label (e.g. hysteresis matching in
    // `player_decision_system`) doesn't drift.
    let cases = [
        (Intent::Movement(MovementIntent::MoveToPosition(Vec2::zero())), "MoveToPosition"),
        (Intent::Movement(MovementIntent::HoldPosition), "HoldPosition"),
        (Intent::Movement(MovementIntent::ChaseBall), "ChaseBall"),
        (Intent::Movement(MovementIntent::Intercept), "Intercept"),
        (Intent::Movement(MovementIntent::SupportRun), "SupportRun"),
        (Intent::Movement(MovementIntent::TrackBack), "TrackBack"),
        (Intent::Action(ActionIntent::PassTo), "PassTo"),
        (Intent::Action(ActionIntent::ShootAtGoal(Vec2::zero())), "ShootAtGoal"),
        // Entity::PLACEHOLDER is a Bevy 0.14 Entity::from_raw(0) idiom; if
        // the Bevy version changes, adjust.
        (Intent::Action(ActionIntent::Tackle(Entity::PLACEHOLDER)), "Tackle"),
        (Intent::Action(ActionIntent::MarkOpponent(Entity::PLACEHOLDER)), "MarkOpponent"),
        (Intent::Action(ActionIntent::Press(Entity::PLACEHOLDER)), "Press"),
    ];
    for (intent, expected) in cases {
        assert_eq!(intent_kind(&intent), expected);
    }
}
```

- [ ] **Step 2: Run test**

```bash
cargo test -p sim-ai-player test_intent_kind_string_for_all_variants
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add crates/sim-ai-player/src/lib.rs
git commit -m "refactor(sim): Phase E PR 4b - intent_kind parity test"
```

### Task 4.3: Final verification for PR 4

- [ ] **Step 1: Full check**

```bash
cargo fmt --all
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: 77+ passed; clippy clean.

- [ ] **Step 2: Push and open PR**

```bash
git push origin refactor/phase-e-4-intent-split
gh pr create --base main --title "Phase E PR 4: Intent split" \
  --body "Splits flat Intent enum into Movement/Action sub-enums (hard cutover). Per docs/superpowers/specs/2026-09-29-phase-e-api-polish-design.md PR 4."
```

After merge, continue with PR 5.

---

## PR 5: Snapshot round-trip tests + `PartialEq` derives

**Branch:** `refactor/phase-e-5-snapshot-roundtrip`
**Goal:** Add `PartialEq` to snapshot view types; delete dead `BallSnapshot`/`PlayerSnapshot` in sim-replay; extend `test_snapshot_serialization`; add a new test file in sim-core for `MatchSnapshot` JSON round-trip.

**Pre-flight check:**
```bash
cargo test --workspace 2>&1 | tail -1   # expect 77+ passed
git checkout main && git pull
git checkout -b refactor/phase-e-5-snapshot-roundtrip
```

### Task 5.1: Add `PartialEq` to sim-core snapshot view types

**Files:**
- Modify: `crates/sim-core/src/lib.rs:715-761`

- [ ] **Step 1: Add derives**

For each of `MatchSnapshot`, `MatchStateView`, `BallView`, `PlayerView`, `ClockView`, add `PartialEq` to the derive list. MatchState contains `BallState` and other types that already derive `PartialEq + Eq`. PlayerView contains `Vec2` (from sim-math) and `Role` — verify both derive `PartialEq`.

```bash
grep -n "pub enum Role\|pub struct Vec2" crates/sim-math/src/lib.rs crates/sim-components/src/lib.rs
```

Expected: both have appropriate derives.

- [ ] **Step 2: Update each derive**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchSnapshot { ... }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchStateView { ... }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BallView { ... }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlayerView { ... }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClockView { ... }
```

- [ ] **Step 3: Verify compilation**

```bash
cargo check --workspace
```

Expected: clean (no callers rely on absence of PartialEq).

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/sim-core/src/lib.rs
git commit -m "refactor(sim): Phase E PR 5a - PartialEq on sim-core view types"
```

### Task 5.2: Add `PartialEq` to `RecordedSnapshot` and delete dead view types

**Files:**
- Modify: `crates/sim-replay/src/lib.rs:113-119, 156-183`

- [ ] **Step 1: Verify the dead types are unused**

```bash
grep -rn "BallSnapshot\|PlayerSnapshot" crates/sim-replay/
```

Expected: only the type definitions match (lines 165-183).

- [ ] **Step 2: Delete `BallSnapshot` and `PlayerSnapshot`**

In `crates/sim-replay/src/lib.rs`, delete lines 165-183 (both structs).

- [ ] **Step 3: Add `PartialEq` to `RecordedSnapshot`**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecordedSnapshot { ... }
```

- [ ] **Step 4: Run tests**

```bash
cargo test --workspace
```

Expected: 77+ passed.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/sim-replay/src/lib.rs
git commit -m "refactor(sim): Phase E PR 5b - delete dead view types + PartialEq on RecordedSnapshot"
```

### Task 5.3: Extend `test_snapshot_serialization` to assert all 6 fields

**Files:**
- Modify: `crates/sim-replay/src/lib.rs:318-340`

- [ ] **Step 1: Replace the test**

Replace:

```rust
    #[test]
    fn test_snapshot_serialization() {
        let snapshot = RecordedSnapshot {
            tick: 1000,
            state_hash: 123456789,
            ball_position: [52.5, 34.0],
            player_positions: vec![([10.0, 20.0], TeamId(0))],
            score: (2, 1),
            clock: MatchClock {
                elapsed_ticks: 45 * 60 * 60,
                half: 1,
                added_time_ticks: 3 * 60,
                is_running: false,
            },
        };

        let encoded = bincode::serialize(&snapshot).unwrap();
        let decoded: RecordedSnapshot = bincode::deserialize(&encoded).unwrap();

        assert_eq!(decoded.tick, snapshot.tick);
        assert_eq!(decoded.state_hash, snapshot.state_hash);
        assert_eq!(decoded.score, snapshot.score);
    }
```

With:

```rust
    #[test]
    fn test_snapshot_serialization() {
        let snapshot = RecordedSnapshot {
            tick: 1000,
            state_hash: 123456789,
            ball_position: [52.5, 34.0],
            player_positions: vec![([10.0, 20.0], TeamId(0))],
            score: (2, 1),
            clock: MatchClock {
                elapsed_ticks: 45 * 60 * 60,
                half: 1,
                added_time_ticks: 3 * 60,
                is_running: false,
            },
        };

        let encoded = bincode::serialize(&snapshot).unwrap();
        let decoded: RecordedSnapshot = bincode::deserialize(&encoded).unwrap();

        // Phase E PR 5: assert every field round-trips, not just three.
        assert_eq!(decoded, snapshot);
    }
```

- [ ] **Step 2: Run test**

```bash
cargo test -p sim-replay test_snapshot_serialization
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add crates/sim-replay/src/lib.rs
git commit -m "refactor(sim): Phase E PR 5c - RecordedSnapshot round-trip asserts all fields"
```

### Task 5.4: Add sim-core snapshot round-trip test file

**Files:**
- Create: `crates/sim-core/tests/snapshot_round_trip.rs`
- Modify: `crates/sim-core/Cargo.toml`

- [ ] **Step 1: Add `serde_json` as dev-dependency**

In `crates/sim-core/Cargo.toml`, append:

```toml
[dev-dependencies]
serde_json = "1"
```

- [ ] **Step 2: Create the test file**

Create `crates/sim-core/tests/snapshot_round_trip.rs`:

```rust
//! Phase E PR 5: JSON round-trip tests for `MatchSnapshot`.
//!
//! Catches silent serialisation breakage when the snapshot shape
//! evolves. Round-trip is via `serde_json` because that's the format
//! `sim-server` uses for live state queries.

use sim_core::{MatchSnapshot, Simulation};
use sim_components::{MatchState, TeamId, Formation, ManagerCommand};

#[test]
fn test_match_snapshot_json_round_trip() {
    let seed = 42u64;
    let mut sim = Simulation::new(seed);
    let (match_entity, _home, _away) = Simulation::create_match(&mut sim.world, seed);

    // Tick a small number of steps so the snapshot has non-trivial state.
    for _ in 0..100 {
        sim.tick();
    }

    let snapshot = sim.get_state(match_entity).expect("get_state should succeed");

    let json = serde_json::to_string(&snapshot).expect("serialise should succeed");
    let decoded: MatchSnapshot =
        serde_json::from_str(&json).expect("deserialise should succeed");

    assert_eq!(decoded, snapshot);
}

#[test]
fn test_determinism_via_snapshot_equality() {
    // Phase E PR 5: two simulations with the same seed must produce
    // equal snapshots at the same tick (regression check for the
    // state-hash determinism contract).
    let seed = 42u64;
    let mut sim1 = Simulation::new(seed);
    let mut sim2 = Simulation::new(seed);

    let (e1, _, _) = Simulation::create_match(&mut sim1.world, seed);
    let (e2, _, _) = Simulation::create_match(&mut sim2.world, seed);

    for _ in 0..200 {
        sim1.tick();
        sim2.tick();
    }

    let snap1 = sim1.get_state(e1).unwrap();
    let snap2 = sim2.get_state(e2).unwrap();

    assert_eq!(snap1, snap2);
    assert_eq!(snap1.state_hash, snap2.state_hash);
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test -p sim-core --test snapshot_round_trip
```

Expected: 2 passed.

- [ ] **Step 4: Add `serde_json` as dev-dependency to sim-replay**

In `crates/sim-replay/Cargo.toml`, append:

```toml
[dev-dependencies]
serde_json = "1"
```

- [ ] **Step 5: Run full workspace tests**

```bash
cargo test --workspace
```

Expected: 79+ passed (was 77 + 2 new = 79).

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/sim-core/tests/snapshot_round_trip.rs crates/sim-core/Cargo.toml crates/sim-replay/Cargo.toml
git commit -m "refactor(sim): Phase E PR 5d - sim-core MatchSnapshot JSON round-trip tests"
```

### Task 5.5: Final verification for PR 5

- [ ] **Step 1: Full check**

```bash
cargo fmt --all
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: 79+ passed; clippy clean.

- [ ] **Step 2: Push and open PR**

```bash
git push origin refactor/phase-e-5-snapshot-roundtrip
gh pr create --base main --title "Phase E PR 5: Snapshot round-trip tests" \
  --body "Adds PartialEq to snapshot types, deletes dead view types, adds JSON round-trip tests. Per docs/superpowers/specs/2026-09-29-phase-e-api-polish-design.md PR 5."
```

After merge, Phase E is complete.

---

## Post-Phase-E verification

- [ ] **Step 1: Confirm full workspace is clean**

```bash
cargo fmt --all
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: clean; 79+ tests pass.

- [ ] **Step 2: Update CODEBASE_REVIEW.md**

Append a new section noting which Phase E items landed and where. Update §5.3 to mark these items complete.

- [ ] **Step 3: Final commit**

```bash
git add docs/reviews/CODEBASE_REVIEW.md
git commit -m "docs(review): mark Phase E items complete in CODEBASE_REVIEW"
```