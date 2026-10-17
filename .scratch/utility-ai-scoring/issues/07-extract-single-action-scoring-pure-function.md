# 07: Extract single-action scoring as a pure function

**What to build:** Scoring one action (raw input per consideration, curve evaluation, geometric aggregation, empty-consideration baseline of 0.6, permission zeroing, hysteresis bonus on matching intent kind) is interleaved with ECS iteration and trace capture. Extract it as a pure function returning the aggregate plus per-consideration breakdown (name, raw input, curve name, weight, score). Behaviour-preserving; the breakdown is what the trace will later consume.

**Blocked by:** 05.

**Status:** ready-for-agent

- [ ] Unit tests, no `World`: empty considerations gives 0.6 when permitted and 0.0 when not; a zero score collapses the aggregate without exactly 0 (1e-4 floor); hysteresis adds exactly the configured bonus only for a matching kind.
- [ ] Breakdown has one entry per consideration in brain order.
- [ ] Not-permitted action returns aggregate 0 and still reports its breakdown.
- [ ] Golden tests from ticket 05 pass unchanged.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
