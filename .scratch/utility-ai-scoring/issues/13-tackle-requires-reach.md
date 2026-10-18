# 13: Tackle needs reach: no attempt outside range

**What to build:** Context: `player_action_execution_system` treats any player holding a `Tackle` intent as tackling the ball carrier whenever the ball has a possessor. There is no distance check, so a tackler on the other side of the pitch still rolls RNG, pays the stamina cost and records a `PendingTackle` every tick. The decision layer only makes distant tackles unlikely; it cannot prevent them. Add a reach limit as a `Tuning` value (suggested default 2.5 m, to be calibrated in ticket 17). Distance is tackler to carrier, taken from the tackler's perception snapshot (`nearby_opponents`); a carrier not in the snapshot is out of reach. Outside reach the tackler behaves as a closing runner: no roll, no stamina cost, no pending attempt, no recovery window.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] Test seam: `Simulation` (step ticks, read ball state, player state and the event log), per the TDD seam agreed for the tackle tickets. If a scenario cannot be built through that seam, stop and ask before testing elsewhere.
- [ ] Scenario: tackler holds a Tackle intent 30 m from the carrier for 12 ticks: zero tackle events, stamina changes only by the normal running drain and recovery, no `TackleRecovery` inserted.
- [ ] Scenario: same tackler 1 m from the carrier: a tackle attempt is recorded (event present).
- [ ] Boundary: just inside and just outside the reach value behave as above (reach read from `Tuning`, not hardcoded in the test).
- [ ] Reach is documented next to the other tackle tuning values.
- [ ] Existing tackle tests are updated only if they placed the tackler beyond reach; list any such change in the PR.
- [ ] Playability gate is deferred to ticket 17; note that in the PR.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
