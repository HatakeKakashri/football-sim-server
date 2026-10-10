# 26: Match registry, in-memory cache with TTL, evict-and-regenerate, concurrency cap

**What to build:** The Phase 6 (a) hardening. `sim-server` keeps an in-memory cache keyed by `match_id` with `result_ttl_secs` (default 3600) holding the finished or failed simulation result, the seed, the headers, and the uncompressed SSE body (so reconnect + evict-and-regenerate are byte-identical per PRD §10.4). After expiry a `GET` returns 410 and a re-`POST` re-simulates the same seed producing the identical uncompressed stream body (excluded from this are gzip bytes which depend on library version). A concurrency cap default `cores − 1` (configurable); over the cap `POST` returns 503 with `Retry-After`; the cap never holds up `GET` of cached results. The cache survives the OS-thread-per-match lifecycle set up in ticket 12.

**Blocked by:** 12 (the API slice must exist to extend with the cache and cap).

**Status:** ready-for-agent

- [ ] Eviction-after-TTL behaviour (410 after expiry; re-`POST` re-simulates) proven end-to-end.
- [ ] Concurrency cap default `cores − 1`; `POST` returns `503 + Retry-After` over the cap; cached `GET`s never block.
- [ ] Evict-and-regenerate produces byte-identical uncompressed SSE body (unit test).
- [ ] `cargo test --workspace` green; long-running end-to-end `#[ignore]`d test for cap behaviour.
