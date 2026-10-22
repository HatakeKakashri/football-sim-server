# 09: Define the artifact URL scheme once

**What to build:** The segment URL is described in three places that have to stay
in step and are not checked against each other:

- `segment_url` in `crates/sim-server/src/api/render.rs:414` **builds** it, as a
  `format!` string, and writes it into every `segments[].url` the manifest
  publishes;
- the route pattern in `crates/sim-server/src/http/service_api.rs:163`
  **routes** it, as a second string literal;
- `segment_number` in
  `crates/sim-server/src/http/service_api/segments.rs:106` **parses** it, by
  stripping `.json` and parsing the digits — and it is the only place that knows
  the `.json` suffix is mandatory.

Nothing proves those three agree. If the suffix or the path order changed in one
place, the manifest would publish URLs the server refuses to route, and no test
would fail until a client followed one. Define the scheme once — one place that
owns the pattern, can build a URL and can parse a path back into its parts — and
make the builder, the router and the parser all read from it. The URLs themselves
do not change: `/match/{match_id}/segments/{build_id}/{number}.json` stays
exactly as published today, so no build id and no pinned hash moves.

The proof is a round trip: every URL a rendered manifest publishes is fetched
through the real router and comes back as the segment whose `sha256` the manifest
says it has.

**Blocked by:** 03 (the segment route reads through the published index and the
collapsed status matrix, so the parser has one home to move into).

**Status:** done

- [x] One definition owns the segment URL scheme: the path pattern, the `.json` suffix, and the order of the parts.
- [x] The manifest's `segments[].url` is produced by that definition's builder, not by a `format!` literal in `render.rs`.
- [x] The router's route pattern is derived from the same definition, so the route and the published URL cannot drift.
- [x] The segment path is parsed by that definition's parser into `(match_id, build_id, segment_number)`, and the handler no longer strips `.json` or parses digits itself.
- [x] The URLs are unchanged: `/match/{match_id}/segments/{build_id}/{number}.json` for every row of every manifest, so no build id, no `segments[].url` value and no pinned golden segment hash moves.
- [x] A path that is not a segment URL — no `.json` suffix, a non-numeric segment, an extra path segment — is still rejected with `404 no_segment`, exactly as today.
- [x] Round-trip test: for a ready match, every URL in the rendered manifest is requested through the real router (`tower::ServiceExt::oneshot`), answers `200`, and the decoded body hashes to the `sha256` that manifest row publishes.
- [x] The round-trip test covers every segment of the match, not a sample, so an off-by-one in the numbering cannot hide.
- [x] A test fails if the builder and the route are changed apart: the definition owns both, so there is one place to change, and a change that moves only one of them does not compile.
- [x] `cargo test --workspace` green and `cargo clippy --workspace --all-targets -- -D warnings` clean.

## How to execute

1. Add a `SegmentUrl` type next to the other URL concerns — most likely in the
   render module (it publishes the URLs) or a small `http/urls.rs` if the router
   should own it. Give it `build(match_id, build_id, number) -> String`, a
   `route_pattern()` that yields the axum path pattern, and
   `parse(&str) -> Option<SegmentPath>`.
2. Replace `segment_url` in `render.rs` with `SegmentUrl::build(..)`.
3. Replace the route literal in `service_api.rs::routes` with the pattern from
   `SegmentUrl::route_pattern()`.
4. Replace `segment_number` in `segments.rs` with `SegmentUrl::parse(..)`,
   matching on the parsed parts. Keep the existing `404 no_segment` answers.
5. Add the round-trip test to `crates/sim-server/src/http/service_api/tests/segments.rs`:
   read the ready match's `manifest.segments`, `oneshot` each `row.url`, and
   compare `hex(Sha256::digest(&decoded))` against `row.sha256`.
6. Run `cargo test -p sim-server` and the ignored real-match end-to-end test to
   confirm the committed sample payloads in `docs/api-samples/` are unchanged.

## Comments

Follows ticket 03 rather than running beside it: 03 rewrites the same handler
path, and this ticket's parser has to land where 03 leaves the handler. The
`render.rs` half is independent of 03 if you want to split the PR.

### Done

**One definition, in the render layer.** `api::render::segment_url` owns the
whole scheme: `SCHEME` is the path with a slot for each part
(`/match/{match_id}/segments/{build_id}/{segment}`), and `SEGMENT_SUFFIX` is
the `.json` every published segment URL ends in. `SegmentUrl` renders that
one string three ways — `route_pattern()` (the axum pattern), `build()`
(the published URL) and `parse()` (a path read back into
`(match_id, build_id, number)`). The builder, the router and the handler
all read from it, so there is no second literal to keep in step: the route
pattern is the scheme as it stands, and a change to the scheme moves the
route, the published URL and the parser together.

The definition lives in the render layer, not in `http`: the render layer
publishes the URLs and the HTTP layer serves them, so the dependency runs
one way (`http` → `api`), never back.

**The handler reads the path, it does not pick it apart.** The segment route
takes the request's `OriginalUri` and hands the path to the scheme's
parser; it no longer strips `.json` and parses digits itself. The status
answer is unchanged: the match's status is checked first (the scheme's
reader names the match even when the rest of the path is malformed), so an
unknown match still answers `404 unknown` and a match that is not ready
still answers `409 not_ready`. A ready match answers a path that is not a
segment URL with `404 no_segment`.

**The wire did not move.** The published URLs are the same bytes
(`/match/{match_id}/segments/{build_id}/{number}.json`), the build id is
the same digest (`ac98bf10…` for `m-0001`), and the release end-to-end
test leaves `docs/api-samples/` byte-identical. The unit tests pin the
scheme's own strings, so a change to the URL is a deliberate one.

**The proof is the round trip.** `every_published_segment_url_serves_the_body_the_manifest_hashed`
asks the real router for *every* URL of a ready match's manifest (90
segments in the fake match, 110 in the real one) and checks the body
against the hash that row publishes. It also reads each URL back with the
scheme's parser, so the URL, the row's number and the served body all
agree. Breaking the builder on purpose (a `.json.bak` suffix) fails the
test; changing the scheme's literal (`segments` → `segment`) fails the
pinned unit tests.

**Verification.** The new tests live in
`crates/sim-server/src/api/render/segment_url_tests.rs` (the scheme) and
`crates/sim-server/src/http/service_api/tests/segment_urls.rs` (the
round trip), with the malformed paths in
`crates/sim-server/src/http/service_api/tests/segments.rs`. `cargo test
-p sim-server` is green (157 tests), `cargo clippy --workspace
--all-targets -- -D warnings` is clean, and the ignored release
end-to-end test passes with the sample payloads unchanged. Fourteen
failures in `sim-core`, `sim-components`, `sim-ai-player` and
`sim-rules` pre-date this ticket — they fail on a clean checkout of
`main` too, and none of them is in a crate this ticket touches — so they
are left alone here.