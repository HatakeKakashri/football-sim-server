# 31: Match driver abstraction

**What to build:** A single `MatchDriver` that runs a match to `FullTime` and enforces the six PRD §12 invariants, replacing duplicated tick loops and bookkeeping in `harness.rs`, `simulate.rs`, `runner.rs`, and `sim-replay`.

**Blocked by:** 30 (`MatchDriver` should consume a world produced by the new `MatchSetup` interface).

**Status:** ready-for-agent

- [ ] `MatchDriver` module exists with `run_to_full_time` that ticks and enforces invariants.
- [ ] `harness.rs` and `simulate.rs` refactored to use `MatchDriver` via adapters (metrics, JSONL snapshots); duplicated loop logic deleted.
- [ ] `cargo test --workspace` green; ignored full-match tests remain deterministic.
