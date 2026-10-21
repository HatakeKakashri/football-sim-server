# 04: Revalidate the manifest with `If-None-Match`

**What to build:** The manifest's 200 carries `ETag: "<build_id>"` and `Cache-Control: no-cache`, and PRD §10.5 tells the client it "may revalidate it" — but no code anywhere reads `If-None-Match`, so a client that revalidates still receives the whole 36 KB manifest every time. Answer the conditional request: when the client's `If-None-Match` names the manifest's current validator, answer 304 with the validator and no body. A client that never sends the header keeps receiving the full manifest, byte for byte as today.

This is the same document the manifest route already serves (ticket 03), so the 304 and the 200 describe one body; the manifest's bytes and its build id are unchanged by this ticket.

**Blocked by:** 03.

**Status:** ready-for-agent

- [ ] A `GET /match/{match_id}/manifest` whose `If-None-Match` names the manifest's current validator answers 304 with the same `ETag` and no body.
- [ ] A `GET /match/{match_id}/manifest` with no `If-None-Match`, or with one that names a different validator, answers 200 with the full manifest, exactly as today.
- [ ] The 304 carries the manifest's `Cache-Control`; it is not `no-store`, because the answer is cacheable by design.
- [ ] A `If-None-Match` that lists several validators, or that carries the weak `W/` form, is matched against any of them (RFC 9110 §13.1.2 uses the weak comparison function).
- [ ] A `304` is only produced for a manifest that exists; an unknown match, a match that was never started, a queued or running match and a failed match keep answering exactly as they do today.
- [ ] `POST /match/{match_id}/start` on a ready match answers 200 with the manifest exactly as today; the conditional-request path applies to the manifest route only.
- [ ] PRD §10.1's status matrix gains the 304 row, and §10.5 records that revalidation is answered rather than merely permitted.
- [ ] The HTTP test for the manifest route covers the matching validator, the absent header, a mismatching validator and a validator list.
- [ ] `cargo test --workspace` green and `cargo clippy --workspace --all-targets -- -D warnings` clean.
