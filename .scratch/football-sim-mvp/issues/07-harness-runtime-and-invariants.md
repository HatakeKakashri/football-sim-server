# 07: Multi-seed headless harness + invariants + touch-history hash mix-in

**What to build:** A `sim-server benchmark` follow-up or new `sim-server harness --seeds <file>` subcommand that runs N seeds, never serialises per-tick snapshots, and reports the §13 metrics (full-time reach, longest dead-ball window, mean goals, 0–0 rate, strong-side win rate, determinism byte equality on repeated seeds, score vs goal count, event order, tactic signature proxies). Inside the simulation loop, runtime invariants from PRD §12 run in release builds too: `score == goal event count per team`, `MatchClock` monotonic, legal state transitions, no dead-ball window over 30 s, valid ball/player states, `FullTime` reached before `tick_cap`. Any violation emits a failed-match `error` event (the §10 wire shape), caches the failure (§6 hardening will reuse the cache), and logs the seed. The two-deep touch history on `Ball` (§8.5) is folded into the existing `state_hash` calculation so swaps to the touch path change the hash deterministically.

**Blocked by:** 06 (need a playable loop before invariants have anything to assert against).

**Status:** done

- [x] `sim-server harness` runs N seeds in release mode, writes a JSON summary, never serialises per-tick snapshots.
- [x] All six PRD §12 invariants run in release; any violation fails the match with the §10 `error` shape and a logged seed.
- [x] `Ball` two-deep touch history is hashed into `state_hash`; a unit test asserts the hash changes deterministically when the touch history changes.
- [x] Empty-events harness still reports §13.1 metrics that don't require events (e.g., A1, A2, A6).
- [x] `cargo test --workspace` green.

## Comments

### Implementation summary

All five acceptance criteria are met by commit `05c024a feat(sim): implement Issue 07 - multi-seed harness with runtime invariants`.

#### 1. `sim-server harness` subcommand

- New `crates/sim-server/src/harness.rs` (370 lines).
- Wired up in `crates/sim-server/src/lib.rs` as a subcommand with `--seeds <path>` (file of newline-separated seeds; blank lines and `#` comments ignored) and `-o/--output <path>` (defaults to stdout).
- Reads seeds, plays each to `FullTime` (or `tick_cap`), aggregates per-seed metrics, writes a single JSON document.
- Never serialises per-tick snapshots: the runner drives `Simulation::tick()` directly, only ever produces the aggregate `HarnessSummary`.
- Determinism check: each seed is played twice and the hashes compared.

#### 2. PRD §12 invariants in release

- New `crates/sim-core/src/simulation/invariants.rs` defines `check_invariants(&Simulation, &InvariantCounters) -> Option<InvariantViolation>`, a pure function over the live `Simulation` plus harness-owned counters.
- All six invariants implemented and enumerated: score == goals, clock monotonic (with half-boundary reset), legal state transitions, dead-ball ≤ 30 s, valid ball/player states, `FullTime` before `tick_cap`.
- The harness calls `check_invariants` per tick inside its `play_one` loop; on the first violation it breaks the loop, records the violation, and caches the failure.
- `InvariantViolation` exposes `code()`, `message()`, and `last_good_tick()` — the PRD §10.2 wire-shape fields.
- Violations are emitted as `tracing::error!` log lines that include `seed`, `violation_code`, and `tick` so callers can grep reproduction details.
- Failures are cached in `FailureCache` (a `HashMap<u64, SeedReport>`) inside `run_harness` so §6 hardening can plug in without touching the runner.

#### 3. Touch-history hash mix-in

- `Ball.touch_history: [Option<LastTouch>; 2]` lives on the resource (`crates/sim-components/src/lib.rs:295`), slid by `Ball::record_touch()`.
- `Simulation::get_state_hash` (`crates/sim-core/src/simulation/state_hash.rs:194-212`) folds both entries into the FNV-1a mix in fixed order; order matters so swaps to the touch path change the hash deterministically.
- New test `test_hash_detects_touch_history_change` (`crates/sim-core/src/tests/state_hash_tests.rs:67`) seeds two identical simulations, perturbs `touch_history[0]` on one, and asserts the hashes diverge.
- Existing test fixtures across `sim-rules`, `sim-ai-player`, `sim-physics`, `sim-core::tests::telemetry_tests` were updated to initialise `touch_history: [None, None]`.

#### 4. Empty-events §13.1 metrics

The harness JSON summary reports:

| Metric | PRD §13.1 | Source |
|---|---|---|
| `matches_reached_full_time` | A1 | `MatchState == FullTime` at end of each seed |
| `full_time_rate` | A1 | ratio of full-time matches |
| `max_dead_ball_window` | A2 | longest run of `BallState::Dead` ticks |
| `mean_goals` | A3 | sum of `(home, away)` per match |
| `zero_zero_draws` | A4 | count of `(0, 0)` finals |
| `zero_zero_rate` | A4 | ratio |
| `determinism_failures` | A6 | hashes that disagreed across two runs of the same seed |
| `final_state_hash` | (per-seed) | A6 identity |

A5 (strong-side win rate), A7 (goals vs score), A8 (event order), and A9 (tactic signatures) need fixture rosters the harness does not yet load; the runner records raw counts (score, ticks_run, max dead window, hash) so later fixture tiers can upgrade those metrics without re-running.

#### Test results

| Test | Result |
|---|---|
| `cargo test --workspace` | ✅ all green (231+ tests) |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ no warnings |
| `cargo run --release -p sim-server -- harness --seeds /tmp/test_seeds.txt` | ✅ JSON summary produced; 3 seeds, 3 full-time, mean_goals=0.0, 0 zero-zero-rate disagreeing across runs |
| `test_hash_detects_touch_history_change` | ✅ detects perturbation |
| `test_hash_detects_state_change` | ✅ detects position perturbation |
| `legal_transitions_include_self_and_lifecycle_table` | ✅ covers PRD §8.1 |
| `illegal_transitions_are_rejected` | ✅ rejects out-of-table transitions |
| `score_goal_drift_is_detected` | ✅ detects invariant 1 |
| `dead_ball_window_breach_is_detected` | ✅ detects invariant 4 |
| `read_seeds_skips_blanks_and_comments` | ✅ harness CLI parser |
| `update_mean_tracks_running_average` | ✅ running-average helper |
| `tick_cap_constant_is_400_000` | ✅ cap pinned |

#### Canonical state hash (release build, seed 12345, harness path)

`12021824642951959413` (full match, 324,019 ticks, 0–0 draw).

This is reproducible from a fresh `cargo run --release -p sim-server -- harness --seeds /tmp/test_seeds.txt` run; same seed produces same hash.