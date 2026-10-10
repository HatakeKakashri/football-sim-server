# 14: 14-position grid + Chebyshev fit distance + fit multipliers

**What to build:** `enum Position` with `GK, DL, DC, DR, DML, DMC, DMR, ML, MC, MR, AML, AMC, AMR, ST` and a position-grid representation (rows back to front as in PRD §6.5). Fit distance is the minimum Chebyshev distance on the grid from the slot to any of the player's trained positions, returned by a single function. The fit multiplier table (`1.00 / 0.92 / 0.75` for distance `0 / 1 / ≥ 2`) lives in the tuning struct (§11), and `effective_attribute(player, slot)` multiplies every consumed attribute by the table entry — feeding both Quality-in-slot (§6.4 ticket 15) and every mechanic downstream. Properties: a trained-position placement is unchanged from today; out-of-position placement is significantly penalized (initial assumption: 25 % at distance ≥ 2). Deliberately out-of-position starters in the Phase 3 fixture content visibly degrade the team (acceptance criterion A5 directional test will rely on this).

**Blocked by:** 13 (attributes exist for effective_attributes to multiply).

**Status:** ready-for-agent

- [ ] `enum Position` with the 14 variants and the 5×3 grid coordinates.
- [ ] `fit_multiplier(slot, trained_positions) -> f32` returns 1.00 / 0.92 / 0.75 by Chebyshev bucket; multipliers live in the tuning struct.
- [ ] Unit tests: each trained position returns 1.00; distance 1 returns 0.92; distance ≥ 2 returns 0.75; trained position outside the outfield 13 grid (GK on `DC`) returns the appropriate penalty.
- [ ] `effective_attribute` consumed by every stat read downstream of this ticket (the mechanic tickets in Phase 4).
- [ ] `cargo test --workspace` green.
