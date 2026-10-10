# Ball-Touch Registration Refactor — Design

> **Status relative to PRD (docs/football-sim-mvp-prd.md, 10 Oct 2026): RETAINED with PRD alignment notes.** The touch-registration mechanism (`PendingBallTouches`, `LastTouch` with `TouchKind::Play`/`Restart`, two-deep history) aligns with PRD §8.5 goal attribution and §6.2 Ball model. **Strip offside-related tests/assertions** (PRD §2: offside stub stays inert); `last_deliberate_play` field is YAGNI for MVP (offside out of scope) — keep only if explicitly reserved with a comment.

**Date:** 2026-10-03
**Source ticket:** "Replace proximity-based last-toucher with an explicit
ball-touch registration system" (this is the same ticket whose Open Items
were resolved before this design was written).
**Path classification:** Architectural. Five crates touched, one new
resource type, one new system, determinism golden hashes will change.
**Status:** Approved design — pending implementation plan.
**Sibling design:** Half-time attacking-direction swap is a separate
spec at `docs/superpowers/specs/2026-10-03-half-time-attacking-direction-design.md`
(see §I for the seam).

## Motivation

`Ball.last_touch` is the input to Law 11 (offside), Law 15 (throw-in
taker), Law 16 (goal-kick taker), and Law 17 (corner taker). Today it
is set by direct writes from five systems, four of which mix proximity,
position, kick direction, and restart taker selection in ways that
make a single audit difficult. One writer — `possession_resolution_system`'s
1.5 m closest-player scan — is proximity-based: a defender standing
next to a stationary ball is recorded as the toucher the moment they
reach the 1.5 m threshold.

The motivating bug is that there is no single code path that *defines*
"a player touched the ball." Five writers touch it; one of them is
silent. The OOB restart fix (a separate ticket) reads
`last_touch_team` from the `RuleEvent` payload, and the existing
payload captures only the team, not the player entity. Replay output
omits `last_touch` entirely (`BallSnapshot` at
`sim-replay/src/lib.rs:153-159`).

This design unifies the five writers behind a single touch-registration
mechanism (`PendingBallTouches` resource + `apply_ball_touched_system`
consumer), denormalises the team into the touched record so the
rule-event payload survives an entity despawn, surfaces the touch in
match-report and replay views, and adds a parallel
`last_deliberate_play` slot reserved for future Law 11 work (no system
writes to it in this ticket — see §H for the cost).

Behavior is unchanged: the proximity acquisition in
`possession_resolution_system` still sets `Ball.possessor` and still
emits a `Play` touch when the closest-within-1.5 m player picks up the
ball. The motivating "stationary-ball near defender" attribution bug
is *not* fixed by this ticket — it is filed separately as the
**AI pickup refactor** (see §I).

## Smell baseline

| # | Smell | Fix |
|---|-------|-----|
| T1 | Five direct writers to `Ball.last_touch` in three crates; no audit point | One `PendingBallTouches` buffer + one consumer |
| T2 | `RuleEvent::OutOfBounds.last_touch_team: Option<TeamId>` and `RuleEvent::Goal.last_touch_team: Option<TeamId>` lose the player entity and the `kind` | Carry `Option<LastTouch>` instead |
| T3 | `Ball.last_touch` invisible in `BallView` / `BallSnapshot`; not in JSONL match reports; not in replay files | Add to both views |
| T4 | `out_of_bounds_system` and `goal_detection_system` clear `Ball.last_touch` after capturing the team | Stop clearing; preserve history |
| T5 | `LastTouch { entity: Entity }` cannot survive entity despawn | Add `team: TeamId` |
| T6 | `attacking_direction(team)` is not half-aware | Deferred to sibling spec |

## What's in scope

Eight files modified across five crates; one new resource type; one
new system; ~15 test updates; replay golden hashes updated.

| Order | Step | Title | Files touched | Depends on |
|-------|------|-------|---------------|------------|
| 1 | **M1** | `LastTouch { team, player, kind }` and `Ball.last_deliberate_play` | `sim-components/src/lib.rs`, all `Ball` constructors in tests + `construct.rs` + `tests.rs` | — |
| 2 | **M2** | `RuleEvent::{OutOfBounds, Goal}.last_touch: Option<LastTouch>` | `sim-components/src/lib.rs` | M1 |
| 3 | **M3** | `PendingBallTouches` resource type | `sim-components/src/lib.rs`, `sim-core/src/simulation/construct.rs` (resource init) | — |
| 4 | **M4** | `apply_ball_touched_system` consumer | `sim-rules/src/possession.rs` (new fn), `sim-rules/src/lib.rs`, `sim-core/src/simulation/construct.rs` (registration) | M1, M3 |
| 5 | **M5** | `kick_execution_system` pushes to `PendingBallTouches` | `sim-ai-player/src/execution.rs` | M3 |
| 6 | **M6** | `possession_resolution_system` pushes to `PendingBallTouches` | `sim-rules/src/possession.rs` | M3, M4 |
| 7 | **M7** | `restart_system` (OOB arm + kickoff-after-goal arm) push to `PendingBallTouches` | `sim-rules/src/goals.rs` | M3, M4 |
| 8 | **M8** | `out_of_bounds_system` and `goal_detection_system` carry full touch, stop clearing | `sim-rules/src/out_of_bounds.rs`, `sim-rules/src/goals.rs`, `sim-components/src/lib.rs` (RuleEvent payload type changed in M2) | M2 |
| 9 | **M9** | `BallView` and `BallSnapshot` expose `last_touch` and `last_deliberate_play` | `sim-core/src/lib.rs`, `sim-replay/src/lib.rs` | M1 |
| 10 | **M10** | State hash mixes `last_deliberate_play` | `sim-core/src/simulation/state_hash.rs` | M1 |
| 11 | **M11** | Update test fixtures + replay golden hashes | every test file touching `Ball`; `sim-replay` determinism tests | M1-M10 |
| 12 | **M12** | New tests for the touch-registration flow | `sim-rules/src/touch_registration_tests.rs` (new file), `sim-ai-player/src/execution_tests.rs` (extend), `sim-rules/src/goals_tests.rs` (extend), `sim-rules/src/out_of_bounds_tests.rs` (extend) | M5-M8 |

## Section A: Data model

`crates/sim-components/src/lib.rs`:

```rust
/// Records a single ball touch. `player` is the player who touched the
/// ball (or `None` when no player touched — e.g. a kickoff placement
/// where no specific player is designated). `team` is denormalised so
/// the record resolves even if the entity despawns. `kind`
/// distinguishes active play from restart placements so Law 11
/// (offside) can ignore restart touches via
/// `offside_detection_system`'s existing `kind == Play` filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LastTouch {
    pub team: TeamId,
    pub player: Option<Entity>,
    pub kind: TouchKind,
}

#[derive(Resource, Debug, Clone)]
pub struct Ball {
    pub spin: f32,
    pub state: BallState,
    pub possessor: Option<Entity>,
    pub last_touch: Option<LastTouch>,
    /// Reserved for Law 11's "deliberate play" wording. No system
    /// writes to this field in this design; it is added now so the
    /// future deflection system (see §I) does not need a Ball-shape
    /// change. Always `None` until that work is done. The state hash
    /// mixes it (§G); every Ball constructor must initialise it.
    pub last_deliberate_play: Option<LastTouch>,
    pub kick_velocity: Option<Vec2>,
}
```

`Serialize`/`Deserialize` are added to `LastTouch`. Existing derives
on `TouchKind` (`lib.rs:134`) get `Serialize, Deserialize` too.
`RuleEvent` already derives them; the field-type change in M2 keeps
the derives valid.

`Entity::PLACEHOLDER` is retired as a stand-in for "no player" — it
moves through `Option<Entity>` as `None` everywhere. All call sites
that today use `Entity::PLACEHOLDER` for the OOB taker fallback
(`sim-rules/src/goals.rs:242`) keep the fallback but push `None` to
the buffer.

## Section B: Touch registration via Resource

The original ticket and brainstorming session proposed a Bevy
`#[derive(Event)]` channel. The codebase's established pattern for
same-tick producer-consumer flows is a `Resource` (see
`PendingRestart` at `sim-rules/src/lib.rs:54-72` and the OOB restart
bug ticket at `.superpowers/sdd/oob-restart-bug.md`, which recommends
`Resource` over component over events). Bevy `Events<T>` is not used
in any current code (verified by `grep` — zero usages). This design
follows the existing pattern: a `Resource` accumulates touch events
across the schedule run; the consumer drains it at the start of
the `Rules` set.

`crates/sim-components/src/lib.rs`:

```rust
/// Pending ball touches registered this tick. Writers (kick, possession
/// acquisition, restart placements) append to this buffer; the
/// consumer `apply_ball_touched_system` drains it at the start of
/// the Rules set and writes the final entry to `Ball.last_touch`.
///
/// Determinism: writes happen in fixed system-set order
/// (Execution → Possession → Rules → MatchAdmin), and Bevy
/// `ResMut` access serialises those writes. Last-touch-wins semantics
/// are equivalent to a same-tick EventReader ordered by emission
/// time.
#[derive(Resource, Debug, Default)]
pub struct PendingBallTouches(pub Vec<LastTouch>);
```

The resource is registered in
`sim-core/src/simulation/construct.rs` alongside the existing
`PendingRestart` resource. Initial state is `Default::default()` —
empty buffer.

**Five writes:**

| # | System | Crate | Set | Trigger | `kind` |
|---|--------|-------|-----|---------|--------|
| 1 | `kick_execution_system` | `sim-ai-player` | Execution | possessor + non-trivial kick dir (`length() > 0.01`) | `Play` |
| 2 | `possession_resolution_system` | `sim-rules` | Possession | ball `Free` + closest-within-1.5 m wins | `Play` |
| 3 | `restart_system` OOB arm | `sim-rules` | Rules | `RuleEvent::OutOfBounds` consumed + ball placed | `Restart` |
| 4 | `restart_system` kickoff-after-goal arm | `sim-rules` | Rules | `PendingRestart.restart_tick` reached + ball placed | `Restart` |
| 5 | `lifecycle_system` (kickoff placement) | `sim-core` | MatchAdmin | `TransitionDecision.place_ball_center == true` | `Restart` |

**Site #5 is a direct write, not a buffer push.** Rationale:
`lifecycle_system` runs in the `MatchAdmin` set, which is the last
set in the schedule. The buffer's drain happens at the start of the
`Rules` set, so a buffer push from `MatchAdmin` would not be visible
to this tick's Rules-set consumers (offside, OOB, goal). It would
also not be visible to *next* tick's Rules-set consumer because the
buffer is drained at the end of `apply_ball_touched_system` (§B
"Consumer" below). A direct write to `ResMut<Ball>` (already done
today) avoids the gap and matches the established pattern of
`lifecycle_system` mutating `Ball` directly (`lifecycle.rs:248-252`).

**One consumer:** `apply_ball_touched_system` in `sim-rules`. Reads
`ResMut<PendingBallTouches>` and writes `ResMut<Ball>`. Registered at
the **start** of the `Rules` set in
`sim-core/src/simulation/construct.rs` (the same `register_systems`
helper referenced in `AGENTS.md` §"Project structure"). System-set
order becomes:

```
Perception → Decision → Execution → Physics → Possession → Rules → MatchAdmin
                                                  │
                                                  └─ (apply_ball_touched_system runs FIRST in Rules)
                                                       then offside, OOB, goal, restart
```

**Last-touch-wins semantics:** the buffer is a `Vec<LastTouch>`. The
consumer keeps the last entry and drains the buffer:

```rust
pub fn apply_ball_touched_system(
    mut pending: ResMut<PendingBallTouches>,
    mut ball: ResMut<Ball>,
) {
    if let Some(last) = pending.0.pop() {
        ball.last_touch = Some(last);
    }
    pending.0.clear(); // drain any earlier entries
}
```

`Vec::pop()` returns the most recent push; `clear()` is defensive
against any prior partial consumption. Writes happen in fixed
system-set order (Execution → Possession → Rules writers), so the
"last push" is the latest touch in schedule order. A same-tick kick
followed by a same-tick restart would resolve to the restart — which
is the correct Laws-of-the-Game interpretation (the restart placement
supersedes the kick for Law 11 input).

## Section D: RuleEvent payload carries full touch

`crates/sim-components/src/lib.rs`:

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

**Critical change:** OOB and Goal events **stop clearing**
`Ball.last_touch`. Today both systems clear it
(`out_of_bounds.rs:131`, `goals.rs:125`). After M8 the touch record
persists on `Ball` until the next `PendingBallTouches` push overwrites
it. This means:

- Own-goal attribution is preserved in `Ball.last_touch` for replay
  data (scorer attribution by ball position is unchanged).
- The next restart placement re-writes `last_touch` to the new taker
  (`Restart` kind), as today.
- `offside_detection_system` continues to filter by `kind == Play`;
  the preserved `last_touch` after a goal does not flip offside
  because `kind == Restart`.

`restart_system`'s OOB-arm taker-resolution at
`sim-rules/src/goals.rs:242-250` continues to compute the taker as
"first player on the opposite team" — no change.

## Section E: Pre-touch OOB tightened error

`crates/sim-rules/src/out_of_bounds.rs:122-131`:

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

Same treatment for `goal_detection_system` at `goals.rs:125`. Both
paths are unreachable today (kickoff placement in M5/M7 + the buffer
consumer in M4 ensure `last_touch` is always populated after the
first restart), but the release-build `tracing::error!` defends against
future regressions in the touch-registration wiring. Bail, do not emit a
`RuleEvent` — avoids the "silent skip + phantom restart" pattern
flagged by the original ticket.

## Section F: Snapshot visibility

`crates/sim-core/src/lib.rs:37-43` (`BallView`):

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

impl BallView {
    pub fn from_ball(ball: &Ball) -> Self {
        Self {
            position: 0,            // resolved from entity Position
            velocity: 0,            // resolved from entity Velocity
            spin: ball.spin,
            state: ball.state,
            possessor: ball.possessor,
            last_touch: ball.last_touch,
            last_deliberate_play: ball.last_deliberate_play,
        }
    }
}
```

`crates/sim-replay/src/lib.rs:153-159` (`BallSnapshot`): same fields,
serialised via the existing `Serialize`/`Deserialize` derives on
`LastTouch`/`TouchKind`. `RecordedSnapshot` at `lib.rs:143-150` is
unchanged (it does not embed a ball — `state_hash` only).

## Section G: State hash

`crates/sim-core/src/simulation/state_hash.rs:163-169` — add a
mirror block for `last_deliberate_play`:

```rust
// existing last_touch block: unchanged

// new last_deliberate_play block:
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

This changes the `final_state_hash` for every seed. **Determinism
tests in `sim-replay/src/lib.rs:256-310` (`test_replay_determinism`,
`test_divergence_detection`) need their expected hashes updated** by
running `cargo run --release -p sim-server -- simulate --full-match
--seed <seed>` and capturing the new `final_state_hash` from the
printed line. The hashes are deterministic given a fixed seed, so the
update is a one-shot per seed.

`AGENTS.md` §"Determinism" documents the contract that two runs of
the same seed print the same hash; M11 verifies this.

## Section H: Tests

### Existing tests touched

| File | Change |
|------|--------|
| `sim-rules/src/tests.rs:34-42` (`ball_fixture`) | Add `last_deliberate_play: None` |
| `sim-rules/src/possession_tests.rs` (5 tests, lines 14, 62, 122, 175, 288) | `last_touch` assertions stay (proximity still emits the touch) but require a post-`apply_ball_touched_system` run. Existing tests call `possession_resolution_system` standalone; M11 updates each to run `apply_ball_touched_system` after, or wraps in `Schedule::default().add_systems((possession_resolution_system, apply_ball_touched_system).chain())`. |
| `sim-rules/src/goals_tests.rs:51-54, 73-76` | Pre-seeds `Ball.last_touch` — keep, but with new `LastTouch { team, player, kind }` shape |
| `sim-rules/src/goals_tests.rs:88-93, 156-161` | `RuleEvent::Goal` payload assertions — change from `last_touch_team` to `last_touch` (full record) |
| `sim-rules/src/goals_tests.rs:211-280` (`test_kickoff_restart_registers_restart_touch`) | Asserts the kickoff arm pushes `Restart` to `PendingBallTouches`. Schedule must include `apply_ball_touched_system`. |
| `sim-rules/src/out_of_bounds_tests.rs:67-69` | `ball.last_touch.is_none()` post-OOB assertion — **deleted** (M8 stops clearing) |
| `sim-rules/src/out_of_bounds_tests.rs:82-86` | `RuleEvent::OutOfBounds` payload — change to full record |
| `sim-rules/src/referee_tests.rs:99-192` (`test_offside_ignores_restart_touch`) | Unchanged — still asserts `kind == Restart` short-circuits offside |
| `sim-ai-player/src/execution.rs:387-451` (`kick_execution_registers_play_touch`) | Schedule includes `apply_ball_touched_system` before assertion |
| `sim-ai-player/src/perception_tests.rs:27` | Add `last_deliberate_play: None` |
| `sim-physics/src/lib.rs:374` | Add `last_deliberate_play: None` |
| `sim-core/src/tests/telemetry_tests.rs:306` | Add `last_deliberate_play: None` |
| `sim-core/src/simulation/construct.rs:190` | Add `last_deliberate_play: None` to the `Ball` resource init |
| `sim-replay/src/lib.rs:256-310` (replay determinism tests) | Update expected hashes (run full match, capture new `final_state_hash`) |

### New tests (sim-rules/src/touch_registration_tests.rs, new file)

| Test | What it asserts |
|------|----------------|
| `test_kick_pushes_to_buffer_and_consumer_applies` | Kick → push to `PendingBallTouches` → `apply_ball_touched_system` → `Ball.last_touch` is the kicker's record |
| `test_two_consecutive_kicks_update_record` | Kick by A, then kick by B → record is B |
| `test_same_tick_double_kick_resolves_deterministically` | A kicks then B kicks in the same tick (impossible in normal play but constructible by pushing twice to the buffer directly) → B wins, record is B |
| `test_possession_acquisition_pushes_play_touch` | Ball `Free`, closest-within-1.5 m reaches ball → `last_touch` is the closest player |
| `test_kickoff_registers_restart_touch` | Kickoff placement → `last_touch.kind == Restart`, `team` is the taker's team |
| `test_oob_restart_registers_restart_touch` | OOB → restart placement → `last_touch.kind == Restart`, `team` is the opposite of the prior `last_touch.team` |
| `test_oob_event_carries_full_touch` | OOB → `RuleEvent::OutOfBounds.last_touch` is the prior `last_touch`, **and** `Ball.last_touch` is preserved (not cleared) |
| `test_goal_event_carries_full_touch` | Goal → `RuleEvent::Goal.last_touch` is the prior `last_touch`, `Ball.last_touch` preserved |
| `test_own_goal_attribution_preserved` | Kicker on team 0 kicks into team 0's own goal → scorer is team 1 (positional), `RuleEvent::Goal.last_touch.team` is team 0 (own-goal preserved in record) |
| `test_pre_touch_oob_logs_error_and_skips` | Ball with `last_touch: None` + position OOB → no `RuleEvent::OutOfBounds` emitted, `tracing::error!` fires (capture via `tracing-test` or stub) |
| `test_pending_ball_touches_resource_registered` | Schedule wiring check: `apply_ball_touched_system` runs before `offside_detection_system` in the same set; `PendingBallTouches` resource is drained after consumption |
| `test_apply_ball_touched_consumer_registered_first_in_rules_set` | Schedule wiring check: `apply_ball_touched_system` runs before `offside_detection_system` in the same set |

### New tests (sim-rules/src/goals_tests.rs, extend)

| Test | What it asserts |
|------|----------------|
| `test_missed_shot_gives_goal_kick` | Shot that exits over the goal line in the goal-area y-range → `RuleEvent::OutOfBounds.oob_type == GoalKick` |
| `test_deflected_shot_gives_corner` | Shot that exits over the goal line outside the goal-area y-range → `RuleEvent::OutOfBounds.oob_type == Corner` |
| `test_throw_in_to_opposite_team` | Ball crosses touchline → `RuleEvent::OutOfBounds.last_touch.team` is the team opposite the previous kicker's team |

### New tests (sim-rules/src/referee_tests.rs, extend)

| Test | What it asserts |
|------|----------------|
| `test_offside_flagged_on_teammate_pass` | Kicker on team 0 passes to offside-position teammate → offside detected |
| `test_offside_reset_by_opponent_kick` | Kicker on team 0 passes to offside teammate, then opponent kicks → next tick offside flagged for opponent's team |

### Tests for `last_deliberate_play` (YAGNI acceptance)

`last_deliberate_play` is always `None` after every tick in this
design. The cost is documented in §I; the test
`test_last_deliberate_play_always_none` asserts that field is `None`
on every tick of a `--full-match` run. This is a guardrail against
future regressions that might accidentally start writing to the
field.

### Tests NOT in scope (deferred)

- "Defender within 1 m of a stationary ball does not register a
  touch" — the motivating bug. Deferred to the **AI pickup refactor**
  ticket (§I). Today a defender at 1.0 m from a stationary ball is
  below the 1.5 m acquisition threshold, so the test would pass
  trivially. The pickup refactor's explicit pickup intent is what
  makes the test meaningful.
- Corner vs goal kick in both halves (positional-only classification
  is unchanged in both halves). Deferred to either the **AI pickup
  refactor** ticket or a future Laws-of-the-Game fidelity ticket.

## Section I: Out of scope

This ticket explicitly defers four items to siblings:

1. **AI pickup refactor** — replace
   `possession_resolution_system`'s 1.5 m proximity scan with an
   explicit pickup intent in the AI's decision layer. The motivating
   "stationary-ball near defender" attribution is fixed here. The
   **pickup refactor** is filed separately (see ticket draft
   `docs/superpowers/specs/2026-10-03-pickup-refactor-ticket.md`).

2. **Half-time attacking-direction swap** — `attacking_direction`
   becomes `(team, half)`-aware, so offside in the second half
   judges against the correct goal. Corner vs goal kick classification
   remains positional (out of bounds for this Laws-of-the-Game work
   in the pickup ticket). Spec at
   `docs/superpowers/specs/2026-10-03-half-time-attacking-direction-design.md`.

3. **Save / block / deflection systems** — no such systems today.
   When added, they push `LastTouch { kind: Play }` to `PendingBallTouches` from the
   appropriate contact path. They may also write to
   `last_deliberate_play` (the reserved slot). Not in scope.

4. **`last_deliberate_play` is unused in this ticket.** It exists
   only because the user explicitly requested it (a YAGNI override).
   The cost: one extra `Option<LastTouch>` per `Ball` (16 bytes
   plus Option discriminant), three `mix` calls in the state hash,
   one extra `last_deliberate_play: None` line at every `Ball`
   constructor site (~15 sites). The benefit is a no-shape-change
   addition when deflections are introduced.

## Files modified (concrete list)

```
crates/sim-components/src/lib.rs                   # M1, M2, M3
crates/sim-rules/src/lib.rs                        # M4 (re-export apply_ball_touched_system)
crates/sim-rules/src/possession.rs                 # M4, M6
crates/sim-rules/src/out_of_bounds.rs              # M8
crates/sim-rules/src/goals.rs                      # M7, M8
crates/sim-rules/src/touch_registration_tests.rs                # M12 (new)
crates/sim-rules/src/possession_tests.rs           # M11
crates/sim-rules/src/goals_tests.rs                # M11, M12
crates/sim-rules/src/out_of_bounds_tests.rs        # M11
crates/sim-rules/src/referee_tests.rs              # M11, M12
crates/sim-rules/src/tests.rs                      # M11 (ball_fixture)
crates/sim-ai-player/src/execution.rs              # M5, M11
crates/sim-ai-player/src/execution_tests.rs        # M11 (extend)
crates/sim-ai-player/src/perception_tests.rs       # M11
crates/sim-physics/src/lib.rs                      # M11 (Ball fixture)
crates/sim-core/src/lib.rs                         # M9 (BallView)
crates/sim-core/src/simulation/construct.rs        # M4 (system registration), M11 (Ball fixture)
crates/sim-core/src/simulation/state_hash.rs       # M10
crates/sim-core/src/tests/telemetry_tests.rs       # M11
crates/sim-replay/src/lib.rs                       # M9 (BallSnapshot), M11 (replay tests)
```

## Acceptance criteria (from original ticket)

| # | Criterion | Where verified |
|---|-----------|----------------|
| 1 | `last_touch` is written only through the touch-registration path. No proximity writes remain. | Section B (5 push sites + 1 direct write from `lifecycle_system`), `grep "ball.last_touch\s*="` after M7 returns only direct-writes from `lifecycle_system` and the consumer. |
| 2 | All contact paths push to the touch buffer: kick, gaining possession, and any modeled save, block or deflection. | Section B site 1 + 2 cover today's paths; save/block/deflection deferred (§I.3). |
| 3 | Each restart registers a touch for the taking team. `None` is unreachable after the first restart, so no `None` policy is needed downstream. | Section D stops OOB/Goal clears; restart sites (3, 4) push `Restart` touch. |
| 4 | The OOB and goal `RuleEvent` payloads carry the touch record. | Section D enum change in M2; tests in M12. |
| 5 | The state hash includes `last_touch`. Same seed gives the same touch sequence. | Already true today; M10 adds `last_deliberate_play`. M11 verifies `final_state_hash` is byte-identical across two `--full-match` runs of the same seed. |
| 6 | `cargo test --workspace` passes. | M11 + M12. |

## Open items

None at design time. All four open items in the original ticket were
resolved before this design was written:

1. ✓ Scope of contact paths — single touch-registration path across kick + possession + restart placements; save/block/deflection deferred.
2. ✓ Half-time swap — deferred to a sibling spec (§I.2).
3. ✓ Offside coupling — `last_touch` retains `kind == Play` filter; `last_deliberate_play` slot reserved for future Law 11 work.
4. ✓ Pre-touch OOB — tightened debug_assert + release-build `tracing::error!` (§E).

## Risks

- **Determinism hashes change.** Every seed's `final_state_hash` shifts
  by the `last_deliberate_play` block in §G. Replay tests need their
  golden hashes captured from one full-match run. Mitigation: the
  hashes are deterministic given a fixed seed; the capture is a
  one-shot per seed.

- **Test harness changes are needed** wherever `possession_resolution_system`
  is run standalone (§H "Existing tests touched"). Mitigation:
  Schedule-chain pattern is uniform across all touched tests.

- **Same-tick ordering**: a same-tick kick followed by a same-tick
  restart resolves to the restart. This is the correct Laws-of-the-Game
  interpretation (the restart placement supersedes the kick for
  Law 11), but a future test may need to assert it explicitly.

- **Deferred half-time swap (§I.2)**: corners and goal kicks do not
  invert in the second half. Offside will judge against the correct
  attacking direction once the sibling spec lands, but corner vs.
  goal kick will still misclassify. Flagged in §H "Tests NOT in
  scope".

## Spec self-review (inline)

- **Placeholder scan:** No "TBD" or "TODO" — all decisions resolved.
- **Internal consistency:** §B lists 5 push sites + 1 direct write,
  §A/B/D/G/M8 references match. §H test list matches §B push sites.
- **Scope check:** One design, one implementation plan. Half-time
  swap (§I.2) and AI pickup refactor (§I.1) are explicit sibling
  tickets.
- **Ambiguity check:** §B "Last-event-wins" is concrete (§B code
  block). §H "Existing tests touched" lists every file. §I cost
  analysis for `last_deliberate_play` is concrete.

## After this spec is approved

Invoke the writing-plans skill to produce a step-by-step implementation
plan that lands M1..M12 in order. Each step is small enough that a
single review + fix-up cycle will close it. No step is expected to
take more than 30 minutes of focused work.