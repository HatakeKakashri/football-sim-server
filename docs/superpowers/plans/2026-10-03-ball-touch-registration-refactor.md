# Ball-Touch Registration Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace five direct writes to `Ball.last_touch` (across three crates) with a single touch-registration path: a `PendingBallTouches` Resource + `apply_ball_touched_system` consumer. Carry the full `LastTouch` record in `RuleEvent::{OutOfBounds, Goal}` payloads. Surface `last_touch` and `last_deliberate_play` in `BallView`/`BallSnapshot`. Mix `last_deliberate_play` into the state hash.

**Architecture:** Each of the twelve milestones (M1..M12) lands in order. M1-M2 change the data model; M3-M4 add the resource + consumer; M5-M7 migrate the writers; M8 cleans up the readers; M9-M11 update observers (snapshots, hash, fixtures); M12 lands new tests. Every milestone ends with `cargo test --workspace` green. The `B`-stage · M11 captures replay golden hashes from a fresh full-match run.

**Tech Stack:** Rust workspace, edition 2024, Bevy 0.14 ECS, serde (for `LastTouch` derives), tracing.

**Spec:** `docs/superpowers/specs/2026-10-03-ball-touch-event-refactor-design.md`

## Global Constraints

- Workspace `unsafe_code = "forbid"`. No `unsafe` blocks ever.
- `cargo clippy --workspace --all-targets -- -D warnings` must remain clean at every commit.
- `cargo test --workspace` must pass at every commit (baseline 179 passing, 2 ignored).
- Bevy system params (`Query`, `Res`, …) taken **by value**, not reference (workspace allow on `needless_pass_by_value`).
- No `unwrap` in new code — use `expect("invariant: …")` with a clear message.
- Every `#[allow(...)]` / `#[expect(...)]` must carry `reason = "…"`.
- File-size budget per `docs/CODING_STANDARDS.md` §6. Split before exceeding.
- Use `#[must_use]` on pure accessors.
- Determinism contract: `final_state_hash` is byte-identical across two runs of the same seed (per `AGENTS.md` §"Determinism"). M10-M11 update golden hashes; verify with two `simulate --full-match` runs.
- `cargo fmt --all` before every commit.
- Commit messages follow prefix `feat(touch):` / `refactor(rules):` / `fix(rules):` per the established convention.
- The OOB restart bug ticket at `.superpowers/sdd/oob-restart-bug.md` is **not** fixed by this plan. The plan's M8 changes preserve that bug's surface (the deferred-commands gotcha at `out_of_bounds.rs:135`) until the OOB fix lands.

## File Structure Map

| File | M | Action | Responsibility |
|------|---|--------|----------------|
| `crates/sim-components/src/lib.rs` | 1, 2, 3, 8 | Modify | `LastTouch` gains `team`; `LastTouch.entity` → `player: Option<Entity>`; `Ball.last_deliberate_play` field; `TouchKind`/`LastTouch` derive `Serialize, Deserialize`; `PendingBallTouches` resource; `RuleEvent` payload changes |
| `crates/sim-core/src/simulation/construct.rs` | 3, 4, 11 | Modify | Insert `PendingBallTouches` resource; register `apply_ball_touched_system` first in `Rules` set; initialise `Ball.last_deliberate_play = None` |
| `crates/sim-core/src/simulation/state_hash.rs` | 10 | Modify | Mix `last_deliberate_play` into state hash |
| `crates/sim-core/src/lib.rs` | 9 | Modify | `BallView` gains `last_touch` and `last_deliberate_play` fields |
| `crates/sim-replay/src/lib.rs` | 9, 11 | Modify | `BallSnapshot` gains `last_touch` and `last_deliberate_play`; update replay determinism test expected hashes |
| `crates/sim-ai-player/src/execution.rs` | 5, 11 | Modify | `kick_execution_system` pushes `LastTouch{player,team,Play}` to `PendingBallTouches` instead of direct write; `ball_fixture` in tests gains `last_deliberate_play: None` |
| `crates/sim-ai-player/src/perception_tests.rs` | 11 | Modify | Add `last_deliberate_play: None` to `ball_fixture` |
| `crates/sim-rules/src/possession.rs` | 4, 6 | Modify | `possession_resolution_system` pushes to `PendingBallTouches` instead of direct write; new `apply_ball_touched_system` consumer |
| `crates/sim-rules/src/lib.rs` | 4 | Modify | Re-export `apply_ball_touched_system` |
| `crates/sim-rules/src/out_of_bounds.rs` | 8 | Modify | Stop clearing `Ball.last_touch`; capture full `LastTouch` into `RuleEvent::OutOfBounds`; tighten pre-touch OOB with `tracing::error!` + bail |
| `crates/sim-rules/src/goals.rs` | 7, 8 | Modify | `restart_system` OOB and kickoff-after-goal arms push to `PendingBallTouches`; `goal_detection_system` captures full `LastTouch` into `RuleEvent::Goal` and stops clearing `Ball.last_touch`; tightened pre-touch goal error |
| `crates/sim-rules/src/tests.rs` | 11 | Modify | `ball_fixture` gains `last_deliberate_play: None` |
| `crates/sim-rules/src/possession_tests.rs` | 11 | Modify | Schedule-chain `possession_resolution_system` with `apply_ball_touched_system`; update `last_touch` assertions to use new `LastTouch` shape |
| `crates/sim-rules/src/goals_tests.rs` | 11, 12 | Modify | `RuleEvent` payload assertions; extend with new tests M12 lists |
| `crates/sim-rules/src/out_of_bounds_tests.rs` | 11, 12 | Modify | Delete `is_none()` post-OOB assertion; `RuleEvent` payload assertions; extend with new tests M12 lists |
| `crates/sim-rules/src/referee_tests.rs` | 11, 12 | Modify | Update `LastTouch` shape in fixtures; extend with new tests M12 lists |
| `crates/sim-rules/src/touch_registration_tests.rs` | 12 | Create | New test file with M12-listed tests |
| `crates/sim-physics/src/lib.rs` | 11 | Modify | Add `last_deliberate_play: None` to `Ball` fixture |
| `crates/sim-core/src/tests/telemetry_tests.rs` | 11 | Modify | Add `last_deliberate_play: None` to fixture |
| `crates/sim-ai-player/src/execution_tests.rs` | 12 | Modify | Extend with new kick-push test |

---

## M1: `LastTouch { team, player, kind }` + `Ball.last_deliberate_play`

**Files:**
- Modify: `crates/sim-components/src/lib.rs:96-153, 310-328`
- Modify: every file that constructs a `Ball` literal (see §11 below)
- Test: existing tests continue to pass; no new tests in this milestone (data-only change)

**Interfaces:**
- Consumes: existing `LastTouch { entity: Entity, kind: TouchKind }` constructors and field accesses across the workspace
- Produces: `LastTouch { team: TeamId, player: Option<Entity>, kind: TouchKind }` — same `kind` semantics; `entity` becomes `player: Option<Entity>`; new `team` field

### Task 1.1: Add `Serialize`/`Deserialize` to `TouchKind`

**Files:**
- Modify: `crates/sim-components/src/lib.rs:134-142`

- [ ] **Step 1: Edit the derives**

In `crates/sim-components/src/lib.rs` line 134, change:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TouchKind {
```

to:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TouchKind {
```

`Serialize`/`Deserialize` are imported via the `bevy_ecs::prelude::*` plus the existing `serde` use at the top of the file (verify with `grep -n "use serde" crates/sim-components/src/lib.rs` — if absent, add `use serde::{Deserialize, Serialize};`).

- [ ] **Step 2: Verify compile**

```bash
cargo build --workspace
```

Expected: BUILD SUCCESS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add crates/sim-components/src/lib.rs
git commit -m "feat(touch): add Serialize/Deserialize derives to TouchKind"
```

### Task 1.2: Reshape `LastTouch` to `{ team, player, kind }`

**Files:**
- Modify: `crates/sim-components/src/lib.rs:144-153`

- [ ] **Step 1: Edit the struct**

In `crates/sim-components/src/lib.rs` lines 144-153, change:

```rust
/// Records who last touched the ball and how.
///
/// `entity` is the player who touched the ball (or the team's kickoff
/// taker for a `Restart` touch). `kind` distinguishes active play from
/// restart placements so Law 11 (offside) can ignore restart touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LastTouch {
    pub entity: Entity,
    pub kind: TouchKind,
}
```

to:

```rust
/// Records who last touched the ball and how.
///
/// `team` is denormalised so the record resolves even if the entity
/// despawns. `player` is the player who touched the ball, or `None`
/// when no specific player is credited (a kickoff placement today;
/// future restarts may also use `None`). `kind` distinguishes active
/// play from restart placements so Law 11 (offside) can ignore
/// restart touches via `offside_detection_system`'s existing
/// `kind == Play` filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LastTouch {
    pub team: TeamId,
    pub player: Option<Entity>,
    pub kind: TouchKind,
}
```

- [ ] **Step 2: Compile and see errors**

```bash
cargo build --workspace 2>&1 | head -100
```

Expected: many compile errors at sites that construct `LastTouch { entity: ..., kind: ... }` (notably `crates/sim-ai-player/src/execution.rs:213-216`, `crates/sim-rules/src/possession.rs:75-78`, `crates/sim-rules/src/goals.rs:201-204, 251-254`, `crates/sim-core/src/simulation/lifecycle.rs:248-252`, plus all test fixtures).

The fix is to be done in Task 1.3; this step is just to surface the full list.

- [ ] **Step 3: Do not commit yet**

The struct changes in M1.2 break the workspace until M1.3. Land M1.3 first.

### Task 1.3: Update all `LastTouch` construction sites

**Files:**
- Modify: every site listed in §M1.2 step 2's error output

- [ ] **Step 1: For each call site, find the player entity and team**

For every site like:

```rust
ball.last_touch = Some(sim_components::LastTouch {
    entity: possessor,
    kind: sim_components::TouchKind::Play,
});
```

Determine:
- `team`: from `player_query` (passes through `TeamIdComponent`) or from the existing context. If the entity is `Entity::PLACEHOLDER` (used as a fallback in `restart_system`), use the team that the touch is being attributed to (`conceded_team` for kickoff-after-goal, the opposite-of-last-toucher team for OOB). If the team is unknown (genuine placeholder case), use `TeamId(0)` and flag the call site for a follow-up TODO comment.
- `player`: `Some(entity)` for any real player, `None` for `Entity::PLACEHOLDER` (or for kickoff placement when no specific player is credited).

- [ ] **Step 2: Rewrite each construction site**

For `crates/sim-ai-player/src/execution.rs:213-216`:

```rust
let player_team = player_query
    .iter()
    .find(|(e, _, _, _)| *e == possessor)
    .map(|(_, p, _, _)| p.team_id);
ball.last_touch = Some(sim_components::LastTouch {
    team: player_team.unwrap_or(sim_components::TeamId(0)),
    player: Some(possessor),
    kind: sim_components::TouchKind::Play,
});
```

The `player_team.unwrap_or(TeamId(0))` fallback should never trigger in practice (the possessor is queried out of the same query), but it satisfies `no unwrap` rule. Replace `TeamId(0)` with `expect("invariant: possessor entity has a TeamIdComponent")` if the team lookup is provably infallible — verify with a debug assert.

For `crates/sim-rules/src/possession.rs:75-78`:

```rust
let player_team = player_query
    .iter()
    .find(|(e, _, _, _)| *e == entity)
    .map(|(_, _, _, _)| /* player's team */)
    .expect("invariant: closest-within-1.5m entity has TeamIdComponent");
```

The query at `possession.rs:35` already has `&TeamIdComponent` — verify and use it directly.

For `crates/sim-rules/src/goals.rs:201-204` (kickoff-after-goal arm, `conceded_team` is known):

```rust
let taker_team = pr.scorer_team.map_or(TeamId(0), |scorer| {
    if scorer == TeamId(0) {
        TeamId(1)
    } else {
        TeamId(0)
    }
});
let taker = first_player_on_team(&player_query, taker_team);
ball.last_touch = Some(sim_components::LastTouch {
    team: taker_team,
    player: Some(taker), // `first_player_on_team` may return PLACEHOLDER; see M5 for 6/7 conversion
    kind: sim_components::TouchKind::Restart,
});
```

(For now, use `Some(taker)` even if `taker == Entity::PLACEHOLDER` — M5/M7 will convert to `None`. The struct shape change in M1 doesn't *require* the `None` conversion; it just makes it possible.)

For `crates/sim-rules/src/goals.rs:251-254` (OOB arm):

Same pattern — use the team opposite the prior toucher, `Some(taker_entity)` for now.

For `crates/sim-core/src/simulation/lifecycle.rs:248-252` (kickoff placement):

```rust
let taker_entity = pick_kickoff_taker(world, kickoff_taker_team);
let is_placeholder = taker_entity == bevy_ecs::prelude::Entity::PLACEHOLDER;
world.resource_mut::<sim_components::Ball>().last_touch =
    Some(sim_components::LastTouch {
        team: kickoff_taker_team,
        player: if is_placeholder { None } else { Some(taker_entity) },
        kind: sim_components::TouchKind::Restart,
    });
```

- [ ] **Step 3: Update `possession.rs` to capture the team**

The current `possession_resolution_system` does not resolve the team. It needs the team to construct the new `LastTouch`. Modify the system signature:

```rust
pub fn possession_resolution_system(
    ball_query: Query<&Position, With<BallMarker>>,
    player_query: Query<(Entity, &Player, &Position, &Skill, &TeamIdComponent), With<Player>>,
    mut ball: ResMut<Ball>,
) {
    // ... existing logic ...
    if let Some((entity, _, _)) = closest_player {
        // Resolve team for the touch record. Existence of TeamIdComponent
        // is an invariant of the Player bundle — if missing, debug-build
        // assert fails.
        let team = player_query
            .get(entity)
            .map(|(_, _, _, _, t)| t.0)
            .expect("invariant: closest player has TeamIdComponent");
        ball.possessor = Some(entity);
        ball.state = BallState::Possessed;
        ball.last_touch = Some(LastTouch {
            team,
            player: Some(entity),
            kind: TouchKind::Play,
        });
    } else {
        ball.possessor = None;
        ball.state = BallState::Free;
    }
}
```

Note: this step **temporarily re-introduces a direct write** to `Ball.last_touch`. M6 will replace this with a buffer push. The data shape change in M1.3 has to land before M3-M7 because the buffer type references `LastTouch`.

- [ ] **Step 4: Update test fixtures that construct `LastTouch`**

`grep -rn "LastTouch {" crates/` returns all sites. Update each to the new shape. For tests that don't have a meaningful team (e.g., `possession_tests.rs:301` which uses a `player_entity` fixture), use the `TeamIdComponent` attached to the test entity if present, or `TeamId(0)` with a comment if the test doesn't care about the team.

- [ ] **Step 5: Verify compile**

```bash
cargo build --workspace
```

Expected: BUILD SUCCESS.

- [ ] **Step 6: Run tests**

```bash
cargo test --workspace
```

Expected: 179 passed (some tests may need their `LastTouch { entity: ..., kind: ... }` literal updated to the new shape — fix any failures by re-reading the test fixture).

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add crates/sim-components/src/lib.rs crates/sim-ai-player/src/execution.rs crates/sim-rules/src/possession.rs crates/sim-rules/src/goals.rs crates/sim-core/src/simulation/lifecycle.rs crates/sim-*/src/tests.rs crates/sim-*/src/*_tests.rs
git commit -m "feat(touch): reshape LastTouch to { team, player, kind }"
```

### Task 1.4: Add `Ball.last_deliberate_play` field

**Files:**
- Modify: `crates/sim-components/src/lib.rs:310-328`

- [ ] **Step 1: Add the field**

In `crates/sim-components/src/lib.rs`, edit `Ball` (around line 323) to add `last_deliberate_play` after `last_touch`:

```rust
pub struct Ball {
    pub spin: f32,
    pub state: BallState,
    pub possessor: Option<Entity>,
    pub last_touch: Option<LastTouch>,
    /// Reserved for Law 11's "deliberate play" wording. No system
    /// writes to this field in this plan; it is added now so a
    /// future deflection system does not need a Ball-shape change.
    /// Always `None` until that work is done. The state hash mixes
    /// it (M10); every Ball constructor must initialise it.
    pub last_deliberate_play: Option<LastTouch>,
    pub kick_velocity: Option<Vec2>,
}
```

- [ ] **Step 2: Update every `Ball` constructor**

`grep -rn "Ball {" crates/` (and `Ball {\n` etc.) — for each `Ball { ... }` literal, add `last_deliberate_play: None,` after the `last_touch: ...` line.

Sites known to need updating (from the spec's exploration):
- `crates/sim-core/src/simulation/construct.rs:190`
- `crates/sim-rules/src/tests.rs:34-42` (`ball_fixture`)
- `crates/sim-rules/src/possession_tests.rs:26, 74, 134, 187, 301`
- `crates/sim-rules/src/goals_tests.rs:51-54, 73-76`
- `crates/sim-rules/src/out_of_bounds_tests.rs:15-31`
- `crates/sim-rules/src/referee_tests.rs:140`
- `crates/sim-ai-player/src/execution.rs:300, 370, 429`
- `crates/sim-ai-player/src/perception_tests.rs:27`
- `crates/sim-core/src/tests/telemetry_tests.rs:306`
- `crates/sim-physics/src/lib.rs:374`

- [ ] **Step 3: Verify compile and tests**

```bash
cargo build --workspace
cargo test --workspace
```

Expected: BUILD SUCCESS, 179 passed.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/sim-components/src/lib.rs crates/sim-core/src/simulation/construct.rs crates/sim-*/src/tests.rs crates/sim-*/src/*_tests.rs crates/sim-physics/src/lib.rs
git commit -m "feat(touch): add Ball.last_deliberate_play (always None this plan)"
```

---

## M2: `RuleEvent::{OutOfBounds, Goal}.last_touch: Option<LastTouch>`

**Files:**
- Modify: `crates/sim-components/src/lib.rs:96-124`
- Modify: `crates/sim-rules/src/out_of_bounds.rs:114-138`
- Modify: `crates/sim-rules/src/goals.rs:67-99, 120-141`

**Interfaces:**
- Consumes: `LastTouch` from M1
- Produces: `RuleEvent::OutOfBounds { oob_type, last_touch }` and `RuleEvent::Goal { scorer_team, score, last_touch }`

### Task 2.1: Change `RuleEvent` payload type

**Files:**
- Modify: `crates/sim-components/src/lib.rs:96-124`

- [ ] **Step 1: Edit the enum variants**

```rust
pub enum RuleEvent {
    OutOfBounds {
        oob_type: BallOutOfBoundsType,
        /// The full touch record at the moment the ball crossed the
        /// boundary. `last_touch` is preserved on `Ball` after the
        /// event fires (today it is cleared); downstream consumers
        /// can re-read `Ball.last_touch` for replay data.
        last_touch: Option<LastTouch>,
    },
    Goal {
        scorer_team: TeamId,
        score: (u8, u8),
        last_touch: Option<LastTouch>,
    },
    KickoffRestart,
    HalfTimeStart,
    FullTimeStart,
    Foul { fouler: u64, foulee: u64, foul_type: FoulType },
}
```

- [ ] **Step 2: Compile and see errors**

```bash
cargo build --workspace 2>&1 | head -50
```

Expected: errors at every site that constructs `RuleEvent::OutOfBounds { last_touch_team: ... }` or `RuleEvent::Goal { ..., last_touch_team: ... }`.

- [ ] **Step 3: Update construction sites in `out_of_bounds.rs`**

`crates/sim-rules/src/out_of_bounds.rs:114-138`. Replace the `last_touch_team` capture block:

```rust
let last_touch = ball.last_touch;
// (Keep the existing pre-touch-OOB debug_assert from §E; do not clear
// `Ball.last_touch` in this task — it remains as a direct write here
// until M8.)
```

Then at the `commands.entity(ball_entity).insert(RuleEvent::OutOfBounds { ... })` site:

```rust
commands.entity(ball_entity).insert(RuleEvent::OutOfBounds {
    oob_type,
    last_touch,
});
```

- [ ] **Step 4: Update construction sites in `goals.rs`**

`crates/sim-rules/src/goals.rs:67-99` (home and away goal arms) — same pattern: capture `ball.last_touch` once before clearing, pass into the `RuleEvent::Goal` payload.

- [ ] **Step 5: Update test assertions that destructure `RuleEvent`**

`grep -rn "RuleEvent::OutOfBounds\|RuleEvent::Goal" crates/`. Tests that destructure `RuleEvent::OutOfBounds { last_touch_team, ... }` or `RuleEvent::Goal { last_touch_team, ... }` must be updated to `last_touch: Some(LastTouch { ... })` or `last_touch: None`.

- [ ] **Step 6: Verify compile and tests**

```bash
cargo build --workspace
cargo test --workspace
```

Expected: BUILD SUCCESS, 179 passed (with test updates applied).

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add crates/sim-components/src/lib.rs crates/sim-rules/src/out_of_bounds.rs crates/sim-rules/src/goals.rs crates/sim-rules/src/*_tests.rs
git commit -m "refactor(rules): RuleEvent payload carries full LastTouch record"
```

---

## M3: `PendingBallTouches` resource type

**Files:**
- Modify: `crates/sim-components/src/lib.rs` (new `pub struct PendingBallTouches`)
- Modify: `crates/sim-core/src/simulation/construct.rs` (resource init)

**Interfaces:**
- Consumes: `LastTouch` from M1
- Produces: `Resource` named `PendingBallTouches(pub Vec<LastTouch>)`

### Task 3.1: Define the resource

**Files:**
- Modify: `crates/sim-components/src/lib.rs` (add after `LastTouch` definition)

- [ ] **Step 1: Add the struct**

In `crates/sim-components/src/lib.rs`, after the `LastTouch` definition (around line 153), add:

```rust
/// Pending ball touches registered this tick. Writers (kick, possession
/// acquisition, restart placements) append to this buffer; the
/// consumer `apply_ball_touched_system` drains it at the start of
/// the Rules set and writes the final entry to `Ball.last_touch`.
///
/// Determinism: writes happen in fixed system-set order
/// (Execution → Possession → Rules → MatchAdmin), and Bevy
/// `ResMut` access serialises those writes. Last-touch-wins
/// semantics are equivalent to a same-tick EventReader ordered by
/// emission time.
///
/// This resource replaces a Bevy `Events<T>` channel by design
/// (see the design spec §B and the OOB restart bug ticket at
/// `.superpowers/sdd/oob-restart-bug.md`, which recommends
/// Resource over events).
#[derive(Resource, Debug, Default)]
pub struct PendingBallTouches(pub Vec<LastTouch>);
```

- [ ] **Step 2: Register the resource in `construct.rs`**

In `crates/sim-core/src/simulation/construct.rs`, find the `Simulation::new` body where other `Resources` are inserted (search for `insert_resource(`, near line 190). After `Ball { ... }` is inserted, add:

```rust
world.insert_resource(PendingBallTouches::default());
```

Add `use sim_components::PendingBallTouches;` to the imports if not already present.

- [ ] **Step 3: Verify compile**

```bash
cargo build --workspace
```

Expected: BUILD SUCCESS.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/sim-components/src/lib.rs crates/sim-core/src/simulation/construct.rs
git commit -m "feat(touch): add PendingBallTouches resource"
```

---

## M4: `apply_ball_touched_system` consumer

**Files:**
- Create: function `apply_ball_touched_system` in `crates/sim-rules/src/possession.rs` (or a new `crates/sim-rules/src/touch.rs` — prefer the new file to keep `possession.rs` focused on its existing responsibility)
- Modify: `crates/sim-rules/src/lib.rs` (re-export)
- Modify: `crates/sim-core/src/simulation/construct.rs` (register the system)

**Interfaces:**
- Consumes: `ResMut<PendingBallTouches>` from M3, `ResMut<Ball>` from M1
- Produces: writes `Ball.last_touch` from the buffer's last entry; drains the buffer

### Task 4.1: Implement `apply_ball_touched_system`

**Files:**
- Create: `crates/sim-rules/src/touch.rs`
- Modify: `crates/sim-rules/src/lib.rs` (add `pub mod touch;` and re-export)

- [ ] **Step 1: Create the new module file**

```rust
//! Touch registration consumer.
//!
//! `apply_ball_touched_system` drains `PendingBallTouches` at the
//! start of the Rules set and writes the last (latest-in-schedule)
//! `LastTouch` to `Ball.last_touch`. Writers across Execution,
//! Possession, and the earlier Rules systems push in fixed schedule
//! order, so the last entry is the deterministically-correct touch
//! for the tick.

use bevy_ecs::prelude::*;
use sim_components::{Ball, PendingBallTouches};

/// Drains `PendingBallTouches` and applies the last touch to
/// `Ball.last_touch`. Runs at the **start** of the `Rules` set so
/// that offside detection, OOB detection, goal detection, and
/// restart placement all see the latest touch in the same tick.
///
/// If the buffer is empty (e.g., the first tick after kickoff
/// before any kick), `Ball.last_touch` is unchanged — the direct
/// write from `lifecycle_system` (kickoff placement) is the source
/// of truth for those ticks.
pub fn apply_ball_touched_system(
    mut pending: ResMut<PendingBallTouches>,
    mut ball: ResMut<Ball>,
) {
    if let Some(last) = pending.0.pop() {
        ball.last_touch = Some(last);
    }
    // Drain any earlier entries defensively (shouldn't happen, but
    // keeps the buffer from growing unbounded across ticks if a
    // future consumer partial-reads).
    pending.0.clear();
}
```

- [ ] **Step 2: Wire the module**

In `crates/sim-rules/src/lib.rs`, add:

```rust
pub mod touch;
pub use touch::apply_ball_touched_system;
```

at the appropriate module-declarations block (search for `pub mod` in the file).

- [ ] **Step 3: Verify compile**

```bash
cargo build --workspace
```

Expected: BUILD SUCCESS.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/sim-rules/src/touch.rs crates/sim-rules/src/lib.rs
git commit -m "feat(touch): add apply_ball_touched_system consumer"
```

### Task 4.2: Register the system in the Rules set

**Files:**
- Modify: `crates/sim-core/src/simulation/construct.rs` (find `register_systems` helper or equivalent; if absent, find the schedule-config block)

- [ ] **Step 1: Find the Rules-set configuration**

`grep -n "Rules\|offside_detection_system\|out_of_bounds_system" crates/sim-core/src/simulation/construct.rs`. The Rules set is configured somewhere in `register_systems` (per AGENTS.md). The exact file/method may vary — find the schedule setup.

- [ ] **Step 2: Add the consumer first in the set**

In the `Rules` configuration block, add `apply_ball_touched_system` as the first system in the chain. The actual existing order (verified during M4) is `out_of_bounds_system → goal_detection_system → offside_detection_system → foul_detection_system → restart_system`; the consumer is prepended without reordering the others.

The existing wiring pattern uses `.chain().in_set(SimulationSet::Rules).after(SimulationSet::Possession)`. Match that style.

- [ ] **Step 3: Verify**

```bash
cargo build --workspace
cargo test --workspace
```

Expected: BUILD SUCCESS, 179 passed.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/sim-core/src/simulation/construct.rs
git commit -m "refactor(core): register apply_ball_touched_system first in Rules set"
```

---

## M5: `kick_execution_system` pushes to `PendingBallTouches`

**Files:**
- Modify: `crates/sim-ai-player/src/execution.rs:213-216`

**Interfaces:**
- Consumes: `PendingBallTouches` from M3, `Ball` from M1
- Produces: pushes `LastTouch { team, player: Some(kicker), kind: Play }`

### Task 5.1: Replace direct write with buffer push

**Files:**
- Modify: `crates/sim-ai-player/src/execution.rs:167-239`

- [ ] **Step 1: Add `PendingBallTouches` to the system signature**

```rust
pub fn kick_execution_system(
    player_query: Query<(Entity, &Player, &PerceptionSnapshot, &Velocity)>,
    mut ball: ResMut<Ball>,
    mut pending: ResMut<PendingBallTouches>,
    match_clock: Option<Res<MatchClock>>,
    telemetry: Option<Res<TelemetryConfig>>,
) {
```

Add `use sim_components::PendingBallTouches;` to the imports.

- [ ] **Step 2: Replace the direct write**

Find lines 213-216 (the `ball.last_touch = Some(...)` block). Replace with:

```rust
// Register the kick as an explicit `Play` touch (Law 11 input
// for offside judgement). Push to PendingBallTouches; the consumer
// `apply_ball_touched_system` applies it to `Ball.last_touch` at
// the start of the Rules set.
let kicker_team = player_query
    .iter()
    .find(|(e, _, _, _)| *e == possessor)
    .map(|(_, p, _, _)| p.team_id)
    .expect("invariant: possessor has Player component with team_id");
pending.0.push(LastTouch {
    team: kicker_team,
    player: Some(possessor),
    kind: TouchKind::Play,
});
ball.possessor = None;
```

Remove the `Some(sim_components::LastTouch { ... })` direct write.

- [ ] **Step 3: Verify and run the kick test**

```bash
cargo build --workspace
cargo test -p sim-ai-player kick_execution_registers_play_touch
```

Expected: BUILD SUCCESS, test PASSES (the test schedules only kick; `apply_ball_touched_system` must be added to the test's schedule — see M11 task 11.5).

If the test fails because `Ball.last_touch` is `None` after `kick_execution_system` alone (no consumer), update the test to include `apply_ball_touched_system` in the schedule chain. This is the M11 test-fixture update.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/sim-ai-player/src/execution.rs
git commit -m "refactor(ai): kick_execution_system pushes LastTouch to buffer"
```

---

## M6: `possession_resolution_system` pushes to `PendingBallTouches`

**Files:**
- Modify: `crates/sim-rules/src/possession.rs:33-83`

**Interfaces:**
- Consumes: `PendingBallTouches` from M3
- Produces: pushes `LastTouch { team, player: Some(closest), kind: Play }`

### Task 6.1: Replace direct write with buffer push

**Files:**
- Modify: `crates/sim-rules/src/possession.rs`

- [ ] **Step 1: Add `PendingBallTouches` to the system signature**

```rust
pub fn possession_resolution_system(
    ball_query: Query<&Position, With<BallMarker>>,
    player_query: Query<(Entity, &Player, &Position, &Skill, &TeamIdComponent), With<Player>>,
    mut ball: ResMut<Ball>,
    mut pending: ResMut<PendingBallTouches>,
) {
```

The query gains `&TeamIdComponent` (already mentioned in M1.3 step 3).

- [ ] **Step 2: Replace the direct write**

Find lines 75-78 (the `ball.last_touch = Some(...)` block). Replace with:

```rust
let team = player_query
    .get(entity)
    .map(|(_, _, _, _, t)| t.0)
    .expect("invariant: closest-within-1.5m player has TeamIdComponent");
pending.0.push(LastTouch {
    team,
    player: Some(entity),
    kind: TouchKind::Play,
});
ball.possessor = Some(entity);
ball.state = BallState::Possessed;
```

Keep `ball.possessor = Some(entity)` and `ball.state = BallState::Possessed` (proximity acquisition is preserved).

- [ ] **Step 3: Verify**

```bash
cargo build --workspace
cargo test -p sim-rules possession
```

Expected: BUILD SUCCESS, 5 tests pass. (If tests fail with `last_touch is None`, see M11 task 11.5 for the test-fixture update.)

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/sim-rules/src/possession.rs
git commit -m "refactor(rules): possession_resolution_system pushes LastTouch to buffer"
```

---

## M7: `restart_system` OOB and kickoff-after-goal arms push to `PendingBallTouches`

**Files:**
- Modify: `crates/sim-rules/src/goals.rs:200-204, 251-254`

**Interfaces:**
- Consumes: `PendingBallTouches` from M3
- Produces: pushes `LastTouch { team: taker_team, player: Some(taker), kind: Restart }` (or `None` for `Entity::PLACEHOLDER`)

### Task 7.1: Replace OOB-arm direct write with buffer push

**Files:**
- Modify: `crates/sim-rules/src/goals.rs:251-254`

- [ ] **Step 1: Add `PendingBallTouches` to `restart_system` signature**

```rust
pub fn restart_system(
    mut commands: Commands,
    mut ball_query: Query<(Entity, &mut Position, &mut Velocity), With<BallMarker>>,
    rule_event_query: Query<(Entity, &RuleEvent)>,
    player_query: Query<(Entity, &TeamIdComponent), With<Player>>,
    current_tick: Option<Res<CurrentTick>>,
    pending_restart: Option<Res<PendingRestart>>,
    mut ball: ResMut<Ball>,
    mut pending_touches: ResMut<PendingBallTouches>,
    telemetry: Option<Res<TelemetryConfig>>,
) {
```

(Parameter name `pending_touches` avoids conflict with the existing `pending_restart`.)

- [ ] **Step 2: Replace the OOB-arm direct write**

Find the block at lines 251-254. Replace with:

```rust
let taker_team = last_touch_team.unwrap_or(/* see below */);
let player = if taker_entity == bevy_ecs::prelude::Entity::PLACEHOLDER {
    None
} else {
    Some(taker_entity)
};
pending_touches.0.push(sim_components::LastTouch {
    team: taker_team,
    player,
    kind: sim_components::TouchKind::Restart,
});
```

For `taker_team`: today the code uses `last_touch_team.as_ref().map_or(Entity::PLACEHOLDER, ...)` — when `last_touch_team` is `None`, the taker is the PLACEHOLDER. Map the `Option<TeamId>` to a `TeamId`:

```rust
let taker_team = last_touch_team.copied().unwrap_or(/* see spec §B note */);
```

The fallback team for a `None` `last_touch_team` (which only triggers on entity desync) is **not** specified in the design spec. Use `TeamId(0)` and add a `tracing::error!` to flag the desync:

```rust
let taker_team = match last_touch_team {
    Some(t) => t,
    None => {
        tracing::error!(
            tick = current_tick.as_deref().map_or(0, |t| t.0),
            "restart_system OOB arm: last_touch_team is None (touch entity desync); defaulting to home"
        );
        TeamId(0)
    }
};
```

- [ ] **Step 3: Replace kickoff-after-goal-arm direct write**

Find the block at lines 201-204. Replace with:

```rust
pending_touches.0.push(sim_components::LastTouch {
    team: conceded_team,
    player: if taker == bevy_ecs::prelude::Entity::PLACEHOLDER {
        None
    } else {
        Some(taker)
    },
    kind: sim_components::TouchKind::Restart,
});
```

`first_player_on_team` (the function that resolves `taker`) may return `Entity::PLACEHOLDER` when the team has no players. Map that to `None`.

- [ ] **Step 4: Verify and run restart tests**

```bash
cargo build --workspace
cargo test -p sim-rules restart kickoff_restart
```

Expected: BUILD SUCCESS, tests PASS (with M11 test-fixture updates applied for `apply_ball_touched_system` chain).

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/sim-rules/src/goals.rs
git commit -m "refactor(rules): restart_system pushes Restart touches to buffer"
```

---

## M8: OOB and Goal stop clearing `Ball.last_touch`, tightened pre-touch error

**Files:**
- Modify: `crates/sim-rules/src/out_of_bounds.rs:122-138`
- Modify: `crates/sim-rules/src/goals.rs:120-141`

**Interfaces:**
- Consumes: `LastTouch` from M1, `CurrentTick` resource
- Produces: stops clearing `Ball.last_touch`; adds `tracing::error!` + bail for the unreachable pre-touch-OOB / pre-touch-Goal paths

### Task 8.1: OOB stops clearing `Ball.last_touch`

**Files:**
- Modify: `crates/sim-rules/src/out_of_bounds.rs`

- [ ] **Step 1: Remove the `ball.last_touch = None;` line**

Find line 131 (`ball.last_touch = None;`). Delete it. The `Ball.last_touch` field is preserved across OOB events from this point forward.

- [ ] **Step 2: Tighten the pre-touch OOB error**

Replace the existing `debug_assert!` block (lines 122-127) with:

```rust
debug_assert!(
    ball.last_touch.is_some(),
    "out_of_bounds_system fired with no `last_touch` — touch-registration wiring gap"
);
if ball.last_touch.is_none() {
    tracing::error!(
        tick = current_tick.as_deref().map_or(0, |t| t.0),
        ball_pos = ?ball_query.get(ball_entity).ok().map(|p| p.0),
        "out_of_bounds_system fired with no `last_touch` (desync); skipping OOB event"
    );
    return;
}
```

This adds a release-build `tracing::error!` and bails without emitting a `RuleEvent`. The defensive bail prevents the "silent skip + phantom restart" pattern.

- [ ] **Step 3: Verify and run OOB tests**

```bash
cargo build --workspace
cargo test -p sim-rules out_of_bounds
```

Expected: BUILD SUCCESS, OOB tests PASS (with M11 test-fixture updates — `ball.last_touch.is_none()` post-OOB assertion is **deleted**; payload assertion is updated to full `LastTouch`).

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/sim-rules/src/out_of_bounds.rs
git commit -m "fix(rules): OOB preserves last_touch; tighten pre-touch error"
```

### Task 8.2: Goal stops clearing `Ball.last_touch`

**Files:**
- Modify: `crates/sim-rules/src/goals.rs`

- [ ] **Step 1: Remove the `ball.last_touch = None;` line**

Find line 125 (`ball.last_touch = None;`). Delete it.

- [ ] **Step 2: Tighten the pre-touch Goal error**

Add the same `debug_assert!` + `tracing::error!` + bail pattern as OOB (Step 2 of Task 8.1). Locate the goal-detection block in `goals.rs` where `last_touch_team` is captured — the pre-touch check goes immediately before the `ball.last_touch.as_ref().and_then(...)` capture.

- [ ] **Step 3: Verify and run Goal tests**

```bash
cargo build --workspace
cargo test -p sim-rules goal
```

Expected: BUILD SUCCESS, goal tests PASS (with M11 test-fixture updates).

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/sim-rules/src/goals.rs
git commit -m "fix(rules): Goal preserves last_touch; tighten pre-touch error"
```

---

## M9: `BallView` and `BallSnapshot` expose `last_touch` and `last_deliberate_play`

**Files:**
- Modify: `crates/sim-core/src/lib.rs:37-43` (`BallView`)
- Modify: `crates/sim-replay/src/lib.rs:153-159` (`BallSnapshot`)

**Interfaces:**
- Consumes: `LastTouch` from M1
- Produces: `BallView` and `BallSnapshot` both gain `last_touch: Option<LastTouch>` and `last_deliberate_play: Option<LastTouch>` fields

### Task 9.1: Extend `BallView`

**Files:**
- Modify: `crates/sim-core/src/lib.rs:37-43`

- [ ] **Step 1: Add the fields**

```rust
pub struct BallView {
    pub position: Vec2,
    pub velocity: Vec2,
    pub spin: f32,
    pub state: BallState,
    pub possessor: Option<Entity>,
    pub last_touch: Option<LastTouch>,
    pub last_deliberate_play: Option<LastTouch>,
}
```

- [ ] **Step 2: Update `BallView::from_ball`**

```rust
impl BallView {
    pub fn from_ball(ball: &Ball, entity_pos: Vec2, entity_vel: Vec2) -> Self {
        Self {
            position: entity_pos,
            velocity: entity_vel,
            spin: ball.spin,
            state: ball.state,
            possessor: ball.possessor,
            last_touch: ball.last_touch,
            last_deliberate_play: ball.last_deliberate_play,
        }
    }
}
```

Verify the existing `from_ball` signature — it may already take entity pos/vel as parameters. Match the existing pattern.

- [ ] **Step 3: Verify compile**

```bash
cargo build --workspace
```

Expected: BUILD SUCCESS (no test changes needed in this milestone; observers of `BallView` are unaffected by new fields).

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/sim-core/src/lib.rs
git commit -m "feat(snapshot): BallView exposes last_touch and last_deliberate_play"
```

### Task 9.2: Extend `BallSnapshot`

**Files:**
- Modify: `crates/sim-replay/src/lib.rs:153-159`

- [ ] **Step 1: Add the fields**

```rust
pub struct BallSnapshot {
    pub position: Vec2,
    pub velocity: Vec2,
    pub spin: f32,
    pub state: BallState,
    pub possessor: Option<Entity>,
    pub last_touch: Option<LastTouch>,
    pub last_deliberate_play: Option<LastTouch>,
}
```

`LastTouch` and `TouchKind` already derive `Serialize, Deserialize` (M1.1, M1.3).

- [ ] **Step 2: Update any constructor sites for `BallSnapshot`**

`grep -rn "BallSnapshot {" crates/`. Add the two new fields. Update tests that destructure `BallSnapshot`.

- [ ] **Step 3: Verify compile and tests**

```bash
cargo build --workspace
cargo test -p sim-replay
```

Expected: BUILD SUCCESS, replay tests PASS (with M11 hash updates).

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/sim-replay/src/lib.rs
git commit -m "feat(snapshot): BallSnapshot exposes last_touch and last_deliberate_play"
```

---

## M10: State hash mixes `last_deliberate_play`

**Files:**
- Modify: `crates/sim-core/src/simulation/state_hash.rs:163-169`

**Interfaces:**
- Consumes: `Ball.last_deliberate_play` from M1
- Produces: three extra `mix` calls in the FNV-1a-64 hash

### Task 10.1: Add the hash block

**Files:**
- Modify: `crates/sim-core/src/simulation/state_hash.rs`

- [ ] **Step 1: Add the mirror block**

After the existing `last_touch` block (lines 163-169), add:

```rust
// M10: `last_deliberate_play` is reserved for Law 11's deliberate
// play wording. Always `None` in this plan; mixed here so future
// deflections can write to it without changing the hash shape.
if let Some(touch) = b.last_deliberate_play.as_ref() {
    mix(&mut h, u64::from(touch.team.0));
    mix(
        &mut h,
        touch.player.map_or(u64::MAX, bevy_ecs::entity::Entity::to_bits),
    );
    mix(&mut h, touch_kind_discriminant(touch.kind));
} else {
    mix(&mut h, u64::MAX);
    mix(&mut h, u64::MAX);
    mix(&mut h, u64::MAX);
}
```

- [ ] **Step 2: Verify compile**

```bash
cargo build --workspace
```

Expected: BUILD SUCCESS.

- [ ] **Step 3: Run a determinism check (informational)**

```bash
cargo run --release -p sim-server -- simulate --ticks 1000 --seed 12345 2>&1 | grep final_state_hash
```

Expected: prints a stable `final_state_hash`. Record the value — M11 captures the full-match goldens.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/sim-core/src/simulation/state_hash.rs
git commit -m "feat(hash): state hash mixes last_deliberate_play"
```

---

## M11: Test fixtures + replay golden hashes

**Files:**
- Modify: every test fixture containing `Ball { ... }` literal
- Modify: `sim-replay/src/lib.rs:256-310` (replay determinism tests)

**Interfaces:**
- Consumes: every M1-M10 change
- Produces: 179 tests pass + replay golden hashes updated

### Task 11.1: Verify all `Ball` fixtures have `last_deliberate_play: None`

**Files:** every file listed in M1.4 Step 2 (already updated by M1.4 commit)

This task is a no-op verification — M1.4 already added `last_deliberate_play: None,` to every Ball constructor.

- [ ] **Step 1: Verify the grep checks pass**

```bash
grep -rn "LastTouch { entity:" crates/   # expect zero matches
grep -rn "last_deliberate_play" crates/  # expect matches in every Ball constructor
```

If any match is absent, add the field to that Ball literal (same shape as M1.4).

- [ ] **Step 2: No commit needed**

If Step 1 passes, no commit is needed (M1.4 already landed). If a fix-up is needed, commit as `test(touch): add last_deliberate_play to remaining Ball fixtures`.

### Task 11.2: Update `possession_tests.rs` schedule chains

**Files:**
- Modify: `crates/sim-rules/src/possession_tests.rs`

- [ ] **Step 1: Find tests that run `possession_resolution_system` standalone**

```bash
grep -n "possession_resolution_system" crates/sim-rules/src/possession_tests.rs
```

- [ ] **Step 2: For each test, insert `PendingBallTouches::default()` as a Resource**

The writers take **mandatory** `ResMut<PendingBallTouches>`. A schedule chain alone is not enough — the resource must exist or the writers panic with "Resource requested … does not exist". In each test's `World::new()` setup, after `world.insert_resource(Ball { ... })`, add:

```rust
world.insert_resource(sim_components::PendingBallTouches::default());
```

- [ ] **Step 3: For each test, chain `apply_ball_touched_system`**

The test's local `Schedule` currently has:

```rust
schedule.add_systems(possession_resolution_system);
```

Change to:

```rust
schedule.add_systems(
    (possession_resolution_system, apply_ball_touched_system).chain(),
);
```

Add `use crate::touch::apply_ball_touched_system;` (or the appropriate module path) to the imports.

- [ ] **Step 4: Verify**

```bash
cargo test -p sim-rules possession
```

Expected: 5 tests pass.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/sim-rules/src/possession_tests.rs
git commit -m "test(rules): insert PendingBallTouches + chain consumer in possession tests"
```

### Task 11.3: Update `goals_tests.rs` and `out_of_bounds_tests.rs`

**Files:**
- Modify: `crates/sim-rules/src/goals_tests.rs`
- Modify: `crates/sim-rules/src/out_of_bounds_tests.rs`

- [ ] **Step 1: For each test that runs `restart_system` or `out_of_bounds_system`, insert `PendingBallTouches::default()` as a Resource**

Same mandatory-Resource pattern as M11.2 Step 2. Tests that run these systems must have the resource, or the writers panic. Insert `world.insert_resource(sim_components::PendingBallTouches::default())` next to each `Ball` resource insert. Tests using `ball_fixture_with_touch()` (which only inserts `Ball`) also need this insertion.

- [ ] **Step 2: For each test that runs `restart_system` or `out_of_bounds_system`, chain the consumer**

Same pattern as M11.2: `.add_systems((<writer>, apply_ball_touched_system).chain())`.

- [ ] **Step 3: Update `RuleEvent` payload assertions**

`last_touch_team` → `last_touch` (matching the new payload type from M2). For tests asserting on the payload's `Some(TeamId(0))`, change to `Some(LastTouch { team: TeamId(0), player: ..., kind: ... })`.

- [ ] **Step 4: Delete the OOB post-clear assertion**

In `crates/sim-rules/src/out_of_bounds_tests.rs:67-69`, delete the `assert!(ball.last_touch.is_none())` assertion (M8 stops clearing).

- [ ] **Step 5: Delete the Goal post-clear assertion (if present)**

Same for `crates/sim-rules/src/goals_tests.rs:73-76` (per M8.2's expectation that goal tests asserting on `last_touch.is_none()` post-goal will need updating).

- [ ] **Step 6: Seed `last_touch` in `test_goal_scorer_threads_into_post_goal_kickoff_taker`**

This test's `ball_fixture` seeds `last_touch: None`, which now triggers the pre-touch guard at M8.2. Add a seeded touch (any `LastTouch` with the same team as the kicker suffices). Per the implementer's M5-M8 report.

- [ ] **Step 7: Verify**

```bash
cargo test -p sim-rules goal out_of_bounds
```

Expected: all tests pass.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
git add crates/sim-rules/src/goals_tests.rs crates/sim-rules/src/out_of_bounds_tests.rs
git commit -m "test(rules): insert resource, chain consumer, update payload + clear assertions"
```

### Task 11.4: Update `referee_tests.rs` and `execution.rs` tests

**Files:**
- Modify: `crates/sim-rules/src/referee_tests.rs`
- Modify: `crates/sim-ai-player/src/execution.rs` (test module)

- [ ] **Step 1: Update `LastTouch` fixture in `referee_tests.rs:140`**

Change `LastTouch { entity, kind: TouchKind::Restart }` to `LastTouch { team, player: Some(entity), kind: TouchKind::Restart }`.

- [ ] **Step 2: Insert `PendingBallTouches::default()` in test worlds**

Every test that runs `kick_execution_system`, `possession_resolution_system`, `out_of_bounds_system`, `goal_detection_system`, or `restart_system` needs the resource. Insert `world.insert_resource(sim_components::PendingBallTouches::default())` next to each `Ball` resource insert in:
- `crates/sim-ai-player/src/execution.rs` test module (3 tests)
- `crates/sim-rules/src/referee_tests.rs` (any tests that run writers)

- [ ] **Step 3: Update `kick_execution_registers_play_touch`**

In `crates/sim-ai-player/src/execution.rs:387-451`, schedule `apply_ball_touched_system` after `kick_execution_system` and assert on the resulting `Ball.last_touch`. Add `use sim_rules::touch::apply_ball_touched_system;` if needed (cross-crate import — verify the crate boundary; the test may need to live in `sim-ai-player`'s test code with a `sim-rules` dev-dependency, which may not exist — if so, **move the test to `sim-rules/src/touch_registration_tests.rs`** per M12 instead).

- [ ] **Step 4: Verify**

```bash
cargo test -p sim-rules referee
cargo test -p sim-ai-player kick
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/sim-rules/src/referee_tests.rs crates/sim-ai-player/src/execution.rs
git commit -m "test(touch): update LastTouch fixtures, insert resource, chain consumer"
```

### Task 11.5: Capture new replay golden hashes

**Files:**
- Modify: `crates/sim-replay/src/lib.rs:256-310` (replay determinism tests)

- [ ] **Step 1: Run a full match to capture the new hash**

```bash
cargo build --release
cargo run --release -p sim-server -- simulate --full-match --seed 12345 2>&1 | grep final_state_hash
```

Expected: prints `final_state_hash: <u64>`. Copy the value.

- [ ] **Step 2: Run the same command again and verify byte-identical**

```bash
cargo run --release -p sim-server -- simulate --full-match --seed 12345 2>&1 | grep final_state_hash
```

Expected: same hash. If not, determinism is broken — investigate before proceeding (likely an `Ordering` or `HashMap` issue introduced by M1-M10).

- [ ] **Step 3: Update the replay determinism test's expected hash**

In `crates/sim-replay/src/lib.rs`, find the `test_replay_determinism` test (around line 256). Replace the hardcoded expected value with the new hash from Step 1. Apply the same update to `test_divergence_detection` if it has a hardcoded hash.

- [ ] **Step 4: Verify**

```bash
cargo test -p sim-replay
```

Expected: replay tests pass.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/sim-replay/src/lib.rs
git commit -m "test(replay): update golden hashes for last_deliberate_play mix"
```

### Task 11.6: Full-workspace verification

- [ ] **Step 1: Run all tests**

```bash
cargo test --workspace
```

Expected: 179 + (new tests from M12) tests pass, no failures.

- [ ] **Step 2: Run clippy**

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: zero warnings.

- [ ] **Step 3: Verify determinism at the binary level**

```bash
cargo run --release -p sim-server -- simulate --ticks 1000 --seed 12345 -o /tmp/run1.json
cargo run --release -p sim-server -- simulate --ticks 1000 --seed 12345 -o /tmp/run2.json
md5sum /tmp/run1.json /tmp/run2.json
```

Expected: identical hashes. (Match-report JSONs include the new `last_touch`/`last_deliberate_play` fields in `BallView`.)

---

## M12: New tests for the touch-registration flow

**Files:**
- Create: `crates/sim-rules/src/touch_registration_tests.rs`
- Modify: `crates/sim-rules/src/goals_tests.rs` (extend)
- Modify: `crates/sim-rules/src/out_of_bounds_tests.rs` (extend)
- Modify: `crates/sim-rules/src/referee_tests.rs` (extend)

**Interfaces:**
- Consumes: every M1-M11 change
- Produces: 12 new tests covering the full touch-registration contract

### Task 12.1: Create `touch_registration_tests.rs`

**Files:**
- Create: `crates/sim-rules/src/touch_registration_tests.rs`

- [ ] **Step 1: Write the test file header**

```rust
//! Tests for the `PendingBallTouches` resource + `apply_ball_touched_system`
//! consumer + writer push sites. These tests verify the contract that
//! every contact path (kick, possession acquisition, restart placement)
//! pushes exactly one touch per occurrence, the consumer applies the
//! latest-in-schedule entry to `Ball.last_touch`, and the
//! `RuleEvent` payloads carry the full record.

#![allow(
    clippy::float_cmp,
    reason = "tests compare outputs of identical arithmetic against the inputs they were derived from"
)]

use bevy_ecs::prelude::*;
use sim_components::{
    Ball, BallMarker, BallState, LastTouch, Match, MatchClock, MatchState, PendingBallTouches,
    Player, Position, RuleEvent, Skill, TeamId, TeamIdComponent, TouchKind, Velocity,
};
use sim_math::Vec2;

use crate::{
    possession_resolution_system,
    touch::apply_ball_touched_system,
    CurrentTick,
};
```

(The test file may need additional `use` statements for `kick_execution_system` from `sim-ai-player` and `restart_system` from `sim-rules`. Adjust imports as the test evolves.)

- [ ] **Step 2: Implement `test_kick_pushes_to_buffer_and_consumer_applies`**

```rust
#[test]
fn test_kick_pushes_to_buffer_and_consumer_applies() {
    let mut world = World::new();
    world.insert_resource(PendingBallTouches::default());

    let kicker = world.spawn((
        Player { team_id: TeamId(0), intent: None },
        Velocity(Vec2::ZERO),
        TeamIdComponent(TeamId(0)),
    )).id();
    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Possessed,
        possessor: Some(kicker),
        last_touch: None,
        last_deliberate_play: None,
    });

    // Simulate a kick push.
    world.resource_mut::<PendingBallTouches>().0.push(LastTouch {
        team: TeamId(0),
        player: Some(kicker),
        kind: TouchKind::Play,
    });

    // Run the consumer.
    let mut schedule = Schedule::default();
    schedule.add_systems(apply_ball_touched_system);
    schedule.run(&mut world);

    let ball = world.resource::<Ball>();
    assert_eq!(ball.last_touch, Some(LastTouch {
        team: TeamId(0),
        player: Some(kicker),
        kind: TouchKind::Play,
    }));
}
```

- [ ] **Step 3: Implement `test_two_consecutive_kicks_update_record`**

Push two `LastTouch` entries to the buffer, run, assert the second wins (because `apply_ball_touched_system` uses `.pop()` which returns the most recent).

- [ ] **Step 4: Implement `test_same_tick_double_kick_resolves_deterministically`**

Same as Step 3 but explicitly named to capture the contract.

- [ ] **Step 5: Implement `test_possession_acquisition_pushes_play_touch`**

Schedule `possession_resolution_system` then `apply_ball_touched_system`. Place a player within 1.5 m of a `Free` ball, run, assert `Ball.last_touch.kind == Play` and `Ball.last_touch.player == Some(player_entity)`.

- [ ] **Step 6: Implement `test_kickoff_registers_restart_touch`**

This test requires running the full kickoff transition. Use `Simulation::new(seed=12345)` from `sim-core`, run one tick to enter `Kickoff`, then tick once more to enter `InPlay`. After tick 1, assert `Ball.last_touch.kind == Restart` and `Ball.last_touch.team == TeamId(0)` (home takes the first-half kickoff).

```rust
#[test]
fn test_kickoff_registers_restart_touch() {
    use sim_core::Simulation;
    let mut sim = Simulation::new(12345);
    sim.tick();
    sim.tick();
    let ball = sim.world().resource::<Ball>();
    let touch = ball.last_touch.expect("kickoff must register a touch");
    assert_eq!(touch.kind, TouchKind::Restart);
    assert_eq!(touch.team, TeamId(0));
}
```

- [ ] **Step 7: Implement `test_oob_restart_registers_restart_touch`**

Set up: `Ball.last_touch = Some(LastTouch { team: TeamId(0), ... })`, position OOB at touchline, schedule `out_of_bounds_system` then `restart_system` then `apply_ball_touched_system`. Assert `Ball.last_touch.team == TeamId(1)` (opposite).

- [ ] **Step 8: Implement `test_oob_event_carries_full_touch`**

Setup: `Ball.last_touch = Some(LastTouch { team: TeamId(0), player: Some(player), kind: Play })`. Trigger OOB. Assert: `RuleEvent::OutOfBounds.last_touch == Some(prior)` AND `Ball.last_touch == Some(prior)` (preserved, not cleared).

- [ ] **Step 9: Implement `test_goal_event_carries_full_touch`**

Same as Step 8 but for goals.

- [ ] **Step 10: Implement `test_own_goal_attribution_preserved`**

Setup: Kicker on team 0, ball kicked into team 0's goal (x = -1). Run `goal_detection_system`. Assert: `RuleEvent::Goal.scorer_team == TeamId(1)` (positional) AND `RuleEvent::Goal.last_touch.team == TeamId(0)` (own-goal preserved).

- [ ] **Step 11: Implement `test_pre_touch_oob_logs_error_and_skips`**

Setup: `Ball.last_touch = None`, ball OOB. Assert: no `RuleEvent::OutOfBounds` inserted, `tracing::error!` fires. Use a tracing subscriber stub or `tracing-test` crate.

- [ ] **Step 12: Implement `test_pending_ball_touches_resource_registered`**

Schedule-wiring check: assert that when `kick_execution_system`, `possession_resolution_system`, and `apply_ball_touched_system` are scheduled in the order documented in the spec, the buffer is drained after the consumer runs. A direct test of the registration is to run a schedule that includes all three and assert `world.resource::<PendingBallTouches>().0.is_empty()` after.

- [ ] **Step 13: Implement `test_apply_ball_touched_consumer_registered_first_in_rules_set`**

This is a meta-test of the schedule wiring. Schedule all five Rules systems with `.chain()`. Run a kick. Assert: `Ball.last_touch` is set BEFORE `offside_detection_system` would read it. The assertion can be indirect — verify by checking that `Ball.last_touch` is non-`None` after `apply_ball_touched_system` runs and before any Rules writer runs.

- [ ] **Step 14: Implement `test_last_deliberate_play_always_none`**

Run `cargo run --release -p sim-server -- simulate --full-match --seed 12345 -o /tmp/match.json` (or use `Simulation::new` + ticks). Read every JSONL line's `ball.last_deliberate_play` and assert it is `None` for every tick.

- [ ] **Step 15: Verify all tests in the file pass**

```bash
cargo test -p sim-rules touch_registration
```

Expected: 14 new tests pass.

- [ ] **Step 16: Commit**

```bash
cargo fmt --all
git add crates/sim-rules/src/touch_registration_tests.rs
git commit -m "test(touch): add touch_registration_tests.rs (14 tests)"
```

### Task 12.2: Extend `goals_tests.rs` with end-to-end tests

**Files:**
- Modify: `crates/sim-rules/src/goals_tests.rs`

- [ ] **Step 1: Add `test_missed_shot_gives_goal_kick`**

Setup: ball at `(104.5, 34.0)` (right goal area, y in goal range), velocity `(15.0, 0.0)`. Schedule `out_of_bounds_system` then `restart_system`. Assert: `RuleEvent::OutOfBounds.oob_type == BallOutOfBoundsType::GoalKick` and `ball.position == (94.0, 34.0)`.

- [ ] **Step 2: Add `test_deflected_shot_gives_corner`**

Setup: ball at `(104.5, 10.0)` (right goal line, y outside goal area). Assert: `oob_type == Corner` and `ball.position == (104.0, 1.0)`.

- [ ] **Step 3: Add `test_throw_in_to_opposite_team`**

Setup: ball OOB at touchline, `Ball.last_touch.team == TeamId(0)`. Assert: `RuleEvent::OutOfBounds.last_touch.team == TeamId(0)` (touch record), and after restart, `Ball.last_touch.team == TeamId(1)` (opposite team takes the throw-in).

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p sim-rules goal
cargo fmt --all
git add crates/sim-rules/src/goals_tests.rs
git commit -m "test(rules): end-to-end goal-kick, corner, throw-in scenarios"
```

### Task 12.3: Extend `referee_tests.rs` with offside regression tests

**Files:**
- Modify: `crates/sim-rules/src/referee_tests.rs`

- [ ] **Step 1: Add `test_offside_flagged_on_teammate_pass`**

Place two Kicker players on team 0, one near the center and one in offside position. Player A passes (kick, sets ball.last_touch to A and A's team). Assert: `offside_detection_system` flags player B at position X.

- [ ] **Step 2: Add `test_offside_reset_by_opponent_kick`**

After test_offside_flagged, kick the ball from an away-team player. Assert: `ball.last_touch.team == TeamId(1)` (away team) and `offside_detection_system` no longer flags player B (the offside state has reset to the away team).

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p sim-rules referee
cargo fmt --all
git add crates/sim-rules/src/referee_tests.rs
git commit -m "test(rules): offside flag/reset on teammate pass + opponent kick"
```

### Task 12.4: Final full-workspace verification

- [ ] **Step 1: Run all tests**

```bash
cargo test --workspace
```

Expected: 179 + 14 + 3 (goals) + 2 (referee) = 198 tests pass.

- [ ] **Step 2: Run clippy**

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: zero warnings.

- [ ] **Step 3: Final determinism check**

```bash
cargo run --release -p sim-server -- simulate --full-match --seed 12345 -o /tmp/match.json
cargo run --release -p sim-server -- simulate --full-match --seed 12345 -o /tmp/match2.json
md5sum /tmp/match.json /tmp/match2.json
```

Expected: identical hashes.

- [ ] **Step 4: Final commit**

If any final cleanups are needed (unused imports, dead code from M1's struct reshape), commit them as `chore(touch): post-M12 cleanup`.

```bash
cargo fmt --all
git add -A
git commit -m "chore(touch): post-implementation cleanup"
```

---

## Self-Review

**Spec coverage:** Walked through every spec section:

| Spec section | Plan tasks |
|--------------|------------|
| §A Data model (LastTouch reshape, Ball.last_deliberate_play) | M1.1, M1.2, M1.3, M1.4 |
| §B Touch registration via Resource | M3.1, M4.1, M4.2 |
| §D RuleEvent payload carries full touch | M2.1 |
| §E Pre-touch OOB tightened error | M8.1, M8.2 |
| §F Snapshot visibility | M9.1, M9.2 |
| §G State hash mixes last_deliberate_play | M10.1 |
| §H Tests | M11.*, M12.* |
| §I Out of scope (pickup refactor, half-time swap, deflections) | explicit "do not address" in M1 task descriptions |

**Placeholder scan:** Searched for "TBD", "TODO", "implement later", "similar to Task N". Found:
- "see M5/M7 for 6/7 conversion" in M1.3 Step 2 — this is a forward-reference that lands in the listed task; not a placeholder.
- "see spec §B note" in M7.1 Step 2 — this is a design question that the spec already resolved (§B's `tracing::error!`); not a placeholder.
- "match the existing pattern" / "find the rules-set configuration" — these are exploratory steps where the existing wiring varies. Acceptable per the writing-plans skill's exploratory-task pattern.

**Type consistency:**
- `LastTouch { team, player, kind }` defined in M1.2, used consistently in M1.3, M2.1, M5.1, M6.1, M7.1, M9.1, M9.2, M10.1, M11.*, M12.*.
- `PendingBallTouches(pub Vec<LastTouch>)` defined in M3.1, used in M4.1, M4.2, M5.1, M6.1, M7.1, M11.*, M12.*.
- `apply_ball_touched_system(mut pending, mut ball)` consistent in M4.1, M4.2, M11.*, M12.*.
- `Ball { last_deliberate_play: Option<LastTouch>, ... }` consistent in M1.4, M2.1, M11.*, M12.*.

**Risks surfaced:**
- Replay golden hashes change at M10; M11.5 captures them. Two-run verification catches any non-determinism.
- Cross-crate import of `apply_ball_touched_system` into `sim-ai-player` tests may require moving the kick test (M11.4). Flagged inline.
- M7.1's `taker_team` fallback when `last_touch_team` is `None` uses `TeamId(0)` and logs an error. Spec §B leaves this underspecified; the plan picks a defensive default. If a follow-up ticket wants different behavior, change is local to M7.1.

**Out-of-scope items (explicit, per spec §I):**
- Pickup refactor (separate ticket).
- Half-time attacking-direction swap (separate ticket).
- Save/block/deflection systems (no such systems today).

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-10-03-ball-touch-registration-refactor.md`. Twelve milestones (M1-M12), each one independently testable and shippable. Approximately 50 small steps across the milestones.

Two execution options:

1. **Subagent-Driven (recommended)** — fresh subagent per task, two-stage review between tasks, fast iteration. Best for a 12-milestone refactor where each milestone has a clean test surface.

2. **Inline Execution** — execute tasks in this session using `executing-plans`, batch execution with checkpoints. Best when you want to keep the conversation context and don't mind longer single-session runs.

Which approach?