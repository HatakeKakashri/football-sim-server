# 12: Real full-match end-to-end test through the new API

**What to build:** One `#[ignore]`d `--release` test drives a real full match through the new API the way a client would: start with `POST`, poll the manifest until it is ready, fetch every segment, and verify each segment's SHA-256 against the manifest. The test uses the same composition `sim-server serve` builds (real runner, loaded fixtures, new router), not a test-only assembly, and measures the first-frame path. It follows the existing convention of keeping full-match tests out of the default suite.

**Blocked by:** 

- 11 - Hard cut of the old SSE path

**Status:** ready-for-agent

- [ ] The test starts a real m-0001 with `POST`, polls with `Retry-After` until ready, then fetches every listed segment.
- [ ] Every decoded segment hash matches the manifest.
- [ ] Two runs in separate processes produce identical hashes.
- [ ] The test prints total gzipped bytes and elapsed time, and the pull request description records them against the 5 MB budget.
- [ ] It is `#[ignore]`d and documented as a `--release` run.
- [ ] The app under test is built by the same constructor the `serve` command uses.
- [ ] The test reports the time from a ready match to the manifest plus first segment being received (cache hit, local), compared with the 1 s target.
- [ ] The fetched manifest and the first segment are written out as sample payloads the frontend can build against.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).
