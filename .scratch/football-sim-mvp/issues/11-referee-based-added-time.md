# 11: Referee-based added time per PRD §8.4

**What to build:** `Referee.stoppage_events` accumulates dead-ball tick counts: ball in `Dead` and restart delays (60-tick goal celebration + OOB restarts). Half time added time is the up-rounded whole minute count, capped at the existing 10-minute cap. `MatchClock.added_time_ticks: [h1, h2]` is set on both halves so the `FullTime` `added_time_ticks: [h1, h2]` field has real values, the `minute` / `added_minute` fields of the event envelope (§9.1) display `45+n / 90+n`, and the display clock uses `MatchClock` (never raw tick division). Determinism preserved because the only signals are tick counts (no wall-clock reads).

**Blocked by:** 04 (OOB restart counts toward stoppage), 06 (`Referee` entity is in place), 08 (event envelope needed for the `added_minute` field to round-trip).

**Status:** ready-for-agent

- [ ] `MatchClock.added_time_ticks` non-zero when there are dead-ball windows; capped per half.
- [ ] Event envelope `minute`/`added_minute` populate correctly; first added minute is `1` not `0`.
- [ ] `full_time` event's `added_time_ticks` array reports both halves (PRD §9.2).
- [ ] `cargo test --workspace` green; `final_state_hash` for canonical seed recorded in `## Comments`.
