# 02: Answer a match's status in one call

**What to build:** Pure expand. The match service's `state()` cannot tell "this id is not a fixture" from "this fixture was never started" — it answers `NotStarted` for both — so every route asks `is_known()` and then `state()`: two calls, two acquisitions of the state map's lock, and the rule that separates a 404 from a 409 lives in the HTTP layer rather than in the service that owns the rule. Give the service one lifecycle type that can express every case, including "unknown fixture", and one method that answers a match's status in a single call. The service's existing `is_known()` and `state()` keep working, so no route and no HTTP test has to change in this ticket.

The ready case carries the published match (ticket 01), so a caller that is told "ready" can read the manifest's bytes and a segment's bytes without asking a second question. The failed case carries the failure's code, message and last good tick plus the time left before a retry, so a caller never has to recompute a wait from a deadline.

**Blocked by:** 01.

**Status:** ready-for-agent

- [ ] The service has exactly one lifecycle type, whose cases are: unknown fixture, not started, queued, running, ready, failed. There is no second, overlapping lifecycle type.
- [ ] One method answers a match's status in a single call, taking the state map's lock once.
- [ ] The ready case carries the published match; the failed case carries the code, the message, the last good tick and the time left before the match may be retried.
- [ ] `is_known()` keeps answering exactly as it does today.
- [ ] `state()` keeps answering exactly as it does today for every known fixture. For an id that is not a fixture it now answers "unknown fixture" instead of "not started"; no caller can tell the difference yet, because the routes ask `is_known()` first. The one assertion that pins the old behaviour — the unknown-id case in the service's lifecycle tests — is updated to the new answer.
- [ ] The service's own tests cover every case of the new answer: an id that is not a fixture, a fixture that was never started, a queued match, a running match, a ready match and a failed match.
- [ ] The state map's lock is still never held across an await point.
- [ ] No HTTP route, status code or response body changes, and the HTTP test suite passes unchanged.
- [ ] `cargo test --workspace` green and `cargo clippy --workspace --all-targets -- -D warnings` clean.
