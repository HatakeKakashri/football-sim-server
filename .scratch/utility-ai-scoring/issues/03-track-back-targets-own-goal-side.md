# 03: `TrackBack` targets the own goal side, not a fixed point

**What to build:** `MovementIntent::TrackBack` steers to a fixed pitch point (0, 34), which is the home team's goal mouth only while it defends the left. The intent is not in the default brain yet, so this is a latent bug that would bite the first brain that uses it. Steer towards a point on the player's own side derived from the snapshot's own-goal position (ticket 01), keeping the existing speed. Document the chosen target (own goal mouth vs a point between goal and ball) in the ticket PR.

**Blocked by:** 01.

**Status:** ready-for-agent

- [ ] Steering test: home player first half, away player first half, and both after the swap each move towards their own goal side.
- [ ] Steering returns a target for every team and half; speed unchanged from today.
- [ ] Existing steering tests for other intents unchanged.
- [ ] The existing `steer_target` test that pins the `TrackBack` target to the literal (0, 34) is rewritten: it must assert the new own-goal-side behaviour, not the old constant (TDD audit).
- [ ] No literal pitch coordinate for the target remains in the dispatch table.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
