# 03: Formation 4-4-2 home slots replace `MoveToPosition(52.5, 34)`

**What to build:** The brain's default home position `(52.5, 34)` is replaced with a formation-driven slot table. For Phase 0 the only formation is hardcoded 4-4-2 (per the PRD §3 baseline: today only one formation exists in code); each player receives a `home_slot: (x, y)` from that table, indexed by the player's slot in the team (`GK`, `DL`, `DC`, `DR`, `ML`, `MC`, `MR`, `ST`). Phase 3's canonical five-formation work will replace this hardcoded table; this ticket's deliverable is that the home slot varies by slot and the constant `(52.5, 34)` no longer appears in default brain code. Slot coordinates are paired with the team's per-half attack direction (PRD §8.2: ends swap at half-time) so a player on the home team in the first half and the same player in the second half sit on opposite ends of the pitch through their home slot, not through trickery.

**Blocked by:** 02 (attack direction must be in place or slot coordinates can't flip correctly).

**Status:** ready-for-agent

- [ ] Hardcoded 4-4-2 slot table lives in one place (e.g., `sim_components` or `sim_ai_player`), referenced by both halves via the team's `attack_direction`.
- [ ] Grep finds zero `52.5` or `(52.5, 34)` literals in default brain code.
- [ ] Slot positions in a windowed match show meaningful spread (not all 22 players on top of each other); assertion in the existing windowed smoke test.
- [ ] `cargo test --workspace` green; `final_state_hash` for canonical seed recorded in `## Comments`.
