# 03: Serve the published match and collapse the status matrix

**What to build:** Contract. The HTTP layer writes the match-status-to-answer rule more than once: the start route re-derives it from the start outcome's arms, the manifest route has its own mapper, and the segment route destructures the ready case itself. The 404 "unknown match" response exists verbatim in two files, and the `state` values the wire uses are declared in two files. Switch every route to the service's single status answer, produce each route's answer from one place, and serve the published bytes so that a manifest request copies immutable bytes instead of cloning a value and re-serialising it, and a segment request finds its segment through the index instead of scanning the list. Every status code, body shape, header and `state` value stays exactly as it is today — this ticket changes where the rules live, not what the client sees.

**Blocked by:** 01, 02.

**Status:** done

- [x] Every route reads the match's status through the service's single status method; no route calls `is_known()` or `state()`.
- [x] The start route's answers come from the same status-to-answer mapping the manifest route uses, so "running" means the same thing in both routes.
- [x] The 404 "unknown match" response is written once; the segment route's copy is deleted.
- [x] The `state` values the wire uses (`unknown`, `not_started`, `queued`, `running`, `ready`, `failed`, `not_ready`, `no_segment`, `unauthorized`, `broken`) are declared once, in the module that owns the HTTP surface, and both route modules use them.
- [x] The manifest route serves the published manifest bytes: it does not clone the manifest value and it does not re-serialise it.
- [x] The segment route finds its segment through the published index; it does not scan the segment list.
- [x] The build-id check, the `.json` suffix parse and the segment lookup move out of the segment handler into the resolution step, so the handler keeps only content negotiation and headers.
- [x] The `Instant` import stays inside the lifecycle module: the HTTP layer never calls `Instant::now()` or computes a retry wait itself, so "time left before a retry" is the service's answer rather than the route's arithmetic.
- [x] The service's `is_known()` and `state()` are removed from its public interface, and every caller is updated: the routes, the service's own tests and the shared test helper that waits for a match to reach a state.
- [x] Every row of the status matrix still answers with the same status code, the same body shape, the same `state` value, the same `Cache-Control` and the same `Retry-After`. The existing matrix test passes unchanged.
- [x] The gzip and the non-gzip segment paths still serve the same bytes as before, and the decoded body still hashes to the manifest's hash.
- [x] A manifest request no longer allocates a string per manifest row: the served body is a copy of bytes that were produced once, at render time.
- [x] `cargo test --workspace` green and `cargo clippy --workspace --all-targets -- -D warnings` clean.

## Comments

**What landed.** The HTTP layer now has one rule and one vocabulary. The status-to-answer
mapping lives in `crates/sim-server/src/http/service_api.rs` as `status_answer`: the start
route and the manifest route both come through it, so "running" means the same thing in
both. The start route answers a start it just performed from the state the start decided
(`Running` or `Queued`) — it cannot ask the service for the status there, because the run
may already have finished — and asks the service for the status in every other case
(already running, already complete, failed inside the retry window). The retry wait is
always the service's answer: the route reads `retry_in` from the status and never
computes a wait, and the HTTP layer does not import `Instant` at all.

Every route reads the status through `MatchService::status`; `is_known()` and `state()`
are gone from the service's public interface, and every caller (the routes, the service's
own tests, the shared `wait_until` helper) asks the one method.

The `state` values the wire uses are declared once, in the module that owns the HTTP
surface, and the segment route imports them from there. The 404 "unknown match" response
is written once; the segment route's copy is deleted.

**The published bytes are served.** The manifest route serves `PublishedMatch::manifest_body()`
— the bytes the render step produced — instead of cloning the manifest value and
re-serialising it, so a request allocates no string per manifest row. The segment route
finds its segment through the published index (`PublishedMatch::segment`) instead of
scanning the segment list, and the build-id check, the `.json` suffix parse and the lookup
moved out of the handler into the resolution step (`resolve_segment`), so the handler keeps
only content negotiation and headers.

**The ready case holds the published match only.** `MatchState::Ready` now carries
`published` alone: the rendered manifest value and the segment list are dropped when the
run ends, so the match is represented once. That answers the memory note left by ticket
02 — the ready case no longer holds the bytes twice over. `PublishedMatch` grew the build
id it publishes, because the routes need it for the manifest's `ETag` and for the
build-id check of a segment URL, and the manifest value is no longer kept.

**Two trade-offs worth knowing about.**

1. The published match keeps the uncompressed bodies of the whole match (137 MB against
   6.5 MB of gzip for `m-0001`), as ticket 01 designed. They are now used: a client that
   does not accept gzip is served the published body instead of a body decompressed per
   request. The alternative — decompressing the stored coding on demand — would keep the
   137 MB and never use it.
2. The `broken` answer (500) can no longer be produced by a corrupt gzip stream, because
   the route no longer decompresses anything. The state and the answer are kept as a
   safety net: a published body that is empty is a body that cannot be read.

**Verification.** The status matrix passes unchanged — same status codes, same bodies,
same headers — and the segment tests still prove that the gzip path serves the stored
bytes, that the plain path serves the same body, and that the decoded body hashes to the
manifest's hash. A new test pins the manifest route to the published bytes, byte for
byte. The release end-to-end test (`cargo test --release -p sim-server --test
real_full_match_e2e -- --ignored`) passes: two server processes produce the same build id
(`ac98bf10…`), the same 110 segments and the same state hash, and `docs/api-samples/`
comes back byte-identical — the wire did not move. `cargo clippy --workspace --all-targets
-- -D warnings` is clean.

The `sim-server` suite is green (129 tests). Fourteen failures in `sim-core`,
`sim-components`, `sim-ai-player` and `sim-rules` pre-date this ticket — they fail on a
clean checkout of `main` too, and none of them is in a crate this ticket touches — so
they are left alone here.

**Three things worth knowing about.**

1. The start route does not ask the service for its status in the two arms where the
   start just decided the state (`Started`, `Queued`): the run may already have finished,
   and the answer would be a 200 instead of a 202. It passes the state the service just
   set through the same mapping the manifest route uses, so "running" is written once.
2. The failure answer comes from the service's status, not from the start outcome's
   arms, so `retry_after_ms` is measured a fraction of a millisecond later than before.
   The wait is a live value either way; the `Retry-After` header is unchanged.
3. The unified mapping answers a match that was never started with the 409 the manifest
   route has always answered. The start route never answers for a match that was never
   started — it has just started it — so the wire does not move.

**One deviation, on purpose.** The ticket asks for the `state` values to be declared in
the module that owns the HTTP surface. Nine of the ten are; `ready` stays with the body
that carries it, in the render module (`crate::api::render::STATE_READY`). The manifest
body is produced at render time, so the render layer has to know the value, and a lower
layer must not depend on a higher one.

**Left for a later ticket.** The render step still builds the segment list next to the
published match, and the service drops it: the list has no production reader any more,
but removing it from the render step's output is a change to that layer's shape, not to
the HTTP surface.
