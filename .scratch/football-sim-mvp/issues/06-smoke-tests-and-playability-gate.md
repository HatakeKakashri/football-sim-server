# 06: Windowed smoke tests + Phase 0 playability gate (with Referee spawn)

**What to build:** A `cargo test`-default windowed smoke test (≤ 3 game minutes, per PRD §14) that asserts the ball crosses halfway, both teams touch it, and at least one shot occurs — so the goal-direction bug cannot return unnoticed. A `--release` Phase 0 playability harness runs 10 seeds and prints the four gate metrics (each team holds ball ≥ 30 % in-play ticks; ball crosses halfway ≥ 20×/half; ball in each pitch half 25–75 % in-play ticks; ≥ 5 shots/match average). Additionally a `Referee` entity is spawned at construction so `added_time_calculation_system` produces output (added time will still be 0 in this ticket because no OOB or goal dead-ball windows exist yet — Phase 1 ticket 12 closes the loop). Phase 0 exit gate: both gate checks pass.

**Blocked by:** 02, 03, 04, 05 (all Phase 0 fixes must be in place; Referee spawn is folded into this ticket because it is a one-line change at construction).

**Status:** ready-for-agent

- [ ] Windowed smoke test lives in default `cargo test`; passes for every tested seed.
- [ ] Playability gate harness reports all four metrics and passes over 10 seeds (release build).
- [ ] `Referee` entity exists for the duration of the match; `added_time_calculation_system` runs.
- [ ] `cargo test --workspace` green; `final_state_hash` for canonical seed recorded in `## Comments`.
