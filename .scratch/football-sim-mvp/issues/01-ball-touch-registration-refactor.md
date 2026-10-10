# 01: Ball-touch registration refactor

**What to build:** A single, audit-defining code path for "a player touched the ball." The five writers that today mutate `Ball.last_touch` directly (proximity-based acquisition, deliberate kicks, OOB taker, kickoff taker, etc.) go through one registration mechanism (`PendingBallTouches` resource + `apply_ball_touched_system` consumer) that records `TouchKind::Play` or `TouchKind::Restart`, denormalises the team onto the touched record, and surfaces `last_touch` in match-report and replay views. Behaviour at the boundaries stays identical to today; state hashes change because the underlying writes change path. Offside-related fields and tests remain stripped (PRD §2: offside stub inert).

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [x] All five `Ball.last_touch` writers route through the registration mechanism; no direct mutation remains.
- [x] Touch record carries `TouchKind` and the team, captured even if the entity is later despawned, so OOB restart payloads survive.
- [x] `Ball.last_touch` and the player-id mapping appear in `BallSnapshot` for both `sim-core` report and `sim-replay`.
- [x] Test coverage: each writer has a unit test asserting the registered touch; replay fixture golden hashes refreshed and committed.
- [x] `cargo test --workspace` green; state-hash change documented in the ticket's `## Comments`.

## Comments

### The five writers

| # | Writer | Site | Kind |
|---|--------|------|------|
| 1 | `kick_execution_system` | `sim-ai-player/src/execution.rs` | `Play` |
| 2 | `possession_resolution_system` | `sim-rules/src/possession.rs` | `Play` |
| 3 | `restart_system` — kickoff-after-goal arm | `sim-rules/src/goals.rs` | `Restart` |
| 4 | `restart_system` — OOB arm | `sim-rules/src/goals.rs` | `Restart` |
| 5 | `lifecycle_system` — kickoff placement | `sim-core/src/simulation/lifecycle.rs` | `Restart` |

`grep -rn "last_touch\s*=" crates/` returns only the consumer
(`sim-rules/src/touch.rs`) and test fixtures. Writer 5 was the last
direct mutation; it now pushes to the buffer like the other four.

### Two drain sites, both load-bearing

`apply_ball_touched_system` runs twice per tick, and **both** are required:

1. **First in the `Rules` set** (`sim-core`'s `register_systems`), so OOB /
   goal / offside / foul / restart read the same-tick touch written by the
   `Execution` and `Possession` sets.
2. **Post-schedule in `Simulation::tick`** (`world.run_system_once`),
   immediately after the non-schedule `lifecycle_system` returns.
   `lifecycle_system` runs outside the schedule, so its kickoff placement
   is invisible to site 1 on the tick it happens; site 2 is what lands
   that touch on `Ball.last_touch` before the tick ends.

Removing site 2 leaves the kickoff touch one tick stale — the
`out_of_bounds_system` / `goal_detection_system` pre-touch guards would
then see a stale or absent touch at each half start, and the
second-half team flip (`apply_transition`) would be masked. Pinned by
`crates/sim-core/src/tests/touch_drain_tests.rs` (3 tests; all three fail
if site 2 is removed).

### Despawn survival

`LastTouch` now carries three denormalised fields, so the record stays
meaningful after the touching entity is despawned:

- `team` — copied off `Player.team_id` at registration time. What Law 9
  restart-taker resolution reads.
- `player` — the live `Entity`, for same-tick readers that need a world
  lookup (`offside_detection_system` reads the toucher's position).
- `player_id` — the stable identity paired with `player`
  (`sim_components::PlayerId`, PRD §9.1: *"Players are identified by
  stable fixture `player_id`, never by engine entity ids."*). Minted from
  `Entity::to_bits()` today; when the PRD's roster fixture lands,
  `PlayerId` becomes the fixture id and every call site keeps compiling.

This also fixes a live bug: `offside_detection_system` previously
re-resolved the toucher's team via `player_query.get(..)`, which silently
skipped offside judgement if the toucher had despawned — even though
`LastTouch.team` already carried the answer. It now reads the
denormalised team and only requires a live entity for the position.

Constructors `LastTouch::by_player(team, entity, kind)` and
`LastTouch::by_team(team, kind)` replace the repeated struct literals so
`player` / `player_id` cannot drift apart.

### State-hash change

The hash **changed** (expected — the ticket says so: *"state hashes
change because the underlying writes change path"*). Cause: the kickoff
touch now lands on `Ball.last_touch` within the tick it is created
(drain site 2), where before it lagged by one tick, so the per-tick hash
sequence shifted from tick 1 onward.

`LastTouch.player_id` is deliberately **not** mixed into the hash: it is
derived from `Entity::to_bits()`, so mixing it would hash the same
information twice. `team` is also not mixed — it is denormalised from
the same `Player` component `player` points at, so it carries no state
the identity does not already imply.

Determinism re-verified after the change (same seed → identical
per-tick hashes):

| Run | Command | Final `state_hash` |
|-----|---------|-------------------|
| 1000 ticks | `simulate --ticks 1000 --seed 12345` | `14167462516872484156` |
| full match | `simulate --full-match --seed 12345` | `16930036101688498291` (tick 324019, `FullTime`, score `[0, 0]`) |

`sim-replay` holds no literal golden hashes — its determinism tests
assert *same-seed ⇒ same per-tick hash history* rather than absolute
values, so they needed no refresh and stayed green throughout.

### Verification

- `cargo test --workspace` — 223 passed, 0 failed, 2 ignored
  (the two full-match tests; unchanged from before this work).
- `bash scripts/clippy-changed.sh` — clean.
- Full-match run reaches `FullTime` at tick 324019.