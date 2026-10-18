# 29: Movement execution behaviour tests

**What to build:** Context (TDD audit): `player_action_execution_system` has tests only for its pure arrival and acceleration helpers and the tackle record. Nothing checks that an intent becomes motion. Add behaviour tests through the system.

**Blocked by:** 21.

**Status:** ready-for-agent

- [ ] Test seam: the execution systems run in an installed world (`ai_world()` style), asserting only on observable outputs (velocity, stamina, ball velocity, pending-pass/shot resources). No assertions on private helpers.
- [ ] A player with no intent ends the tick at zero velocity.
- [ ] ChaseBall: velocity points toward the ball and its change from rest is limited by the per-tick acceleration cap (value written as a literal from the documented constant).
- [ ] HoldPosition: the player decelerates to rest rather than continuing at previous velocity.
- [ ] Movement intents drain stamina and non-movement intents do not; stamina recovers every tick and stays within 0 to 1.
- [ ] A tackler in recovery moves at half speed compared with an identical tackler not in recovery.
- [ ] Same seed gives identical results across two runs (determinism).
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
