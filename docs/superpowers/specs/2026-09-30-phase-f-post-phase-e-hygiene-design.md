# Phase F: Post-Phase-E Hygiene — Design

**Date:** 2026-09-30
**Source review:** `docs/reviews/CODEBASE_REVIEW.md` §3-5 + the recent
turn-by-turn findings captured at the end of the `code-review` skill run
(see "Smell baseline" below).
**Path classification:** Architectural (one spec, 12 phased PRs).
**Status:** Approved design — pending implementation plan.

## Motivation

Phases A-E addressed correctness and structural items from the original
review. Several **hygiene** items remain:

1. There is no `CODING_STANDARDS.md`. Every reviewer re-derives the same
   rules ad hoc, and some of those rules are not written down anywhere
   (e.g. "no `HashMap` in hot paths" is implied by code comments, never
   stated).
2. Three `lib.rs` files exceed 500 LoC (the budget this spec adopts):
   `sim-core` (1807), `sim-ai-player` (1632), `sim-rules` (1357).
3. Several Fowler smells flagged in the review (and re-confirmed in the
   recent code-review turn) are unfixed.

This phase lands a standards doc and the refactors it prescribes.

## Smell baseline (from the recent code-review turn)

These were surfaced by the parallel Standards + Spec sub-agents in the
previous review. Each one has a PR assigned below.

| # | Smell | PR |
|---|-------|----|
| S1 | `Consideration::weight` / `curve` accessors are 18-arm-or-pattern matches repeated twice | **F1** |
| S2 | `min_by(... .distance.partial_cmp ...)` pattern duplicated 4× | **F2** |
| S3 | Tactical thresholds (5.0, 8.0, 12.0, 20.0, 30.0) scattered, unnamed | **F3** |
| S4 | `sim-ai-player/src/lib.rs` is 1632 LoC | **F4** |
| S5 | `apply_command` validation duplicated in `sim-core` and `sim-server` | **F5** |
| S6 | `sim-core/src/lib.rs` is 1807 LoC | **F6** |
| S7 | `sim-rules/src/lib.rs` is 1357 LoC | **F7** |
| S8 | `Intent` / `MovementIntent` / `ActionIntent` match repeated in ≥3 dispatch sites | **F8** |
| S9 | Per-tick `Vec<T>` allocations in `perception_system` | **F9** |
| S10 | `sim-replay` crate is dead code (no other crate imports it) | **F10** |
| S11 | `Data Clumps`: `(entity, position, team_id, velocity)` travels together | **F11** (PlayerSnapshot) |
| S12 | `Refused Bequest` / `pub` leakage: `DecisionEvaluationCount.0` was `pub` | **F0** (done in last turn) + codified in standards doc |

Note: the previous review also flagged `IntentKind` as Speculative
Generality. After inspection, `IntentKind::kind()` *is* used as a
dispatch tag (see `sim-components/src/lib.rs:114-116`), so the finding
is downgraded to "keep and document". Not in scope for this phase.

## What's in scope

The standards doc + 11 PRs (F1-F11), ordered by dependency so each PR
lands cleanly:

| Order | PR | Title | Files touched | Depends on |
|-------|----|-------|---------------|------------|
| 0 | **F0** | Add `docs/CODING_STANDARDS.md` | new file | — |
| 1 | **F1** | `refactor(sim-ai-player): Consideration accessors via macro` | `sim-ai-player/src/lib.rs` | F0 |
| 2 | **F2** | `refactor(sim-ai-player): extract closest_by_distance helper` | `sim-ai-player/src/lib.rs` | F0 |
| 3 | **F3** | `refactor(sim-ai-player): tactical thresholds as named constants` | `sim-ai-player/src/lib.rs` | F0 |
| 4 | **F4** | `refactor(sim-ai-player): split lib.rs into perception/decision/execution/brain submodules` | `sim-ai-player/src/lib.rs` + 5 new files | F1, F2, F3 |
| 5 | **F5** | `refactor(sim-core): consolidate apply_command into sim-core; sim-server delegates` | `sim-core/src/lib.rs`, `sim-server/src/lib.rs`, `sim-components/src/lib.rs` | F0 |
| 6 | **F6** | `refactor(sim-core): split lib.rs into simulation/ submodules` | `sim-core/src/lib.rs` + 5 new files | F5 |
| 7 | **F7** | `refactor(sim-rules): split lib.rs into clock/goals/possession/referee/oob submodules` | `sim-rules/src/lib.rs` + 6 new files | F0 |
| 8 | **F8** | `refactor(sim-ai-player, sim-components): centralise Intent dispatch via Intent::steer_target / Intent::kick / Intent::is_action` | `sim-ai-player/src/{brain.rs,execution.rs}`, `sim-components/src/lib.rs` | F4 |
| 9 | **F9** | `perf(sim-ai-player): Local scratch buffers in perception_system` | `sim-ai-player/src/perception.rs` | F4 |
| 10 | **F10** | `feat(sim-server): wire sim-replay into a `replay` subcommand in main.rs` | `sim-server/src/main.rs`, `sim-replay/src/lib.rs` | F0 |
| 11 | **F11** | `refactor(sim-components): introduce PlayerSnapshot struct for repeated (entity, position, team_id, velocity) clump` | `sim-components/src/lib.rs` | F4, F7 |

F0 is already partially done (the last turn delivered `DecisionEvaluationCount`
privatization and the dead-code removal); this spec folds those into the
"standards doc" PR so the audit trail is clean.

## What's out of scope

- **Spec §10 `tick % 22 == player_index` vs. SplitMix64 staggering** —
  gameplay-correctness change with no harness; defer.
- **Phase D `Query<&MatchClock>` → `Res<MatchClock>` migration** — touches
  `lifecycle_system`; defer to Phase F.2.
- **Cross-process determinism audit** (`pitch_control_system`,
  `possession_resolution_system`) — separate spec, separate PR series.
- **`RecordedSnapshot` round-trip tests** (`PartialEq` + bincode) —
  separate PR; was on the Phase E list but didn't land.
- **Removing `sim-replay`** — we're going the *other way* (F10 wires it
  up).
- **Per-crate `#![deny(clippy::pedantic)]`** — too noisy for the
  existing codebase; the standards doc recommends `pedantic` as the
  working set for *new* code but doesn't ban it workspace-wide.

## Design decisions (locked in)

1. **`docs/CODING_STANDARDS.md` lives at `docs/CODING_STANDARDS.md`**
   (next to `docs/reviews/`). Not at repo root, not under
   `docs/superpowers/`. Reasoning: groups with the existing review and
   phase documents.
2. **File-size budget is uniform: 500 LoC for `lib.rs` across all
   crates.** No per-crate tiers.
3. **`sim-components` owns `CommandError`.** After F5, `sim-core::Simulation::apply_command`
   returns `Result<(), sim_components::CommandError>`. `sim-server::ServerSimulation::apply_command`
   is a thin pass-through. This makes `sim-core`'s public API typed
   instead of `Result<(), String>`.
4. **`sim-replay` is wired into `sim-server` (F10).** The `replay`
   subcommand in `main.rs` calls `sim_replay::replay_with_ticks`. The
   determinism regression test in `sim-server/tests/determinism_test.rs`
   stays where it is (it tests `ServerSimulation` directly, not
   `sim-replay`).
5. **F1, F2, F3 land as standalone small PRs before F4 (the file
   split).** Reasoning: each small PR is reviewable in 5-10 minutes;
   the F4 split becomes mechanical once the inside is clean.
6. **F8 (Intent dispatch centralization) does not change public API
   surface.** `Intent::steer_target(&self, &PerceptionSnapshot) -> Option<Vec2>`
   and `Intent::kick(&self, &Velocity) -> Option<Vec2>` are added as
   inherent methods on `Intent` in `sim-components`. Existing call
   sites migrate to use them. New methods are `#[must_use]` const where
   possible.

## Detailed submodule layouts (per the standards doc §11)

These are the proposed submodule layouts for F4, F6, F7. Reviewers can
amend per-PR.

### F4 — `sim-ai-player/src/`

```
lib.rs              // DecisionEvaluationCount, DECISION_CADENCE_TICKS,
                    // player_stagger_slot, Consideration, ConsiderationContext
brain.rs            // UtilityBrain, PlayerAction, intent_kind
perception.rs       // PerceptionSnapshot, NearbyEntity, perception_system
decision.rs         // player_decision_system, consideration_scoring_system
execution.rs        // player_action_execution_system, kick_execution_system,
                    // steer, nearest_teammate_position, kick constants
tests.rs            // #[cfg(test)] mod tests
```

(The `Consideration::weight` and `Consideration::curve` accessors live
in `lib.rs` next to the enum; the F1 macro generates them there. They
do not need to move to `brain.rs`.)

### F6 — `sim-core/src/`

```
lib.rs                  // re-exports, Simulation, SimulationSet, view types,
                        // register_systems
simulation/
    mod.rs              // Simulation::new, Simulation::tick, Simulation::get_state,
                        // Simulation::create_match
    lifecycle.rs        // lifecycle_system, apply_player_slot
    formation.rs        // reset_formation_to_4_4_2
    brain_default.rs    // default_utility_brain
    state_hash.rs       // get_state_hash, discriminants
tests.rs                // #[cfg(test)] mod tests
```

### F7 — `sim-rules/src/`

```
lib.rs              // re-exports, CurrentTick, PendingRestart, AttackingDirection
out_of_bounds.rs    // out_of_bounds_system, calculate_oob_restart_position
goals.rs            // goal_detection_system, restart_system
possession.rs       // possession_resolution_system
referee.rs          // offside_detection_system, foul_detection_system,
                    // minimum_player_count_system
clock.rs            // added_time_calculation_system,
                    // match_duration_enforcement_system
tests.rs            // #[cfg(test)] mod tests
```

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| F1 macro misfires on a future `Consideration` variant | low | Const-assertion in test that exhaustively lists all variant names; macro emits a compile error if a variant is missed |
| F4 file split breaks a re-export chain | medium | Each new submodule has its own `#[cfg(test)] mod tests`; the standards-doc rule "lib.rs has only re-exports and primary types" is enforced by `cargo doc --no-deps` which fails on a missing re-export |
| F5 changes sim-core's public `Result<(), String>` to `Result<(), CommandError>` — call sites break | medium | Phase F covers all known call sites (`sim-server`, `sim-replay`); the spec lists them explicitly |
| F8 adds inherent methods on `Intent` in `sim-components` — downstream crates need updating | medium | F8 PR includes migration of all known call sites (`sim-ai-player` only; no other crate pattern-matches on `Intent` variants directly per the diff audit) |
| F10 wires `sim-replay` and a previously-unused crate hits a hidden bug | low | The existing 5 `sim-replay` tests pass today; F10 surfaces any startup-time issues via the CLI integration test |

## Test strategy

Each PR lands with the existing 75-test suite staying green. New tests
added per PR:

- **F1**: a `const _: () = assert!(...)` that the macro emitted all 18
  variant names.
- **F2**: a unit test that `closest_by_distance(&[])` returns `None`,
  `&[a]` returns `Some(a)`, etc.
- **F3**: a unit test that `TACTICAL_RANGES::DISTANCE_TO_BALL_THRESHOLD_M`
  matches the old literal value used in `DistanceToBall`'s arm.
- **F5**: the existing `sim-server::tests::test_validation_*` tests stay
  green; add a new test that `sim-core::Simulation::apply_command` returns
  the same `CommandError` variants.
- **F8**: a `Intent::kick` table test that covers each
  `MovementIntent` / `ActionIntent` variant.
- **F10**: end-to-end CLI test that `football-sim replay --seed 42
  --commands foo.json --output bar.json` produces the same `bar.json`
  as a direct `sim_replay::replay` call.

Long-running tests (full-match, 324 000 ticks) remain `#[ignore]`d per
the existing convention.

## Documentation deliverables

- `docs/CODING_STANDARDS.md` (F0) — the new doc.
- `docs/superpowers/specs/2026-09-30-phase-f-post-phase-e-hygiene-design.md`
  (this file).
- Per-PR update to `docs/reviews/CODEBASE_REVIEW.md` §5 checklist
  ticking off the items as they land.
- Update to `CLAUDE.md` / `AGENTS.md` (if it exists) noting that the
  standards doc is authoritative.

## Implementation handoff

Once this spec is approved by the user, the next step is to invoke the
`writing-plans` skill to produce the per-PR implementation plan. Each
PR's plan should be small enough to execute in one agent session.

## Appendix A: Pre-spec verification commands

These are the commands run to verify the design decisions in this spec.
They were run on the working tree at commit `a6649c9` + the last turn's
bug fixes.

- `wc -l crates/*/src/*.rs` → confirms file sizes cited above.
- `grep -rn "use sim_replay" crates/` → empty (confirms S10).
- `grep -rn "match self {" crates/sim-ai-player/src/lib.rs` → confirms
  the 18-arm pattern matches at lines 84, 108.
- `grep -rn "min_by(|a, b| a.distance.partial_cmp" crates/sim-ai-player/src/lib.rs` →
  confirms 4 sites.
- `grep -rn "apply_command" crates/sim-core/src/lib.rs crates/sim-server/src/lib.rs` →
  confirms the duplication.
- `cargo clippy --workspace --all-targets -- -D warnings` → green at start.
- `cargo test --workspace --lib` → 75 passed at start.
