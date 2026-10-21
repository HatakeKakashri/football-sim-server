# 01: Publish the rendered match as immutable bytes

**What to build:** Pure expand. The render step produces a `Manifest` value and a list of segments, and the HTTP layer re-derives the manifest's wire bytes on every single request — a deep clone of the whole manifest (110 segment rows, 22 roster rows, 429 string allocations) plus a fresh serialisation of 36 KB — even though the manifest never changes after it has been rendered. Give the render step a second output: a published match that owns the exact bytes the manifest route serves and the segment bodies indexed by segment number, so a served manifest is a copy of immutable bytes rather than a value re-derived per request. Nothing a client sees changes in this ticket: the HTTP routes still build their responses the way they do today, and the manifest's JSON, the build id and every pinned segment hash stay exactly as they are.

The published bytes are produced where the manifest is produced, not in the HTTP module: the body type that produces the manifest's response bytes moves out of `http::service_api` into the module that owns the manifest, so the wire format of a match lives in one place.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] The render step returns a published match alongside the manifest and the segments, holding the manifest's response body as shared bytes and the segments in a map keyed by segment number.
- [ ] The published manifest body is byte-identical to what the manifest route serves today: the same JSON object, the same key order, and the `state` field last.
- [ ] Every published segment body is byte-identical to the body the segment route serves today, and its SHA-256 still equals the uncompressed hash the manifest publishes.
- [ ] Looking a segment up by number returns the same segment for every number the manifest lists, and nothing for a number outside that range.
- [ ] Publishing happens once per render, not once per read: rendering the same fixture and completed match twice produces byte-identical published bytes.
- [ ] The published bytes are shared, never mutated, and serving one costs no copy of the underlying buffer.
- [ ] The HTTP routes, the status matrix, the manifest's JSON shape, the build id and the pinned golden segment hashes are all unchanged by this ticket.
- [ ] The render module's own tests cover the published bytes and the index; no HTTP test changes.
- [ ] `cargo test --workspace` green and `cargo clippy --workspace --all-targets -- -D warnings` clean.
