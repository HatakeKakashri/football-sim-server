# 07: Retry window for transient failures

**What to build:** Resource failures get a short retry window instead of being cached forever like deterministic failures. After the window a later start re-queues the match.

First step: identify a concrete catchable transient failure source in the new design. The old thread-spawn `.expect` no longer exists, and the blocking-thread pool may not expose one. If no real source can be named, close this ticket as won't-do, record that in the pull request description, and remove the 503 rows from tickets 08 and 13.

**Blocked by:** 

- 05 - Run a match through a MatchRunner seam in a new MatchService

**Status:** ready-for-agent

- [ ] A concrete transient failure source is identified and written in the pull request description, or the ticket is closed as won't-do with the 503 rows removed from tickets 08 and 13.
- [ ] A transient failure records a retry time; a start before that time reports the failure with the remaining wait.
- [ ] A start after the retry time re-queues the match.
- [ ] Deterministic failures are not retried and keep their cached result.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).
