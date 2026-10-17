# 01: Perception snapshot carries own-goal position and attack direction

**What to build:** Prefactor for the team-relative frame. The perception module already resolves each team's per-half attack direction to pick the opponent goal and the home slot, but considerations cannot see the direction or the own goal, so they re-assume 'home attacks right' with hardcoded numbers. Add the own-goal position and the attack direction to the per-player perception snapshot, populated from the same per-team direction the goal position already uses. Pure expand: nothing reads the new fields yet, so behaviour must not change. Give the new fields defaults or constructor helpers so existing test fixtures that build snapshots by hand do not all need editing.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] For a home player in the first half, the snapshot's attack direction is +1 and own goal is the left goal; for an away player it is -1 and the right goal.
- [ ] After the half-time swap the values flip for both teams (unit test over both halves, both teams).
- [ ] Own goal and opponent goal positions are never equal.
- [ ] Players whose team id is unexpected fall back to the same safe default the goal position already uses.
- [ ] No consideration or system output changes; existing decision, perception and golden tests pass unchanged.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
