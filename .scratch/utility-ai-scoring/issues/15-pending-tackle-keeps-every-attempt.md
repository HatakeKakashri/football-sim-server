# 15: Keep every simultaneous tackle attempt

**What to build:** Context: `PendingTackle` holds a single `Option<TackleAttempt>`, and `set` overwrites it. When two defenders tackle the same carrier in the same tick, both roll and pay stamina but only the later one (entities are processed in id order) is emitted, so the event log under-reports. Turn the pending slot into an ordered collection drained by `emit_tackle_events`, one `tackle` event per attempt, in entity-id order (the order execution already uses for deterministic RNG draws). Possession effects are ticket 16; this ticket changes reporting only.

**Blocked by:** 14.

**Status:** ready-for-agent

- [ ] Test seam: `Simulation` (step ticks, read ball state, player state and the event log), per the TDD seam agreed for the tackle tickets. If a scenario cannot be built through that seam, stop and ask before testing elsewhere.
- [ ] Scenario: two defenders in reach of the same carrier both tackle in one tick: the event log contains two tackle events, with distinct tacklers, in ascending entity-id order.
- [ ] Single-attempt scenarios produce exactly the same event as before (sequence numbers, fields).
- [ ] Draining twice in one tick yields no duplicate events.
- [ ] Event sequence numbering stays contiguous (no gaps or repeats).
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
