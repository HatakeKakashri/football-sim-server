# Tasks: Football Match Simulation Engine

**Input**: Design documents from `/specs/001-football-sim-engine/`

**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, data-model.md, contracts/

**Tests**: Not explicitly requested in feature specification. Tasks exclude test tasks unless user requests TDD approach.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project initialization and workspace structure

- [X] T001 Convert single crate to 11-crate Cargo workspace with crates/ directory structure
- [X] T002 Create sim-math crate with Vec2 type, basic operations, and Mulberry32 PRNG implementation
- [X] T003 Create sim-components crate with Position, Velocity, TeamId, Role, Stamina, BallState, Skill components
- [X] T004 [P] Create sim-physics crate with basic vector math and integration stubs
- [X] T005 [P] Create sim-ai-core crate with Consideration, ResponseCurve, geometric-mean aggregator types
- [X] T006 [P] Create sim-ai-player crate with player consideration and action stubs
- [X] T007 [P] Create sim-ai-manager crate with weighted decision table type
- [X] T008 [P] Create sim-rules crate with rule engine stubs
- [X] T009 [P] Create sim-referee crate with referee type stubs
- [X] T010 Create sim-core crate with World wrapper, Schedule, and explicit system ordering
- [X] T011 [P] Create sim-replay crate with replay harness and state-hash stubs
- [X] T012 Create sim-server crate with server-authoritative loop stub
- [X] T013 [P] Configure workspace-level Cargo.toml with shared dependencies (bevy_ecs, smallvec)
- [X] T014 [P] Add workspace-level linting configuration (clippy, rustfmt)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core infrastructure that MUST be complete before ANY user story can be implemented

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T015 Implement DeterministicRng in sim-math with clone_for_entity() method per data-model.md
- [X] T016 Implement fixed-timestep accumulator in sim-core with 60 Hz (1/60 second) default step
- [X] T017 Implement Schedule in sim-core with explicit system chaining (perception → decision → intent → execution)
- [X] T018 Implement PitchDimensions in sim-math with standard field measurements (105m x 68m)
- [X] T019 Create base MatchState enum in sim-components per data-model.md (PreMatch, Kickoff, InPlay, Stoppage, HalfTime, FullTime, PenaltyShootout)
- [X] T020 Create base BallState enum in sim-components per data-model.md (Free, Possessed, InFlight, OutOfPlay, Dead)
- [X] T021 Implement state hash computation in sim-core for determinism verification (include positions, velocities, stamina, ball state, score, clock; exclude timing)

**Checkpoint**: Foundation ready - user story implementation can now begin in parallel

---

## Phase 3: User Story 1 - Deterministic Match Simulation (Priority: P1) 🎯 MVP

**Goal**: Run a football match simulation that produces identical results given the same seed and inputs

**Independent Test**: Run same simulation twice with identical seeds and verify output hashes match across 100 consecutive runs

### Implementation for User Story 1

- [X] T022 [P] [US1] Implement Ball component with position, velocity, spin, state, possessor fields per data-model.md
- [X] T023 [P] [US1] Implement Player component with team_id, position, velocity, stamina, role, skill, perception, intent, active_action per data-model.md
- [X] T024 [P] [US1] Implement Team component with id, name, formation, mentality, manager, players, substitutes, tactics per data-model.md
- [X] T025 [P] [US1] Implement Match component with id, home_team, away_team, score, clock, state, seed per data-model.md
- [X] T026 [P] [US1] Implement MatchClock component with elapsed, half, added_time, is_running per data-model.md
- [X] T027 [US1] Implement basic ball physics system in sim-physics (velocity integration, boundary clamping)
- [X] T028 [US1] Implement player movement system in sim-physics (velocity integration, sprint speed clamp at ~10 m/s)
- [X] T029 [US1] Create Simulation struct in sim-core with world, schedule, rng, tick, accumulator fields
- [X] T030 [US1] Implement Simulation::new() with seed initialization in sim-core
- [X] T031 [US1] Implement Simulation::tick() with deterministic fixed-timestep update in sim-core
- [X] T032 [US1] Implement Simulation::get_state_hash() for determinism verification in sim-core
- [X] T033 [US1] Implement Simulation::create_match() with default team setup in sim-server
- [X] T034 [US1] Add determinism verification test: run 1000 ticks twice with same seed, verify hashes match
- [X] T035 [US1] Add per-tick state hash logging for divergence detection

**Checkpoint**: Deterministic simulation substrate proven - can run identical matches with same seed

---

## Phase 4: User Story 2 - Server-Authoritative Match Control (Priority: P1)

**Goal**: Simulation is single source of truth; clients only render snapshots and send validated commands

**Independent Test**: Verify client commands are validated and applied only at tick boundaries

### Implementation for User Story 2

- [X] T036 [P] [US2] Implement ManagerCommand enum in sim-server (ChangeFormation, Substitute, ChangeMentality, SetTactic)
- [X] T037 [P] [US2] Implement CommandError enum in sim-server per contracts/simulation-api.md (InvalidForState, NoSubstitutesRemaining, etc.)
- [X] T038 [US2] Implement command validation logic in sim-server (validate against match state and available resources)
- [X] T039 [US2] Implement command queue in sim-server with tick-boundary application
- [X] T040 [US2] Implement Simulation::apply_command() in sim-core per contracts/simulation-api.md
- [X] T041 [US2] Implement MatchSnapshot struct in sim-server per contracts/client-protocol.md
- [X] T042 [US2] Implement Simulation::get_state() returning read-only snapshot in sim-core
- [X] T043 [US2] Add validation test: invalid command rejected, valid command applied at next tick
- [X] T044 [US2] Add state query test: all state comes from server simulation

**Checkpoint**: Server-authoritative control complete - commands validated and applied correctly

---

## Phase 5: User Story 3 - Realistic Player Behavior (Priority: P2)

**Goal**: AI-controlled players make intelligent decisions based on match context

**Independent Test**: Observe player decisions in various match scenarios; verify context-appropriate choices

### Implementation for User Story 3

- [X] T045 [P] [US3] Implement PerceptionSnapshot in sim-ai-core per data-model.md (nearby_teammates, nearby_opponents, ball_position, goal_position, pitch_bounds)
- [X] T046 [P] [US3] Implement Intent enum in sim-components per data-model.md (MoveTo, PassTo, Shoot, Tackle, Intercept, Press, HoldPosition, SupportAttack, TrackBack)
- [X] T047 [P] [US3] Implement UtilityBrain component in sim-ai-player (actions, hysteresis, evaluation interval)
- [X] T048 [US3] Implement perception/sensing system in sim-ai-player (spatial hash, nearby entity detection)
- [X] T049 [US3] Implement player consideration scoring in sim-ai-player (stamina factor, tactical importance, passing options)
- [X] T050 [US3] Implement geometric-mean aggregator in sim-ai-core for multi-factor trade-offs
- [X] T051 [US3] Implement player decision system in sim-ai-player (perception → intent pipeline)
- [X] T052 [US3] Implement player action execution in sim-physics (intent → steering forces)
- [X] T053 [US3] Add stamina-based decision test: low stamina player conserves energy appropriately
- [X] T054 [US3] Add passing option test: player considers teammates in better positions
- [X] T055 [US3] Add defender tackle test: defender attempts tackle when attacker shoots

**Checkpoint**: Player AI complete - intelligent decisions based on match context

---

## Phase 6: User Story 4 - Tactical Management (Priority: P2)

**Goal**: Manager AI makes strategic decisions about formations, substitutions, and mentality

**Independent Test**: Verify manager actions occur at appropriate times based on match state

### Implementation for User Story 4

- [X] T056 [P] [US4] Implement WeightedDecisionTable in sim-ai-manager per data-model.md (factor weights for decisions)
- [X] T057 [P] [US4] Implement Manager component in sim-components per data-model.md (decision_table, last_decision_tick, decision_cooldown)
- [X] T058 [US4] Implement manager decision factors in sim-ai-manager (score difference, time remaining, stamina levels, momentum, tactical matchup)
- [X] T059 [US4] Implement manager evaluation system in sim-ai-manager (weighted scoring with momentum)
- [X] T060 [US4] Implement formation change logic in sim-ai-manager (validate against available players)
- [X] T061 [US4] Implement substitution logic in sim-ai-manager (low stamina detection, substitute selection)
- [X] T062 [US4] Implement mentality shift logic in sim-ai-manager (score-based, time-based)
- [X] T063 [US4] Add manager decision test: team losing by 2 goals considers aggressive tactics
- [X] T064 [US4] Add substitution test: low stamina player substituted when window opens
- [X] T065 [US4] Add mentality test: team leading late shifts to defensive mentality

**Checkpoint**: Manager AI complete - strategic decisions adapt to match circumstances

---

## Phase 7: User Story 5 - Deterministic Replay & Debugging (Priority: P3)

**Goal**: Replay matches from recorded seeds to reproduce and debug issues

**Independent Test**: Record match seed, reproduce bug, verify same seed reproduces same bug

### Implementation for User Story 5

- [X] T066 [P] [US5] Implement TimedCommand struct in sim-replay (tick, command)
- [X] T067 [P] [US5] Implement ReplaySession struct in sim-replay (seed, commands, current_tick, state_hash_history)
- [X] T068 [US5] Implement Simulation::replay() in sim-core per contracts/simulation-api.md
- [X] T069 [US5] Implement event recording in sim-replay (goal, foul, card, substitution events)
- [X] T070 [US5] Implement divergence detection in sim-replay (hash mismatch logging)
- [X] T071 [US5] Implement debug logging for decision-making processes in sim-ai-core
- [X] T072 [US5] Add replay determinism test: same seed produces identical event sequence
- [X] T073 [US5] Add divergence detection test: modified input produces logged divergence point

**Checkpoint**: Replay capability complete - deterministic debugging and regression testing

---

## Phase 8: Rules & Referee (Cross-Cutting)

**Purpose**: FIFA/IFAB Laws of the Game enforcement

### Implementation

- [X] T074 [P] Implement offside detection in sim-rules (Law 11)
- [X] T075 [P] Implement out-of-bounds detection in sim-rules (Law 9)
- [X] T076 [P] Implement goal detection in sim-rules (Law 10)
- [X] T077 [P] Implement foul detection and card system in sim-rules (Law 12)
- [X] T078 Implement referee advantage decision in sim-referee
- [X] T079 Implement added time calculation in sim-referee (Law 7)
- [X] T080 Implement match duration enforcement in sim-referee (90 minutes + added time)
- [X] T081 Implement minimum player count enforcement in sim-referee (Law 3: minimum 7 players)
- [X] T082 [P] Implement possession resolution in sim-rules (first contact wins; skill difference > 0.1 wins contested)
- [X] T083 Integrate referee system into sim-core schedule (after physics, before next tick)

---

## Phase 9: State Persistence & Recovery

**Purpose**: State persistence for recovery and replay

### Implementation

- [X] T084 [P] Implement MatchSnapshot serialization in sim-replay per contracts/state-snapshot.md
- [X] T085 [P] Implement BallSnapshot serialization in sim-replay per contracts/state-snapshot.md
- [X] T086 [P] Implement PlayerSnapshot serialization in sim-replay per contracts/state-snapshot.md
- [X] T087 Implement snapshot saving at configurable intervals (default: 100 ticks) in sim-replay
- [X] T088 Implement snapshot loading and state restoration in sim-replay
- [X] T089 Implement state hash verification on recovery in sim-replay
- [X] T090 Add recovery test: restore from snapshot within 5 seconds of last saved state

---

## Phase 10: CLI & Integration

**Purpose**: Command-line interface and end-to-end integration

### Implementation

- [X] T091 Implement CLI binary in sim-server with simulate, replay, recover subcommands
- [X] T092 Implement simulate subcommand (seed, ticks, full-match, output options)
- [X] T093 Implement replay subcommand (seed, commands file, output options)
- [X] T094 Implement recover subcommand (match-id, snapshot file)
- [X] T095 Implement benchmark subcommand (seed, ticks, duration measurement)
- [X] T096 Add end-to-end match test: full 90-minute simulation with valid lifecycle
- [X] T097 Run quickstart.md validation scenarios

---

## Phase 11: Polish & Cross-Cutting Concerns

**Purpose**: Improvements that affect multiple user stories

- [X] T098 [P] Add comprehensive documentation for public API types
- [X] T099 [P] Add error handling for all public functions
- [X] T100 [P] Add performance benchmarks for tick execution
- [X] T101 Code cleanup and clippy warnings resolution
- [X] T102 [P] Add determinism lint/test for RNG-consuming queries (stable EntityId sorting)
- [X] T103 Run full test suite and verify all success criteria
- [X] T104 [P] Add same-tick pipeline isolation validation test: verify no system observes uncommitted writes from another system in the same pipeline stage (per FR-005, Constitution Principle III)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies - can start immediately
- **Foundational (Phase 2)**: Depends on Setup completion - BLOCKS all user stories
- **User Stories (Phase 3-7)**: All depend on Foundational phase completion
  - US1 (Deterministic): No dependencies on other stories
  - US2 (Server Authority): Depends on US1 components
  - US3 (Player AI): Depends on US1 components
  - US4 (Manager AI): Depends on US1, may integrate with US3
  - US5 (Replay): Depends on US1, US2
- **Rules & Referee (Phase 8)**: Depends on US1, integrates with all stories
- **Persistence (Phase 9)**: Depends on US1, US2
- **CLI (Phase 10)**: Depends on all previous phases
- **Polish (Phase 11)**: Depends on all desired user stories being complete

### User Story Dependencies

- **User Story 1 (P1)**: Can start after Foundational (Phase 2) - No dependencies on other stories
- **User Story 2 (P1)**: Can start after Foundational (Phase 2) - Depends on US1 components but independently testable
- **User Story 3 (P2)**: Can start after Foundational (Phase 2) - May integrate with US1 but independently testable
- **User Story 4 (P2)**: Can start after Foundational (Phase 2) - May integrate with US1/US3 but independently testable
- **User Story 5 (P3)**: Can start after Foundational (Phase 2) - Depends on US1/US2 but independently testable

### Within Each User Story

- Models/components before systems
- Systems before integration
- Core implementation before tests
- Story complete before moving to next priority

### Parallel Opportunities

- All Setup tasks marked [P] can run in parallel
- All Foundational tasks marked [P] can run in parallel (within Phase 2)
- Once Foundational phase completes, US1 and US2 can start in parallel (both P1)
- US3 and US4 can start in parallel after Foundational (both P2)
- All component creation tasks within a story marked [P] can run in parallel
- Rules tasks (Phase 8) can run in parallel with each other
- Persistence tasks (Phase 9) can run in parallel with each other

---

## Parallel Example: User Story 1

```bash
# Launch all component creation for US1 together:
Task: "Implement Ball component with position, velocity, spin, state, possessor fields per data-model.md"
Task: "Implement Player component with team_id, position, velocity, stamina, role, skill, perception, intent, active_action per data-model.md"
Task: "Implement Team component with id, name, formation, mentality, manager, players, substitutes, tactics per data-model.md"
Task: "Implement Match component with id, home_team, away_team, score, clock, state, seed per data-model.md"
Task: "Implement MatchClock component with elapsed, half, added_time, is_running per data-model.md"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (11-crate workspace)
2. Complete Phase 2: Foundational (PRNG, timestep, schedule, base types)
3. Complete Phase 3: User Story 1 (deterministic simulation)
4. **STOP and VALIDATE**: Run determinism verification test
5. Deploy/demo if ready

### Incremental Delivery

1. Complete Setup + Foundational → Foundation ready
2. Add User Story 1 → Test independently → Deploy/Demo (MVP!)
3. Add User Story 2 → Test independently → Deploy/Demo
4. Add User Story 3 → Test independently → Deploy/Demo
5. Add User Story 4 → Test independently → Deploy/Demo
6. Add User Story 5 → Test independently → Deploy/Demo
7. Add Rules/Referee → Full FIFA compliance
8. Add Persistence → Recovery capability
9. Add CLI → User-facing interface
10. Polish → Production-ready

### Parallel Team Strategy

With multiple developers:

1. Team completes Setup + Foundational together
2. Once Foundational is done:
   - Developer A: User Story 1 (deterministic substrate)
   - Developer B: User Story 2 (server authority)
   - Developer C: User Story 3 (player AI)
3. After US1 complete:
   - Developer A: User Story 4 (manager AI)
   - Developer B: User Story 5 (replay)
4. Rules/Referee, Persistence, CLI can be parallelized across team

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- Each user story should be independently completable and testable
- Commit after each task or logical group
- Stop at any checkpoint to validate story independently
- Avoid: vague tasks, same file conflicts, cross-story dependencies that break independence
- All validation rules from data-model.md are quoted verbatim in relevant task descriptions
