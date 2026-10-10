# 21: Passing, Tackling, Marking, Positioning + tackle events

**What to build:** The Phase 4 (b) mechanics. `Passing` narrows pass error spread. `Tackling` raises tackle success probability; a failed tackle leaves the tackler out of position for `tackle_recovery_ticks` (initial 90 ticks) and lets the carrier skip past. `Marking` reduces distance-to-mark error against an assigned opponent (zonal cover error). `Positioning` reduces target-position error against the ball and threats. All four consumers read through §15 curves and §14 fit multiplier. `tackle` events from PRD §9.2 are emitted at every tackle attempt with `tackler`, `carrier`, and `success`. Tunable constants (`tackle_recovery_ticks`, `tackle_stamina_cost`, base probability) live in the §11 tuning struct. PRD §13.1 A10 unit-level monotonicity tests for all four; `Passing` and `Tackling` get the full-match sweep in Phase 7 ticket 29.

**Blocked by:** 13, 15, 14, 20.

**Status:** ready-for-agent

- [ ] Pass error spread monotonic in `Passing`; `pass` event `to` field reflects improved accuracy.
- [ ] Tackle success probability monotonic in `Tackling`; failed tackle imposes `tackle_recovery_ticks` recovery window on the tackler.
- [ ] `Marking` reduces mark-distance error (unit test); `Positioning` reduces target-position error (unit test).
- [ ] `tackle` events emitted at every tackle attempt with `tackler`, `carrier`, `success` (PRD §9.2).
- [ ] Tuning constants (`tackle_recovery_ticks`, `tackle_stamina_cost`, base probability) live in the §11 tuning struct.
- [ ] `cargo test --workspace` green.
