# Football-Sim Server – Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Using superpowers:subagent-driven-development (recommended) or superpowers:executing-plans.

## Goal
Refactor the football-sim-server to eliminate three architectural seams:
1. **Match setup** – fixtures validated at startup but not used; every match spawns default squads.
2. **Tick loops** – five separate loops (runner, harness, simulate, lib, replay) duplicate invariant bookkeeping.
3. **Rule-to-event signals** – nine order-sensitive emitters scattered across `sim-rules` and `sim-core`.

## Architecture

### Match Setup
- **Current:** `Simulation::create_match` spawns 22 default squads regardless of fixture config. Fixtures are validated early but discarded.
- **Target:** One `MatchSetup` adapter (similar to `TeamSetup`) owns squad spawn and resource installation. Two adapters (home/away) justify the seam.
- **Files to modify:**
  - `crates/sim-core/src/simulation/construct.rs` – extract setup logic into a dedicated module/function.
  - `crates/sim-core/src/simulation/register_systems.rs` – wire the new `MatchSetup` adapter into the schedule.
  - `crates/sim-core/src/simulation/construct.rs` – call the new setup function instead of duplicating logic.

### Tick Loops
- **Current:** Five independent loops (`runner.rs`, `harness.rs`, `simulate.rs`, `lib.rs`, `sim-replay/lib.rs`) each advance the match clock and collect metrics. Invariants (score, half-time, added time) are tracked separately in each loop.
- **Target:** Single `MatchDriver` tick loop that advances the match, enforces invariants, and aggregates metrics. Four adapters (real match, HTTP harness, replay, benchmark, simulation) plug in as adapters.
- **Files to modify:**
  - `crates/sim-core/src/simulation/tick.rs` – consolidate into a single `tick()` method.
  - `crates/sim-server/api/runner.rs` – remove redundant loop, delegate to `MatchDriver`.
  - `crates/sim-server/harness.rs` – remove redundant loop, delegate to `MatchDriver`.
  - `crates/sim-server/lib.rs` – remove redundant loop, delegate to `MatchDriver`.
  - `crates/sim-replay/lib.rs` – remove redundant loop, delegate to `MatchDriver`.

### Rule-to-Event Signals
- **Current:** Nine order-sensitive emitters (`resolve_pass_events`, `emit_goal_events`, `emit_kickoff_restart_events`, `emit_tackle_events`, `emit_cross_header_events`, `emit_oob_and_restart_events`, `emit_possession_change_on_acquisition`, `emit_possession_change_on_oob`, `resolve_shot_events`) are scattered across `sim-core/simulation/` and `sim-rules/`. The ordering is fragile and error-prone.
- **Target:** Collapse all into a single `event_emit.rs` funnel. The sequence remains the same (pass → goal → kickoff → shot → tackle → cross → OOB/restart → possession change), but all systems write to a unified `EventLog` resource in a deterministic order enforced by the schedule.
- **Files to modify:**
  - `crates/sim-core/src/simulation/event_emit.rs` – centralize all emission logic; ensure ordered execution via the existing `Schedule` chain.
  - `crates/sim-rules/` – remove redundant emission logic; systems should only populate the `EventLog` resource.
  - `crates/sim-core/src/simulation/construct.rs` – ensure `PendingPass`, `PendingShot`, `PendingTackle`, `PendingOutOfBounds`, `GoalForEmit`, `OobRestartForEmit` are properly drained by the centralized emitter.

## Task Breakdown

### Task 1: Match Setup Refactor
- [ ] Extract match setup logic into `MatchSetup` module/struct.
- [ ] Create two adapters: `HomeMatchSetup` and `AwayMatchSetup`.
- [ ] Wire adapters into `RegisterSystems` schedule.
- [ ] Replace `Simulation::create_match` with a call to the new setup function.
- [ ] Verify that fixture-based squads are now used instead of default squads.

### Task 2: Unified Tick Loop
- [ ] Consolidate `Simulation::tick()` into a single method that runs the full match timeline.
- [ ] Implement invariant enforcement (score, half-time, added time) inside the tick.
- [ ] Remove the five legacy loops (`runner`, `harness`, `simulate`, `lib`, `replay`).
- [ ] Create adapter systems: `RealMatch`, `HttpHarness`, `Replay`, `Benchmark`, `Simulation`.
- [ ] Ensure each adapter plugs into the unified tick loop.

### Task 3: Centralized Rule-to-Event Signals
- [ ] Review `event_emit.rs` to confirm all nine emitters are present and correctly sequenced.
- [ ] Simplify individual rule systems to only populate the `EventLog` resource.
- [ ] Verify the schedule chain in `event_emit.rs` maintains the correct order (pass → goal → kickoff → shot → tackle → cross → OOB/restart → possession change).
- [ ] Add defensive checks to prevent duplicate emissions (e.g., drain signals before emitting).

## Testing
- Unit tests for `MatchSetup` (fixture vs default squads).
- Integration tests for the unified tick loop (score tracking, half-time, added time).
- Full-match simulations comparing old vs new behavior (ensure behavioral parity).

## Risks & Mitigations
- **Risk:** Breaking existing test worlds that rely on default squads.
  **Mitigation:** The new `MatchSetup` adapter preserves backward compatibility by accepting the same `TeamSetup` inputs; the difference is internal squad selection.
- **Risk:** Ordering of events may differ slightly due to scheduling nuances.
  **Mitigation:** The existing `Schedule` chain and `EventLog` resource guarantee deterministic ordering; run regression tests against the official PRD.

## Deliverables
- Updated `crates/sim-core/src/simulation/construct.rs`
- New `MatchSetup` module (likely `crates/sim-core/src/simulation/match_setup.rs`)
- Updated `crates/sim-core/src/simulation/register_systems.rs`
- Consolidated `crates/sim-core/src/simulation/tick.rs`
- Adapter systems for the unified tick loop
- Refactored `crates/sim-rules/` emission logic
- Updated `crates/sim-server/api/runner.rs`, `harness.rs`, `lib.rs`, `sim-replay/lib.rs`

## Success Criteria
- All three seams are eliminated (no more duplicate loops, no more fixture leaks, no more scattered event emitters).
- Existing tests pass (including the ignored `phase_0_playability_gate`).
- Match setup now uses fixture-derived squads instead of default squads.
- Tick loop runs a full match with correct invariant enforcement.
- Rule-to-event signaling is deterministic and matches the PRD ordering.
