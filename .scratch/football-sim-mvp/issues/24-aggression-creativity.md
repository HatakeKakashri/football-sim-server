# 24: Aggression (outfield + keeper) + Creativity (outfield + keeper)

**What to build:** The Phase 4 (e) mechanics. Outfield `Aggression` is press frequency, press trigger distance, and tackle propensity (already partially wired in tickets 21 + 22 via placeholder scaling). `Creativity` is the weight of key-pass / through-ball options. Goalkeeper `Aggression` is rush-out and claim propensity; goalkeeper `Creativity` (the †-marked proposal in §6.2) is the weight of long / through-ball distribution options. Consumers read through §15 curves and §14 fit multipliers. If either `Creativity` consumer cannot be made honest, it moves to the §6.6 reserved list with a reason. Stamina costs scale by `Aggression` and `Fitness` per PRD §7.3; tuning constants live in the §11 struct. Per-attribute monotonicity unit tests for each.

**Blocked by:** 13, 15, 14, 20, 21, 22, 23 (full Phase 4 foundation; creativity weights key-pass/through-ball options which need the §9 event infrastructure from Phase 1, and the pass error spread from ticket 21).

**Status:** ready-for-agent

- [ ] Outfield `Aggression` raises press frequency, trigger distance, tackle propensity (unit tests).
- [ ] `Creativity` raises the weight of key-pass / through-ball options (unit test).
- [ ] Goalkeeper `Aggression` rush-out and claim propensity monotonic (unit test).
- [ ] Goalkeeper `Creativity` distribution-option weight monotonic (unit test), or moves to §6.6 reserved list with reason.
- [ ] Press and tackle stamina costs scale by `Aggression` and `Fitness` per PRD §7.3.
- [ ] Reserved list (PRD §6.6) final after this ticket; PRD §13.1 A11 test passes.
- [ ] `cargo test --workspace` green.
