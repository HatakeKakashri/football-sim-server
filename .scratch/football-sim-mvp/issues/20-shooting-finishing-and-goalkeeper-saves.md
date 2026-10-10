# 20: Shooting, Finishing, and goalkeeper saves

**What to build:** The Phase 4 (a) mechanics. The shot pipeline (`shot_event` ticket 10 already emits at resolution; this ticket makes `outcome` distribution sensitive to `Shooting` and `Finishing`). `Shooting` raises shot power and accuracy; `Finishing` raises conversion quality close to goal and under pressure. Goalkeeper `Reflexes` raise close-and-fast shot save probability; `Anticipation` positions the keeper relative to the shooter; `Rushing Out` adds a rush-out decision path. Save events (`save` ticket 10) split into `catch | parry | punch` per `kind`. All four consumers read through the §15 response curve and the §14 fit multiplier. PRD §13.1 A10 full-match sweep tests for `Shooting`, `Finishing`, `Passing`, `Tackling`, `Reflexes` are added in Phase 7 ticket 29; here we add the mechanics and unit tests for monotonicity (direction-of-effect) on each.

**Blocked by:** 13, 15, 14, 06 (orderly progression; shooting cannot be tuned without shoot-power affecting ball physics, which depends on Phase 0 perception basics).

**Status:** ready-for-agent

- [ ] Higher `Shooting` ⇒ higher shot power and accuracy (monotonic unit test).
- [ ] Higher `Finishing` ⇒ higher close-to-goal and under-pressure conversion (monotonic unit test).
- [ ] Higher `Reflexes` ⇒ higher save probability on close/fast shots (monotonic unit test).
- [ ] `Anticipation` shrinks keeper-to-shooter positioning error (unit test on positional output).
- [ ] `Rushing Out` adds a rush-out intent with angle-closing quality (unit test on rush-out choice).
- [ ] `save` events emit with `kind` populated; `shot` events emit `outcome` matching pipeline output.
- [ ] `cargo test --workspace` green.
