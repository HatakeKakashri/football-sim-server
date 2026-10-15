# 10: `goal`, `shot`, `save`, `kickoff`, `half_time`, `full_time` events + 2-deep touch history

**What to build:** `goal` carries `scorer`, `assister` (nullable), `own_goal` (bool), `team`, and a running `score: {home, away}`; scorer is the last `Play` toucher, with `own_goal: true` set if that player is on the conceding team (no assister). `assister` is the previous `Play` toucher on the scoring team within the last 10 s of game time. This requires the small two-deep touch history on `Ball` (also mixed into `state_hash` per ticket 07). `shot` carries `shooter`, `on_target`, `outcome` enum (`goal | saved | blocked | off_target | post`). `save` carries `keeper`, `shooter`, `kind` (`catch | parry | punch`). `kickoff` carries `taker`, `reason` (`match_start | second_half | after_goal`), and on half starts `attack_direction` per team. `half_time` carries the running score. `full_time` carries `score`, `event_count`, and `state_hash`. The final-score-equals-goal-count invariant (`A7`) becomes testable from this ticket onward. Phase 4 mechanic tickets land shooting/reflexes afterwards and refine shot `outcome` distribution; the events exist now.

**Blocked by:** 08 (envelope, pass events, possession history).

**Status:** ready-for-agent

- [x] Every goal event has a `scorer`; `own_goal` flag and `assister` selection match PRD §8.5.
- [x] Running score in `goal` events equals the count of goals per team at emission time (`cargo test` per-match assertion).
- [x] `kickoff`, `half_time`, `full_time` events emitted on the correct state transitions; `full_time` carries `event_count` and `state_hash`.
- [x] Two-deep touch history is read for both `goal` (`assister`) and hashing (`cargo test` per ticket 07 invariants).
- [x] `cargo test --workspace` green; `final_state_hash` for canonical seed recorded in `## Comments`.

## Comments

Implemented 2026-10-15. `cargo test --workspace` green (incl. 5 updated
`sim-rules` restart-tick assertions); `cargo clippy --workspace --all-targets --
-D warnings` green; release `full_match_lifecycle_events_and_score_fold`
(ignored) passes in ~10 s.

- `final_state_hash` for canonical seed 12345 (release `harness`, full match to
  `FullTime`, 324,019 ticks, score 0–0, no invariant violations, determinism
  agreed): `5025145673584427257`.
- Goal attribution (`attribute_goal` in `sim-core/src/simulation/events.rs`):
  scorer = most recent `Play` touch in `[last_touch, history[0], history[1]]`;
  conceding-team scorer => `own_goal: true`, no assister; otherwise assister =
  next `Play` touch on the scoring team within 600 ticks (10 s). Touches carry
  a new `tick` field (`LastTouch::with_tick`), stamped by the four production
  writers (kick, possession, OOB/kickoff restart, lifecycle kickoff); `tick: 0`
  means unknown and counts as within-window. Tick is also mixed into
  `state_hash` alongside the touch window.
- `shot` resolution mapping (documented placeholder for ticket 20/Phase 4):
  goal => `goal`; corner OOB / opponent acquisition => `blocked`; goal-kick /
  throw-in OOB => `off_target`; opposing keeper => `saved` + companion `save`
  (`catch` at ball speed ≤ 0.25 m/tick else `parry`; `punch`/`post` have no
  producer yet). Stale pending shots are cancelled by later kicks (unreachable
  in practice: acquisition resolves first).
- `full_time` additionally carries per-half `added_time_ticks` (PRD §9.2);
  first-half value is stashed at the whistle (`FirstHalfAddedTime`); both are
  0 until ticket 11 lands referee-based added time. `event_count` includes the
  `full_time` event itself.
- Harness `play_one` now folds per-team goal counts from `goal` events
  (incremental high-water mark) instead of scanning `RuleEvent::Goal`, so
  invariant 1 checks A7 live every tick.
