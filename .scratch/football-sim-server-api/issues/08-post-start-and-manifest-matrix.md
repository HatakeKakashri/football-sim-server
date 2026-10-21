# 08: POST start and GET manifest with the full status matrix

**What to build:** A new router lets a client start a match and read its manifest, implementing the agreed status matrix. `POST` on a match: unknown fixture 404; not started 202 (queued or running) and starts the job; queued or running 202 idempotently; ready 200 with the manifest; deterministic failure 500 with `{state: failed, code, message, last_good_tick}`, no-store; transient failure 503 with `Retry-After`. `GET` on the manifest: unknown 404; not started 409 `{state: not_started}`; queued or running 202 with `Retry-After`, no-store; ready 200 with `ETag` and `Cache-Control: no-cache`; failed 500 (503 with `Retry-After` if transient), no-store.

Because a browser's cross-origin `fetch` cannot read `Retry-After`, every 202 body also carries `retry_after_ms`. `GET` never starts a job. The new routes live beside the old ones without disturbing them. Paths are provisional; keep the existing prefix if the PRD uses a different one.

**Blocked by:** 

- 06 - Queue jobs FIFO under the concurrency cap
- 07 - Retry window for transient failures

**Status:** done

- [x] Every row of the matrix is covered by a table-driven test that goes through `router()` with `tower::ServiceExt::oneshot` and the fake runner, with no sockets.
- [x] 202 and failed responses carry `Cache-Control: no-store`; 202 carries `Retry-After`; the 200 manifest carries an `ETag` and `no-cache`.
- [x] Response bodies are JSON with a `state` field.
- [x] `GET` on a never-started fixture does not start work.
- [x] `tower` is added as a dev-dependency.
- [x] The old SSE routes and their tests still pass.
- [x] A failed response body keeps the existing `code`, `message` and `last_good_tick` fields alongside `state`.
- [x] Every 202 body includes `retry_after_ms` in addition to the `Retry-After` header.
- [x] `sim-server serve` builds the match service from the loaded fixtures and the real runner and mounts the new routes beside the old ones; the pull request description includes a manual smoke check against the running binary (POST, poll the manifest until 200).
- [x] The 200 manifest includes the `result` field from ticket 03.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).

## Comments

Implemented in commit `01cd211` ("feat(sim-server): start a match and read its manifest over HTTP").

### What landed

Two new routes sit beside the legacy SSE routes, driven by the `MatchService`
(`crates/sim-server/src/http/service_api.rs`):

- `POST /match/{match_id}/start` — starts the match and reports what the
  start did.
- `GET /match/{match_id}/manifest` — reads the manifest of a match that has
  already started. It never starts work.

Every body is JSON and carries a `state` field. The 200 body is the manifest
itself plus `state: ready`, so it keeps the manifest's `result` field from
ticket 03. Every 202 body carries `retry_after_ms` next to the `Retry-After`
header, because a browser's cross-origin `fetch` cannot read the header. A
failed body keeps the old error wire shape (`code`, `message`,
`last_good_tick`) next to `state`.

### Evidence per criterion

| Criterion | Test | File |
|---|---|---|
| Every row of the matrix is covered by a table-driven test through the router with `oneshot` and the fake runner, no sockets | `the_status_matrix_is_served` — 14 rows, one table, each request driven through `router_with_service` with `tower::ServiceExt::oneshot` | `crates/sim-server/src/http/service_api/tests/matrix.rs:13` |
| 202 and failed carry `no-store`; 202 carries `Retry-After`; the 200 manifest carries an `ETag` and `no-cache` | the matrix asserts the `Cache-Control` of every row, the presence of `Retry-After`, and that the 200's `ETag` is the manifest's `build_id` | `crates/sim-server/src/http/service_api/tests/matrix.rs:39`, `:79` |
| Bodies are JSON with a `state` field | the matrix parses each body as JSON and asserts `json["state"]` against the row's expected state | `crates/sim-server/src/http/service_api/tests/matrix.rs:32` |
| `GET` on a never-started fixture does not start work | `get_on_a_never_started_match_starts_no_work` — asserts 409 and that the service state is still `NotStarted` | `crates/sim-server/src/http/service_api/tests/routes.rs:8` |
| `tower` is a dev-dependency | `tower = { version = "0.5", features = ["util"] }` | `crates/sim-server/Cargo.toml` |
| The old SSE routes and their tests still pass | `the_legacy_routes_are_mounted_beside_the_new_ones` — the legacy stream route still answers 409 on a never-started match; the whole `http` suite passes | `crates/sim-server/src/http/service_api/tests/routes.rs:106` |
| A failed body keeps `code`, `message` and `last_good_tick` alongside `state` | the matrix asserts all three on the failure rows | `crates/sim-server/src/http/service_api/tests/matrix.rs:61` |
| Every 202 body includes `retry_after_ms` | the matrix asserts the body's `retry_after_ms` is present on every 202 row | `crates/sim-server/src/http/service_api/tests/matrix.rs:55` |
| `sim-server serve` builds the service from the loaded fixtures and the real runner, mounting the new routes beside the old ones; the description has a manual smoke check | `dispatch.rs`'s `serve` builds a `MatchService` from the loaded `FixtureSet` with `SimulationRunner`; the commit message carries the smoke check below | `crates/sim-server/src/dispatch.rs:180` |
| The 200 manifest includes the `result` field from ticket 03 | the matrix asserts `json["result"]["score"]` is an array on the 200 rows | `crates/sim-server/src/http/service_api/tests/matrix.rs:92` |

The matrix's rows: `POST` on an unknown fixture (404), not started (202),
already running (202, run once), queued (202), ready (200 + manifest),
deterministic failure (500) and transient failure (503 + `Retry-After`); and
the same seven states for `GET` on the manifest, with not-started answering
409.

### The manual smoke check (from the commit message)

```
cargo build --release -p sim-server
./target/release/football-sim serve --bind 127.0.0.1:18080
curl -i -X POST http://127.0.0.1:18080/match/m-0001/start
# HTTP/1.1 202 Accepted
# cache-control: no-store
# retry-after: 1
# {"state":"running","match_id":"m-0001","retry_after_ms":1000}

curl -i http://127.0.0.1:18080/match/m-0001/manifest   # 202 while it runs
# ... poll every 2 s, after 9 polls:
# HTTP/1.1 200 OK
# etag: "ac98bf108118630289bc0e425f34286d37ea075c72b209956295f555aec36706"
# cache-control: no-cache
# {"build_id":"ac98bf...","result":{"score":[3,5],...},"state":"ready",...}

curl -i http://127.0.0.1:18080/match/nope/manifest      # 404 unknown
curl -i http://127.0.0.1:18080/match/m-0002/manifest    # 409 not started
curl -i http://127.0.0.1:18080/match/m-0002/stream      # 409, legacy route
```

### Test results

```
cargo test -p sim-server --lib    →  136 passed; 0 failed; 8 ignored
```
