# 22: Speed, Fitness, Strength

**What to build:** The Phase 4 (c) mechanics. `Speed` is sprint speed (lower with the ball than without, read through the §15 curve and the §14 fit multiplier). `Fitness` is stamina drain (every costful action — running, pressing, tackling) and recovery. `Strength` decides hold-off, tackle, and aerial-duel outcomes (kept-force tests; the keeper variant contested-claims lands in ticket 24). `cargo test --workspace` defaults run the per-attribute monotonicity tests (`A10`); `Fitness` and `Strength` are out of the full-match sweep list (ticket 29). Stamina cost constants (drain per action, recovery rate, press drain multiplier, tackle drain multiplier) live in the §11 tuning struct. The windowed smoke test continues to pass — stamina cannot collapse the simulation within 3 minutes unless a tuning bug exists.

**Blocked by:** 13, 15, 14 (Phase 4 foundation): `Speed` follows `Fitness` reachability, `Strength` follows `Tackling` position-tacking.

**Status:** ready-for-agent

- [ ] Sprint speed monotonic in `Speed`; ball-carrier speed < free speed (unit test).
- [ ] Stamina drain and recovery wired to every costful action; per-action cost constant lives in the §11 tuning struct.
- [ ] `Strength` raises held-ball retention probability against an incoming tackle (unit test).
- [ ] Press and tackle stamina costs scaled by `Aggression` (lands in ticket 25); placeholder scaled by 1.0 here.
- [ ] Windowed smoke test still passes (regression check).
- [ ] `cargo test --workspace` green.
