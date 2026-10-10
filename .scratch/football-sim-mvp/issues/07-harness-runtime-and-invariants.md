# 07: Multi-seed headless harness + invariants + touch-history hash mix-in

**What to build:** A `sim-server benchmark` follow-up or new `sim-server harness --seeds <file>` subcommand that runs N seeds, never serialises per-tick snapshots, and reports the §13 metrics (full-time reach, longest dead-ball window, mean goals, 0–0 rate, strong-side win rate, determinism byte equality on repeated seeds, score vs goal count, event order, tactic signature proxies). Inside the simulation loop, runtime invariants from PRD §12 run in release builds too: `score == goal event count per team`, `MatchClock` monotonic, legal state transitions, no dead-ball window over 30 s, valid ball/player states, `FullTime` reached before `tick_cap`. Any violation emits a failed-match `error` event (the §10 wire shape), caches the failure (§6 hardening will reuse the cache), and logs the seed. The two-deep touch history on `Ball` (§8.5) is folded into the existing `state_hash` calculation so swaps to the touch path change the hash deterministically.

**Blocked by:** 06 (need a playable loop before invariants have anything to assert against).

**Status:** ready-for-agent

- [ ] `sim-server harness` runs N seeds in release mode, writes a JSON summary, never serialises per-tick snapshots.
- [ ] All six PRD §12 invariants run in release; any violation fails the match with the §10 `error` shape and a logged seed.
- [ ] `Ball` two-deep touch history is hashed into `state_hash`; a unit test asserts the hash changes deterministically when the touch history changes.
- [ ] Empty-events harness still reports §13.1 metrics that don't require events (e.g., A1, A2, A6).
- [ ] `cargo test --workspace` green.
