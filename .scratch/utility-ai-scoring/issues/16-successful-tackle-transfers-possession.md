# 16: A successful tackle transfers possession

**What to build:** Context: today `TackleAttempt.success` only reaches the event log (via `emit_tackle_events`) and the failure slow-down. `possession_resolution_system` is proximity-based and never reads tackles, so a 'successful' tackle leaves the ball with the carrier. Make success take the ball: the tackler becomes `Ball.possessor`, a touch is registered for the tackler (as kick execution does with `PendingBallTouches`), any pending pass or shot of the old carrier is cancelled, and the old carrier loses the ball. Failure leaves possession unchanged. When several attempts against the same carrier succeed in one tick, the first in entity-id order takes the ball and later attempts report `success: false` (the carrier no longer holds it). Check existing event kinds in `event_emit` before choosing how any possession-change event is emitted; it must appear once.

**Blocked by:** 15.

**Status:** ready-for-agent

- [ ] Test seam: `Simulation` (step ticks, read ball state, player state and the event log), per the TDD seam agreed for the tackle tickets. If a scenario cannot be built through that seam, stop and ask before testing elsewhere.
- [ ] Scenario: seeded tackle that succeeds: after the tick `Ball.possessor` is the tackler, `last_touch` is the tackler, and the previous carrier is no longer possessor.
- [ ] Scenario: seeded tackle that fails: possessor unchanged, tackler enters recovery.
- [ ] Scenario: two successful attempts in one tick: exactly one possession change, the lower entity id wins, the other attempt's event has `success: false`.
- [ ] The event log's tackle `success` flag always agrees with who holds the ball afterwards (assert in all three scenarios).
- [ ] A stale pending pass of the old carrier does not complete after the tackle.
- [ ] The test that only checked 'records a pending tackle' is replaced, not kept alongside.
- [ ] Playability gate deferred to ticket 17.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
