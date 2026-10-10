# 12: HTTP + SSE API slice (one-fixture match streaming)

**What to build:** `sim-server` gains an `axum` + `tokio` HTTP server (new code in the `sim-server` crate or a sibling). `POST /match/{match_id}` starts the match on a dedicated OS thread (not on the async executor) and returns 202 / 200 / 404 per PRD §10.1. `GET /match/{match_id}/stream` is `text/event-stream` and emits exactly the §10.2 layout: message 0 `manifest` (match metadata, pitch dimensions, segment `from_tick`/`to_tick`), messages 1–10 `segment` (each half has exactly 5 segments at 0–10, 10–20, 20–30, 30–40, 40–45+added / 45–55, ..., 85–90+added; never crosses a half boundary), message 11 `full_time`. Resumability via `Last-Event-ID` (sets the start message index). Concurrency cap default `cores − 1` returns `503` with `Retry-After`. Cache of finished/failed results in memory with TTL `result_ttl_secs` (default 3600); eviction returns `410` and a re-`POST` re-simulates. Network default `bind=127.0.0.1`. This ticket deliberately defers auth, CORS, concurrency cap config, and full `client-protocol.md` update to ticket 27/28 — Phase 6 — but the status codes for the happy path and 404/409/410/204/503 are in place to prove the contract end-to-end on a real stream.

**Blocked by:** 11 (event envelope exists so segments can carry `events[]` and `full_time` has data); can run in parallel with Phase 3 work.

**Status:** ready-for-agent

- [ ] `POST /match/{id}` 202/200/404 mapping; `GET .../stream` emits manifest + 10 segments + `full_time`; resume via `Last-Event-ID`.
- [ ] Concurrency cap default `cores − 1`, `503 + Retry-After` over the cap.
- [ ] Cache TTL default `3600`; `410` after expiry; re-`POST` re-simulates with byte-identical uncompressed body (per PRD §10.4).
- [ ] Each match runs on its own OS thread; the async executor stays free.
- [ ] `cargo test --workspace` green; one end-to-end integration test asserts message count = 12 and segment tick ranges match §10.3.
