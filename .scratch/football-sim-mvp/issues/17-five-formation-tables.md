# 17: Five formation tables as 11-slot sets (canonicalise from Phase 0 placeholder)

**What to build:** The PRD §7.1 formation table is the canonical source for the 11 slots of each formation: `4-4-2`, `4-3-3`, `3-5-2`, `4-2-3-1`, `5-3-2`. The Phase 0 4-4-2 placeholder (ticket 03) is replaced by a registry that looks up the formation by name. Slot positions for each formation are coded in one module and combined with the team's per-half attack direction (ticket 02) so a home DC at slot index `2` flips ends at half-time through the same code path. The formation name lives on the per-side setup in the fixture (ticket 19).

**Blocked by:** 03, 14 (the placeholder works; position grid + fit are in place before formations compose with fit).

**Status:** ready-for-agent

- [ ] All five formations expressed as 11-slot sets; coverage unit test asserts each formation has exactly 11 slots across the 14 positions and respects the GK-is-goalkeeper-only rule.
- [ ] Home slot resolution composes with `attack_direction` so a single formation's slot table works for both halves and both teams.
- [ ] Phase 0 windowed smoke test still passes (regression check on the formation-driven home slot from ticket 03).
- [ ] `cargo test --workspace` green.
