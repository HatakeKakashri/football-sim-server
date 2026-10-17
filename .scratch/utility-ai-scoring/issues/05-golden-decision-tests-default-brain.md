# 05: Golden decision tests over the default brain

**What to build:** Safety net before refactoring scoring. Using the real default brain (home and away variants via the tactic parameters), run a table of perception scenarios through the current decision system and pin the winning intent kind and aggregate score (tolerance 1e-4). Scenarios: possessor with a clear lane; possessor in the shooting zone under and not under pressure; free ball near and far; opponent possessing with the player close and far; player displaced from slot; low stamina; player already holding an intent (hysteresis). Lives in `sim-core` tests because the default brain is authored there. Characterisation only: no production change.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] At least 12 scenarios covering every ball state (possessor, free, opponent possessing) and each of the 9 default actions winning at least once.
- [ ] Each case asserts intent kind and aggregate score; hysteresis case shows the bonus applied.
- [ ] Failure message prints the scenario name and per-action scores.
- [ ] Runs without file I/O and under one second.
- [ ] Tests pass against unmodified production code.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
