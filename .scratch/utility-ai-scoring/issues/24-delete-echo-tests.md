# 24: Delete tests that only echo code

**What to build:** Context (TDD audit): these tests cannot fail unless someone edits a constant or a derive on purpose, and assert no app behaviour. Delete: `tactical_thresholds_have_expected_values`, `weight_curve_exhaustive_for_all_variants` (named for all variants, checks one), `every_variant_has_its_snake_case_name` (mirrors the match arm by arm, pins the variant count), `test_score_constants`, `test_score_partial_eq`. The PR must name, for each deleted test, where the behaviour is still observed (for example the trace tests assert the emitted consideration names) or state that none is needed.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] The five tests (six functions counting both Score tests) are gone; no replacement echo tests added.
- [ ] PR lists the surviving observation point for consideration names and curve names (trace tests).
- [ ] Unused imports and helpers left behind are removed.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
