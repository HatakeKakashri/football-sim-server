# AI Pickup Intent — Ticket Body

**Filed as:** separate ticket (will be filed in current sprint per user)
**Depends on:** the `Ball-Touch Registration Refactor` spec at
`docs/superpowers/specs/2026-10-03-ball-touch-event-refactor-design.md`
must land first. The pickup system consumes the `PendingBallTouches`
Resource introduced there.
**Type:** AI feature (not a refactor) — adds a new intent and decision
arm.
**Status:** Ticket draft, not yet designed. This document is the task
description, not the design.

## Why this is a separate ticket

The motivating bug from the parent ticket — *"a defender within 1 m of
a stationary ball registers as the last toucher"* — is *not* fixed by
the Ball-Touch Registration Refactor. That spec routes the existing
proximity-based acquisition through a single `PendingBallTouches`
buffer, but the proximity scan itself is preserved. A defender at
1.4 m from a stationary ball still becomes the possessor and the
last-toucher the moment they reach the threshold.

The reason this is not in the same ticket: `possession_resolution_system`
is the *only* place `Ball.possessor` is set today (besides test
fixtures). Replacing it with an explicit pickup decision requires
adding a new intent variant, extending the utility-AI scoring,
updating the kick gate in `kick_execution_system`, and writing a new
state machine for "ball held vs. ball free." This is enough scope
to deserve its own design + plan + review cycle.

## What "pickup" means

Replace the proximity-acquisition side effect in
`possession_resolution_system` with an AI intent. When a player's
utility-AI scores a new `Intent::PickUpBall` higher than their current
movement/action intent, the player emits `BallTouched { player, team,
kind: Play }` and sets `Ball.possessor = Some(self)` in a single
step. Possession becomes a decision, not a side effect.

`possession_resolution_system` shrinks to **only** clearing possession
when a kicked ball goes out of range (or never reaches a player)
and updating `Ball.state` to mirror `Ball.possessor.is_some()`. The
1.5 m closest-player scan moves into the AI's decision layer.

## Constraints

- **Determinism preserved**: the AI's pickup decision must be
  deterministic given a seed. Utility-AI scores are already
  deterministic (they consume `SimRng` only inside the player's own
  intent sampling); the new `PickUpBall` consideration follows the
  same pattern.
- **Same-tick pickup-then-kick is impossible**: today a player within
  1.5 m becomes the possessor and can kick on the same tick
  (`kick_execution_system` runs in the next set, after the Possession
  set). After the pickup refactor, pickup happens in the Decision set
  (AI), possession is registered in the Possession set (rules), and
  the kick can fire on the *following* tick (Execution set). This is a
  1-tick latency increase for the first kick after any rest, and a
  behavior change worth surfacing in the design.
- **`kick_off_impulse` interaction**: today `lifecycle_system`'s
  kickoff state machine applies a `(2.0, 0.0)` velocity to the ball
  after placement (`lifecycle.rs:254-258`). After the pickup
  refactor, the kickoff taker still gets this impulse but does not
  auto-pickup. They walk to the ball and pick it up. The ball still
  moves immediately, so the "kickoff impulse" effect is preserved
  visually — it just doesn't come with free possession.
- **`offside_detection_system` already reads `Ball.last_touch`**.
  After this refactor, the pickup player's `LastTouch` (kind: Play)
  flips offside to that player's team, same as today.
- **OOB and Goal systems unchanged**. They read `Ball.last_touch` and
  fire their `RuleEvent` payloads; pickup neither helps nor hinders.

## Suggested scope (placeholder; design will refine)

| Item | Where | Effort |
|------|-------|--------|
| New `Intent::PickUpBall` variant | `sim-components/src/intent.rs` | small |
| New utility-AI consideration `PickUpBall` | `sim-ai-player/src/decision.rs` | medium |
| Move 1.5 m scan from `possession_resolution_system` to the AI decision | `sim-ai-player`, `sim-rules` | medium |
| Pickup emits `BallTouched` + sets `Ball.possessor` | new system in `sim-rules` | small |
| Update kick gate: kick on next tick after pickup | `sim-ai-player/src/execution.rs` | small |
| Update `possession_resolution_system` to only mirror `Ball.possessor` into `Ball.state` | `sim-rules/src/possession.rs` | small |
| Tests: `test_pickup_emits_touch`, `test_kick_after_pickup_same_player`, `test_no_auto_pickup_on_kickoff_impulse`, `test_pickup_deterministic_with_seed` | `sim-ai-player/src/pickup_tests.rs`, `sim-rules/src/possession_tests.rs` | medium |
| Replay golden hashes update (pickup timing changes one tick per rest) | `sim-replay/src/lib.rs:256-310` | small |

## Open items (placeholder; resolve in design)

1. **Pickup animation/state**: when a player picks up the ball, do
   they "stop and hold it" (zero velocity for one tick) or "scoop it
   on the run" (continue at current velocity)? Affects player
   `Velocity` in the same tick as `Ball.possessor` is set.
2. **Dribble vs. pickup**: is there a difference between "picking up
   a stationary ball" and "dribbling a ball that's already in
   motion"? Today there isn't (proximity treats both as possession).
   The new system might want to distinguish.
3. **Goalkeeper priority**: a goalkeeper should auto-pickup a ball in
   their box without AI utility scoring. Today the proximity scan
   applies to all players uniformly. After the refactor, do goalkeepers
   get a `PickUpBall` consideration with a strong weight, or a
   dedicated `Intent::GKPickUpBall`?
4. **Tackle interaction**: today `ActionIntent::Tackle(Entity)` is a
   separate intent that produces a velocity-continuation kick. After
   the refactor, does it require the tackler to also have picked up
   the ball first, or does it auto-pickup as a side effect?

## Acceptance criteria

1. `possession_resolution_system`'s 1.5 m closest-player scan is
   removed from `sim-rules` and lives only in the AI decision layer.
2. A `Ball.last_touch` is set via `BallTouched { kind: Play }` only
   when the AI explicitly decides to pick up (not on proximity).
3. The defender-near-stationary-ball test passes: a defender 1.0 m
   from a stationary ball does NOT register a touch until they
   actually kick it.
4. `cargo test --workspace` passes with replay golden hashes
   updated for the 1-tick pickup latency.
5. `cargo run --release -p sim-server -- simulate --full-match
   --seed 12345` produces byte-identical output across two runs
   (determinism preserved).

## Out of scope

- Half-time attacking-direction swap (separate spec at
  `docs/superpowers/specs/2026-10-03-half-time-attacking-direction-design.md`).
- Save / block / deflection systems (no such systems today).