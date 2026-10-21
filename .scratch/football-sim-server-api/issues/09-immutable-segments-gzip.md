# 09: Serve immutable segments with gzip negotiation

**What to build:** A client can fetch each segment of a ready match from its versioned URL. Responses are immutable and cacheable by the browser: `Cache-Control: private, max-age=31536000, immutable` (private, not public, until a CDN exists) and `Vary: Accept-Encoding`. When the client accepts gzip it receives the stored gzipped bytes with `Content-Encoding: gzip`; otherwise it receives the decompressed bytes. A segment of a match that is not ready, or that the server has forgotten after a restart, answers 409; an unknown match, out-of-range segment or wrong build id answers 404.

**Blocked by:** 

- 08 - POST start and GET manifest with the full status matrix

**Status:** ready-for-agent

- [ ] A ready segment returns 200 with the required cache headers and `Vary: Accept-Encoding`.
- [ ] With gzip accepted the body is the stored gzipped bytes; otherwise it is decompressed bytes; in both cases the decoded SHA-256 equals the manifest hash.
- [ ] A not-ready or restarted match returns 409; unknown id, bad segment number or wrong build id returns 404.
- [ ] Tests go through `router()` with `oneshot` and the fake runner, covering both encodings and every error row.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).
