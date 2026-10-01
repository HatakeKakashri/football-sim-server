# Feature Specification: Tick Observability (Post-Match Decision Trace)

**Feature Branch**: `002-tick-observability`

**Created**: 2026-09-27

**Status**: Draft

**Input**: User description: "Observability into every tick (or a configurable granularity), covering ball state, referee state, and full per-player Utility AI decision traces — every option considered and its score, not just the chosen action — recorded for post-hoc inspection, without eliminating data points for the sake of simplicity."

## Clarifications

### Session 2026-09-27

- Q: Live streaming dashboard, or post-hoc file-based inspection? → A: Post-hoc only. No server/networking work is in scope.
- Q: What granularity does the use case actually need? → A: Coarse by default (whole-match, e.g. 1/sec), with the ability to force full every-tick resolution across an explicit tick range for zooming into a specific passage of play.
- Q: Is fixing the never-instantiated `Referee` component in scope? → A: No. The trace schema includes referee data and reads it when present; wiring `Referee` into `create_match` is explicitly out of scope and left as a pre-existing gap.
- Q: Build a custom viewer, or feed an existing tool? → A: Feed an existing tool — Perfetto UI (`ui.perfetto.dev`), via the Chrome/`tracing-chrome` JSON trace format.
- Q: When both a coarse interval and a full-resolution range are recorded, one file or two? → A: Single file, one shared timeline.
- Q: Keep any of the existing ad hoc debug `println!` output in `main.rs`? → A: No — full removal, no stdout replacement.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Post-Match AI Decision Debugging (Priority: P1)

As the developer, I want to open a recorded match trace and see, for any player at any recorded moment, every action their Utility AI considered and every consideration's score behind it, so I can understand *why* a specific decision was made rather than only *what* was decided.

**Why this priority**: This is the core motivating problem — today, once `player_decision_system` picks a winning action, every intermediate score is discarded. Nothing else in this feature matters if this isn't satisfied.

**Independent Test**: Run a short simulation with tracing enabled, open the resulting file in Perfetto UI, select a player's Decision track at a recorded tick, and confirm every action considered at that tick is visible along with each of its considerations' raw value, weight/curve, and score, with the chosen action clearly marked.

**Acceptance Scenarios**:

1. **Given** a recorded tick where a player considered `ShootAtGoal`, `PassTo`, `ChaseBall`, and `HoldPosition` (action names are the `intent_kind()` strings of the real brain), **When** the trace is opened in Perfetto, **Then** all four actions appear as nested spans under that player's Decision track for that tick, none omitted.
2. **Given** an action span for "Pass" with three considerations evaluated, **When** the span is inspected in Perfetto's argument panel, **Then** each consideration's name, raw input, weight/curve, and resulting score are all visible.
3. **Given** a tick where "Shoot" was the winning action, **When** the trace is inspected, **Then** the "Shoot" span is distinguishably marked as chosen among the sibling action spans.

---

### User Story 2 - Coarse Whole-Match Overview (Priority: P1)

As the developer, I want a recording that spans an entire match by default without producing an unmanageably large file, so routine runs can be traced without deciding in advance whether something interesting will happen.

**Why this priority**: Full every-tick recording for a full match is tens of millions of data points; a usable default is required for this feature to be reached for casually rather than only after a bug is already known.

**Independent Test**: Run a full-length simulation with only a coarse interval configured (no full-resolution range) and confirm the resulting file size and snapshot count are proportional to `total_ticks / interval_ticks`, not `total_ticks`.

**Acceptance Scenarios**:

1. **Given** a simulation of `T` ticks and `--trace-interval-ticks 600` (the default, *amended 2026-10-01*), **When** the run completes, **Then** the trace contains one recorded snapshot roughly every 600 ticks, not every tick.
2. **Given** `--trace-out` is not supplied, **When** the simulation runs, **Then** no trace file is produced and no tracing subscriber is installed.

---

### User Story 3 - Dense Investigation Window (Priority: P2)

As the developer, having noticed something questionable in the coarse overview, I want to re-run (or configure) the same match to capture full every-tick detail across a specific span of ticks, so I can see the exact sequence of considerations leading up to a single controversial moment.

**Why this priority**: This is what makes the coarse default acceptable — without it, "coarse by default" would mean genuinely losing the ability to do frame-by-frame debugging when it's actually needed.

**Independent Test**: Run a simulation with both a coarse interval and a `--trace-full-range` set, and confirm every tick inside that range produces a full decision-trace snapshot, in the same file as the coarse recordings.

**Acceptance Scenarios**:

1. **Given** `--trace-interval-ticks 60` and `--trace-full-range 12000-12600`, **When** the run completes, **Then** every tick from 12000 to 12600 inclusive has a full recorded snapshot, in addition to the coarse snapshots elsewhere in the match.
2. **Given** the same run, **When** the trace is opened, **Then** both densities appear on one continuous timeline in a single file.

---

### User Story 4 - Ball, Referee, and Full-Pitch Context Alongside Decisions (Priority: P2)

As the developer, I want ball state and referee state visible on the same timeline as player decisions, so I can correlate a player's decision with what was actually happening in the match at that instant, not just view player AI in isolation.

**Why this priority**: A decision trace without match context (was the ball loose? was a card just shown?) is harder to interpret correctly.

**Independent Test**: Inspect a recorded tick and confirm ball position/velocity/state/possessor and (when a `Referee` component exists) card/stoppage data are present alongside the player decision data for that same tick.

**Acceptance Scenarios**:

1. **Given** any recorded tick, **When** inspected in Perfetto, **Then** the Ball process shows position, velocity, spin, state, and possessor for that tick.
2. **Given** a match with no `Referee` component ever spawned on the match entity, **When** the trace is produced, **Then** the run completes normally with no Referee process in the trace and no error raised.
3. **Given** a match with a `Referee` component present bearing card data, **When** the trace is produced, **Then** card events appear on a Referee process at the ticks they occurred.

---

### Edge Cases

- `--trace-out` omitted: tracing fully disabled; no subscriber installed; zero measurable per-tick overhead.
- `--trace-interval-ticks 0`: rejected at CLI parse time as invalid, not treated as "record every tick."
- `--trace-full-range` specifying a range outside `0..total_ticks`: clamped to the valid range rather than erroring.
- `--trace-full-range` with `start > end`: rejected at CLI parse time.
- No `Referee` component present on the match entity (the current, actual state of the codebase): Referee process is simply absent from the trace; this is expected, not an error condition.
- Disk write failure while a trace is being written (disk full, invalid path, permissions): not specially handled by this feature; accepted as a known risk consistent with this track's stated priority of a working simulation over infra hardening. Not covered by any acceptance scenario or success criterion below.
- Simulation determinism: recording must be a pure side channel — the state hash of a run must be identical whether or not tracing is enabled (see SC-004).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST allow enabling trace recording via a CLI flag (`--trace-out <path>`) on the `simulate` subcommand; omitting it MUST fully disable tracing with no subscriber installed.
- **FR-002** *(amended 2026-10-01)*: System MUST default to a coarse recording interval of 600 ticks (~10 seconds at the fixed 60 Hz timestep), overridable via `--trace-interval-ticks <n>`. *(Originally 60 ticks; at that default a full match produced ~660 MB, contradicting User Story 2's intent that a default recording stays manageable. The default lives in the single constant `TelemetryConfig::DEFAULT_INTERVAL_TICKS`.)*
- **FR-003**: System MUST support an optional `--trace-full-range <start>-<end>` flag causing every tick within that inclusive range to be recorded at full resolution, regardless of `--trace-interval-ticks`.
- **FR-004**: System MUST write both the coarse-interval recordings and any full-resolution range recordings into a single output file on one shared timeline.
- **FR-005**: System MUST emit, for every recorded tick, position and velocity for the ball and for every player entity.
- **FR-006**: System MUST emit ball state (position, velocity, spin, `BallState`, possessor entity if any) for every recorded tick.
- **FR-007**: System MUST emit, for every recorded tick and every player, every action considered by that player's Utility AI decision system that tick, including — for each consideration evaluated within that action — its name, raw input value, applied weight/curve, and resulting score.
- **FR-008**: System MUST mark, among the actions considered for a player at a given recorded tick, which action was ultimately chosen.
- **FR-009**: System MUST emit referee state (cards issued, stoppage events) for a recorded tick when a `Referee` component exists on the match entity, and MUST NOT error or halt the simulation when no `Referee` component is present.
- **FR-010**: Output MUST conform to the Chrome/Perfetto JSON trace event format, openable in Perfetto UI (`ui.perfetto.dev`) without modification.
- **FR-011**: System MUST structure the trace with one process per player, decomposed into a Position thread (counter tracks for position/velocity/stamina) and a Decision thread (nested spans: tick → action → consideration).
- **FR-012**: System MUST structure the ball, and the referee (when present), as their own distinct processes in the trace.
- **FR-013**: System MUST NOT install a tracing subscriber, and MUST incur no meaningful per-tick cost, when `--trace-out` is not supplied.
- **FR-014**: System MUST remove the existing ad hoc `println!`-based "DEBUG INSTRUMENTATION (temporary, for phase2 validation)" block in `sim-server`'s `main.rs` as part of this work, with no stdout replacement. **Status: Already satisfied** — the block does not exist in the current codebase; `main.rs` already uses `tracing::info!` for all output.

### Key Entities

- **TelemetryConfig**: Recording configuration — coarse interval (ticks), optional full-resolution tick range. Lives in the new `sim-telemetry` crate; inserted as a per-tick resource the same way `CurrentTick` is today.
- **Decision Trace Event**: Not a persisted Rust struct — realized as `tracing` spans/events emitted inline from `player_decision_system` (nested tick → action → consideration), carrying the same data that already exists transiently in that system's scoring loop.
- **Player, Ball, Referee**: Existing entities/components (`sim-components`); this feature reads their current state for emission and does not add new fields to them. Note: `Ball` is a `Resource` (not a `Component`) in the current codebase; `Referee` is a `Component` that is never instantiated.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A `simulate` run with `--trace-out` produces a file that parses as well-formed Chrome Trace JSON.
- **SC-002** *(amended 2026-10-01)*: For a run with only `--trace-interval-ticks` set (no full-resolution range), the number of recorded snapshots (position/ball counters) equals `total_ticks / interval_ticks` (±1 at the boundary), and **decisions are traced for every tick in `[t, t+DECISION_CADENCE_TICKS-1]` of each recorded tick `t`**, so every player — each is evaluated exactly once per cadence window — has its complete action/consideration tree recorded; no evaluated player, action, or consideration is omitted. (Original wording required all 22 players at the single tick `t`, which is impossible: only ~1/6 of players are evaluated on any given tick.)
- **SC-003**: Adding `--trace-full-range` covering `R` ticks adds full-detail snapshots for each of those `R` ticks (beyond whatever coarse snapshots already existed inside that range), in the same output file.
- **SC-004**: A `simulate` run's final state hash is identical whether or not `--trace-out` is supplied — confirms recording has zero effect on simulation determinism.
- **SC-005**: Opening the produced file in Perfetto UI shows one process per player plus a Ball process (and a Referee process, when applicable) with no manual transformation of the file required.

## Assumptions

- Post-hoc only; no live/streaming dashboard or server/networking work is in scope for this feature.
- Perfetto UI is the intended viewer; no custom dashboard is built as part of this work.
- `data-model.md` for `001-football-sim-engine` already documents the match entity as containing "one Referee entity" — in the current codebase, no such entity is ever spawned. This feature treats that as a pre-existing gap to read around, not to fix.
- Disk/IO failures during trace writing are accepted as an unhandled risk, consistent with this track's documented priority of a working simulation over infra hardening.
- `tracing` is already a workspace dependency; `tracing-subscriber` (and `serde_json`, in `sim-telemetry`) are added instead of `tracing-chrome`, which cannot produce the contract (see `docs/superpowers/specs/2026-10-01-tick-observability-design.md`). The new public API outside `sim-telemetry` is exactly: `Consideration::name()`, `ResponseCurve::name()` and `Simulation::enable_telemetry(TelemetryConfig)` (the single switch that turns tracing on; without it no trace code runs). `sim_core::simulation::{brain_default, trace_emit}` are `pub mod` only inside the private `simulation` module (the same pattern as `lifecycle` and `state_hash`), so they are crate-internal and not part of the crate's external surface.
- The Utility AI's per-consideration scoring data already exists transiently inside `player_decision_system`'s scoring loop; this feature adds inline `tracing::event!` calls at that existing computation site rather than restructuring the loop's return type.
