# 23: Require a player profile; fail loudly without one

**What to build:** Context: ticket 29 made missing resources fail loudly, but execution still takes `Option<&PlayerProfile>` and quietly runs profile-less players on defaults. Production always inserts profiles, so make the profile required in the execution queries and add a match invariant (alongside the existing invariants in sim-core) reporting any player entity without one when a match is created.

**Blocked by:** 22.

**Status:** ready-for-agent

- [ ] Test: creating a match whose player lacks a profile reports an invariant violation naming that player (use the existing invariant reporting path).
- [ ] Test: standard matches (home and away squads, goalkeepers included) report no violation.
- [ ] Execution queries no longer use `Option<&PlayerProfile>`; fixtures use the shared builder (ticket 21).
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
