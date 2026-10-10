# 04: Out-of-bounds restart reads the same tick

**What to build:** OOB detection marks the ball and the restart system reads that mark within the same tick, eliminating the deferred-command one-tick delay that today stalls the ball at the centre circle. The fix follows the gotcha in the project's gotchas list: route the "ball is out of bounds" signal through a `Resource` (e.g., `PendingRestart`) rather than a component, so consumers in the same system-run step see the signal. The restarter then resolves to the correct restart (throw-in / goal-kick / corner) and placement+ taker using the team captured on the touched record from ticket 01. Same-tick reading is verified by a unit test that runs one rule step then one restart step on the same world.

**Blocked by:** 01 (restart taker selection depends on the team denormalised on the touched record).

**Status:** ready-for-agent

- [ ] OOB signal flows through a `Resource`, not a deferred component; the restarter sees it within the same tick.
- [ ] Restart selection (throw-in / goal-kick / corner) and taker selection match PRD §8.2 and the team from ticket 01.
- [ ] Stall window invariant passes (≤ 30 s of game time per §12) over 10 seeds in the Phase 1 harness when that lands; before Phase 1, a manual run over 5 seeds shows no centre-circle stalling.
- [ ] `cargo test --workspace` green; `final_state_hash` for canonical seed recorded in `## Comments`.
