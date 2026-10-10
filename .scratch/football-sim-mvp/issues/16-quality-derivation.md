# 16: Quality derivation (display and reporting only)

**What to build:** `quality(player, slot) -> f32` returns a position-weighted mean of the attributes relevant to the position, range `0..=200`. Weights are a code table per position group (defence, wide, midfield, attacking, striker, goalkeeper) — tunable. Goalkeeper Quality uses only the 15 goalkeeper-profile attributes (never the 15 outfield ones). `Team::strength()` is the sum (or mean — selectable) of the eleven slot Qualities as players play in their slots. Critical invariant from PRD §6.4: the simulation never reads `Quality`; only reporting and acceptance tests (A5) do. So the production match code path is uninstrumented by this ticket — only match-report snapshot code, replay serialization, and the A5 acceptance test read `Quality`.

**Blocked by:** 13, 14 (need the accessor and position group definitions).

**Status:** ready-for-agent

- [ ] `quality(player, slot) -> f32` defined; GK Quality uses only GK-profile attributes.
- [ ] Weights table per position group; tunable from the tuning struct.
- [ ] Grep confirms `Quality` is read only in report / replay / test files, never in `sim-core` match systems.
- [ ] `cargo test --workspace` green.
