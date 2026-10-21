# 09: Serve immutable segments with gzip negotiation

**What to build:** A client can fetch each segment of a ready match from its versioned URL. Responses are immutable and cacheable by the browser: `Cache-Control: private, max-age=31536000, immutable` (private, not public, until a CDN exists) and `Vary: Accept-Encoding`. When the client accepts gzip it receives the stored gzipped bytes with `Content-Encoding: gzip`; otherwise it receives the decompressed bytes. A segment of a match that is not ready, or that the server has forgotten after a restart, answers 409; an unknown match, out-of-range segment or wrong build id answers 404.

**Blocked by:** 

- 08 - POST start and GET manifest with the full status matrix

**Status:** done

- [x] A ready segment returns 200 with the required cache headers and `Vary: Accept-Encoding`.
- [x] With gzip accepted the body is the stored gzipped bytes; otherwise it is decompressed bytes; in both cases the decoded SHA-256 equals the manifest hash.
- [x] A not-ready or restarted match returns 409; unknown id, bad segment number or wrong build id returns 404.
- [x] Tests go through `router()` with `oneshot` and the fake runner, covering both encodings and every error row.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).

## Comments

Implemented in commit `86b3929` ("feat(sim-server): serve immutable segments with gzip negotiation").

### What landed

A third route beside the start and manifest routes
(`crates/sim-server/src/http/service_api/segments.rs`):

```
GET /match/{match_id}/segments/{build_id}/{number}.json
```

- **The 200** carries `Cache-Control: private, max-age=31536000, immutable`
  (private, not public, until a CDN exists) and `Vary: Accept-Encoding`. The
  bytes behind the URL never change, because the build id is a digest of the
  rendered content.
- **The coding is negotiated per request**: a client that accepts gzip gets
  the stored gzip bytes with `Content-Encoding: gzip`; every other client
  gets the decompressed bytes. Both decode to the same body, whose SHA-256
  is the hash the manifest publishes, so the URL means the same thing either
  way. The gzip bytes are served without a copy — the stored body is shared
  and never mutated, so the response points at it.
- **The status matrix**: 404 for an unknown match id, a wrong build id or an
  unknown segment number; 409 for a match that is not ready (never started,
  queued, running, failed, or forgotten by a restart); 200 for a ready
  segment. The route never starts work.
- **The negotiation follows RFC 9110 §12.5.3**: gzip is served only when the
  client names it with a quality above zero, or names the wildcard while gzip
  itself is not named. An explicit `gzip;q=0` refuses gzip even beside a
  wildcard.
- The errors are `no-store`, so they name no coding; the `Vary` header is
  appended rather than inserted, so another layer may add its own condition
  without losing it.

### Evidence per criterion

| Criterion | Test | File |
|---|---|---|
| A ready segment returns 200 with the required cache headers and `Vary: Accept-Encoding` | `a_ready_segment_is_served_with_the_immutable_cache_headers` — asserts the exact `Cache-Control` string, `Vary: Accept-Encoding` and the JSON content type | `crates/sim-server/src/http/service_api/tests/segments.rs:125` |
| With gzip accepted the body is the stored gzipped bytes; otherwise it is decompressed bytes; the decoded SHA-256 equals the manifest hash in both cases | `a_gzip_client_gets_the_stored_gzipped_bytes` (asserts `Content-Encoding: gzip`, the body is byte-identical to the stored bytes, and the gunzipped body's SHA-256 equals the manifest's hash) and `a_client_without_gzip_gets_the_decompressed_bytes` (asserts no `Content-Encoding`, the body is the decompressed bytes, and its SHA-256 equals the manifest's hash) | `crates/sim-server/src/http/service_api/tests/segments.rs:157`, `:188` |
| A not-ready or restarted match returns 409 | `a_match_that_was_never_started_is_409` (a restarted server forgets the match the same way), `a_running_match_is_409`, `a_failed_match_is_409` | `crates/sim-server/src/http/service_api/tests/segments.rs:235`, `:259`, `:286` |
| Unknown id, bad segment number or wrong build id returns 404 | `an_unknown_match_is_404`, `a_wrong_build_id_is_404`, `an_out_of_range_segment_is_404`, `a_bad_segment_number_is_404` | `crates/sim-server/src/http/service_api/tests/segments.rs:311`, `:330`, `:351`, `:373` |
| The tests go through the router with `oneshot` and the fake runner, covering both encodings and every error row | every case drives `router_with_service` with `tower::ServiceExt::oneshot`; the fake runners are the `InstantRunner`, `GatedRunner` and `TickCapFailureRunner` from `test_support`; no socket is opened | `crates/sim-server/src/http/service_api/tests/segments.rs` |

`a_client_that_refuses_gzip_gets_the_decompressed_bytes`
(`crates/sim-server/src/http/service_api/tests/segments.rs:214`) adds the
`gzip;q=0` case, and three unit tests in the
module itself pin the negotiation's q-value semantics:
`gzip_is_served_only_when_the_client_asks_for_it`,
`gzip_is_not_served_when_the_client_refuses_it` and
`an_explicit_gzip_entry_wins_over_the_wildcard`
(`crates/sim-server/src/http/service_api/segments.rs:254`, `:265`, `:276`).

### The manual smoke check (from the commit message)

```
cargo build --release -p sim-server
./target/release/football-sim serve --bind 127.0.0.1:18080
curl -i -X POST http://127.0.0.1:18080/match/m-0001/start
# HTTP/1.1 202 Accepted
# cache-control: no-store
# retry-after: 1
# {"state":"running","match_id":"m-0001","retry_after_ms":1000}

# poll the manifest until it answers 200
curl -s http://127.0.0.1:18080/match/m-0001/manifest > manifest.json
URL=$(jq -r '.segments[0].url' manifest.json)

curl -i -H "Accept-Encoding: gzip" "http://127.0.0.1:18080$URL"
# HTTP/1.1 200 OK
# content-type: application/json
# cache-control: private, max-age=31536000, immutable
# vary: Accept-Encoding
# content-encoding: gzip
# content-length: 64999

curl -s -H "Accept-Encoding: gzip" "http://127.0.0.1:18080$URL" | gunzip | sha256sum
# 45e0153a7bfacd6a75d76eccf7ec5e584941ff8fa8251c531d823f7bb2ccc464  -
# — the manifest's hash

curl -s "http://127.0.0.1:18080$URL" | sha256sum
# 45e0153a7bfacd6a75d76eccf7ec5e584941ff8fa8251c531d823f7bb2ccc464  -
# — the same body, uncompressed (605346 bytes against 64999 gzipped)

curl -i "http://127.0.0.1:18080/match/nope/segments/$BUILD/0.json"
# HTTP/1.1 404 Not Found
# {"state":"unknown","match_id":"nope","message":"unknown match_id"}

curl -i "http://127.0.0.1:18080/match/m-0001/segments/0000.../0.json"
# HTTP/1.1 404 Not Found
# {"state":"no_segment","match_id":"m-0001","message":"unknown build id"}

curl -i "http://127.0.0.1:18080/match/m-0001/segments/$BUILD/9999.json"
# HTTP/1.1 404 Not Found
# {"state":"no_segment","match_id":"m-0001","message":"unknown segment number"}

curl -i "http://127.0.0.1:18080/match/m-0002/segments/$BUILD/0.json"
# HTTP/1.1 409 Conflict
# {"state":"not_ready","match_id":"m-0002","message":"match not ready; read the manifest before its segments"}
```

A restarted server was checked too: it answers 409 for the match it has
forgotten.

### Test results

```
cargo test -p sim-server --lib    →  136 passed; 0 failed; 8 ignored
```
