# Tasks: Tick Observability (Post-Match Decision Trace)

**Input**: Design documents from `/specs/002-tick-observability/`

**Prerequisites**: plan.md (required), spec.md (required for user stories), data-model.md, contracts/trace-schema.md

**Tests**: Included — spec.md's success criteria (SC-001, SC-004) are directly testable and this feature touches determinism-sensitive code paths.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story, per this repo's existing convention.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3, US4)

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: New crate scaffolding and workspace wiring, no behavior yet

- [ ] T001 Create `sim-telemetry` crate: `TelemetryConfig` struct, `should_record(tick, config) -> bool`, and CLI-arg parsing helpers for `--trace-interval-ticks` / `--trace-full-range` validation (reject `interval_ticks == 0`, reject `start > end`, clamp out-of-bounds ranges)
- [ ] T001a [P] Add `Consideration::name()` method to `sim-ai-player` returning snake_case variant identifiers (e.g. `"distance_to_goal"`)
- [ ] T001b [P] Add `ResponseCurve::name()` method to `sim-ai-core` returning variant identifiers (e.g. `"linear"`, `"logistic"`, `"step"`)
- [ ] T002 [P] Add `tracing-chrome` to the workspace `Cargo.toml` (`tracing` already present)
- [ ] T003 [P] In `sim-telemetry`, add the `tracing-chrome` subscriber bootstrap function returning the flush guard, taking the `--trace-out` path
- [ ] T004 Add `sim-telemetry` as a dependency of `sim-core`, `sim-ai-player`, `sim-rules`, `sim-server`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Per-tick plumbing that every user story's emission code depends on

- [ ] T005 In `sim-core::Simulation::tick()`, compute `should_record()` once per tick and insert/update a resource carrying that flag (mirrors the existing `CurrentTick` resource insertion pattern)
- [ ] T006 In `sim-server`, wire `--trace-out`, `--trace-interval-ticks` (default 60), `--trace-full-range` onto the `simulate` subcommand; construct `TelemetryConfig`; install the subscriber (and hold its guard for the run's duration) only when `--trace-out` is present; reject `--trace-full-range` without `--trace-out` at startup (FR-011)

**Checkpoint**: Foundation ready — user story emission work can begin.

---

## Phase 3: User Story 1 - Post-Match AI Decision Debugging (Priority: P1) 🎯 MVP

**Goal**: Full per-action, per-consideration decision traces visible in Perfetto for recorded ticks.

**Independent Test**: Run a short simulation with tracing enabled, open the file in Perfetto UI, confirm every action considered by a player at a recorded tick is visible with all of its considerations' raw/weight/curve/score values, and the chosen action is marked.

- [ ] T007 [US1] In `sim-ai-player::player_decision_system`, read the per-tick "should record" resource; when true, open a `tracing::span!("player_decision", tick, player = ?entity)` for the player
- [ ] T008 [US1] Within that span, wrap the existing per-action scoring loop with a nested `tracing::span!` per action (name derived from `Intent` variant, e.g. `"Shoot"`, `"Pass"`, `"HoldPossession"`), recording `aggregate_score` and `chosen` as span fields once the winner is known
- [ ] T009 [US1] Within each action span, emit a `tracing::event!` per consideration evaluated, with `raw`, `curve`, `weight`, `score` fields, at the point those values are already computed in the existing loop — no restructuring of the loop's return type
- [ ] T010 [US1] Integration test: run ~120 ticks with tracing enabled to a temp file, parse the output as JSON, assert every expected action/consideration name appears at least once

**Checkpoint**: US1 independently testable and demoable — this is the MVP.

---

## Phase 4: User Story 2 - Coarse Whole-Match Overview (Priority: P1)

**Goal**: Default interval keeps a full-match recording small; disabled tracing costs nothing.

**Independent Test**: Run a full-length simulation with only the default interval; confirm snapshot count is proportional to `total_ticks / interval_ticks`. Run without `--trace-out`; confirm no file is produced.

- [ ] T011 [US2] Confirm (via T005's resource) that non-selected ticks skip all emission call sites added in Phase 3 — add a unit test asserting `should_record()` returns false for the expected majority of ticks at the default interval
- [ ] T012 [US2] [P] Unit tests for `should_record()` boundary conditions: exact interval multiples, one-past-boundary, `full_range` edges
- [ ] T013 [US2] Verify (manual or automated benchmark, not gating) that a `simulate` run without `--trace-out` shows no installed subscriber and no meaningfully different tick throughput versus pre-feature baseline (SC-002 — qualitative, not a hard numeric gate per spec.md)

**Checkpoint**: US2 independently testable — coarse default behaves correctly and disabled path is free.

---

## Phase 5: User Story 3 - Dense Investigation Window (Priority: P2)

**Goal**: `--trace-full-range` forces every-tick resolution across an explicit range, merged into the same file as the coarse recording.

**Independent Test**: Run with both a coarse interval and a full-resolution range set; confirm every tick in range is fully recorded and no tick is double-recorded.

- [ ] T014 [US3] Extend `should_record()` (from T001) to OR the coarse-interval check with the full-resolution-range check, de-duplicating ticks that satisfy both (already covered by T001's implementation — this task is the test)
- [ ] T015 [US3] Integration test: run with `--trace-interval-ticks 60 --trace-full-range 100-160`, confirm exactly one recorded snapshot per tick in `[100, 160]` (not two) and coarse-interval snapshots elsewhere, all in one output file

**Checkpoint**: US3 independently testable.

---

## Phase 6: User Story 4 - Ball, Referee, and Full-Pitch Context (Priority: P2)

**Goal**: Ball state on every recorded tick; referee state when present; no error when absent.

**Independent Test**: Confirm ball counters/events appear at every recorded tick; confirm a run with no `Referee` entity completes without error and with no Referee process in the trace.

- [ ] T016 [P] [US4] Add a ball-state emission system (or extend an existing per-tick system) reading the "should record" resource, emitting `x`/`y`/`vx`/`vy`/`spin` counters and `state_change`/`possession_change` instant events per `contracts/trace-schema.md`
- [ ] T017 [P] [US4] Add a referee-state emission system gated on `Query<&Referee>` being non-empty, emitting `card`/`stoppage` instant events per `contracts/trace-schema.md`; confirmed to no-op (not error) when the query is empty
- [ ] T018 [US4] Integration test: record a short match, confirm ball data present at every recorded tick, confirm no panic/error occurs given the current codebase's lack of a spawned `Referee` entity

**Checkpoint**: All user stories independently functional; full feature demoable end-to-end.

---

## Phase 7: Determinism Verification & Cleanup

**Purpose**: Cross-cutting correctness and the explicit cleanup decision from this session's clarifications

- [ ] T019 Test (SC-004): run the same seed/tick-count simulation twice, once with `--trace-out` set and once without; assert the final state hash is identical
- [x] T020 Remove the existing "DEBUG INSTRUMENTATION (temporary, for phase2 validation)" `println!` block from `sim-server`'s `main.rs`, with no stdout replacement (FR-014 / this session's explicit decision) — **Already satisfied**: the block does not exist in the current codebase; `main.rs` already uses `tracing::info!` for all output.
- [ ] T021 [P] Update `specs/002-tick-observability/checklists/requirements.md` status if any items were deferred during implementation

---

## Dependencies & Execution Order

- Phase 1 → Phase 2 → Phases 3–6 (US1–US4 can proceed in parallel once Phase 2 is done, since they touch different systems: player decision, config/CLI, ball, referee) → Phase 7 last (cleanup and determinism check should follow, not precede, all emission code existing)
- Within Phase 3, T007 → T008 → T009 are sequential (same function, nested spans build on each other); T010 depends on all three
- T016 and T017 are independent of each other and of Phase 3/4/5's tasks — can run in parallel with them once Phase 2 lands
