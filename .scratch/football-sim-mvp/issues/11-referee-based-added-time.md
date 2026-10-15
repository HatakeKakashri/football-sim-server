# 11: Referee-based added time per PRD §8.4

**What to build:** `Referee.stoppage_events` accumulates dead-ball tick counts: ball in `Dead` and restart delays (60-tick goal celebration + OOB restarts). Half time added time is the up-rounded whole minute count, capped at the existing 10-minute cap. `MatchClock.added_time_ticks: [h1, h2]` is set on both halves so the `FullTime` `added_time_ticks: [h1, h2]` field has real values, the `minute` / `added_minute` fields of the event envelope (§9.1) display `45+n / 90+n`, and the display clock uses `MatchClock` (never raw tick division). Determinism preserved because the only signals are tick counts (no wall-clock reads).

**Blocked by:** 04 (OOB restart counts toward stoppage), 06 (`Referee` entity is in place), 08 (event envelope needed for the `added_minute` field to round-trip).

**Status:** ready-for-agent

- [x] `MatchClock.added_time_ticks` non-zero when there are dead-ball windows; capped per half.
- [x] Event envelope `minute`/`added_minute` populate correctly; first added minute is `1` not `0`.
- [x] `full_time` event's `added_time_ticks` array reports both halves (PRD §9.2).
- [x] `cargo test --workspace` green; `final_state_hash` for canonical seed recorded in `## Comments`.

## Comments

Implemented 15 Oct 2026 (session `ses_ebfdb28fdffeNxJBBYz9RXoA1V`):

- `stoppage_tracking_system` (`sim-rules::referee`): one `stoppage_events`
  entry per `Dead`-ball tick while `InPlay`; `restart_system` credits one
  `oob_restart` tick per OOB restart (same-tick settle has no `Dead`
  window); goal celebrations accrue tick-by-tick through the 60-tick
  kickoff delay. Tick counts only — determinism preserved.
- `added_time_calculation_system` rewritten: queue length = stoppage
  ticks, rounded up to whole minutes, capped at 10 per half, monotonic
  within the half, written to the `MatchClock` Resource (source of truth;
  previously wrote only the Component mirror, so live matches never saw
  added time). `tick_clock` mirrors `added_time_ticks` Resource →
  Component each tick.
- Half-time transition resets the second half to 0 added ticks and clears
  the queue (`FirstHalfAddedTime` still carries h1 for the `full_time`
  `[h1, h2]` report); envelope `added_minute` is `None` until the
  whistle threshold, then the up-rounded minute count (`45+n` / `90+n`,
  first minute `1`).
- Trace `stoppage` emission switched from drain-assumes to a running
  seen-count (with half-time reset); `simulate_trace` e2e now asserts
  Referee-process/event consistency instead of Referee-absent.
- Live verification, seed 12345 full match: H1 `165600 = 162000 + 3600`
  added, H2 reset to 0 then accrued its own `3600`; `FullTime` at sim tick
  331219, score 0–0, `full_time` `added_time_ticks = [3600, 3600]`.
- `cargo test --workspace` green (all suites, 0 failures);
  `cargo clippy --workspace --all-targets -- -D warnings` green;
  20k-tick seed-12345 runs byte-identical.
- `final_state_hash` (seed 12345, full match): `9497114412887055889`.
