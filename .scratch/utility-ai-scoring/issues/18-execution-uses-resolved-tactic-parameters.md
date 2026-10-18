# 18: Execution reads the team's resolved tactic parameters

**What to build:** Context: construct inserts a `TacticParameters` component on each team entity, and brains are built from the same values, but `player_action_execution_system` (press stamina factor) and `kick_execution_system` (pass length factor) recompute `TacticParameters::from_setup(team.mentality, team.tactic)` from the live `Team` fields every tick. If `Team.mentality` ever changes, execution diverges from brains and formation. Read the team's `TacticParameters` component instead. Behaviour is unchanged while mentality is constant.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] Test seam: the execution systems run in an installed world (`ai_world()` style), asserting only on observable outputs (velocity, stamina, ball velocity, pending-pass/shot resources). No assertions on private helpers.
- [ ] Test: mutate `Team.mentality` mid-run; the press stamina cost and the pass kick speed of subsequent ticks equal those of an unmutated run (compare two worlds).
- [ ] Test: with mentality constant, press cost and pass length are the same as before this change (literal values from the existing fixtures).
- [ ] No call to `TacticParameters::from_setup` remains in execution.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
