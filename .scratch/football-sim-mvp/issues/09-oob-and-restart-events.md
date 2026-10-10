# 09: `out_of_bounds`, `throw_in`, `goal_kick`, `corner` events

**What to build:** The OOB family of events: `out_of_bounds` (carries `last_touch` player and `exit_point`); the appropriate restart event (`throw_in`, `goal_kick`, or `corner`) is emitted with the `taker` and `at` placement. Taker selection uses the team denormalised on the touched record (tickets 01 + 04); placement uses the same logic the restart system already computes. The `cause` of the corresponding `possession_change` (from ticket 08) is `out_of_bounds`. Ordering invariant: `out_of_bounds` precedes its restart event by exactly one `seq` and the restart event's `tick` is the same as the out-of-bounds tick (single-tick resolution now that the OOB deferred-component bug is fixed in ticket 04).

**Blocked by:** 08 (envelope + `possession_change` with `out_of_bounds` cause must already exist).

**Status:** ready-for-agent

- [ ] OOB family emitted in the correct sequence across every restart scenario in §8.2.
- [ ] Restart `taker` is on the team opposite to `last_touch`'s team per the touched record.
- [ ] `seq` and `tick` invariants hold across the OOB family (`cargo test` assertion).
- [ ] `cargo test --workspace` green; `final_state_hash` for canonical seed recorded in `## Comments`.
