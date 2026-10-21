# 12: Real full-match end-to-end test through the new API

**What to build:** One `#[ignore]`d `--release` test drives a real full match through the new API the way a client would: start with `POST`, poll the manifest until it is ready, fetch every segment, and verify each segment's SHA-256 against the manifest. The test uses the same composition `sim-server serve` builds (real runner, loaded fixtures, new router), not a test-only assembly, and measures the first-frame path. It follows the existing convention of keeping full-match tests out of the default suite.

**Blocked by:** 

- 11 - Hard cut of the old SSE path

**Status:** done

- [x] The test starts a real m-0001 with `POST`, polls with `Retry-After` until ready, then fetches every listed segment.
- [x] Every decoded segment hash matches the manifest.
- [x] Two runs in separate processes produce identical hashes.
- [x] The test prints total gzipped bytes and elapsed time, and the pull request description records them against the 5 MB budget.
- [x] It is `#[ignore]`d and documented as a `--release` run.
- [x] The app under test is built by the same constructor the `serve` command uses.
- [x] The test reports the time from a ready match to the manifest plus first segment being received (cache hit, local), compared with the 1 s target.
- [x] The fetched manifest and the first segment are written out as sample payloads the frontend can build against.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).

## Comments

Implemented in the ticket-12 commit (see the patch).

### What landed

- **A real end-to-end test**
  (`crates/sim-server/tests/real_full_match_e2e.rs`) spawns the actual
  `football-sim serve` binary twice. Each run `POST`s `/match/m-0001/start`,
  polls `GET /manifest` with the server's `Retry-After`, fetches each listed
  segment with `Accept-Encoding: gzip`, gunzips it, and compares its SHA-256
  with the manifest. It is `#[ignore]`d and documented for a `--release` run.
- **The production constructor is shared**
  (`crates/sim-server/src/app.rs`): `app::match_service` builds the loaded
  fixtures, the real `SimulationRunner` and the cap; `dispatch::serve` now
  calls it, and the spawned `serve` process the test drives therefore uses
  exactly the production assembly, not a test-only one.
- **Two processes are compared byte for byte**: build id, every segment hash,
  the final state hash, the manifest body and the first segment body must all
  match between the two runs.
- **Measured reporting**: the test prints the actual gzipped bytes against the
  5,000,000-byte budget, total elapsed time, and the time from the request
  that first sees `ready` to the manifest plus first segment being decoded,
  with a `PASS`/`FAIL` verdict against the 1 s target.
- **Sample payloads**: each run writes the fetched manifest and decoded first
  segment to `docs/api-samples/m-0001-manifest.json` and
  `docs/api-samples/m-0001-segment-0.json`, with a README explaining how to
  regenerate them.

### Measured result (release, local loopback)

Run with:

```text
cargo test --release -p sim-server --test real_full_match_e2e -- --ignored --nocapture
```

```text
run 1 of 2: 110 segments, 6,500,009 gzipped bytes (6.20 MiB) in 20.43 s
run 1 of 2: manifest lists 6,500,009 gzipped bytes; OVER budget by 1,500,009 bytes against 5,000,000 (5 MB)
run 1 of 2: ready -> manifest + first segment in 52.07 ms (target 1000 ms) — PASS
run 2 of 2: 110 segments, 6,500,009 gzipped bytes (6.20 MiB) in 20.84 s
run 2 of 2: ready -> manifest + first segment in 22.70 ms (target 1000 ms) — PASS
determinism: build_id=ac98bf108118630289bc0e425f34286d37ea075c72b209956295f555aec36706 segments=110 state_hash=17386774765339670369
both processes produced identical payloads and hashes
```

The 6.5 MB transfer is 30% over the 5 MB budget, matching ticket 04's
render-level measurement. The wire-format revisit trigger is therefore
already met; this ticket records it and changes no wire format.

### Test results

```text
cargo test -p sim-server                       →  113 passed; 0 failed; 7 ignored
cargo clippy -p sim-server --all-targets       →  no warnings in the changed files
```
