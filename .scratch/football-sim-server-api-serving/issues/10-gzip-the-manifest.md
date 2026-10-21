# 10: Gzip the manifest

**What to build:** The manifest is ~36 KB of JSON and is served uncompressed.
Every client that fetches it pays for 36 KB on the wire, and a client that
revalidates pays it on every poll interval. Segments are already stored gzipped
and negotiated per request; give the manifest the same treatment. A client that
accepts gzip gets the compressed bytes with `Content-Encoding: gzip`; every other
client gets the same JSON it gets today. Both decode to the same body, so the
manifest means the same thing either way.

**The validator scheme, settled.** Gzipping puts two representations behind one
URL, and a single `ETag` cannot correctly validate both: a cache holding the
gzip representation must not be told it is fresh against the identity one. So
each representation carries its own **strong validator**:

| Representation | `ETag` |
|---|---|
| identity (the client's request did not accept gzip) | `"<build_id>"` |
| gzip (the client's request accepted gzip) | `"<build_id>-gzip"` |

A `304` is decided against the representation the client would actually receive,
so a gzip client revalidating with `"<build_id>"` gets the full gzipped body, not
a 304. This is wire-compatible: the identity validator is byte-for-byte what the
server sends today, and the build id is unchanged.

**Blocked by:** 08 (the shared negotiation), 03 (the manifest route serves the
published bytes), 04 (the manifest route is the one that answers `304`).

**Status:** ready-for-agent

- [ ] A `GET /manifest` whose `Accept-Encoding` accepts gzip answers 200 with `Content-Encoding: gzip`, `Vary: Accept-Encoding`, `ETag: "<build_id>-gzip"` and `Cache-Control: no-cache`.
- [ ] A `GET /manifest` whose request does not accept gzip answers 200 with the identity bytes, no `Content-Encoding`, `Vary: Accept-Encoding` and `ETag: "<build_id>"` — the validator and the bytes it had before this ticket.
- [ ] Both representations decode to the same JSON document, byte for byte once decompressed.
- [ ] A conditional request is answered against the representation it would receive: `Accept-Encoding: gzip` plus `If-None-Match: "<build_id>-gzip"` → `304`, empty body, `ETag: "<build_id>-gzip"`, `Cache-Control: no-cache`.
- [ ] A gzip client that sends `If-None-Match: "<build_id>"` (the identity validator) does **not** get a 304 — it gets the full gzipped body, because the identity validator does not describe the gzip representation.
- [ ] A client that sends no `Accept-Encoding` and `If-None-Match: "<build_id>"` gets `304`, exactly as ticket 04 established.
- [ ] The manifest's `build_id` is unchanged: gzipping the response never changes a build id or a published URL, because identity is still the uncompressed body.
- [ ] Both representations are produced once, at render time, alongside the published segment bytes; a request never compresses or decompresses a manifest on the fly.
- [ ] `202`, `409`, `404`, `500` and `503` answers are unchanged and stay `no-store`; they never carry `Content-Encoding`, `Vary` or an `ETag`.
- [ ] The matrix test's "ETag equals build id" assertion is updated for the gzip case only, and the identity case still asserts `"<build_id>"` exactly.
- [ ] PRD §10.5 records the two per-representation validators and that the manifest negotiates gzip; PRD §10.6 records that the manifest, like a segment, is served gzipped to a client that accepts it, with `Vary: Accept-Encoding`.
- [ ] The HTTP tests cover: gzip 200, identity 200, gzip 304, cross-validator 200, identity 304, and `gzip;q=0` falling back to the identity representation.
- [ ] `cargo test --workspace` green and `cargo clippy --workspace --all-targets -- -D warnings` clean.

## How to execute

1. In `PublishedMatch::publish`
   (`crates/sim-server/src/api/render/published.rs`), compress the manifest body
   next to the identity bytes using `render::gzip::compress`, and add a
   `gzipped_manifest_body()` accessor beside `manifest_body()`.
2. In `manifest_response` (`crates/sim-server/src/http/service_api.rs`), call the
   shared `accepts_gzip` (ticket 08) to pick the representation. Set
   `Vary: Accept-Encoding` on both, `Content-Encoding: gzip` only on the gzip
   one, and choose the `ETag` from the representation:
   `format!("\"{}-gzip\"", build_id)` or `format!("\"{}\"", build_id)`.
3. Extend the `304` path added by ticket 04 so it compares the client's
   `If-None-Match` against the validator of the representation the request would
   receive, not against the identity validator alone.
4. Add the six cases listed above to the manifest route's HTTP tests, reading the
   gzipped body back through a `GzDecoder` to prove both representations decode
   to the same document.
5. Update PRD §10.5 and §10.6.
6. Run `cargo test -p sim-server`, then the ignored release end-to-end test to
   regenerate `docs/api-samples/` — the sample payloads are the identity
   representation and should be unchanged.

## Comments

**Why the validators are not one ETag.** A `304` is a claim about the bytes the
client would receive. If a cache stored the gzip body under `"<build_id>"`, a
later identity request could be answered 304 with bytes that do not match the
validator. Two representations behind one URL therefore need two validators, and
the client's `Vary`-aware cache keeps them apart.

**The suffix is derived, not stored.** `"<build_id>-gzip"` is built from the same
build id as the identity validator, so the two can never disagree about which
build they describe. A weak validator (`W/`) is not used: these are strong
digests, so there is nothing to weaken.

**Depends on 08, not just 03/04.** The `Accept-Encoding` parser is private to the
segment route until ticket 08 lifts it.