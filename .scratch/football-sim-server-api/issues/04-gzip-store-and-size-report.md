# 04: Store segments pre-gzipped and report real sizes

**What to build:** Rendered segments are gzipped one at a time as they are produced, and only the gzipped bytes and the content hash are kept, so peak extra memory is about one segment rather than the whole ~46 MB uncompressed match (a synthetic estimate). A client that does not accept gzip can be served by decompressing the stored segment on demand.

An `#[ignore]`d `--release` test runs a real m-0001 match and prints raw and gzipped bytes per segment, event counts, and totals. That output is the first real measurement of the synthetic ~5.8 MB gzip estimate and of the 5 MB total-transfer budget.

**Blocked by:** 

- 03 - Cut a completed match into 60 s segments with a full manifest

**Status:** done

- [x] Each segment is gzipped as rendered, and only gzipped bytes (as shared immutable bytes) plus the hash are retained.
- [x] Decompressing a stored segment reproduces the uncompressed bytes exactly; the SHA-256 matches the manifest.
- [x] The manifest's gzipped byte length field is populated for every segment.
- [x] The gzip level is fixed and documented; segment identity depends on the uncompressed hash, not on the compressed bytes.
- [x] The ignored `--release` test prints per-segment raw and gzipped bytes, per-segment event counts and totals for a real m-0001 run.
- [x] The measured totals are recorded in the pull request description, with a note on whether they fit the 5 MB budget.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).

## Comments

Implemented in commit `240003a` ("feat(sim-server): store the rendered segments pre-gzipped").

### What landed

- **A gzip module** (`crates/sim-server/src/api/render/gzip.rs`): `compress`,
  `decompress` and a fixed `GZIP_LEVEL` of 6, on the pure-Rust backend, so the
  stored bytes never depend on the platform's zlib.
- **`StoredSegment::render`** compresses each segment as it is produced and
  drops the uncompressed body immediately: the step's extra memory is about
  one segment rather than the whole match. Only the gzip bytes (in an
  `Arc<[u8]>`, shared between clones and never mutated) and the uncompressed
  SHA-256 are kept.
- **`RenderedSegment::decompress`** reproduces the uncompressed body on
  demand, for a client that does not accept gzip.
- **The manifest's `gzipped_bytes`** is populated for every segment, so the
  transfer cost of a match is known from the manifest alone.

### Evidence per criterion

| Criterion | Test | File |
|---|---|---|
| Each segment is gzipped as rendered; only shared gzip bytes plus the hash are kept | `the_stored_bytes_are_a_gzip_stream_at_the_fixed_level` (magic bytes, and the stored bytes equal a fresh `compress` of the body) and `the_stored_bytes_are_shared_between_clones` (`Arc::ptr_eq`) | `crates/sim-server/src/api/render/gzip_tests.rs:120`, `:183` |
| Decompressing reproduces the uncompressed bytes exactly; the SHA-256 matches the manifest | `a_stored_segment_decompresses_to_its_uncompressed_body` (length and segment number) and `the_manifest_hash_is_the_hash_of_the_uncompressed_body` — the digest is taken with `sha2` directly, not with the render step's own helper, so the check does not lean on the code it checks | `crates/sim-server/src/api/render/gzip_tests.rs:66`, `:88` |
| The gzipped length field is populated for every segment | `the_manifest_lists_the_gzipped_byte_length_of_every_segment` | `crates/sim-server/src/api/render/gzip_tests.rs:104` |
| The gzip level is fixed and documented; identity does not depend on the compressed bytes | `the_stored_bytes_are_a_gzip_stream_at_the_fixed_level` asserts `GZIP_LEVEL == 6`; `the_segment_identity_does_not_depend_on_the_compressed_bytes` compresses the same body at level 9, shows the bytes differ and the hash does not, and recomputes the build id from the uncompressed hashes alone | `crates/sim-server/src/api/render/gzip_tests.rs:120`, `:142` |
| The ignored `--release` test prints the per-segment and total figures | `a_real_match_reports_its_gzipped_size` — `#[ignore]`d, prints raw and gzipped bytes, the ratio and the event count per segment, then the totals and the budget verdict | `crates/sim-server/src/api/render/gzip_tests.rs:206` |
| The measured totals are recorded, with a note on the 5 MB budget | the commit message records the measurement (below) | commit `240003a` |

### The measurement

Run with `cargo test --release -p sim-server --lib -- --ignored a_real_match_reports --nocapture`
on a real m-0001 match (110 segments, 110 minutes of match time, 389,896 events):

```
raw total:  137,285,784 bytes (137.2 MB)
gzip total: 6,500,009 bytes (6.5 MB)
ratio:      4.7%
events:     389,896
```

**The match does not fit the 5 MB budget** (5,000,000 bytes): 6.5 MB is 1.5 MB,
30% over. The synthetic estimates this ticket started from (~46 MB raw, ~5.8 MB
gzip) were both low — the uncompressed match is three times the estimate and
the gzipped match 12% over it. The wire-format revisit trigger is therefore
met; the fix belongs to the wire format, not to the storage, so this ticket
records the measurement and leaves the wire format alone. The same figures
are recorded in `GLOSSARY.md` under **transfer budget**.

### Test results

```
cargo test -p sim-server --lib    →  136 passed; 0 failed; 8 ignored
```
