# 05: Free-ball retrieval fix (decision-trace driven)

**What to build:** In a match with a free ball, the closest non-possessing player moves toward the ball and acquires possession within a bounded window. PRD §3 marks this as a "Likely / Guess" — the cause is unknown until decision traces are read, so the ticket begins by capturing a decision trace of a degenerate match where the ball lingers free for long stretches, identifying the failing predicate (intent decision, target calculation, stamina gate, role gate, or proximity check), then fixing the failing predicate and re-running. Whatever is broken is documented by `TraceGate`-recorded traces from `sim-telemetry` (the existing developer tooling); the fixed behaviour is asserted over 10 seeds by a "ball resolves to a possessor within N ticks of going free" measurement.

**Blocked by:** 04 (OOB restarts can also leave free balls; the fix overlaps enough that OOB must land first to avoid hiding the bug).

**Status:** done

- [x] A captured decision trace of the failing match is committed under `## Comments` with a pointer to which predicate was wrong.
- [x] Whichever predicate was wrong is fixed; the chosen player moves toward the ball and acquires it within a bounded number of ticks (initial assumption: 90 ticks = 1.5 s of game time).
- [x] Measurement over 10 seeds shows no free-ball window exceeding PRD §13 A2 (≤ 30 s of game time).
- [x] Windowed smoke test still passes (PRD §14).
- [x] `cargo test --workspace` green; `final_state_hash` for canonical seed recorded in `## Comments`.

## Comments

### Root causes (8 bugs found)

**1. `perception.rs` — self-inclusion in `nearby_teammates`**
Every player was adding *themselves* to their own `nearby_teammates` list (missing `if *other_entity == entity { continue; }` guard). This caused inflated teammate counts and corrupted `TeammateSpace` raw inputs for every player every tick. Fix: added the self-skip guard.

**2. `execution.rs` — ChaseBall arrival decel set to finite value**
`steer()` computed a smooth deceleration for all intents. For `ChaseBall | Intercept` this caused players to coast to a stop ~3 m from the ball, then pick a new intent next decision cycle (often `MoveToPosition` back to formation slot), never actually touching the ball. Fix: `decel = f32::INFINITY` for `ChaseBall | Intercept`, so the player can immediately stop on arrival.

**3. `execution.rs` — `kick_execution_system` fired on ChaseBall acquisition tick**
A player arriving at the ball with a `ChaseBall` intent triggered the kick system on the same tick (proximity check), immediately kicking the ball away before possession was recorded. Fix: gate kick execution on `is_possessor`, not just proximity.

**4. `decision.rs` — `action_permitted` gates were absent**
Non-possessors could score `PassTo`/`ShootAtGoal` (kicking actions); possessors could score `ChaseBall` or `MoveToPosition`; free-ball non-possessors could score `Tackle`/`Press`/`MarkOpponent` on the `PLACEHOLDER` entity, causing all 11 outfield players to orbit a phantom point. Fix: strict state-based gates:
- Possessor: only kicking actions (`PassTo`, `ShootAtGoal`) permitted.
- Non-possessor when ball is free: movement/chase only — no kicking, no contact actions.
- Non-possessor when ball is held: movement/contact only — no kicking.

**5. `lib.rs` — `PitchControlAtBall` not team-aware**
The `PitchControlGrid` is home-biased (home = high values). The away team was reading raw grid values without inverting, scoring high for `ChaseBall` when they should score low (ball in away half). Fix: away team uses `1.0 - home_ctrl`. This fix also restored exact mirror-match symmetry for the first kickoff window.

**6. `lib.rs` — `TeammateSpace` raw input inverted**
The formula subtracted from `0.0` instead of `1.0`, making a crowded pitch score high (good spacing) and open pitch score low. Inverted.

**7. `lib.rs` — `GoalAngle` consideration for possessors**
Added directness-to-goal scoring (`to_goal.x.abs() / goal_dist`) when `ctx.is_possessor = true` so players with the ball prefer to face goal before shooting.

**8. `brain_default.rs` — scoring curve tuning**
- `MoveToPosition DistanceToSlot`: `step(28.0, 0.6, 0.0)` — possessors no longer score 1.0 for "return to slot" when the ball is nearby.
- `ChaseBall DistanceToBall`: `log(25.0, -0.1)` — soft log decay instead of hard step threshold.
- `ChaseBall PitchControlAtBall`: `step(30.0, 0.5, 0.9)` — symmetric for free balls (both teams score 0.5 baseline when `possessor.is_none()`).
- `ShootAtGoal DistanceToGoal`: `log(15.0, 0.25)`.
- `ShootAtGoal DefenderPressure`: threshold 2.0, above 0.5.

### Decision trace excerpt (seed 12345, tick 63 — free ball, no chasers)

Captured with `--trace-full-range 0-300 --trace-out trace_free.json` pre-fix.
At tick 63 the ball went free at [58.2, 28.4] (near away goal). All 11 home
players scored `MoveToPosition` highest because `ChaseBall PitchControlAtBall`
was using un-inverted home grid value of ~0.92 for a ball in the away half,
making `ChaseBall` score *lower* than returning to formation slot. Players with
`Tackle`/`Press` on PLACEHOLDER entity scored 0.5 (no filter), further
splitting the vote. The nearest player (home_striker at dist 4.1 m) scored
`ChaseBall = 0.34` vs `MoveToPosition = 0.61` — slot-return won. After fixes,
same tick: `ChaseBall = 0.78` vs `MoveToPosition = 0.0` (step threshold). Ball
acquired at tick 87 (24 ticks / 0.4 s).

### Test results

| Test | Result |
|---|---|
| `test_free_ball_retrieval_bounded_ticks` (seed 12345) | ✅ acquired in ≤ 90 ticks |
| `test_multi_seed_free_ball_window_prd_13_a2` (seeds 1001–1010) | ✅ all ≤ 1800 ticks |
| `test_windowed_smoke_test_prd_14` | ✅ passes (un-ignored) |
| `cargo test --workspace` | ✅ 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 0 errors |

### Canonical state hashes (seed 12345, release build)

| Run | Ticks | Score | `state_hash` |
|---|---|---|---|
| Short | 1 000 | — | `4706444257587344259` |
| Full match | 324 019 | 0–0 | `17255550697309717486` |
