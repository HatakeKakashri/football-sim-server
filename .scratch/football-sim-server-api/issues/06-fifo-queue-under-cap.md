# 06: Queue jobs FIFO under the concurrency cap

**What to build:** The service limits concurrent simulations with a fair semaphore sized from the existing cap setting. Starts beyond the cap wait in `Queued` and run in the order they arrived, instead of being rejected. A permit is released on every path, including success, failure and panic.

**Blocked by:** 

- 05 - Run a match through a MatchRunner seam in a new MatchService

**Status:** done

- [x] With the cap at N, the (N+1)th start reports `Queued` and runs only when a permit frees.
- [x] Queued matches start in arrival order, proven with the gated fake runner.
- [x] A match moves `Queued` to `Running` when it gets a permit.
- [x] A permit is released after success, failure and panic, so the cap never shrinks.
- [x] There is no over-cap rejection path in the service.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).

## Comments

Implemented in commit `8e45307` ("feat(sim-server): queue match jobs FIFO under the concurrency cap").

### What landed

- **A fair semaphore** sized to the cap (`MatchService::with_cap`): a start
  that finds no free permit waits in `Queued` and runs when a permit frees,
  in arrival order. There is no over-cap rejection — a start beyond the cap is
  queued, never refused.
- **The permit travels with the run** and is released on every path out of it
  — success, failure and panic — so the cap never shrinks. `finish` hands the
  permit to the match that has waited longest, so the queue drains in arrival
  order and a later start can never jump it.
- **`MatchService::schedule`** decides under the lock and drops the guard
  before any blocking work.

### Evidence per criterion

| Criterion | Test | File |
|---|---|---|
| With the cap at N, the (N+1)th start reports `Queued` and runs only when a permit frees | `over_cap_start_is_queued_and_runs_when_a_permit_frees` — cap of 2, three starts; the third is `Queued`, has not run, and only starts after a permit frees | `crates/sim-server/src/service/tests/queue_tests.rs:159` |
| Queued matches start in arrival order | `queued_matches_start_in_arrival_order` — cap of 1; the runner records the order it was entered in, and the test asserts `m-0`, then `m-1`, then `m-2` | `crates/sim-server/src/service/tests/queue_tests.rs:188` |
| A match moves `Queued` to `Running` when it gets a permit | `over_cap_start_is_queued_and_runs_when_a_permit_frees` and `queued_matches_start_in_arrival_order` both assert `MatchState::Running` once the permit frees | `crates/sim-server/src/service/tests/queue_tests.rs:183`, `:215` |
| A permit is released after success, failure and panic | `permit_is_released_after_success`, `permit_is_released_after_failure`, `permit_is_released_after_panic` | `crates/sim-server/src/service/tests/queue_tests.rs:283`, `:307`, `:338` |
| There is no over-cap rejection path | `over_cap_start_is_never_rejected` — every start beyond the cap returns `Ok(StartOutcome::Queued)`, none is an error; `the_cap_is_never_exceeded` asserts every outcome is `Started` or `Queued` and that the peak concurrency never exceeded the cap | `crates/sim-server/src/service/tests/queue_tests.rs:232`, `:369` |

`cap_never_shrinks` (`queue_tests.rs:405`) closes the row: after a success, a
failure *and* a panic, a new start runs at once rather than being queued.
`second_start_of_a_queued_match_is_already_running` (`queue_tests.rs:257`)
adds the single-flight case for a queued match.

The suite's `ScriptedRunner` opens a gate per match, so the test decides when
each match ends and the order is observable.

### Test results

```
cargo test -p sim-server --lib    →  136 passed; 0 failed; 8 ignored
```
