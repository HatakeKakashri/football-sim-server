# 06: Windowed smoke tests + Phase 0 playability gate (with Referee spawn)

**What to build:** A `cargo test`-default windowed smoke test (≤ 3 game minutes, per PRD §14) that asserts the ball crosses halfway, both teams touch it, and at least one shot occurs — so the goal-direction bug cannot return unnoticed. A `--release` Phase 0 playability harness runs 10 seeds and prints the four gate metrics (each team holds ball ≥ 30 % in-play ticks; ball crosses halfway ≥ 20×/half; ball in each pitch half 25–75 % in-play ticks; ≥ 5 shots/match average). Additionally a `Referee` entity is spawned at construction so `added_time_calculation_system` produces output (added time will still be 0 in this ticket because no OOB or goal dead-ball windows exist yet — Phase 1 ticket 12 closes the loop). Phase 0 exit gate: both gate checks pass.

**Blocked by:** 02, 03, 04, 05 (all Phase 0 fixes must be in place; Referee spawn is folded into this ticket because it is a one-line change at construction).

**Status:** ready-for-agent

- [x] Windowed smoke test lives in default `cargo test`; passes for every tested seed.
- [x] Playability gate harness reports all four metrics and passes over 10 seeds (release build).
- [x] `Referee` entity exists for the duration of the match; `added_time_calculation_system` runs. (Partial: the system runs but writes the `MatchClock` *component* on the match entity, while `get_state` and the lifecycle read the *Resource*; observable added time stays 0 until ticket 11 closes that. The Referee spawn is in place.)
- [x] `cargo test --workspace` green; `final_state_hash` for canonical seed recorded in `## Comments`.

## Comments

**Implemented and verified (`cargo build --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all green; the two `#[ignore]`d tests are full-match harnesses):**

- `Simulation::create_match` spawns one `Referee` (last, so other entity ids are unchanged). Tests: `referee_spawn_tests.rs`. `referee_events_appear_once_when_a_referee_exists` now loads that referee instead of spawning a second one (the trace emitter reads the first).
- Windowed smoke test is parametrised by seed (`run_windowed_smoke`); criteria unchanged. Added `test_windowed_smoke_test_prd_14_multi_seed` (seeds 1001–1005).
- `playability_gate_tests.rs`: `PlayabilityTracker` (unit-tested in default `cargo test`) and the `#[ignore]`d `phase_0_playability_gate_over_10_seeds` (seeds 2001–2010). Possession, crossings and territory are checked per seed; shots on average. A shot is a kick by a player whose intent is `ShootAtGoal`. A crossing is a side flip with a 1 m dead band, reset at each restart. Failure variants are structured (`enum GateFailure`) so tests don't string-match on ad-hoc wording.
- Added time is still 0: `added_time_calculation_system` writes the `MatchClock` *component*, not the Resource that `get_state` and the lifecycle read. Ticket 11 should close that.

**Canonical seed (12345) `final_state_hash` at tick 0:** `17316278914852600863` (post-referee-spawn; this value is now part of the determinism contract for this branch).

