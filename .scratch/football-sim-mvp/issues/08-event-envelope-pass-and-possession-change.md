# 08: Event envelope + `pass` + `possession_change` events

**What to build:** Every event in PRD §9.1 has an envelope: `seq` (gapless from 0), `tick` (global, not elapsed), `half`, `minute` (`45 | 90`), `added_minute` (`null` or `n`), `type`, `team` (nullable where it doesn't apply). `pass` events are emitted at resolution (the ball's next acquisition, OOB, or goal); `kick_tick` is when the kick happened; `success` means received by a teammate of the kicker; `to` is the intended receiver or `null` if the kick was astray. `possession_change` fires whenever the possessing team changes, with a `cause` enum (`tackle | interception | out_of_bounds | save | loose_ball | goal`). Both events use the touched-record type from ticket 01 to resolve `from`/`to`/`last_touch`. Client-side event seq protocol (drop ≤ last seen) is documented in the event module header even though `client-protocol.md` will be updated in Phase 6.

**Blocked by:** 07 (harness + invariants + the touch-history path exist before events build on them).

**Status:** ready-for-agent

- [ ] `pass` events emitted at every resolution; `kick_tick` and resolution `tick` correct; `success` and `to` match PRD §9.3.
- [ ] `possession_change` events emitted on every team change with the correct `cause`.
- [ ] `seq` strictly gapless and monotonic across the match (`cargo test -p sim-core` assertion).
- [ ] `cargo test --workspace` green; `final_state_hash` for canonical seed recorded in `## Comments`.
