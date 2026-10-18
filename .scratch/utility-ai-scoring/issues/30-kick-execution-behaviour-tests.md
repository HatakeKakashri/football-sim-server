# 30: Kick execution behaviour tests

**What to build:** Context (TDD audit): `kick_execution_system` is tested only for its telemetry flow events. Add behaviour tests for what it does to the ball and the pending-pass and pending-shot resources.

**Blocked by:** 21.

**Status:** ready-for-agent

- [ ] Test seam: the execution systems run in an installed world (`ai_world()` style), asserting only on observable outputs (velocity, stamina, ball velocity, pending-pass/shot resources). No assertions on private helpers.
- [ ] ShootAtGoal by the possessor gives the ball a velocity toward the resolved shot target and records a pending shot with the shooter and kick tick.
- [ ] PassTo gives the ball a velocity toward the nearest teammate and records a pending pass with the correct passer and intended receiver.
- [ ] A pass clears a stale pending shot; a shot cancels a stale pending pass.
- [ ] A non-possessor, or a possessor whose intent is not a kick, does not move the ball.
- [ ] A touch is registered for the kicker on every kick.
- [ ] Same seed gives identical kick direction across two runs.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
