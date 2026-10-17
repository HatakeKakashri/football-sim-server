# 04: Pitch-control inversion moves next to the grid (optional)

**What to build:** `PitchControlAtBall` inverts the grid value when the team id is 1 (away) inside the consideration. The grid stores home control, so the inversion belongs with the grid. Add a per-team control query to the pitch-control module and use it from the consideration, removing the magic team id from `raw_input`. Optional: low value on its own; drop if the scoring-module work makes it moot. First confirm the grid's current public interface and keep the change additive.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] Grid per-team query returns `home` for team 0 and `1 - home` for team 1 (unit test, several cells).
- [ ] `PitchControlAtBall` results are bit-identical before and after for both teams (test over sample ball positions).
- [ ] No team id literal in the consideration.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
