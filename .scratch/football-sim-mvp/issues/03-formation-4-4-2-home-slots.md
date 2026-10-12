# 03: Formation 4-4-2 home slots replace `MoveToPosition(52.5, 34)`

**What to build:** The brain's default home position `(52.5, 34)` is replaced with a formation-driven slot table. For Phase 0 the only formation is hardcoded 4-4-2 (per the PRD §3 baseline: today only one formation exists in code); each player receives a `home_slot: (x, y)` from that table, indexed by the player's slot in the team (`GK`, `DL`, `DC`, `DR`, `ML`, `MC`, `MR`, `ST`). Phase 3's canonical five-formation work will replace this hardcoded table; this ticket's deliverable is that the home slot varies by slot and the constant `(52.5, 34)` no longer appears in default brain code. Slot coordinates are paired with the team's per-half attack direction (PRD §8.2: ends swap at half-time) so a player on the home team in the first half and the same player in the second half sit on opposite ends of the pitch through their home slot, not through trickery.

**Blocked by:** 02 (attack direction must be in place or slot coordinates can't flip correctly).

**Status:** ready-for-human

- [x] Hardcoded 4-4-2 slot table lives in one place (in `sim_components::formation_slot`), referenced by both halves via the team's `attack_direction`.
- [x] Grep finds zero `52.5` or `(52.5, 34)` literals in default brain code.
- [x] Slot positions in a windowed match show meaningful spread (not all 22 players on top of each other). Asserted by the active `test_slot_positions_distinct_and_spread_in_match` (22 distinct slots, pairwise ≥ 3 m, none on the centre spot, 300 ticks). The PRD §14 windowed test is `#[ignore]`d (see ticket 02), so it is not the carrier of this assertion.
- [x] Second-half mirroring is asserted: after a real `HalfTime → Kickoff` transition every `HomeSlot` is the 180° reflection of its first-half slot (`test_attack_direction_half_time_swap`).
- [x] `cargo test --workspace` green; `final_state_hash` for canonical seed recorded in `## Comments`.

## Comments

### Canonical `final_state_hash` (seed 12345, 1000 ticks)

```
final_state_hash: 4822439035975581459   (as landed; current canonical value is 13289728479385266083, see ticket 02)
```

### Implementation Details

1. **Canonical 4-4-2 Slot Table**:
   - Implemented in `crates/sim-components/src/formation_slot.rs` as `FormationSlot` enum (11 slots: `GK`, `DL`, `DC1`, `DC2`, `DR`, `ML`, `MC1`, `MC2`, `MR`, `ST1`, `ST2`) and `FormationSlot::position_for(attack_direction)` with exact 180° point reflection across pitch center `(PITCH_LENGTH - x, PITCH_WIDTH - y)`.
   - Lineup positions and roles are derived from `FormationSlot`.

2. **HomeSlot Component & Perception**:
   - Added `HomeSlot(pub Vec2)` component and `PerceptionSnapshot.home_slot`.
   - `perception_system` computes `home_slot` using the player's `FormationSlot` and team's `attack_direction`, and populates `HomeSlot` and `PerceptionSnapshot.home_slot`.
   - `steer_target` in `intent_dispatch`, consideration evaluation in `sim-ai-player`, and `player_decision_system` resolve the default sentinel `MoveToPosition(Vec2::zero())` to `perception.home_slot`.

3. **Brain Code Decoupling**:
   - Zero occurrences of `52.5` or `(52.5, 34)` literals in `crates/sim-core/src/simulation/brain_default.rs`.
   - `formation_442_slot_position` and the sort-by-x fallback in `reset_formation_to_4_4_2` are removed; every player is spawned with a `FormationSlot`, so the fallback was dead and its centre-pitch default reintroduced the constant this ticket removes.

4. **Spread Verification** (revised in review):
   - The first version asserted horizontal spread > 50 m, which the two goalkeepers (x≈5 and x≈100) satisfy from tick 0 even if every other player stood still, so it did not test the criterion. It is replaced by `test_slot_positions_distinct_and_spread_in_match`.
   - Slots alone do not make players play the ball: the `MoveToPosition(home_slot)` default still outscores `ChaseBall` once a player is on its slot. See ticket 02.
