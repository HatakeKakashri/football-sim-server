# 01: Free the concurrency slot when a simulation panics or fails to start

**What to build:** A simulation that panics, or whose thread cannot be spawned, no longer wedges the server. Today a panic skips match completion: the concurrency slot never frees, the record stays `Running` forever, `POST` keeps returning 200 and `GET` loops forever, and each panic permanently shrinks the cap. The same leak happens when the thread spawn fails after the slot was taken. After this ticket, a failed run becomes a cached `Failed` result, the slot is released, and `GET` returns a terminal failure instead of waiting.

This is a deliberately small fix on the existing store. Ticket 11 deletes that store, but the bug is live until then.

**Blocked by:** None (can start immediately)

**Status:** done

- [x] A runner panic marks the match `Failed`, releases its slot, and a `GET` for it returns a terminal failed response instead of waiting.
- [x] A thread-spawn failure releases the slot that was taken before the spawn.
- [x] More panics than the configured cap are followed by a normal match that still starts, proving the cap does not shrink.
- [x] The failure can be injected in tests without relying on the tick-cap trick or editing the store's state directly.
- [x] The existing test suite still passes.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Make the changes this ticket requires, and commit them as a single commit.
4. Provide that commit as a downloadable patch file (`git format-patch -1`).

## Comments

Implemented in commit `ca7290c` ("fix(sim-server): free the concurrency slot when a simulation panics or fails to start").

### What landed

- **`MatchRunner` seam** (`crates/sim-server/src/api/runner.rs`): the store's
  simulation is behind a trait, so a panic can be injected without the tick
  cap. `run_catching_panic` turns a panic into a cached `runner_panic`
  failure.
- **`ThreadSpawner` seam** (`crates/sim-server/src/api/spawn.rs`): the store
  takes a concurrency slot *before* it spawns the thread, so a spawn failure
  must be reported back to the store. The store releases the slot, caches the
  match as failed, and answers `POST` with 500.
- **The lost-wakeup race** in `MatchStore::wait_until_done` is closed: the
  waiter registers for the wakeup *before* reading the state, so a match that
  settles while the request is in flight cannot be missed.

### Evidence per criterion

| Criterion | Test | File |
|---|---|---|
| A runner panic fails the match, releases the slot, and the `GET` returns a terminal failure | `runner_panic_fails_the_match_and_releases_the_slot` | `crates/sim-server/src/api/failure_tests.rs:42` |
| A thread-spawn failure releases the slot taken before the spawn | `spawn_failure_releases_the_slot_taken_before_the_spawn` | `crates/sim-server/src/api/failure_tests.rs:107` |
| More panics than the cap, then a normal match — the cap does not shrink | `panics_do_not_shrink_the_concurrency_cap` (three panics through a cap of one, then `ok-0`) | `crates/sim-server/src/api/failure_tests.rs:74` |
| The failure is injectable without the tick cap or poking the store's state | `ScriptedRunner` (panics for `panic-` ids) and `ScriptedSpawner` (refuses `spawn-fail-` ids) behind the two seams | `crates/sim-server/src/api/test_doubles.rs:14`, `:54` |
| The existing test suite still passes | `cargo test -p sim-server --lib` — 136 passed, 0 failed | — |

`spawn_failure_caches_a_terminal_failed_match` (`failure_tests.rs:129`) adds
the terminal-failure half of the spawn-failure row: a match whose thread never
started serves a cached `thread_spawn_failed` error stream rather than
waiting forever.

### Test results

```
cargo test -p sim-server --lib    →  136 passed; 0 failed; 8 ignored
```

The workspace has pre-existing failures in `sim-ai-player`, `sim-components`,
`sim-core` and `sim-rules`; they are identical at the pre-ticket baseline
(`56269b5`) and none of them live in a file this ticket touched.
