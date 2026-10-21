# 04: Store segments pre-gzipped and report real sizes

**What to build:** Rendered segments are gzipped one at a time as they are produced, and only the gzipped bytes and the content hash are kept, so peak extra memory is about one segment rather than the whole ~46 MB uncompressed match (a synthetic estimate). A client that does not accept gzip can be served by decompressing the stored segment on demand.

An `#[ignore]`d `--release` test runs a real m-0001 match and prints raw and gzipped bytes per segment, event counts, and totals. That output is the first real measurement of the synthetic ~5.8 MB gzip estimate and of the 5 MB total-transfer budget.

**Blocked by:** 

- 03 - Cut a completed match into 60 s segments with a full manifest

**Status:** ready-for-agent

- [ ] Each segment is gzipped as rendered, and only gzipped bytes (as shared immutable bytes) plus the hash are retained.
- [ ] Decompressing a stored segment reproduces the uncompressed bytes exactly; the SHA-256 matches the manifest.
- [ ] The manifest's gzipped byte length field is populated for every segment.
- [ ] The gzip level is fixed and documented; segment identity depends on the uncompressed hash, not on the compressed bytes.
- [ ] The ignored `--release` test prints per-segment raw and gzipped bytes, per-segment event counts and totals for a real m-0001 run.
- [ ] The measured totals are recorded in the pull request description, with a note on whether they fit the 5 MB budget.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).
