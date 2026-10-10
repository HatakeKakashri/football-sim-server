# 01: Ball-touch registration refactor

**What to build:** A single, audit-defining code path for "a player touched the ball." The five writers that today mutate `Ball.last_touch` directly (proximity-based acquisition, deliberate kicks, OOB taker, kickoff taker, etc.) go through one registration mechanism (`PendingBallTouches` resource + `apply_ball_touched_system` consumer) that records `TouchKind::Play` or `TouchKind::Restart`, denormalises the team onto the touched record, and surfaces `last_touch` in match-report and replay views. Behaviour at the boundaries stays identical to today; state hashes change because the underlying writes change path. Offside-related fields and tests remain stripped (PRD §2: offside stub inert).

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] All five `Ball.last_touch` writers route through the registration mechanism; no direct mutation remains.
- [ ] Touch record carries `TouchKind` and the team, captured even if the entity is later despawned, so OOB restart payloads survive.
- [ ] `Ball.last_touch` and the player-id mapping appear in `BallSnapshot` for both `sim-core` report and `sim-replay`.
- [ ] Test coverage: each writer has a unit test asserting the registered touch; replay fixture golden hashes refreshed and committed.
- [ ] `cargo test --workspace` green; state-hash change documented in the ticket's `## Comments`.
