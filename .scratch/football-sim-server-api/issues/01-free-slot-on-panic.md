# 01: Free the concurrency slot when a simulation panics or fails to start

**What to build:** A simulation that panics, or whose thread cannot be spawned, no longer wedges the server. Today a panic skips match completion: the concurrency slot never frees, the record stays `Running` forever, `POST` keeps returning 200 and `GET` loops forever, and each panic permanently shrinks the cap. The same leak happens when the thread spawn fails after the slot was taken. After this ticket, a failed run becomes a cached `Failed` result, the slot is released, and `GET` returns a terminal failure instead of waiting.

This is a deliberately small fix on the existing store. Ticket 11 deletes that store, but the bug is live until then.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] A runner panic marks the match `Failed`, releases its slot, and a `GET` for it returns a terminal failed response instead of waiting.
- [ ] A thread-spawn failure releases the slot that was taken before the spawn.
- [ ] More panics than the configured cap are followed by a normal match that still starts, proving the cap does not shrink.
- [ ] The failure can be injected in tests without relying on the tick-cap trick or editing the store's state directly.
- [ ] The existing test suite still passes.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Make the changes this ticket requires, and commit them as a single commit.
4. Provide that commit as a downloadable patch file (`git format-patch -1`).
