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

**Status:** done

- [x] A `GET /manifest` whose `Accept-Encoding` accepts gzip answers 200 with `Content-Encoding: gzip`, `Vary: Accept-Encoding`, `ETag: "<build_id>-gzip"` and `Cache-Control: no-cache`.
- [x] A `GET /manifest` whose request does not accept gzip answers 200 with the identity bytes, no `Content-Encoding`, `Vary: Accept-Encoding` and `ETag: "<build_id>"` — the validator and the bytes it had before this ticket.
- [x] Both representations decode to the same JSON document, byte for byte once decompressed.
- [x] A conditional request is answered against the representation it would receive: `Accept-Encoding: gzip` plus `If-None-Match: "<build_id>-gzip"` → `304`, empty body, `ETag: "<build_id>-gzip"`, `Cache-Control: no-cache`.
- [x] A gzip client that sends `If-None-Match: "<build_id>"` (the identity validator) does **not** get a 304 — it gets the full gzipped body, because the identity validator does not describe the gzip representation.
- [x] A client that sends no `Accept-Encoding` and `If-None-Match: "<build_id>"` gets `304`, exactly as ticket 04 established.
- [x] The manifest's `build_id` is unchanged: gzipping the response never changes a build id or a published URL, because identity is still the uncompressed body.
- [x] Both representations are produced once, at render time, alongside the published segment bytes; a request never compresses or decompresses a manifest on the fly.
- [x] `202`, `409`, `404`, `500` and `503` answers are unchanged and stay `no-store`; they never carry `Content-Encoding`, `Vary` or an `ETag`.
- [x] The matrix test's "ETag equals build id" assertion is updated for the gzip case only, and the identity case still asserts `"<build_id>"` exactly.
- [x] PRD §10.5 records the two per-representation validators and that the manifest negotiates gzip; PRD §10.6 records that the manifest, like a segment, is served gzipped to a client that accepts it, with `Vary: Accept-Encoding`.
- [x] The HTTP tests cover: gzip 200, identity 200, gzip 304, cross-validator 200, identity 304, and `gzip;q=0` falling back to the identity representation.
- [x] `cargo test --workspace` green and `cargo clippy --workspace --all-targets -- -D warnings` clean.

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

### Done

**Both representations are published.** `PublishedMatch` now carries the
manifest body *and* its gzip coding (`gzipped_manifest_body()`), compressed
once at render time next to the identity bytes with the same
`render::gzip::compress` the segments use. A request picks between two sets of
bytes that already exist: nothing compresses or decompresses a manifest on the
fly, and the identity body is byte-for-byte what the server sent before this
ticket.

**The manifest route owns the representations.** The new
`http::service_api::manifest` module holds the whole rule: the coding is
negotiated by the shared `content_negotiation::accepts_gzip` (ticket 08), the
`200` carries `Vary: Accept-Encoding` on both representations and
`Content-Encoding: gzip` on the gzip one, and the `304` is decided against the
validator of the representation the request would receive. The validator is
derived from the build id — `"<build_id>"` for the identity body,
`"<build_id>-gzip"` for the coding — so the two can never disagree about which
build they describe, and the build id itself is unchanged.

**The start route serves the same two representations.** `status_answer` now
takes the request's headers and a `Revalidation` flag instead of
`Option<&HeaderMap>`: the flag says whether the route may answer a `304` (only
a `GET` may, PRD §10.1), while the headers decide the coding on either route.
A client that accepts gzip therefore gets the gzip coding from
`POST /start` too, with the validator that describes it — the `ETag` always
names the bytes the client actually received.

**The proof is in the tests.** `tests/manifest_codings.rs` covers the six
cases: gzip `200`, identity `200`, gzip `304`, the cross-validator `200` (a
gzip client naming the identity validator gets the whole body), the identity
`304`, and `gzip;q=0` falling back to the identity representation. Each case
reads the body back through a `GzDecoder` and compares it with the published
identity body, so the two representations are proved to decode to the same
document. The matrix test gained a gzip row and now asserts the validator of
the representation per row, plus that every answer without a manifest carries
no `ETag` and names no representation.

The rows of the matrix moved into `tests/matrix_table.rs` so the matrix's
assertions stay under the file-size budget.

**Verification:** `cargo clippy --workspace --all-targets -- -D warnings` is
clean. `cargo test -p sim-server` is green — 166 passed, 0 failed, 8 ignored.
The release end-to-end test passes and leaves `docs/api-samples/`
byte-identical: the sample payloads are fetched without `Accept-Encoding`, so
they are the identity representation. The 14 failures `cargo test --workspace`
reports are the ones `main` already had, in crates this ticket does not
touch.