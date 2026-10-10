# 27: Auth, CORS, status code mapping, failure paths, `client-protocol.md` update

**What to build:** `POST /match/{id}` requires `Authorization: Bearer <token>` if `api_token` is configured; missing/invalid tokens return 401. `GET` is open, restricted by CORS to a configured origin list (`cors_origins`, default `[]`). `POST` 401/404 mapping; `GET` 200/204/404/409/410 mapping (404 unknown id, 409 if `POST` not called yet, 410 after TTL, 204 if `Last-Event-ID` is at or beyond the final message). Failure paths: a simulation failure (§12 invariant breach, or a `tick_cap` breach) emits a single `error` SSE message replacing the remaining 11 messages, with `{ code, message, last_good_tick }`; the failure is cached per PRD §10.4 so retries are deterministic. `client-protocol.md` updated to reflect status-code matrix, the failure-event shape, and the manifest + 10-segment + `full_time` layout, replacing the provisional schema. Auth and CORS can be disabled by config for local-only deployments.

**Blocked by:** 26 (cache and registry in place before status mapping).

**Status:** ready-for-agent

- [ ] `POST` requires bearer token if `api_token` set; 401 on missing/invalid; can be disabled for local-only deployment via config.
- [ ] `GET` CORS configured via `cors_origins`; defaults to empty allowlist.
- [ ] Status code matrix for `GET`: 200, 204, 404, 409, 410 proven by integration tests.
- [ ] Failure path: §12 violation / tick-cap breach emits `error` SSE message and is cached.
- [ ] `docs/client-protocol.md` updated and reviewed.
- [ ] `cargo test --workspace` green; PRD §13.1 A12 acceptance test passes.
