# 12: Migrate decision tests to the scoring interface

**What to build:** Replace, don't layer: scoring tests that build a full `World` with eight resources move to pure tests against `evaluate`. Keep exactly two integration tests through the system: cadence (a player is evaluated once per cadence window) and intent write-back (including sentinel resolution reaching the player component). Remove any scoring test fully covered by the new interface tests.

**Blocked by:** 08.

**Status:** ready-for-agent

- [ ] Scenarios from the old stamina, passing, tackle and sentinel tests exist as pure `evaluate` tests with the same assertions.
- [ ] Only two tests construct a `World` for the decision system (cadence, write-back).
- [ ] Test setup helper for perception fixtures is shared, not duplicated.
- [ ] Net test count does not drop below the number of distinct behaviours previously covered (list them in the PR).
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
