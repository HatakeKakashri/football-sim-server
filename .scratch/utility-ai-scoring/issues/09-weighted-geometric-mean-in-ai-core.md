# 09: Add `weighted_geometric_mean` to `sim-ai-core`

**What to build:** Considerations carry weights that scoring ignores today. Add a weighted geometric mean beside the existing one: each input clamped to the same 1e-4 floor, raised to its weight, normalised by the weight sum. Not wired in yet, so no behaviour change. Define behaviour for edge cases (empty input, all-zero weights, mismatched lengths) and document it.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] With equal weights it equals the existing `geometric_mean` (tolerance 1e-6, several inputs).
- [ ] A zero-weight input has no effect on the result.
- [ ] Result is monotonic in each input and in the direction of weight shift towards the larger input.
- [ ] Empty input or all-zero weights returns 0.0; mismatched lengths are rejected deterministically (documented, tested).
- [ ] Not referenced by any production system yet.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
