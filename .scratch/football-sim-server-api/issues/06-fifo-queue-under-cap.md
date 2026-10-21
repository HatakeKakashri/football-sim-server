# 06: Queue jobs FIFO under the concurrency cap

**What to build:** The service limits concurrent simulations with a fair semaphore sized from the existing cap setting. Starts beyond the cap wait in `Queued` and run in the order they arrived, instead of being rejected. A permit is released on every path, including success, failure and panic.

**Blocked by:** 

- 05 - Run a match through a MatchRunner seam in a new MatchService

**Status:** ready-for-agent

- [ ] With the cap at N, the (N+1)th start reports `Queued` and runs only when a permit frees.
- [ ] Queued matches start in arrival order, proven with the gated fake runner.
- [ ] A match moves `Queued` to `Running` when it gets a permit.
- [ ] A permit is released after success, failure and panic, so the cap never shrinks.
- [ ] There is no over-cap rejection path in the service.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).
