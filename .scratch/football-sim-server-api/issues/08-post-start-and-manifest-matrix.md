# 08: POST start and GET manifest with the full status matrix

**What to build:** A new router lets a client start a match and read its manifest, implementing the agreed status matrix. `POST` on a match: unknown fixture 404; not started 202 (queued or running) and starts the job; queued or running 202 idempotently; ready 200 with the manifest; deterministic failure 500 with `{state: failed, code, message, last_good_tick}`, no-store; transient failure 503 with `Retry-After`. `GET` on the manifest: unknown 404; not started 409 `{state: not_started}`; queued or running 202 with `Retry-After`, no-store; ready 200 with `ETag` and `Cache-Control: no-cache`; failed 500 (503 with `Retry-After` if transient), no-store.

Because a browser's cross-origin `fetch` cannot read `Retry-After`, every 202 body also carries `retry_after_ms`. `GET` never starts a job. The new routes live beside the old ones without disturbing them. Paths are provisional; keep the existing prefix if the PRD uses a different one.

**Blocked by:** 

- 06 - Queue jobs FIFO under the concurrency cap
- 07 - Retry window for transient failures

**Status:** ready-for-agent

- [ ] Every row of the matrix is covered by a table-driven test that goes through `router()` with `tower::ServiceExt::oneshot` and the fake runner, with no sockets.
- [ ] 202 and failed responses carry `Cache-Control: no-store`; 202 carries `Retry-After`; the 200 manifest carries an `ETag` and `no-cache`.
- [ ] Response bodies are JSON with a `state` field.
- [ ] `GET` on a never-started fixture does not start work.
- [ ] `tower` is added as a dev-dependency.
- [ ] The old SSE routes and their tests still pass.
- [ ] A failed response body keeps the existing `code`, `message` and `last_good_tick` fields alongside `state`.
- [ ] Every 202 body includes `retry_after_ms` in addition to the `Retry-After` header.
- [ ] `sim-server serve` builds the match service from the loaded fixtures and the real runner and mounts the new routes beside the old ones; the pull request description includes a manual smoke check against the running binary (POST, poll the manifest until 200).
- [ ] The 200 manifest includes the `result` field from ticket 03.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).
