# 02: Answer a match's status in one call

**What to build:** Pure expand. The match service's `state()` cannot tell "this id is not a fixture" from "this fixture was never started" — it answers `NotStarted` for both — so every route asks `is_known()` and then `state()`: two calls, two acquisitions of the state map's lock, and the rule that separates a 404 from a 409 lives in the HTTP layer rather than in the service that owns the rule. Give the service one lifecycle type that can express every case, including "unknown fixture", and one method that answers a match's status in a single call. The service's existing `is_known()` and `state()` keep working, so no route and no HTTP test has to change in this ticket.

The ready case carries the published match (ticket 01), so a caller that is told "ready" can read the manifest's bytes and a segment's bytes without asking a second question. The failed case carries the failure's code, message and last good tick plus the time left before a retry, so a caller never has to recompute a wait from a deadline.

**Blocked by:** 01.

**Status:** done

- [x] The service has exactly one lifecycle type, whose cases are: unknown fixture, not started, queued, running, ready, failed. There is no second, overlapping lifecycle type.
- [x] One method answers a match's status in a single call, taking the state map's lock once.
- [x] The ready case carries the published match; the failed case carries the code, the message, the last good tick and the time left before the match may be retried.
- [x] `is_known()` keeps answering exactly as it does today.
- [x] `state()` keeps answering exactly as it does today for every known fixture. For an id that is not a fixture it now answers "unknown fixture" instead of "not started"; no caller can tell the difference yet, because the routes ask `is_known()` first. The one assertion that pins the old behaviour — the unknown-id case in the service's lifecycle tests — is updated to the new answer.
- [x] The service's own tests cover every case of the new answer: an id that is not a fixture, a fixture that was never started, a queued match, a running match, a ready match and a failed match.
- [x] The state map's lock is still never held across an await point.
- [x] No HTTP route, status code or response body changes, and the HTTP test suite passes unchanged.
- [x] `cargo test --workspace` green and `cargo clippy --workspace --all-targets -- -D warnings` clean. (`sim-server` is green; the workspace's pre-existing failures in other crates are listed under Comments.)

## Comments

**What landed.** The service's one lifecycle type (`MatchState`, in
`crates/sim-server/src/service/state.rs`) grew the case the old `state()` could not express:
`UnknownFixture`, an id that is not a fixture at all — which is not the same answer as
`NotStarted`, a match that has not run yet. The two cases are now told apart by the service
that owns the rule, not by the HTTP layer.

The single status answer is `MatchService::status` (new module
`crates/sim-server/src/service/status.rs`): it takes the state map's lock once and reports
every case. `state()` is kept as the older name for the same question and delegates to it, so
the routes and the HTTP tests are untouched; ticket 03 removes it. `is_known()` answers exactly
as it did.

The ready case now carries the published match (ticket 01's `PublishedMatch`) next to the
manifest and the segments, so a caller that is told "ready" can read the manifest's bytes and
a segment's bytes without asking a second question. The failed case carries the time left
before a retry (`retry_in`), computed by the service, next to the deadline (`retry_at`) the
service keeps to decide whether a later start may run the match again. The HTTP layer reads
the wait from the answer and no longer calls `Instant::now()` itself.

**Verification.** The service's own tests (`service/tests/status_tests.rs`) cover every case of
the answer: an unknown id, a match that was never started, a queued match, a running match, a
ready match (with its published bytes) and a failed match (deterministic and transient, with
the wait). The HTTP status matrix passes unchanged: same status codes, same bodies, same
headers. `cargo clippy --workspace --all-targets -- -D warnings` is clean.

The `sim-server` suite is green. Fourteen failures in `sim-core`, `sim-components`,
`sim-ai-player` and `sim-rules` pre-date this ticket — they fail on a clean checkout of `main`
too, and none of them is in a crate this ticket touches — so they are left alone here.

**Two trade-offs worth knowing about.**

1. The failed case carries both the deadline and the wait. The deadline is what the service
   needs to decide whether a retry is allowed yet; the wait is what a caller needs, and it is
   refreshed on every answer, so the two never drift apart in an answer.
2. The ready case keeps the manifest and the segment list next to the published match, so the
   bytes are held twice over (the published match keeps the uncompressed bodies as well). The
   route still reads the manifest value and the segment list; ticket 03 serves the published
   bytes instead and drops the list.

**Out of scope, left to the later tickets.** The routes still ask `is_known()` and then
`state()` (ticket 03 migrates them to the single status method), the manifest route still
clones the manifest value and re-serialises it (ticket 03 serves the published bytes), and the
segment route still scans the segment list (ticket 03 reads the published index).

**Also in this change.** `service.rs` was at the 500-LoC budget, so the read side of the
service (`status()`, `state()`, `is_known()`) moved into its own module,
`crates/sim-server/src/service/status.rs`.
