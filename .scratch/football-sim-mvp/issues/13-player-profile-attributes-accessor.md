# 13: `PlayerProfile` enum + 30 attributes + accessor enum with declared consumers

**What to build:** `PlayerProfile::Outfield { defence(5), attack(5), physical_mental(5) }` and `PlayerProfile::Goalkeeper { goalkeeping(10), physical_mental(5) }` (`sim-components`). The PRD §6.2 accessor enum declares each attribute variant with its consumer system (or `Reserved`); the `A11` test iterates every variant and fails if any is undeclared. The PRD's "zero reserved attributes" target drives every mechanic ticket in Phase 4 to add a real consumer; if a consumer cannot be made honest, the variant moves to the `Reserved` list (PRD §6.6) with a reason — that move happens in the mechanic ticket, not here. All reads go through the accessor (no direct field access) so a stat-strength sweep (`A10` unit-level) can reason about one attribute in isolation. The test suite includes per-attribute monotonicity tests (`A10`) for the 25 attributes not in the full-match sweep.

**Blocked by:** 06 (Phase 0 must be playable so mechanic work isn't fighting basic perception bugs); can run in parallel with ticket 16 (response-curve layer) — coupling is one-way (curve reads accessor).

**Status:** ready-for-agent

- [ ] `PlayerProfile::Outfield` and `::Goalkeeper` carry the 30 attribute slots across the two profiles.
- [ ] Accessor enum has 30 variants with declared consumers; `A11` test fails on any undeclared variant.
- [ ] All readers go through the accessor; grep finds no direct attribute-name string in match code outside the accessor.
- [ ] Per-attribute monotonicity unit tests for the 25 attributes not in the full-match sweep exist in default `cargo test`.
- [ ] `cargo test --workspace` green.
