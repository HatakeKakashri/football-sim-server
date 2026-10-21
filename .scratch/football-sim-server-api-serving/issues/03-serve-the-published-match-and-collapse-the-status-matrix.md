# 03: Serve the published match and collapse the status matrix

**What to build:** Contract. The HTTP layer writes the match-status-to-answer rule more than once: the start route re-derives it from the start outcome's arms, the manifest route has its own mapper, and the segment route destructures the ready case itself. The 404 "unknown match" response exists verbatim in two files, and the `state` values the wire uses are declared in two files. Switch every route to the service's single status answer, produce each route's answer from one place, and serve the published bytes so that a manifest request copies immutable bytes instead of cloning a value and re-serialising it, and a segment request finds its segment through the index instead of scanning the list. Every status code, body shape, header and `state` value stays exactly as it is today — this ticket changes where the rules live, not what the client sees.

**Blocked by:** 01, 02.

**Status:** ready-for-agent

- [ ] Every route reads the match's status through the service's single status method; no route calls `is_known()` or `state()`.
- [ ] The start route's answers come from the same status-to-answer mapping the manifest route uses, so "running" means the same thing in both routes.
- [ ] The 404 "unknown match" response is written once; the segment route's copy is deleted.
- [ ] The `state` values the wire uses (`unknown`, `not_started`, `queued`, `running`, `ready`, `failed`, `not_ready`, `no_segment`, `unauthorized`, `broken`) are declared once, in the module that owns the HTTP surface, and both route modules use them.
- [ ] The manifest route serves the published manifest bytes: it does not clone the manifest value and it does not re-serialise it.
- [ ] The segment route finds its segment through the published index; it does not scan the segment list.
- [ ] The service's `is_known()` and `state()` are removed from its public interface, and every caller is updated: the routes, the service's own tests and the shared test helper that waits for a match to reach a state.
- [ ] Every row of the status matrix still answers with the same status code, the same body shape, the same `state` value, the same `Cache-Control` and the same `Retry-After`. The existing matrix test passes unchanged.
- [ ] The gzip and the non-gzip segment paths still serve the same bytes as before, and the decoded body still hashes to the manifest's hash.
- [ ] A manifest request no longer allocates a string per manifest row: the served body is a copy of bytes that were produced once, at render time.
- [ ] `cargo test --workspace` green and `cargo clippy --workspace --all-targets -- -D warnings` clean.
