# 05: Free-ball retrieval fix (decision-trace driven)

**What to build:** In a match with a free ball, the closest non-possessing player moves toward the ball and acquires possession within a bounded window. PRD §3 marks this as a "Likely / Guess" — the cause is unknown until decision traces are read, so the ticket begins by capturing a decision trace of a degenerate match where the ball lingers free for long stretches, identifying the failing predicate (intent decision, target calculation, stamina gate, role gate, or proximity check), then fixing the failing predicate and re-running. Whatever is broken is documented by `TraceGate`-recorded traces from `sim-telemetry` (the existing developer tooling); the fixed behaviour is asserted over 10 seeds by a "ball resolves to a possessor within N ticks of going free" measurement.

**Blocked by:** 04 (OOB restarts can also leave free balls; the fix overlaps enough that OOB must land first to avoid hiding the bug).

**Status:** ready-for-agent

- [ ] A captured decision trace of the failing match is committed under `## Comments` with a pointer to which predicate was wrong.
- [ ] Whichever predicate was wrong is fixed; the chosen player moves toward the ball and acquires it within a bounded number of ticks (initial assumption: 90 ticks = 1.5 s of game time).
- [ ] Measurement over 10 seeds shows no free-ball window exceeding PRD §13 A2 (≤ 30 s of game time).
- [ ] Windowed smoke test still passes (PRD §14).
- [ ] `cargo test --workspace` green; `final_state_hash` for canonical seed recorded in `## Comments`.
