# 17: Re-baseline the playability gate after tackle changes

**What to build:** Context: tickets 13-16 change how often and how successfully defenders win the ball, which moves the Phase 0 gate metrics. Run the gate on the commit before ticket 13 and after ticket 16, record the four metrics in a table in the PR, and tune only `Tuning` defaults (tackle reach, cooldown, drain multipliers) until the gate passes. No code changes other than tuning values and their documentation.

**Blocked by:** 16.

**Status:** ready-for-agent

- [ ] PR contains a before/after table of the four gate metrics from `phase_0_playability_gate` (10 full matches each).
- [ ] Gate passes with the final defaults; any default changed is listed with the metric that motivated it.
- [ ] Tactic signature tests (press count, pass length, line height) still pass.
- [ ] No production logic changed (diff limited to tuning values and docs).
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
