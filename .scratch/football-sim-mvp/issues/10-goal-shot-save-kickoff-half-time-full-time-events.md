# 10: `goal`, `shot`, `save`, `kickoff`, `half_time`, `full_time` events + 2-deep touch history

**What to build:** `goal` carries `scorer`, `assister` (nullable), `own_goal` (bool), `team`, and a running `score: {home, away}`; scorer is the last `Play` toucher, with `own_goal: true` set if that player is on the conceding team (no assister). `assister` is the previous `Play` toucher on the scoring team within the last 10 s of game time. This requires the small two-deep touch history on `Ball` (also mixed into `state_hash` per ticket 07). `shot` carries `shooter`, `on_target`, `outcome` enum (`goal | saved | blocked | off_target | post`). `save` carries `keeper`, `shooter`, `kind` (`catch | parry | punch`). `kickoff` carries `taker`, `reason` (`match_start | second_half | after_goal`), and on half starts `attack_direction` per team. `half_time` carries the running score. `full_time` carries `score`, `event_count`, and `state_hash`. The final-score-equals-goal-count invariant (`A7`) becomes testable from this ticket onward. Phase 4 mechanic tickets land shooting/reflexes afterwards and refine shot `outcome` distribution; the events exist now.

**Blocked by:** 08 (envelope, pass events, possession history).

**Status:** ready-for-agent

- [ ] Every goal event has a `scorer`; `own_goal` flag and `assister` selection match PRD §8.5.
- [ ] Running score in `goal` events equals the count of goals per team at emission time (`cargo test` per-match assertion).
- [ ] `kickoff`, `half_time`, `full_time` events emitted on the correct state transitions; `full_time` carries `event_count` and `state_hash`.
- [ ] Two-deep touch history is read for both `goal` (`assister`) and hashing (`cargo test` per ticket 07 invariants).
- [ ] `cargo test --workspace` green; `final_state_hash` for canonical seed recorded in `## Comments`.
