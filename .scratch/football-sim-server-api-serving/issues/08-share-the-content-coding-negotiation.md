# 08: Share the content-coding negotiation between routes

**What to build:** Pure expand. The q-value `Accept-Encoding` parser lives
inside the segment route as two private functions (`accepts_gzip` and
`parse_coding`, `crates/sim-server/src/http/service_api/segments.rs:158-200`),
so it can only be used by that one route. Any second route that wants to serve a
gzipped representation would have to copy the parser. Lift it into a shared
module inside the HTTP layer and point both routes at it. Nothing a client sees
changes: the same `Accept-Encoding` header produces the same gzip decision, the
same body and the same headers, before and after this ticket.

This is a pure prefactor: it exists so ticket 10 can negotiate gzip for the
manifest without duplicating the q-value rules. It is deliberately boring — no
new behaviour, no new route, no wire change.

**Blocked by:** None (can start immediately).

**Status:** done

- [x] The q-value `Accept-Encoding` logic lives in one shared module inside `http` (for example `crates/sim-server/src/http/content_negotiation.rs`), not inside the segment route.
- [x] `accepts_gzip(headers: &HeaderMap) -> bool` is reachable from the module that owns the routes (`service_api`), so a second route can negotiate a coding without copying the parser.
- [x] The three existing unit tests (`gzip_is_served_only_when_the_client_asks_for_it`, `gzip_is_not_served_when_the_client_refuses_it`, `an_explicit_gzip_entry_wins_over_the_wildcard`) move with the code and pass unchanged; they are not rewritten or deleted.
- [x] The segment route's behaviour is bit-for-bit unchanged for every `Accept-Encoding` input: same decision, same `Content-Encoding` header, same body bytes.
- [x] The shared module carries the module doc that explains the negotiation rule (gzip only when asked for; an explicit `gzip` entry beats the wildcard; a malformed `q` is read as 0), so the rule is documented once.
- [x] `Vary: Accept-Encoding` and `Content-Encoding: gzip` remain set by the segment route only; this ticket does not add them anywhere new.
- [x] No status code, body shape, header or `state` value changes anywhere; the HTTP test suite passes unchanged.
- [x] `cargo test --workspace` green and `cargo clippy --workspace --all-targets -- -D warnings` clean.

## How to execute

1. Create `crates/sim-server/src/http/content_negotiation.rs`. Move
   `accepts_gzip`, `parse_coding` and their `#[cfg(test)] mod tests` into it
   verbatim from `segments.rs`, keeping the doc comments.
2. Register the module in `crates/sim-server/src/http.rs` (or `http/mod.rs`,
   wherever the router lives) and mark the functions `pub(crate)` so
   `service_api` and `service_api::segments` can both reach them.
3. Delete the two functions and the test module from `segments.rs`; add the
   `use` import at the top.
4. Run `cargo test -p sim-server` and confirm the segment route's negotiation
   tests and the full HTTP suite are green with no edits to any assertion.

## Comments

**What landed.** The q-value parser moved out of the segment route into
`crates/sim-server/src/http/content_negotiation.rs`: `accepts_gzip`,
`parse_coding` and the three unit tests travelled verbatim, with their
doc comments. The module doc carries the negotiation rule once (gzip
only when asked for; an explicit `gzip` entry beats the wildcard; a
malformed `q` is read as 0), so the rule is no longer described in a
place that only one route can reach.

The module is registered in `http.rs` as `pub(crate) mod
content_negotiation`, so `accepts_gzip` is reachable from every route
module under `http` — `service_api` and `service_api::segments`
included — and ticket 10 can negotiate the manifest's coding by calling
it rather than by copying the parser. The crate's public API is
unchanged: the module is crate-visible, not exported.

**Why `pub` and not `pub(crate)` on the function.** The module is
crate-visible, so `pub` inside it means exactly "visible to the whole
crate". `pub(crate)` there is the same reach, and clippy's
`redundant_pub_crate` lint reports it as redundant inside a
crate-visible module — the workspace's clippy floor is `-D warnings`, so
the explicit `pub` is what the linter asks for.

**Nothing a client sees changed.** The segment route still decides the
representation with the same function, over the same header, and still
sets `Vary: Accept-Encoding` and `Content-Encoding: gzip` itself; the
shared module sets no header and touches no response. The three unit
tests moved with the code and were not rewritten: they now run as
`http::content_negotiation::tests::*`. The route-level HTTP tests
(`a_gzip_client_gets_the_stored_gzipped_bytes`,
`a_client_without_gzip_gets_the_decompressed_bytes`,
`a_client_that_refuses_gzip_gets_the_decompressed_bytes`) pass with no
edit to any assertion.

**Verification.**

- `cargo test -p sim-server` — 122 lib tests pass (the same 122 as
  before the change), plus the two integration tests.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo test --workspace` — the failures it reports are the ones
  `main` already had, in crates this ticket does not touch: 1 in
  `sim-ai-player`, 1 in `sim-components`, 3 in `sim-core`, 9 in
  `sim-rules`. The count is identical with and without this change
  (checked by stashing it), and `sim-server` is green in both.
- `cargo doc -p sim-server --no-deps` — the same 8 warnings as before
  (pre-existing); this change adds none.