# 02: `DefensivePosition` reads the own goal from the snapshot

**What to build:** The `DefensivePosition` consideration returns 1.0 when the player is between the own goal and the ball, but it hardcodes the own goal at x = 0. That is only correct for a team attacking right, so by code reading the away team (and the home team after the half-time swap) is scored against the wrong side of the pitch, which skews `MarkOpponent`. Make the consideration use the own-goal position and attack direction from the perception snapshot (ticket 01). This is a behaviour change: record the before/after in the PR description.

**Blocked by:** 01.

**Status:** ready-for-agent

- [ ] Home player, first half, between own goal and ball: scores 1.0; beyond the ball (towards the opponent goal): 0.0.
- [ ] Away player, first half: mirrored result (unit test with the same geometry reflected).
- [ ] Home player after half-time swap: same as the away first-half case (unit test using the swapped direction).
- [ ] A `MarkOpponent` decision test shows an away defender goal-side of the ball now outscores one that is not.
- [ ] Hardcoded own-goal x literal no longer appears in the consideration.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] Ignored playability gate re-run in release (`cargo test --release -p sim-core -- --ignored phase_0_playability_gate --nocapture`) and its four metrics reported in the PR; no regression versus the pre-change run.
