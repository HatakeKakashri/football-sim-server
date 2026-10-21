# 05: Run a match through a MatchRunner seam in a new MatchService

**What to build:** A new match service owns match lifecycle behind one interface. The simulation is hidden behind a runner trait that returns a completed match, with a real adapter that wraps the existing simulation and fake adapters for tests (instant, gated, panicking, tick-cap failure). The service tracks each match through `NotStarted`, `Queued`, `Running`, `Ready` and `Failed`; starting is single-flight, and the simulation plus rendering run together on a blocking thread so the async executor never does CPU work. A panic on that thread surfaces as a join error and becomes `Failed`; deterministic failures (tick cap, invariant violation, panic) are cached together with their details in the existing wire shape (`code`, `message`, `last_good_tick`) for the life of the process, since a retry would burn the same ~18 s again. `Ready` is set only after rendering is complete, and the raw completed match is dropped afterwards.

The service reports outcomes as enums, not status codes, and does not use any wait or notify primitive: nothing blocks on a running match, callers just read its state. Queueing under a cap comes in ticket 06; here a start runs immediately.

**Blocked by:** 

- 04 - Store segments pre-gzipped and report real sizes

**Status:** ready-for-agent

- [ ] The runner trait, its real adapter and the instant, gated, panicking and tick-cap-failure fake adapters exist.
- [ ] Starting an unknown fixture is rejected; starting a known one moves it out of `NotStarted`; concurrent starts of the same match run it once.
- [ ] The state is `Ready` only after rendering finishes, and a ready match exposes its manifest and gzipped segments.
- [ ] A panicking runner results in `Failed` and the service keeps working for other matches.
- [ ] A deterministic failure is cached and not re-run on a later start.
- [ ] The state map's lock is never held across an await point.
- [ ] Tests drive the service entirely through the fake runner, with no HTTP and no direct state poking.
- [ ] The existing store and routes are untouched.
- [ ] A cached failure keeps its `code`, `message` and `last_good_tick` (as in today's invariant and tick-cap error), and a panic reports a distinct code.
- [ ] The service is constructed from the loaded fixture set and a runner, so unknown ids are rejected from that set.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).
