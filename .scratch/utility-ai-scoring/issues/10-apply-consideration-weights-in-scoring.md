# 10: Apply consideration weights in scoring

**What to build:** Switch aggregation to the weighted mean so each consideration's `weight` changes decisions (today the weight is only recorded in traces, so tuned weights are inert and traces mislead). This is a deliberate behaviour change: review the default brain's weights (they sum to 0.2 to 1.0 across actions) and confirm each is intended as a relative weight. Update golden expectations on purpose, listing each changed scenario and why in the PR. The trace should show the weight actually used.

**Blocked by:** 08, 09.

**Status:** ready-for-agent

- [ ] Unit test: doubling a consideration's weight moves the aggregate towards that consideration's score.
- [ ] Single-consideration actions score identically to before.
- [ ] Golden tests updated with a PR table of old vs new winner and score; no unexplained winner flips.
- [ ] Trace consideration entries carry the weight used by the aggregate (trace test).
- [ ] The ChaseBall aggregate literal in the trace tests (ticket 25) moves from about 0.527 to about 0.428; the PR shows the hand calculation.
- [ ] Tactic signature tests in `sim-core` still pass (press count, pass length, line height).
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] Ignored playability gate re-run in release (`cargo test --release -p sim-core -- --ignored phase_0_playability_gate --nocapture`) and its four metrics reported in the PR; no regression versus the pre-change run.
