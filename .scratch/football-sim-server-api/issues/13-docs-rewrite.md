# 13: Rewrite PRD section 10, add glossary terms, update AGENTS.md

**What to build:** Documentation matches the new API. PRD section 10 describes the endpoints, the status matrix, the 60-second segment rule per half (replacing the 36,000-tick segment invariant and the end-to-end test that asserted it), caching rules and the manifest fields. The glossary gains terms for the HTTP layer: match service, manifest, segment, build id and runner. `AGENTS.md` reflects the new test layers.

It also records guidance for the unbuilt frontend: the recovery rule when a segment returns 409 after a server restart (POST, poll the manifest until ready, retry); a sliding window of about three segments either side, parsed in a worker into typed arrays; the revisit triggers for the wire format (real gzipped total above 5 MB, or parse time or retained heap breaking the client's budget); and what is deferred until leagues (CDN with `public` caching, TTL and eviction, shared storage, fixture listing endpoint, brotli).

**Blocked by:** 

- 11 - Hard cut of the old SSE path

**Status:** ready-for-agent

- [ ] PRD section 10 describes the final endpoints, status matrix, headers and segment rule; the old SSE and 36,000-tick text is gone.
- [ ] The glossary defines match service, manifest, segment, build id and runner.
- [ ] `AGENTS.md` describes the render, service, HTTP and real-match test layers.
- [ ] The client recovery rule, retention guidance, wire-format revisit triggers and deferred items are written down.
- [ ] If ticket 07 was closed as won't-do, no 503 transient row remains in the docs.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).
