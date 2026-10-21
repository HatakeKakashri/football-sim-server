# 10: Split transport config from store config and harden the edges

**What to build:** Transport settings live in one `HttpConfig` (bind address, API token, CORS origins), separate from the service's policy settings (cap, tick cap), so the two can no longer diverge and fixtures stop loading twice in the serve path. The API token comparison becomes constant-time, and the CORS reflection adds `Vary: Origin`. Responses also expose `Retry-After` and `ETag` to browser scripts through `Access-Control-Expose-Headers`, because a cross-origin `fetch` can otherwise not read them, and the preflight allows the headers the client sends.

**Blocked by:** 

- 08 - POST start and GET manifest with the full status matrix

**Status:** ready-for-agent

- [ ] One `HttpConfig` carries bind, token and CORS origins; the service config holds only policy settings.
- [ ] Fixtures load once in the serve path.
- [ ] The token comparison is constant-time and has a test for accept and reject.
- [ ] Responses from the CORS middleware include `Vary: Origin`, with a test.
- [ ] The new router tests run through the shared middleware.
- [ ] Responses for an allowed origin include `Access-Control-Expose-Headers` listing `Retry-After` and `ETag`, with a test.
- [ ] A preflight for `POST` with an `Authorization` header from an allowed origin succeeds, with a test.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).
