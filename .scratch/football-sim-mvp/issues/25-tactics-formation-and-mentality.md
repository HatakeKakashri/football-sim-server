# 25: Mentality + tactic + formation as numeric parameters

**What to build:** Per PRD §7.2, `Mentality` and `Tactic` (and the formation choice) are numeric parameters feeding the existing utility-AI considerations — not special-case code paths. `Mentality::Attack | Balance | Defend` and `Tactic::Possession | CounterAttack | HighPress | ParkTheBus` shift team line heights and risk appetites. Concrete observable effects: Possession shortens pass length and slows transitions; CounterAttack shortens the time-to-attempt after a tackle and lengthens pass length; HighPress increases press trigger distance and stamina cost; ParkTheBus lowers the defensive line. Formation shape (line heights and channels) flows from the formation registry (ticket 17). Each of the four tactics and the three mentalities has at least one acceptance test asserting observable behaviour differs from `Balance`/`Possession` on a signature statistic (PRD §13.1 A9): pass length, average line height, press count, or similar.

**Blocked by:** 21 (tackle/press cost wiring exists), 22 (stamina drain), 23 (aerial support), 24 (Aggression), 17 (formation tables canonicalised); all of Phase 4 because tactics compose the consumer parameters.

**Status:** ready-for-agent

- [ ] One A9 signature test per tactic (Possession, CounterAttack, HighPress, ParkTheBus) over multiple seeds on its signature statistic.
- [ ] `Mentality` (Attack / Balance / Defend) shifts team line height and risk appetite; observable in the snapshot.
- [ ] Formation table from ticket 17 drives `home_slot` per the per-side setup in the fixture.
- [ ] `cargo test --workspace` green; `#[ignore]`d multi-seed full-match signature test (PRD §14) passes manually.
