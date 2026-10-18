# 14: One tackle attempt per tackle intent

**What to build:** Context: decisions run every 6 ticks per player, but execution runs every tick, so a held `Tackle` intent currently rolls the tackle RNG, charges stamina and writes `PendingTackle` up to 6 times per decision. The effective success rate per decision is therefore 1-(1-p)^6 and stamina cost is multiplied by up to 6. A tackler gets one attempt per tackle intent; after the attempt the intent keeps steering (closing run) but does not re-roll until a fresh decision again chooses Tackle, and never before a cooldown of one decision window has passed (cooldown length in `Tuning`, default equal to the decision cadence).

**Blocked by:** 13.

**Status:** ready-for-agent

- [ ] Test seam: `Simulation` (step ticks, read ball state, player state and the event log), per the TDD seam agreed for the tackle tickets. If a scenario cannot be built through that seam, stop and ask before testing elsewhere.
- [ ] Scenario: a tackler in reach holds the intent for 12 ticks: exactly one tackle event for the first window and stamina reduced by exactly one tackle cost plus normal drain (compute the expected number from `Tuning` values written out in the test, not by calling the cost function).
- [ ] Scenario: a second decision that picks Tackle again after the cooldown produces a second attempt.
- [ ] A failed tackle still opens the recovery window and slows the tackler as before.
- [ ] RNG draws per held intent drop to one (assert determinism: same seed gives identical event sequence across two runs).
- [ ] Playability gate deferred to ticket 17.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
