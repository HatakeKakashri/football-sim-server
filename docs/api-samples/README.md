# API sample payloads

Real payloads fetched through the HTTP API by the server-API ticket 12
end-to-end test. The frontend can build and test against these without
running a match.

| File | What it is |
|---|---|
| `m-0001-manifest.json` | The exact `200` body of `GET /match/m-0001/manifest` once the match is ready. It is the manifest plus `"state": "ready"` and carries the build id, result, roster and every segment row. It was fetched without `Accept-Encoding`, so it is the identity representation; a client that accepts gzip receives the gzip coding of the same document, with `ETag: "<build_id>-gzip"` instead of `"<build_id>"`. |
| `m-0001-segment-0.json` | The decoded body of `GET /match/m-0001/segments/{build_id}/0.json` (the first segment: half 1, minutes 0–1). The response was fetched with `Accept-Encoding: gzip` and gunzipped, so this is the JSON the client parses. |

Both files are tied to the committed `fixtures/matches/m-0001.toml` seed
(`20240101`). Regenerating them with a changed fixture or engine replaces the
build id and may change the payloads.

## Regenerate

Full-match tests are excluded from the default suite. Run in release:

```text
cargo test --release -p sim-server --test real_full_match_e2e -- --ignored --nocapture
```

The test starts the real `football-sim serve` app in two separate processes,
polls with `Retry-After` until the match is ready, fetches and verifies every
segment against the manifest's SHA-256, and compares the two runs. It rewrites
these two sample files at the end of each run and prints the gzipped byte
total, the elapsed time, and the ready-to-first-segment time against the 1 s
target.

The current `m-0001` run has 110 segments and 6,500,009 gzipped bytes
(6.20 MiB), 1,500,009 bytes over the 5 MB transfer budget. The first segment
arrives about 36–52 ms after the ready manifest on a local cache-hot run. See
ticket 12 for the recorded measurement.
