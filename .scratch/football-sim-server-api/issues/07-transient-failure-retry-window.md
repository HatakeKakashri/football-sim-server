# 07: Retry window for transient failures

**What to build:** Resource failures get a short retry window instead of being cached forever like deterministic failures. After the window a later start re-queues the match.

First step: identify a concrete catchable transient failure source in the new design. The old thread-spawn `.expect` no longer exists, and the blocking-thread pool may not expose one. If no real source can be named, close this ticket as won't-do, record that in the pull request description, and remove the 503 rows from tickets 08 and 13.

**Blocked by:** 

- 05 - Run a match through a MatchRunner seam in a new MatchService

**Status:** done

- [x] A concrete transient failure source is identified and written in the pull request description, or the ticket is closed as won't-do with the 503 rows removed from tickets 08 and 13.
- [x] A transient failure records a retry time; a start before that time reports the failure with the remaining wait.
- [x] A start after the retry time re-queues the match.
- [x] Deterministic failures are not retried and keep their cached result.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).

## Comments

Implemented in commit `e914f08` ("feat(sim-server): retry window for transient failures").
**This ticket is implemented, not closed as won't-do**, so tickets 08 and 13
keep their 503 rows.

### The concrete transient failure source

The ticket asks for a catchable transient failure source in the new design.
The old thread-spawn `.expect` is gone and the blocking pool exposes none:
`tokio::task::spawn_blocking` panics only when it is called outside a runtime
context (a programmer error) and queues the task when the pool is saturated,
so the pool's spawn cannot fail.

**The source implemented here is the run's own memory appetite: the animation
frame capture.** A full match captures 54,003 frames of 22 players and holds
them all in memory until the render step. Measured on m-0001, that is
148,588 kB peak RSS for one full match and 37,856 kB for a quarter of it
(memory scales with the match length), against a 3,764 kB baseline. With the
default concurrency cap of `cores - 1` that is roughly 1 GB of resident
frames on an eight-core host, and a container with a smaller memory limit
cannot serve it.

That failure was not catchable: `String::clone` and `Vec::push` abort the
process when an allocation fails, so the whole server died with the match.
The capture now reserves its memory fallibly (`try_reserve`, plus a fallible
string copy), the tick hook reports the failure through the error channel
`TickHook::on_tick` already has, and the runner turns it into a transient
failure with the code `resource_exhausted`. The pressure belongs to the host,
not to the match: the same match succeeds once the pressure has passed, so
the failure is not cached forever.

### What landed

- **`FailureKind`** (`crates/sim-server/src/api/runner.rs`): `MatchOutcome::Failed`
  now carries `Deterministic` or `Transient`.
- **`MatchState::Failed.retry_at`**: `Some(instant)` for a transient failure,
  `None` for a deterministic one.
- **`StartOutcome::RetryAfter`** carries the code, message, last good tick and
  the time left before the match may be tried again.
- **`MatchService::with_retry_window`** sets the window; the default is
  `DEFAULT_RETRY_WINDOW` (5 s).

### Evidence per criterion

| Criterion | Test | File |
|---|---|---|
| A concrete transient source is identified and recorded | the frame-capture memory pressure described above; the commit message records the measurement and the reasoning | commit `e914f08` |
| A transient failure records a retry time | `a_transient_failure_records_a_retry_time` — asserts the retry time opens a full window after the failure and is no further away than the window | `crates/sim-server/src/service/tests/retry_tests.rs:118` |
| A start before the retry time reports the failure with the remaining wait | `a_start_inside_the_window_reports_the_failure_with_the_remaining_wait` — asserts `StartOutcome::RetryAfter` with a positive remaining wait no longer than the window, and that the match did **not** run again | `crates/sim-server/src/service/tests/retry_tests.rs:159` |
| A start after the retry time re-queues the match | `a_start_after_the_window_runs_the_match_again` (the match runs to `Ready`; `runner.runs() == 2`) and `a_start_after_the_window_re_queues_the_match_when_the_cap_is_full` (with the cap full it reports `Queued`) | `crates/sim-server/src/service/tests/retry_tests.rs:204`, `:234` |
| Deterministic failures are not retried and keep their cached result | `a_deterministic_failure_is_never_retried` — a later start reports `AlreadyComplete` and the runner ran once | `crates/sim-server/src/service/tests/retry_tests.rs:315` |

`a_transient_failure_that_fails_again_records_a_new_window`
(`retry_tests.rs:267`) adds the repeat case: a second transient failure opens
a fresh window rather than reusing the old one.

The legacy store and its routes are untouched: it has no retry window and
still caches every failure, transient or not.

### Test results

```
cargo test -p sim-server --lib    →  136 passed; 0 failed; 8 ignored
```
