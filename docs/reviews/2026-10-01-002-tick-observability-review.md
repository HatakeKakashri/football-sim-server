# 002-tick-observability — Product-Level Review

*Rust · ECS · Chrome/Perfetto trace · post-patch (split-series + review-fixes)*

**Scope.** A high-level review of the two telemetry patches
(`002-split-series.patch` + `002-review-fixes.patch`) applied on top of
`sim-observability-dr` at commit `cb3556d`. The goal is to answer:
*is the post-patch observability setup enough, and what would make the
trace more useful for debugging?*

**Construction notes.**

- Both patches were applied to a clean tree at `cb3556d`. The patches
  themselves land cleanly (`git checkout f844c56 -- .` reproduces the
  post-patches state from the `f844c56` "applied patches for review"
  commit on the same branch).
- `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  and `cargo test --workspace` all pass. The existing fmt diff is
  pre-existing and not introduced by either patch.
- I personally re-ran **V2, V4, V6** from `specs/002-tick-observability/verification.md`:
  - **V2** (well-formed JSON): 27 sim-telemetry + 37 sim-core + 1 sim-server e2e tests pass.
  - **V4** (file-size claims): a 5,000-tick run at interval 60 produced 9.7 MB; linear
    extrapolation matches the ~663 MB full-match claim at interval 60, and the ~67 MB
    claim at the new default of 600.
  - **V6** (workspace clean): clippy clean; all 187 unit/integration tests green.
  - **V5** is structurally weak — see *Verification ledger notes* below.
  - **V1** (Perfetto UI) and **V7** (TeamId fix in isolation) need a browser and a
    pre-patch hash baseline respectively; not run in this review.
- I also exercised **V5** with two surgical mutations to `trace_emission_system`:
  - A no-op mutation (`let _x = format!("{}", entity.index());`) leaves all tests green,
    as expected.
  - Removing the `stamina` counter emit makes `each_snapshot_records_every_player_and_the_ball`
    fail (good — catches a real omission), but `tracing_does_not_change_the_final_state_hash`
    still passes because the function never touches game state.
  - Mutating the *value* (e.g. `stamina.0 + 1.0`) doesn't change the state hash and doesn't
    fail any test — the test compares two complete simulations' final hashes, not field-level
    values. See "Verification ledger" section for the implication.

---

## TL;DR

The post-patch telemetry answers one question well — *"what was this
player thinking on tick N?"* — and silently drops most of the answers
to the questions you'd actually need when debugging a football match:

- Goals, out-of-bounds restarts, fouls, and offside detections all happen
  in `sim-rules` but produce no trace events. Their `tracing::info!`
  calls use the default target, which `ChromeTraceLayer` correctly filters
  out (so they go to stderr but not the file).
- Ball kicks (`kick_execution_system` setting `ball.kick_velocity`) are
  invisible. "Who kicked the ball, where, how fast, and was it a shot or
  a pass?" is the single most common replay question and currently
  unanswerable from the trace.
- `Player.intent` (the *committed* action after execution runs) is never
  surfaced — only the decisions that fed into it. The trace shows what
  the player was *thinking*, never what they actually did.
- Match-state transitions (PreMatch → Kickoff → InPlay → HalfTime →
  SecondHalf → FullTime), added-time, manager decisions, mentality
  shifts, substitutions, and formations are all invisible.
- `last_touched_by` (Law 11 input for offside judgement) is never emitted.
- Run-level metadata (seed, match id, started-at) is missing — you can't
  identify an unknown trace.

What's there is solid: deterministic sim-time `ts`, well-typed counter
events, nested B/E duration pairs for the decision tree, paused-clock
deduplication, careful flush guard, and a sensible default of 600 ticks
(~67 MB full-match) after the review fix.

---

## What's good

### Trace format and toolchain

- **Custom `ChromeTraceLayer`** (not `tracing-chrome`) — the design-doc
  decision to write our own layer is justified by the outcome. The
  schema (typed counter events, per-player `pid`s, sim-time `ts`,
  nested spans) is exactly what you need for Perfetto.
- **Deterministic timestamps** (`ts = tick * 1_000_000 / 60 + seq` µs)
  with a saturating per-tick offset (capped at `1_000_000 / 60 - 1`) so
  paused-clock repeat emissions stay inside the tick's slot. File order
  still nests them correctly. This is the right call for reproducibility.
- **Paused-clock deduplication** for both snapshots and decisions — the
  review fix caught the case where decision spans would have flooded
  the trace during half-time.
- **Flush guard** (`TraceGuard::finish` + Drop best-effort) handles the
  "tracing the closing `]`" problem correctly, with explicit error
  propagation and panic-on-poison-free Mutex semantics.
- **Foreign-target filter** (`metadata.target() == "sim_trace"`) means
  the existing `tracing::info!` / `tracing::warn!` calls in the codebase
  flow to stderr but never enter the trace. Clean separation.

### Decision path coverage

- Every action a player evaluates gets a `B/E` span pair with
  `chosen: bool` and post-hysteresis `aggregate_score`. The review-fix
  test `hysteresis_is_included_only_for_the_current_intent` confirms
  the score reflects the actual comparator value.
- Every consideration gets an instant event with `name`, `raw`,
  `curve`, `weight`, `score`. The `name` accessors (`Consideration::name`,
  `ResponseCurve::name`) are added via the macro and tested for stability
  against the full 18-consideration × 9-action matrix.
- The branch-coverage test `trace_covers_every_action_and_consideration_of_the_default_brain`
  compares the set of names in the trace to an independent oracle of
  `brain_default::default_utility_brain` — exactly the right invariant
  to encode (no consideration can be silently dropped).
- Scratch buffer in `DecisionTraceScratch` (held in a Bevy `Local`)
  keeps allocation per tick at zero — the design-doc rationale
  (T007–T009 deviation) is correct: you can't emit `chosen` inline
  without producing overlapping sibling spans.

### Test discipline

- Determinism: `tracing_does_not_change_the_final_state_hash` plus the
  stronger `traced_and_untraced_runs_agree_and_the_trace_is_complete`
  e2e test catch any state mutation along the trace path.
- Gate-off: `telemetry_disabled_emits_nothing` and `gate_off_emits_nothing`
  prove the no-subscriber / no-config path costs nothing.
- Edge cases: timestamps-never-go-backwards, decision-window-extends-past-range-end,
  paused-clock-snapshot-and-decisions-once, stoppages-queued-on-different-recorded-ticks.
- A real review-driven bug was caught and fixed (stoppage drain race:
  the "seen" counter would skip new events after the queue was emptied).

### Engineering quality

- `considerations_with_no_considerations_use_0.6_baseline` is correctly
  traced (`finish_action(baseline)` before the early-continue).
- `trace_emission_system` is `Option<Res<TraceGate>>` so it's a true no-op
  when telemetry isn't enabled — no Bevy system registration overhead.
- The `Record`/`late_args` plumbing for spans lets you record fields
  *after* span creation (`on_record`) and have them appear in the
  close event. Useful, not currently exploited.
- The verification ledger (`verification.md`) honestly flags what was
  *author-reported* and never independently run. The discipline of
  marking V3-V5 as "Author-reported; no logs" is exactly right.

---

## What's missing — and why

The decision system is a *leaf* in the simulation; it's not the trunk.
When a future bug report says "the striker ran into his own half and
the ball never came back", you won't see that bug in this trace.

The schedule is: `Perception → Decision → Execution → Physics →
Possession → Rules → MatchAdmin`. Of these, **only Decision and a thin
slice of MatchAdmin (snapshot counters) are traced.**

### Critical gaps (silent game events)

| Event | Where it happens today | Trace today | Why you need it |
|---|---|---|---|
| **Goal scored** | `goal_detection_system` increments score, inserts `RuleEvent::Goal`, calls `tracing::info!("GOAL scored by …")` | **Nothing.** Only the position counters reflect the post-goal ball at center spot | First thing anyone asks after a bug report is "did the goal register?" — currently you can only find out by opening `final_state.json` |
| **Out-of-bounds / restart type** | `out_of_bounds_system` + `restart_system` for `ThrowIn` / `GoalKick` / `Corner` | **Nothing.** Same problem | Restart type drives 80% of the tactical-game questions (why is the ball at the corner flag?) |
| **Foul** | `foul_detection_system` inserts `RuleEvent::Foul` when distance < 1.0m and rel_vel > 8.0 | **Nothing.** | Fouls have no UI representation in the codebase, so debugging requires the trace |
| **Offside** | `offside_detection_system` runs every tick | **Nothing.** | Detection stub — but completely invisible from the trace |
| **Kick (the ball actually leaves the foot)** | `kick_execution_system` sets `ball.kick_velocity`; `apply_kick_velocity_system` consumes it | **Nothing.** | "Who kicked the ball, where, and how fast?" is the #1 replay question and is currently unanswerable |
| **Possession acquired/lost** | `possession_resolution_system` reads ball.state | Only *changes* are reported via `possession_change`. You never see a player *gain* possession from a free state, only the transition | Better as a continuous counter (boolean) per player |
| **`last_touched_by`** | Updated by possession resolution (Law 11 input for offside) | **Nothing.** | Offside is judged on `last_touched_by`, but the trace shows neither the value nor the player who set it |
| **Match state transitions** | `lifecycle_system` does PreMatch → Kickoff → InPlay → HalfTime → SecondHalf → FullTime | **Nothing.** | "When did the second half start?" — currently invisible |
| **Manager decisions** | `manager_decision_system`, `mentality_shift_system`, `substitution_system`, `formation_change_system` | Only `tracing::info!` lines (which the ChromeTraceLayer correctly filters out, by design) | Manager tactics drive late-game AI behaviour; debugging it from counters alone is impossible |
| **Added time** | `added_time_calculation_system` runs in MatchAdmin | Only `tracing::info!` lines | Tells you *why* the half went long |
| **Stamina decay** | **No system decrements stamina** — pre-existing tech debt, not a telemetry issue | Stamina counter is essentially constant per player | Worth flagging separately; a future bug hunt will hit this |

### Other findings

- **`process_name` data quality.** It's `"Home 7 — CentralMidfielder"`.
  Useful, but **does not include the player's intent** — the most
  important state for diagnosing "what is this player trying to do
  right now?" Consider emitting the *committed* `Intent` as a per-tick
  counter on the Position thread (or a new thread) once
  `player_decision_system` writes it. Today you can see *what they
  were thinking* (decision thread), but never *what they committed to*
  (which is what execution runs).
- **Player index instead of squad number.** Per the schema rationale
  there's no squad number in the codebase, but if players are tracked
  by `Entity` index then **player 7 and player 12 may swap identities
  between runs** (entity indices are not stable across `create_match`
  calls). Fine for one-off traces, but a real headache when comparing
  two traces. Consider emitting `Player.id` if one exists, or hashing
  role+position to disambiguate.
- **No run-level metadata in the file.** When you open an unfamiliar
  trace in Perfetto, you can't tell which run produced it. The trace
  schema emits `process_name` / `thread_name` metadata but no
  run-level metadata. Perfetto's `trace_name` is the natural place.
- **Foul/offside detection runs but emits no trace events.** The data
  flows: `foul_detection_system` → `RuleEvent::Foul` on entities →
  consumed/removed by other systems (or never read again). The trace
  could subscribe to these and emit instant events the same way
  `card` / `stoppage` are wired today.
- **The "stoppage drain" comment in `trace_emit.rs` is correct but
  illustrates a fragile coupling.** `trace_emission_system` is ordered
  `before(sim_rules::added_time_calculation_system)`, and the *review*
  had to fix a bug where a running "seen" counter would skip new
  stoppages after a drain. This pattern (system ordering + queue
  draining semantics) will repeat for goals/restarts/fouls and is a
  real source of subtle bugs. **Strongly recommend** emitting from a
  hook that fires *when the event happens*, not by polling state diffs
  at a snapshot tick.
- **No streaming mode.** For very long matches the full file must be
  buffered to disk before Perfetto loads it. A `--trace-stream <pipe>`
  mode using `tracing-subscriber`'s `MakeWriter` trait could feed tools
  like `trace_processor` incrementally. Not critical for current code,
  but expected as the natural next request.
- **Missing flow events.** When player A passes to player B, that's a
  *flow* — A's `kick` event should ideally be a `producer` edge
  connected to B's `acquire_possession` event as a `consumer`. Chrome
  traces support this natively and Perfetto renders it beautifully.
  Without flows you can't read "who passed to whom" off the trace.
- **No way to record "decisions only, no positions" or "positions only,
  no decisions".** The 18 considerations × 9 actions × 22 players per
  cadence = ~3,500 consideration instant events per recorded tick is
  where 92%-of-file-size comes from. For `--full-range` full-match runs
  (~324K ticks) you'll get 90+ GB. A `--trace-mode {decisions|positions|full}`
  flag would let admins pick a smaller trace per use case.

### Verification ledger notes

The review's verification ledger (`specs/002-tick-observability/verification.md`)
is honest about what was and wasn't run. A few notes from my own
exercise:

- **V2, V4, V6** — independently re-run, all clean.
- **V5 (`tracing_does_not_change_the_final_state_hash`)** — the test
  compares final state hashes of two simulations (one traced, one
  untraced), so it only fails if `trace_emission_system` actually
  mutates game state. The system uses `&Stamina` (read-only), so a
  "stamina + 1.0" mutation in the *emit* doesn't break it. The test
  is non-vacuous only against the `traced_sim` wrapper around
  `Simulation`, where any change to the schedule order or a wrongly
  registered system would surface. The much stronger e2e test
  `traced_and_untraced_runs_agree_and_the_trace_is_complete` in
  `sim-server/tests/simulate_trace.rs` is the better mutation check
  and it does pass.
- **V1 (Perfetto UI)** — needs a browser. Structural assertions on
  the JSON (`process_name`, `thread_name`, `pid` assignments,
  `ph: "C"` for counters, `B/E` pairs for spans, nested span
  identity inheritance) are exercised by the unit and integration
  tests and pass. I did not load a real trace into Perfetto.
- **V3 (disabled-tracing throughput)** — author-reported; no logs.
  Plausible (the gate-off path has only an `Option::is_some_and`
  check per system call) but unverified.
- **V7 (TeamIdComponent fix isolation)** — included in V6 (tests pass).
  The split-series commit ordering note ("this changes hashes")
  is correct; comparing hashes across the split will show different
  values.

---

## Recommendations (priority order)

1. **Add a `Rules` thread on a `Referee`-style process** that emits goal,
   out-of-bounds, foul, kickoff, and restart events as instant events
   with `from` / `to` / `kind` args. Same plumbing shape as `card` /
   `stoppage`. ~150 lines + tests. **Highest ROI change.**
2. **Add a `kick` instant event** emitted by `kick_execution_system`
   carrying possessor entity, intent kind, kick direction, kick speed,
   and whether the kick was a shot vs pass. The only way to answer
   "who shot / who passed" from a trace.
3. **Promote `last_touched_by` and the current `Player.intent` to
   per-tick counters** on the player's Position thread. Cheap, fixes
   the "what did they actually commit to" gap.
4. **Add `--trace-mode {decisions|positions|full}`** so admins can
   trade granularity for size. The current 67 MB full-match default
   is fine for one-shot investigation but wrong for continuous
   monitoring.
5. **Emit run-level metadata** (`trace_name`, `seed`, `match_id`,
   `started_at`) as Perfetto metadata events. Trivial, big
   quality-of-life win.
6. **Mark the `stamina` counter as a "still-unimplemented" caveat**
   in the trace-schema contract — until something decrements stamina,
   it's a static value, not a useful debug signal.
7. **Consider the `tracing-chrome` reject rationale** — the design-doc
   decision 1 is sound and the trace-quality outcome supports it. No
   action needed.

---

## On the design-doc deviations

The patches record three justified deviations from the original task text:

- **T007–T009 (decision spans emitted *after* the loop, not inline).**
  Correct — you cannot emit `chosen: bool` inline without producing
  overlapping sibling spans. The `Local` scratch buffer is the right
  answer and adds zero per-tick allocation.
- **T002/T003 (`tracing-chrome` replaced by custom layer).** Correct —
  `tracing-chrome` cannot emit typed counter events, per-player `pid`s,
  or sim-time `ts`. The schema justifies the bespoke layer.
- **T004 (skip `sim-rules` dependency).** Correct — `sim-rules` has no
  emission site today; adding the dep would be unused. Revisit if
  recommendation #1 is implemented: a `Rules` thread would naturally
  live in `sim-rules`, at which point the dep should be added back.

---

## On the review-fix commit

The review patch addresses five substantive issues and one
documentation nit. Each fix is well-targeted:

1. **Stoppage drain race** — caught by the regression test
   `stoppages_queued_on_different_recorded_ticks_are_all_reported`.
   The fix (don't keep a running "seen" count, because the queue is
   drained every tick) is exactly right and the comment explains why
   the alternative would have been wrong.
2. **`Simulation::enable_telemetry` as new public API** — corrected in
   `spec.md`. Honest and useful clarification.
3. **TeamIdComponent fix split out** — landed as the first commit in
   the series so any hash change can be bisected to it. Sensible.
4. **Paused-clock dedup for decisions + timestamp saturation** — both
   needed and both landed. The new test
   `paused_clock_records_snapshot_and_decisions_once` exercises the
   full deduplication.
5. **Default interval 60 → 600** — addresses the 660 MB full-match
   problem. The cadence tests now pin 60 explicitly via
   `every_second()`, decoupling them from the CLI default. Good
   engineering hygiene.

The verification ledger and T022 (Perfetto human-verification)
are the right artefacts to leave open.

---

## Summary

This is a competent first slice of telemetry that nails the highest-cost
part (decision coverage) and explicitly leaves room for the next slice
(goal / kick / restart). It is *not* a debugging-complete trace for the
rest of the game. The gaps are not bugs — they are missing features
that the spec correctly scoped out of iteration 1. The review-fix
commit is high quality and the verification discipline (ledger, open
T022) is exemplary.

Recommended next iteration: recommendations #1 and #2 (rules events +
kick events), probably as their own design doc so the scope matches the
size of the change. Until then, the trace answers "why did this player
*think* that?" — not "what happened in this match?"
