# 05: Run a match through a MatchRunner seam in a new MatchService

**What to build:** A new match service owns match lifecycle behind one interface. The simulation is hidden behind a runner trait that returns a completed match, with a real adapter that wraps the existing simulation and fake adapters for tests (instant, gated, panicking, tick-cap failure). The service tracks each match through `NotStarted`, `Queued`, `Running`, `Ready` and `Failed`; starting is single-flight, and the simulation plus rendering run together on a blocking thread so the async executor never does CPU work. A panic on that thread surfaces as a join error and becomes `Failed`; deterministic failures (tick cap, invariant violation, panic) are cached together with their details in the existing wire shape (`code`, `message`, `last_good_tick`) for the life of the process, since a retry would burn the same ~18 s again. `Ready` is set only after rendering is complete, and the raw completed match is dropped afterwards.

The service reports outcomes as enums, not status codes, and does not use any wait or notify primitive: nothing blocks on a running match, callers just read its state. Queueing under a cap comes in ticket 06; here a start runs immediately.

**Blocked by:** 

- 04 - Store segments pre-gzipped and report real sizes

**Status:** done

- [x] The runner trait, its real adapter and the instant, gated, panicking and tick-cap-failure fake adapters exist.
- [x] Starting an unknown fixture is rejected; starting a known one moves it out of `NotStarted`; concurrent starts of the same match run it once.
- [x] The state is `Ready` only after rendering finishes, and a ready match exposes its manifest and gzipped segments.
- [x] A panicking runner results in `Failed` and the service keeps working for other matches.
- [x] A deterministic failure is cached and not re-run on a later start.
- [x] The state map's lock is never held across an await point.
- [x] Tests drive the service entirely through the fake runner, with no HTTP and no direct state poking.
- [x] The existing store and routes are untouched.
- [x] A cached failure keeps its `code`, `message` and `last_good_tick` (as in today's invariant and tick-cap error), and a panic reports a distinct code.
- [x] The service is constructed from the loaded fixture set and a runner, so unknown ids are rejected from that set.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).

## Comments

Implemented in commit `61321c5` ("feat(sim-server): add MatchService runner seam").

### What landed

A new `sim_server::service::MatchService` owns the match lifecycle behind one
interface. It is built from a loaded `FixtureSet` and a runner, and it tracks
each match through `NotStarted`, `Queued`, `Running`, `Ready` and `Failed`.

- **`MatchRunner`** (`crates/sim-server/src/api/runner.rs`): the trait the
  service calls, with `SimulationRunner` as the real adapter that wraps the
  existing simulation.
- **The fakes** live in `crates/sim-server/src/test_support.rs` (shared with
  the later HTTP tests): `InstantRunner`, `GatedRunner`, `PanickingRunner` and
  `TickCapFailureRunner`, plus `BlockOnIdRunner` and `AlwaysTransientRunner`
  for the later tickets.
- **Single-flight**: concurrent starts of the same match run it once.
- **`spawn_blocking`**: the simulation and the rendering run together on the
  blocking pool, so the async executor never does CPU work. A panic there
  becomes `Failed` with the distinct `runner_panic` code.
- **`Ready` is set only after rendering completes**; the raw completed match
  is dropped then. A ready match exposes its `Arc<Manifest>` and its
  `Arc<[RenderedSegment]>`.
- **The outcomes are enums, not status codes**: `StartOutcome` (`Started`,
  `Queued`, `AlreadyRunning`, `AlreadyComplete`, `RetryAfter`) and
  `StartError::UnknownFixture`.
- **No wait or notify primitive**: callers read the state; the map's lock is
  never held across an await point (the lock guard is dropped inside `decide`,
  before any blocking work).

### Evidence per criterion

| Criterion | Test | File |
|---|---|---|
| The trait, the real adapter and the four fake adapters exist | `MatchRunner` / `SimulationRunner`; `InstantRunner`, `GatedRunner`, `PanickingRunner`, `TickCapFailureRunner` | `crates/sim-server/src/api/runner.rs:119`, `crates/sim-server/src/test_support.rs:62`, `:146`, `:73`, `:87` |
| An unknown fixture is rejected; a known one leaves `NotStarted`; concurrent starts run it once | `unknown_fixture_is_rejected`, `known_match_moves_out_of_not_started_and_becomes_ready`, `concurrent_starts_of_the_same_match_run_once` (asserts `runner.runs() == 1`) | `crates/sim-server/src/service/tests/lifecycle_tests.rs:10`, `:21`, `:36` |
| `Ready` only after rendering; a ready match exposes the manifest and the gzipped segments | `ready_only_after_rendering_exposes_manifest_and_gzipped_segments` — checks the manifest's match id and build id, and that every segment's gzipped length is populated and decompresses | `crates/sim-server/src/service/tests/lifecycle_tests.rs:69` |
| A panicking runner fails the match and the service keeps working | `panicking_runner_fails_and_service_keeps_working` — asserts the `runner_panic` code, then starts another match to `Ready` | `crates/sim-server/src/service/tests/lifecycle_tests.rs:102` |
| A deterministic failure is cached and not re-run | `deterministic_failure_is_cached_and_not_rerun` — asserts `runner.runs() == 1` after a later start, and that the cached code, message, tick and retry time are unchanged | `crates/sim-server/src/service/tests/lifecycle_tests.rs:141` |
| The lock is never held across an await point | `MatchService::decide` takes the lock and drops it inside; `start` has no await point at all (the `#[allow(clippy::unused_async)]` carries that reason) | `crates/sim-server/src/service.rs:232`, `:196` |
| The tests drive the service through the fake runner only | every test in the service's suite uses a fake runner; there is no HTTP and no state poking | `crates/sim-server/src/service/tests/lifecycle_tests.rs` |
| The existing store and routes are untouched | `git show --stat 61321c5` touches only `lib.rs` and the new `service.rs` | — |
| A cached failure keeps its `code`, `message` and `last_good_tick`; a panic has a distinct code | `panicking_runner_fails_and_service_keeps_working` (code `runner_panic`) and `deterministic_failure_is_cached_and_not_rerun` (code `tick_cap_exceeded`, message and tick preserved) | `crates/sim-server/src/service/tests/lifecycle_tests.rs:102`, `:141` |
| The service is built from the fixture set and a runner, so unknown ids are rejected from it | `MatchService::new(fixture_set, runner)` seeds the state map from the fixture set's ids; `unknown_fixture_is_rejected` | `crates/sim-server/src/service.rs:110`, `crates/sim-server/src/service/tests/lifecycle_tests.rs:10` |

`nothing_blocks_on_a_running_match` (`crates/sim-server/src/service/tests/lifecycle_tests.rs:189`) adds the
"the executor is never blocked" case: a gated match runs while another
completes.

### Test results

```
cargo test -p sim-server --lib    →  136 passed; 0 failed; 8 ignored
```
