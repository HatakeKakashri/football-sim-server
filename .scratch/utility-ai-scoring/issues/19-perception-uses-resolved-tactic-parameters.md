# 19: Perception reads the same resolved parameters

**What to build:** Context: `perception_system` maps live `Team.mentality` to a +-0.5 value for each snapshot, a second live reader of tactic state. Derive that value from the team's resolved `TacticParameters` (adding a documented numeric to it if it has none) so every consumer sees one source.

**Blocked by:** 18.

**Status:** ready-for-agent

- [ ] Test: mutating `Team.mentality` after spawn does not change any perception snapshot field.
- [ ] Test: Defend, Balance and Attack teams still produce the same snapshot values as before (-0.5, 0.0, 0.5 for the existing field).
- [ ] Perception no longer reads `Team.mentality`.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
