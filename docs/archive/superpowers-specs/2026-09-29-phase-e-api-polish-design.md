# Phase E: API Polish — Design

**Date:** 2026-09-29
**Source review:** `docs/reviews/CODEBASE_REVIEW.md` §5.3 + §1215-1222 (Phase E list)
**Path classification:** Architectural (5 small PRs, public API surface in 4 crates)
**Status:** Approved design — pending implementation plan

## Motivation

The original review lists five "Phase E" items under "API polish" — small
ergonomic / API-cleanliness changes that don't move correctness but tidy up the
public surface for future work. They were the lowest-priority items in the
review and were deliberately scheduled after Phases A–D landed.

Since the review was written, Phases A–D have all landed:

- ✅ Phase A (mechanical cleanups) — commit `c055d42`
- ✅ Phase B (ECS migration, no struct mirrors) — commit `72497dc`
- ✅ Phase C (Match/Ball as Resource, pitch_control Query, println→tracing) — commits `46643b3`, `61a734b`, `3f9ff7f`
- ✅ Phase D (decision cadence) — commit `d64df2c`

Phases A–D cleared several items the review listed under "Phase E" by accident
(`println!→tracing` landed in Phase C, `MatchSnapshot` rename in Phase A,
`apply_command` consolidation in Phase A). The five items in §1215–1222 remain.

## What's in scope

Five small PRs, ordered by dependency and risk:

1. **PR 1 — `BallStateComponent` removal + `BallState::OutOfPlay` cleanup**
   (mechanical; ~20 LOC + doc updates)
2. **PR 2 — `Score` newtype + `ResponseCurve::Linear` precedence fix**
   (small public-API change; ~50 LOC)
3. **PR 3 — Consideration enum (replace string dispatch)**
   (medium risk — touches all brain templates and one large match; ~400 LOC)
4. **PR 4 — `Intent` split into movement/action hierarchies** (hard cutover)
   (medium risk — public type used everywhere; ~150 LOC + tests)
5. **PR 5 — Snapshot round-trip tests + PartialEq derives** (test-only PR;
   ~100 LOC of tests, ~30 LOC of derives)

## What's out of scope

Deferred to later phases:

- Offside penalty implementation (§6.6 in review) — needs gameplay design.
- `CommandError` structured error type (§3.2) — needs an error-handling ADR.
- `ServerSimulation::command_queue` field privacy (§3.7) — trivial, will fold
  into PR 1 if it's a single-line change; otherwise defer.
- `Intent` flat→split was originally listed as one item; we keep the "hard
  cutover" decision per user direction (2026-09-29).

## PR 1 — `BallStateComponent` removal + `OutOfPlay` cleanup

**Goal:** delete the dead `BallStateComponent` wrapper, remove the never-
constructed `BallState::OutOfPlay` variant, update the spec docs that mention
it. Pure dead-code removal + doc correction.

**Findings (2026-09-29 exploration):**

- `BallStateComponent` (`sim-components/src/lib.rs:150-151`) — **0 usages**
  repo-wide. Superseded by the `Ball` resource's `.state: BallState` field
  (Phase C §4.3).
- `BallState::OutOfPlay` is **never constructed**; its only reference in
  production code is the discriminant arm `B::OutOfPlay => 3` in
  `sim-core/src/lib.rs:810` (`ball_state_discriminant`).
- The variant appears in 3 spec docs:
  - `spec/football-sim-server-spec.md:279, 613` (aspirational enum block)
  - `specs/001-football-sim-engine/data-model.md:69, 76, 221`
  - `specs/002-tick-observability/data-model.md:47`
  - `specs/002-tick-observability/contracts/trace-schema.md:65`
- The 001-data-model.md line 76 transition note (`* → OutOfPlay`) documents
  the *intent* to use `OutOfPlay` for "ball leaves pitch", but no system
  currently implements that transition (OOB events currently route to
  `BallState::Dead` via the restart flow). Per user direction
  (2026-09-29), `OutOfPlay` is being removed.

**Changes:**

- `sim-components/src/lib.rs`:
  - Delete `BallStateComponent` (lines 150-151).
  - Remove `OutOfPlay,` from `BallState` enum (line 68).
- `sim-core/src/lib.rs:804-813`:
  - Remove the `B::OutOfPlay => 3` arm in `ball_state_discriminant`.
  - Renumber `B::Dead => 4` → `B::Dead => 3` to keep contiguous mapping.
- Spec doc updates:
  - `spec/football-sim-server-spec.md` lines 279, 613: remove `OutOfPlay,`
    from both enum blocks. Replace `Possessed(Entity)`-shape with the
    current `Possessed` (we keep this correction out of scope — separate
    doc-only edit folded in for tidy consistency).
  - `specs/001-football-sim-engine/data-model.md`: remove `OutOfPlay` from
    line 69 column listing, line 76 transition note (`* → Dead` instead),
    and line 221 enum block.
  - `specs/002-tick-observability/data-model.md:47`: remove `OutOfPlay`
    from the state listing.
  - `specs/002-tick-observability/contracts/trace-schema.md:65`: remove
    `OutOfPlay` from the `state_change` event schema.

**Risk:** zero. Dead-code removal + enum variant removal. The only
behavioural risk is the discriminant renumbering — `ball_state_discriminant`
is only called inside `get_state_hash`, and the hash already mixes other
state so changing one arm's numeric value *does* change the hash output for
the same simulation state. **Acceptable:** the hash is documented as
internal and cross-process determinism is explicitly not yet guaranteed
(review §1.1, §4.3).

**Files touched:** 5 (1 source + 1 source + 4 spec).

**Tests:** existing tests don't construct `OutOfPlay`; no new tests needed.
Existing `sim-core` determinism tests cover the hash output.

## PR 2 — `Score` newtype + `ResponseCurve::Linear` precedence fix

**Goal:** introduce a `Score(f32)` newtype that enforces `0.0 ≤ x ≤ 1.0`
at the type level, change `ResponseCurve::evaluate` to return `Score`, and
fix a real precedence bug in `Linear` curve evaluation.

**Findings (2026-09-29 exploration):**

- `ResponseCurve::evaluate(&self, input: f32) -> f32` at
  `sim-ai-core/src/lib.rs:24-49` returns a raw `f32` with no contract that
  the result is in `[0.0, 1.0]`. Call sites (e.g. the geometric-mean
  combiner in `player_decision_system`) clamp downstream; the lack of a
  type-level contract means a `ResponseCurve` variant that returns
  out-of-range values is a silent bug.
- **Real bug found:** the `Linear` arm:
  `(input - min) / (max - min).clamp(0.0, 1.0)`
  The `.clamp(0.0, 1.0)` clamps `(max - min)`, not the final result. This
  produces NaN/inf when `max <= min` instead of degrading gracefully.
  The fix: `(input - min) / ((max - min).max(eps))` *or* clamp the final
  result with `Score::new`. We pick the latter — let the `Score`
  constructor enforce the bound.

**Changes:**

- `sim-ai-core/src/lib.rs`:
  - Add `pub struct Score(f32)` with:
      - `pub fn new(value: f32) -> Score` (clamps into `[0.0, 1.0]`).
      - `pub const ZERO: Score = Score(0.0)`.
      - `pub const ONE: Score = Score(1.0)`.
      - `pub fn raw(self) -> f32` (lossy accessor for arithmetic that
        genuinely needs raw f32 — e.g. log-space geometric mean).
      - `#[derive(Debug, Clone, Copy, PartialEq)]`.
  - Change `ResponseCurve::evaluate(&self, input: f32) -> Score` and use
    `Score::new` to enforce the bound. In the `Linear` arm, replace the
    broken `(max - min).clamp(0.0, 1.0)` with `(max - min).max(f32::EPSILON)`
    as the divisor; then wrap in `Score::new`.
  - Add unit tests for the `Score` newtype (`test_score_new_clamps`,
    `test_score_raw_round_trip`).
  - Add a test for the linear-bug fix: `test_linear_response_handles_max_eq_min`.

- `sim-ai-player/src/lib.rs`:
  - All call sites of `ResponseCurve::evaluate` (search in the file) adjust
    to either propagate `Score` (preferred) or call `.raw()` where
    arithmetic really needs raw `f32` (e.g. geometric mean over scores).

- `sim-core/src/lib.rs`:
  - Brain-template construction (`default_utility_brain`, lines
    ~975-1140) is unaffected: it constructs `ResponseCurve` variants, not
    calls `evaluate`. No brain-template change in this PR.

- Test fixtures: `sim-ai-player/src/lib.rs:1213, 1344` (test brain
  constructions with `ResponseCurve::Linear { min: 0.0, max: 1.0 }`) — no
  change needed; they don't call `evaluate`.

**Risk:** medium — `ResponseCurve::evaluate` is a public method. The
signature change ripples to all callers. We mitigate by:

- `Score` carrying a `.raw() -> f32` accessor so downstream arithmetic
  doesn't have to change.
- `Score::new` clamping, so old call sites that depend on out-of-range
  outputs would only ever see clamping (which is what the downstream
  combiners were doing anyway).

**Files touched:** 2 source files, no spec changes.

## PR 3 — Consideration enum (no more string dispatch)

**Goal:** replace `PlayerConsideration { name: String, weight, curve }`
and its 190-line string-based dispatch in `compute_consideration_input`
with an exhaustive enum. This is the high-payoff Phase E item: silent
fall-through on unknown names becomes a compile error.

**Findings (2026-09-29 exploration):**

- `sim_ai_core::Consideration` (`sim-ai-core/src/lib.rs:1-5`) is defined
  but **not used** anywhere. The actually-used type is
  `sim_ai_player::PlayerConsideration` (`sim-ai-player/src/lib.rs:49-58`).
  The `sim_ai_core::Consideration` struct is dead. We delete it.
- `PlayerConsideration` is constructed in two test sites
  (`sim-ai-player/src/lib.rs:1213, 1344`) and ~20 brain-template sites in
  `sim-core/src/lib.rs:996-1180`.
- `compute_consideration_input` (`sim-ai-player/src/lib.rs:386-579`)
  matches on `name: &str` against 24 arms. Unknown names fall through to
  `_ => 0.5`. This is the silent-fallthrough smell the review flagged.

**Changes:**

- `sim-ai-player/src/lib.rs`:
  - Delete `sim_ai_player::PlayerConsideration`.
  - Delete `sim_ai_core::Consideration` (in `sim-ai-core/src/lib.rs:1-5`).
  - Add a new enum (placed in `sim-ai-core` since it's data, not
    behaviour):

    ```rust
    #[derive(Debug, Clone)]
    pub enum Consideration {
        DistanceToTarget     { weight: f32, curve: ResponseCurve },
        DistanceToBall       { weight: f32, curve: ResponseCurve },
        Stamina              { weight: f32, curve: ResponseCurve },
        PitchControlAtBall   { weight: f32, curve: ResponseCurve },
        PassAngleClear       { weight: f32, curve: ResponseCurve },
        TeammateDistance     { weight: f32, curve: ResponseCurve },
        TeammateSpace        { weight: f32, curve: ResponseCurve },
        DistanceToGoal       { weight: f32, curve: ResponseCurve },
        GoalAngle            { weight: f32, curve: ResponseCurve },
        DefenderPressure     { weight: f32, curve: ResponseCurve },
        DistanceToOpponent   { weight: f32, curve: ResponseCurve },
        SkillDiff           { weight: f32, curve: ResponseCurve },
        DistanceToMarked     { weight: f32, curve: ResponseCurve },
        DefensivePosition    { weight: f32, curve: ResponseCurve },
        DistanceToPress      { weight: f32, curve: ResponseCurve },
        SpaceAhead           { weight: f32, curve: ResponseCurve },
        TeammateBall         { weight: f32, curve: ResponseCurve },
        FormationDiscipline  { weight: f32, curve: ResponseCurve },
    }

    impl Consideration {
        pub fn weight(&self) -> f32;
        pub fn curve(&self) -> ResponseCurve;
        pub fn raw_input(&self, ctx: &ConsiderationContext) -> f32;
    }

    pub struct ConsiderationContext<'a> {
        pub perception: &'a PerceptionSnapshot,
        pub intent: &'a Intent,
        pub stamina: f32,
        pub skill: f32,
        pub grid: Option<&'a PitchControlGrid>,
    }
    ```

  - Replace `compute_consideration_input` with `Consideration::raw_input`
    using `match self { ... }`. The `_ => 0.5` fall-through disappears —
    adding a new variant is a compile error until the match arm is added.

- `sim-core/src/lib.rs:996-1180` (`default_utility_brain` and friends):
  - Replace `PlayerConsideration { name: "distance_to_ball".into(), ... }`
    with `Consideration::DistanceToBall { weight, curve }`.

- `sim-ai-player/src/lib.rs:1213, 1344` (test brains):
  - Update to the new enum.

**Trade-off:** the brain-template construction is more verbose at the call
site (no `name` shorthand), but is type-checked. This is the win the
review flagged as "make illegal states unrepresentable".

**Risk:** medium-high. All brain templates change. The exhaustive match
in `raw_input` catches new variants, so a future addition is forced to
update both the enum and the dispatch — that's the point.

**Files touched:** 2 source files.

## PR 4 — `Intent` split into movement/action hierarchies

**Goal:** split the flat 11-variant `Intent` enum into `Movement` (no
target or ball-only) and `Action` (with target/point) sub-enums. The
review flagged this as "intent is a single enum with 10 variants
covering very different actions" — a hierarchy clarifies which actions
care about what. Per user direction (2026-09-29), hard cutover.

**Current shape** (`sim-components/src/lib.rs:108-121`):

```rust
pub enum Intent {
    MoveToPosition(Vec2),   // movement, with point
    PassTo,                  // action, target=teammate
    ShootAtGoal(Vec2),       // action, with point
    Tackle(Entity),          // action, target=opponent
    ChaseBall,               // movement
    MarkOpponent(Entity),    // action, target=opponent
    Intercept,               // movement (reserved variant)
    Press(Entity),           // action, target=opponent
    HoldPosition,            // movement
    SupportRun,              // movement
    TrackBack,               // movement (reserved variant)
}
```

`Intercept` and `TrackBack` are reserved variants — defined, never
constructed by `default_utility_brain`, only matched. We keep them in
the new shape.

**Target shape:**

```rust
pub enum Intent {
    Movement(MovementIntent),
    Action(ActionIntent),
}

pub enum MovementIntent {
    MoveToPosition(Vec2),
    HoldPosition,
    ChaseBall,
    Intercept,
    SupportRun,
    TrackBack,
}

pub enum ActionIntent {
    PassTo,
    ShootAtGoal(Vec2),
    Tackle(Entity),
    MarkOpponent(Entity),
    Press(Entity),
}
```

**Changes:**

- `sim-components/src/lib.rs`:
  - Replace the flat `Intent` enum with the split + the two sub-enums.
  - Add `impl Intent { pub fn kind(&self) -> IntentKind { ... } }` and
    `pub enum IntentKind { Movement, Action }` for sites that don't need
    to discriminate further (currently none; included for future use).

- `sim-ai-player/src/lib.rs` — 3 dispatch sites:
  - `intent_kind` (`sim-ai-player/src/lib.rs:362-376`) — returns
    `&'static str` from discriminant. Update to return from the split
    shape (each variant gets its own arm).
  - `player_action_execution_system` (`sim-ai-player/src/lib.rs:621-662`)
    — matches on `&intent` for stochastic modifiers (Tackle, PassTo,
    ShootAtGoal — all `Action` variants). Replace with
    `match intent { Intent::Action(...) => ..., Intent::Movement(_) => {} }`.
  - `steer` (`sim-ai-player/src/lib.rs:796-820`) — exhaustive match on
    intent. Replace with two-level match.

- `sim-ai-player/src/lib.rs:741-754` (`kick_execution_system`) — also
  matches on `&intent`. Update.

- Brain templates (`sim-core/src/lib.rs:996-1180`,
  `sim-ai-player/src/lib.rs:1213, 1344`) — wrap each construction in the
  appropriate sub-enum:

  ```rust
  // before
  PlayerAction { intent: Intent::PassTo, considerations: ... }
  // after
  PlayerAction { intent: Intent::Action(ActionIntent::PassTo), considerations: ... }
  ```

- `sim_components::Intent::PassTo` users — for sites that just need a
  "do pass now" intent, consider a `Intent::pass_to()` constructor.
  Defer to a later cleanup if it would change more than a few call sites.

- All `match Intent` sites in tests.

**Risk:** medium-high. `Intent` is a public type used in snapshot
serialisation, replay, brain templates, and 4+ dispatch sites. The hard
cutover means a single failed `match` is caught at compile time. The
`Intent::kind()` helper means sites that don't need to care about the
split can be left alone.

**Files touched:** 1 source + 1 source + tests (depends on test
inventory).

## PR 5 — Snapshot round-trip tests + PartialEq derives

**Goal:** add round-trip tests for every public snapshot type, and
derive `PartialEq` (where possible `Eq`) on them. Catches silent
serialisation breakage.

**Findings (2026-09-29 exploration):**

- `sim_core::MatchSnapshot` (`sim-core/src/lib.rs:715-724`) and its
  four view types (`MatchStateView`, `BallView`, `PlayerView`,
  `ClockView`) all derive `Debug, Clone, Serialize, Deserialize` but
  **not `PartialEq`**. The `PlayerView.position: [f32; 2]` field is
  `f32` so we can't derive `Eq`, only `PartialEq`.
- `sim_replay::RecordedSnapshot` has a test (`sim-replay/src/lib.rs:318-340`)
  but it asserts only 3 of 6 fields.
- `sim-replay::BallSnapshot`, `sim-replay::PlayerSnapshot` are
  defined but **never constructed** (dead types — same shape as the
  sim-core view types but unused). They were apparently the original
  replay view types; now superseded by `RecordedSnapshot`'s
  flattened shape. Delete them as part of this PR.
- `sim_replay::ReplayResult` (`sim-replay/src/lib.rs:113-119`) lacks
  `Serialize` — adding it lets us serialise replay results too.

**Changes:**

- `sim-core/src/lib.rs`:
  - Add `PartialEq` to `MatchSnapshot`, `MatchStateView`, `BallView`,
    `PlayerView`, `ClockView`.
  - Add a new test file `crates/sim-core/tests/snapshot_round_trip.rs`
    that:
      1. Runs a minimal simulation (small seed, small tick count) and
         captures a `MatchSnapshot`.
      2. Serialises to JSON via `serde_json` (already in `sim-server` —
         add it as a `[dev-dependencies]` for `sim-core`).
      3. Deserialises and asserts field-by-field equality.
      4. Bonus: assert that running the same seed twice produces
         snapshots that are equal (determinism cross-check at the
         snapshot level).

- `sim-replay/src/lib.rs`:
  - Add `PartialEq` to `RecordedSnapshot`.
  - Delete the dead `BallSnapshot` and `PlayerSnapshot` structs
    (`sim-replay/src/lib.rs:165-183`). The existing
    `create_snapshot_from_simulation` returns `RecordedSnapshot` and
    constructs neither.
  - Extend `test_snapshot_serialization` to assert all 6 fields.

- `sim-server/Cargo.toml` already has `serde_json`; we don't need to
  touch it. We add `serde_json` as a `[dev-dependencies]` to
  `sim-core/Cargo.toml` and `sim-replay/Cargo.toml`.

**Risk:** low. `PartialEq` is a derive; new tests don't change
production code paths. The deletion of dead types is pure cleanup.

**Files touched:** 2 source + 1 Cargo.toml (sim-core) + 1 Cargo.toml
(sim-replay) + 1 new test file.

## Sequencing & verification

PRs land in order. Each PR has a verification step before the next:

| PR | Verification |
|----|---------------|
| 1 | `cargo check --workspace` clean; `cargo test --workspace` 71 tests pass; `cargo clippy --workspace --all-targets -- -D warnings` clean. |
| 2 | Same as PR 1 + new `Score` tests pass + `test_linear_response_handles_max_eq_min` passes. |
| 3 | Same + exhaustive match compiles (no `_ =>` arm); all brain templates construct; all existing tests pass. |
| 4 | Same + `Intent::kind()` matches the old `intent_kind()` for all variants; all dispatch sites compile exhaustively; all tests pass. |
| 5 | Same + new round-trip tests pass; old `test_snapshot_serialization` still passes with extended assertions. |

Each PR is a separate commit (or series of commits — e.g. PR 1 has the
source changes and the spec-doc changes as separate commits in one PR)
on a feature branch off `main`. After CI is green on the PR branch, it
gets merged with `--no-ff` so the 5 phase-1 entries
(`refactor(sim): Phase E …`) are visible in history.

## Decision log (2026-09-29)

- ✅ Adopted 5-PR plan as proposed.
- ✅ Drop `BallState::OutOfPlay` (delete dead variant; update spec docs).
- ✅ Hard cutover for `Intent` split (no deprecation period).
- ⏸️ Fold `ServerSimulation::command_queue` field privacy into PR 1 only if
  it's a single-line change; defer otherwise.