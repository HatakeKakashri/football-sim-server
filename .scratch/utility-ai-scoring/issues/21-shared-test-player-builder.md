# 21: Shared test player builder that always has a profile

**What to build:** Context: tests across `sim-ai-player` hand-roll players (Player, Position, Velocity, Stamina, Skill, role, team id, sometimes no `PlayerProfile`), which is why production code carries profile-less fallbacks. Add one `cfg(test)` builder returning a fully equipped player entity (outfield profile by default, goalkeeper variant available) and migrate the existing execution, decision, trace and roster tests to it. No behaviour change.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] Builder always inserts a `PlayerProfile` and every component the execution systems query.
- [ ] Execution, decision, trace and roster tests use it; the number of hand-rolled player spawn blocks removed is stated in the PR.
- [ ] All previously passing tests pass with identical assertions.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
