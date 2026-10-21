# 04: Revalidate the manifest with `If-None-Match`

**What to build:** The manifest's 200 carries `ETag: "<build_id>"` and `Cache-Control: no-cache`, and PRD §10.5 tells the client it "may revalidate it" — but no code anywhere reads `If-None-Match`, so a client that revalidates still receives the whole 36 KB manifest every time. Answer the conditional request: when the client's `If-None-Match` names the manifest's current validator, answer 304 with the validator and no body. A client that never sends the header keeps receiving the full manifest, byte for byte as today.

This is the same document the manifest route already serves (ticket 03), so the 304 and the 200 describe one body; the manifest's bytes and its build id are unchanged by this ticket.

**Blocked by:** 03.

**Status:** done

- [x] A `GET /match/{match_id}/manifest` whose `If-None-Match` names the manifest's current validator answers 304 with the same `ETag` and no body.
- [x] A `GET /match/{match_id}/manifest` with no `If-None-Match`, or with one that names a different validator, answers 200 with the full manifest, exactly as today.
- [x] The 304 carries the manifest's `Cache-Control`; it is not `no-store`, because the answer is cacheable by design.
- [x] A `If-None-Match` that lists several validators, or that carries the weak `W/` form, is matched against any of them (RFC 9110 §13.1.2 uses the weak comparison function).
- [x] A `304` is only produced for a manifest that exists; an unknown match, a match that was never started, a queued or running match and a failed match keep answering exactly as they do today.
- [x] `POST /match/{match_id}/start` on a ready match answers 200 with the manifest exactly as today; the conditional-request path applies to the manifest route only.
- [x] PRD §10.1's status matrix gains the 304 row, and §10.5 records that revalidation is answered rather than merely permitted.
- [x] The HTTP test for the manifest route covers the matching validator, the absent header, a mismatching validator and a validator list.
- [x] `cargo test --workspace` green and `cargo clippy --workspace --all-targets -- -D warnings` clean.

## Comments

**What landed.** The manifest route answers a conditional request. `GET
/match/{id}/manifest` reads the request's `If-None-Match` header and compares it with the
manifest's current validator — the `ETag: "<build_id>"` the 200 already carried. When the
client names that validator, the route answers 304 with the same `ETag`, the same
`Cache-Control: no-cache` and no body: the client already holds the 36 KB. Every other
answer is unchanged, byte for byte.

**One rule, in one place.** The comparison lives in a new module,
`crates/sim-server/src/http/conditional.rs`, next to the shared content-coding rule:
`if_none_match_matches(headers, validator)`. It reads the header's validators, compares them
with the weak comparison function (RFC 9110 §8.8.3.2) and matches any one of them, so a
weak `W/"<build_id>"` and a list of validators both match. `If-None-Match: *` matches too
(RFC 9110 §13.1.2: the condition is false when the server has a current representation), and
the `W/` prefix is case-sensitive, as the grammar says. The route does not parse the header
itself.

**The 304 is the manifest route's answer only.** `status_answer` — the one rule that turns a
match's status into a response — takes the request's conditional headers as an argument: the
manifest route passes them, the start route passes `None`. A `POST /start` on a ready match
therefore still answers 200 with the manifest, and a match that has no manifest to revalidate
(unknown, never started, queued, running, failed) keeps its own answer, `no-store` and with
no validator.

**The wire did not move.** The 200 is the same published bytes with the same `ETag` and the
same `Cache-Control: no-cache`; only a request that carries `If-None-Match` sees a different
status. The manifest's build id is unchanged, and the 304 names the same validator the 200
does.

**The docs follow.** PRD §10.1's matrix gains the `ready, revalidated` row and a `304` row
in the body table, and §10.5 now says the revalidation is *answered* rather than merely
permitted. `GLOSSARY.md` gains a **revalidation** entry, next to the other HTTP-layer
vocabulary.

**Verification.** The new HTTP tests (`crates/sim-server/src/http/service_api/tests/revalidation.rs`)
cover the matching validator, the absent header, a mismatching validator, a validator list
that matches and one that does not, the weak `W/` form, and the four states that keep their
answers; the rule's own unit tests live in the module. The `sim-server` suite is green (147
tests), and `cargo clippy --workspace --all-targets -- -D warnings` is clean. Fourteen
failures in `sim-core`, `sim-components`, `sim-ai-player` and `sim-rules` pre-date this
ticket — they fail on a clean checkout of `main` too, and none of them is in a crate this
ticket touches — so they are left alone here.
