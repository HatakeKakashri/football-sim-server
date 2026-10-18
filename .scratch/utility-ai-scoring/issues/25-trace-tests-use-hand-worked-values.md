# 25: Rewrite trace tests with independent expected values

**What to build:** Context (TDD audit): `traced_values_match_an_independent_recomputation` computes expected values with the same `raw_input`, curve evaluation and `geometric_mean` the system uses (a lint allow even says 'identical arithmetic'), and `trace_lists_every_action...` derives the expected winner label via `intent_kind`. Replace with hand-worked literals for the existing fixture (player at (50, 34), ball at (60, 34), stamina 0.9): distance raw 10 m, linear 0-30 gives 0.333; stamina 0.9 on the logistic (midpoint 0.5, steepness 4) gives about 0.832; ChaseBall aggregate is sqrt(0.333 x 0.832) = about 0.527 today (the weighted mean of ticket 10 will give about 0.428 with weights 0.8 and 0.3); SupportRun has no considerations so scores the 0.6 baseline; HoldPosition (stamina only) scores about 0.832 and should win on a free ball. Verify these against the first run and correct the numbers in the PR if the fixture differs. Use a tolerance of 1e-3 and remove the float_cmp lint allow.

**Blocked by:** None (can start immediately). Ticket 10 will need to update the ChaseBall aggregate literal.

**Status:** ready-for-agent

- [ ] Expected raw, score, aggregate and baseline values are literals with the working shown in a comment.
- [ ] Chosen action is asserted by literal name, not through `intent_kind`.
- [ ] Test fails if the trace reports a wrong weight, score or aggregate (demonstrate by temporarily perturbing one value locally and noting it in the PR).
- [ ] No `geometric_mean` or `raw_input` calls in the test.
- [ ] The `allow(clippy::float_cmp)` is removed.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
