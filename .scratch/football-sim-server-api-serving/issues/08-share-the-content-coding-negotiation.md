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

**Status:** ready-for-agent

- [ ] The q-value `Accept-Encoding` logic lives in one shared module inside `http` (for example `crates/sim-server/src/http/content_negotiation.rs`), not inside the segment route.
- [ ] `accepts_gzip(headers: &HeaderMap) -> bool` is reachable from the module that owns the routes (`service_api`), so a second route can negotiate a coding without copying the parser.
- [ ] The three existing unit tests (`gzip_is_served_only_when_the_client_asks_for_it`, `gzip_is_not_served_when_the_client_refuses_it`, `an_explicit_gzip_entry_wins_over_the_wildcard`) move with the code and pass unchanged; they are not rewritten or deleted.
- [ ] The segment route's behaviour is bit-for-bit unchanged for every `Accept-Encoding` input: same decision, same `Content-Encoding` header, same body bytes.
- [ ] The shared module carries the module doc that explains the negotiation rule (gzip only when asked for; an explicit `gzip` entry beats the wildcard; a malformed `q` is read as 0), so the rule is documented once.
- [ ] `Vary: Accept-Encoding` and `Content-Encoding: gzip` remain set by the segment route only; this ticket does not add them anywhere new.
- [ ] No status code, body shape, header or `state` value changes anywhere; the HTTP test suite passes unchanged.
- [ ] `cargo test --workspace` green and `cargo clippy --workspace --all-targets -- -D warnings` clean.

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

Sits on the frontier: no blockers, no shared files with any other open ticket.
Take it alongside ticket 05 if two agents are working in parallel.