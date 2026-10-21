# 11: Hard cut of the old SSE path

**What to build:** The new endpoints become the only API. Delete the SSE stream handler and its buffered body, the old match store, `wait_until_done` and the notify primitive, TTL and expiry, `PostResponse`, the 410 path, the socket-boilerplate tests and the `#[ignore]`d 503 test. That includes ticket 01's temporary fix, since the code it patched is gone. The `serve` command's result-TTL option is removed with it, and `serve` builds only the new service. The frontend is not built yet, so there is no migration window.

**Blocked by:** 

- 09 - Serve immutable segments with gzip negotiation

**Status:** ready-for-agent

- [ ] The removed items no longer exist anywhere in the code, tests or configuration.
- [ ] The default `cargo test` suite passes with only the new path.
- [ ] The ignored tests that remain still compile.
- [ ] No route, handler or config field refers to SSE or TTL.
- [ ] The `serve` command no longer accepts a result-TTL option and starts only the new service.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).
