# Architectural review: HTTP deepening (report)

**Date:** 2026-10-22
**Scope:** `crates/sim-server` HTTP surface — the three match-service routes, the
lifecycle read behind them, and the wire representations of the manifest and
segments.
**Status:** reviewed; tickets merged into this project (see "Ticket map" below).

## What the review found

Three independent candidates, each traced through the code:

1. **The manifest is re-derived per request.** `manifest_response`
   (`crates/sim-server/src/http/service_api.rs:291`) deep-clones the whole
   manifest and re-serialises 36 KB on every request, even though the bytes never
   change after render.
2. **The status rule is written several times.** `state()` answers `NotStarted`
   for an id that is not a fixture, so each route calls `is_known()` and then
   `state()` — two calls, two lock acquisitions, and the 404-vs-409 rule living
   in the HTTP layer rather than in the service that owns it. The 404 body is
   duplicated verbatim in `service_api.rs` and `segments.rs`.
3. **Content-coding negotiation is trapped in the segment route.** The
   q-value `Accept-Encoding` parser (`accepts_gzip` / `parse_coding`,
   `segments.rs:158-200`) is private to that route, so the manifest cannot reuse
   it for gzip without a copy.

## Ticket map (how this review merged into the tracker)

The review drafted eight tickets. Five were already covered by existing tickets
in this project; three are new. Existing numbering `01`–`07` was preserved so
the cross-references already written into those files stay valid.

| Review ticket | Merged into | How |
|---|---|---|
| 01 Conditional GET on the manifest | **04** (existing) | Kept the existing ticket: it is strictly richer (weak comparison per RFC 9110 §13.1.2, validator lists, `POST /start` explicitly excluded, PRD §10.1/§10.5 update). The review's simpler variant is superseded. |
| 02 Share content-coding negotiation | **08** (new) | Pure prefactor, no behaviour change. |
| 03 Lifecycle read: unknown ≠ not started; segment route migrated | **02** (existing) | Already covered: 02 adds the single status answer with an explicit "unknown fixture" case; 03 migrates every route off `is_known()`/`state()`. |
| 04 Manifest/start routes migrated; duplicates deleted | **03** (existing) | Already covered; this review added two acceptance criteria (see "Updates to existing tickets" below). |
| 05 Ready match resolves its own segments | **03** (existing) | Covered by "the segment route finds its segment through the published index"; the review added the explicit relocation of the build-id check and `.json` parse. |
| 06 Artifact URL scheme defined once | **09** (new) | Not previously covered. |
| 07 Manifest rendered once, served as stored bytes | **01** (done) + **03** | 01 produced the published bytes; 03 makes the route serve them. Both closed. |
| 08 Gzip the manifest | **10** (new) | Not previously covered. |

### Decisions settled in this review

- **ETag scheme for the gzipped manifest (ticket 10).** One URL will carry two
  representations, so one `ETag` cannot correctly validate both. Adopted: a
  **strong validator per representation** — `"<build_id>"` for the identity
  representation and `"<build_id>-gzip"` for the gzip representation. A 304 is
  decided against the representation the client would actually receive. This is
  wire-compatible (the identity validator is unchanged from today) and it replaces
  ticket 04's "ETag equals build id" invariant only for the gzip case.
- **Ticket granularity.** Left as-is. 03 and 04 stay split (expand then
  contract) because 02 is a pure expand that keeps the HTTP suite green on its
  own, and 03 is the contract step that can be verified against the unchanged
  status matrix. 07 and 08 are already closed by 01 + 03, so no equivalent pair
  survives. Nothing needed splitting further.

### Frontier

- **Ready now:** 05 (roster index in frames), 08 (content-coding negotiation).
- **Next:** 02 → 03 → 04.
- **Later:** 09 (artifact URL), 10 (gzip manifest) — 10 also waits on 08.

## Updates made to existing tickets

`03` gained two acceptance criteria this review surfaced:

- The `Instant` import stays inside the lifecycle module: the HTTP layer must
  not compute `retry_at.saturating_duration_since(Instant::now())` itself, so
  "time left before a retry" is the service's answer, not the route's arithmetic.
- The build-id check, the `.json` suffix parse and the segment lookup move out
  of the segment handler into the resolution operation, so the handler keeps only
  negotiation and headers.

## What this review does not cover

- Offense detection, foul detection and the §10.8 deferred endpoints — out of
  scope for the HTTP layer.
- The wire-format work in tickets 05–07 (roster indices, delta encoding). Those
  change the segment bodies and move the transfer budget; this review is about
  how the server *serves* what it has already rendered.