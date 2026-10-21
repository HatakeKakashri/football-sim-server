# 01: Publish the rendered match as immutable bytes

**What to build:** Pure expand. The render step produces a `Manifest` value and a list of segments, and the HTTP layer re-derives the manifest's wire bytes on every single request — a deep clone of the whole manifest (110 segment rows, 22 roster rows, 429 string allocations) plus a fresh serialisation of 36 KB — even though the manifest never changes after it has been rendered. Give the render step a second output: a published match that owns the exact bytes the manifest route serves and the segment bodies indexed by segment number, so a served manifest is a copy of immutable bytes rather than a value re-derived per request. Nothing a client sees changes in this ticket: the HTTP routes still build their responses the way they do today, and the manifest's JSON, the build id and every pinned segment hash stay exactly as they are.

The published bytes are produced where the manifest is produced, not in the HTTP module: the body type that produces the manifest's response bytes moves out of `http::service_api` into the module that owns the manifest, so the wire format of a match lives in one place.

**Blocked by:** None (can start immediately).

**Status:** done

- [x] The render step returns a published match alongside the manifest and the segments, holding the manifest's response body as shared bytes and the segments in a map keyed by segment number.
- [x] The published manifest body is byte-identical to what the manifest route serves today: the same JSON object, the same key order, and the `state` field last.
- [x] Every published segment body is byte-identical to the body the segment route serves today, and its SHA-256 still equals the uncompressed hash the manifest publishes.
- [x] Looking a segment up by number returns the same segment for every number the manifest lists, and nothing for a number outside that range.
- [x] Publishing happens once per render, not once per read: rendering the same fixture and completed match twice produces byte-identical published bytes.
- [x] The published bytes are shared, never mutated, and serving one costs no copy of the underlying buffer.
- [x] The HTTP routes, the status matrix, the manifest's JSON shape, the build id and the pinned golden segment hashes are all unchanged by this ticket.
- [x] The render module's own tests cover the published bytes and the index; no HTTP test changes.
- [x] `cargo test --workspace` green and `cargo clippy --workspace --all-targets -- -D warnings` clean.

## Comments

**What landed.** The render step (`crates/sim-server/src/api/render.rs`) now returns a
third output next to the manifest and the segments: `published`, a `PublishedMatch`
that owns the manifest's answer body as shared bytes (`Arc<[u8]>`) and every
segment's body in a `BTreeMap<u32, PublishedSegment>` keyed by segment number. The
body type (`ManifestBody`) moved from `http::service_api` into the new
`crates/sim-server/src/api/render/published.rs`, next to the manifest it flattens,
so the wire format of a manifest body lives in one place; the manifest route
builds the very same body, and the bytes it serves are unchanged.

A published segment carries the uncompressed body (whose SHA-256 is the hash the
manifest publishes) and the gzip coding of it, both shared and never mutated, so a
segment can be served without copying either. The manifest's own row for a segment
stays in the manifest: the published match publishes bytes, not a second copy of
the manifest.

**Verification.** The published manifest body is pinned by a golden SHA-256 and by
the manifest's key order (`state` last). The real end-to-end test
(`cargo test --release -p sim-server --test real_full_match_e2e -- --ignored`)
fetches the manifest and every segment through the HTTP API and rewrites
`docs/api-samples/`: the committed payloads are byte-identical after the change,
with the same build id (`ac98bf10…`) and the same 110 segments.

**Two trade-offs worth knowing about.**

1. Publishing keeps the uncompressed bodies of the whole match, so the render
   step's memory is the match's bodies rather than one segment's: 137 MB against
   6.5 MB of gzip for `m-0001`. The published body has to be the body whose hash
   the manifest publishes, so the uncompressed bytes are the ones kept.
2. `ManifestBody` owns its manifest, so publishing clones the manifest once per
   render (the same clone the route makes per request today). Ticket 03 replaces
   the route's per-request clone with the published bytes.

**Out of scope, left to the later tickets.** The routes still build their
responses the way they do today (ticket 03 serves the published bytes and finds a
segment through the index), and the service still keeps only the manifest and the
segment list (ticket 02 puts the published match in the ready case).

**Also in this change.** `render.rs` was already over the 500-LoC budget before
this ticket, so the segmentation moved into `crates/sim-server/src/api/render/ranges.rs`
(a pure move, no behaviour change).
