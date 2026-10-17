# 08: Extract `evaluate` and slim the decision system

**What to build:** Build the scoring module's interface: evaluate a brain against a per-player context (perception, stamina, skill, grid, team, possessor flag, current intent) and return the chosen intent with sentinels resolved (shoot-at-goal target and move-to-position target) plus the per-action breakdown. First maximum wins ties (strict greater-than); no winner keeps the current intent or falls back to hold position. The decision system shrinks to: cadence guard, build context, call evaluate, write the intent, emit the trace from the returned breakdown (only when the trace gate is on).

**Blocked by:** 06, 07.

**Status:** ready-for-agent

- [ ] Golden tests (05), trace tests and sentinel tests pass unchanged.
- [ ] Tie-break test: two equal-scoring actions pick the earlier one in brain order.
- [ ] No-winner test: existing intent kept; with none, hold position set.
- [ ] Trace emission reads the breakdown; decision/action/consideration span order is unchanged.
- [ ] Decision system body is under 60 lines and contains no scoring arithmetic.
- [ ] Decision evaluation count and cadence behaviour unchanged.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
