# Football Match Engine: Final Goal and Release 1 Design

**Date:** 2026-10-05
**Status:** Written spec, awaiting owner review. Every section was approved one by one during the brainstorming session.
**Canonical backend:** the Rust/Bevy `sim-server` workspace.
**Superseded:** the TypeScript/bitECS engine (`ninety-minutes/engine`), `milestone-1-foundation.md`, `milestone-1-verification-report.md`, and the frontend instructions that reference `engine/simulation/types.ts` and `POST /simulate`. Treat them as historical context only.
**Related:** `football-sim-code-review.md` (static review of commit range a49983c..5f17d08). Finding IDs below (B1, D2, R5 and so on) refer to that report.

Confidence labels: **[Certain]** directly supported, **[Likely]** strong inference, **[Guessing]** speculative.

---

## 1. Vision and purpose

A server-authoritative football match engine, written in Rust on Bevy, that powers a football management game in the Top Eleven category. The manager sets the formation, mentality and substitutions, then watches the match play out in a separate React frontend.

The engine must deliver:
1. Matches that football fans find believable.
2. Outcomes that follow from the manager's decisions and the squad's attributes, so tactics matter.
3. Reproducible matches (same inputs, same result), so results can be verified, replayed and disputed.

**Differentiator.** Tactics visibly and understandably change what happens on the pitch. This justifies a real 22-player simulation instead of an event-roll model.

**Boundary.** The Rust backend owns all match logic and truth. The React frontend renders and sends manager commands. It never simulates.

**Accepted risk.** A continuous 22-agent, 60 Hz simulation is expensive at scale. This was assessed separately and approved by the owner. It is not reopened here.

---

## 2. Users and primary use cases

**Primary user:** the human manager, playing through the React frontend.

**Opponents.** Any pairing is possible: human vs human, human vs AI, AI vs human, AI vs AI. The mix depends on how many managers in a league are human. An AI manager is a first-class command source and submits commands through the same validated, logged command path as a human.

**Use cases the engine must support:**
1. **Pre-match setup.** Choose the starting eleven, bench, formation and mentality.
2. **Live match.** Watch the match unfold and, at any moment, change formation, mentality or instructions, and request substitutions.
3. **Post-match.** Read the result, statistics and event timeline, and replay the match.
4. **Operator verification.** Re-run any match from its recorded inputs and reproduce it exactly, so disputed results can be settled.

**Engine consequences of "live":**
- Every manager command is validated, applied at a deterministic tick, and recorded in the command log. Replay equals seed plus command log.
- A command is never dropped and never applied twice. Today commands are dropped (finding B7), so this is a hard requirement.

**Substitution timing (decided).** Tactics and mentality changes apply immediately, or at the next decision cadence. Substitution requests are accepted at any time and queued. The referee executes the actual swap only during a play stoppage, consistent with Law 3. The UI shows queued requests as pending.

---

## 3. Fidelity target

**Priority order for release 1:**
1. **Tactical causality.** Formation, mentality, lineup and player attributes visibly drive outcomes.
2. **Tunability.** The strength of every tactic and attribute effect is configurable data, tuned by trial and error without code changes.
3. **3D ball.** The ball is simulated in three dimensions, with spin and curve, so it can be shot, hit a post, and travel through the air.
4. **Laws of the Game completeness.** Important, not a release blocker.
5. **Statistical realism.** Reached by tuning and measured, not enforced as a hard target.

**3D ball.** Height, gravity, ground bounce, post and crossbar contact, and spin with Magnus curve. Players move on the ground plane. The frontend renders in 2D, as Top Eleven did in its pre-3D era. Every snapshot carries ball height so the 2D renderer can draw it. Law 10 becomes enforceable: a ball over the crossbar is not a goal.

**Tunability.** All effect weights live in versioned configuration, loaded once and immutable for the duration of a match. A match is reproducible only from the full identity in Section 7. The weights must actually take effect: today consideration weights are traced and then ignored (A2) and the formation is hard-coded to 4-4-2.

**Measurement.** Trial-and-error tuning needs a lightweight batch runner: N seeded matches, an aggregate report (goals, shots, possession and so on), and a diff between two configs. No realism targets are set. The runner is the instrument that makes tuning workable.

---

## 4. Scope

### In scope for release 1
- 11 v 11 match from kickoff to full time, two halves of 45:00 each, a halftime break, and an end-of-half side swap.
- Goalkeepers: positioning, saves and distribution.
- Goals, with the 3D ball and whole-ball line crossing.
- Basic restarts: kickoff (after a goal and at the half), throw-ins, corners, goal kicks.
- Live manager commands: formation, mentality and instructions apply during play. Substitutions are requested live and executed at stoppages.
- Player attributes and tactics as configurable, versioned tuning data.
- Any pairing of human and AI managers, through one validated command path.
- Deterministic replay (Section 7).
- A batch runner for tuning.
- Five formations held as data: 4-4-2, 4-3-3, 4-2-3-1, 3-5-2, 5-3-2.

### Out of scope for release 1
- Offside, fouls, cards, free kicks, penalties, advantage. The existing foul and offside detection is removed or disabled so the event stream never reports rules that are not enforced.
- Stamina, fatigue, injuries, pitch and weather effects.
- Stoppage-driven added time (halves end at exactly 45:00), extra time, penalty shootouts.
- VAR and crowd effects.
- Injury- or fatigue-forced substitutions.

### Consequence to keep in view
With fouls and offside out, defensive-line height and pressing aggression have weaker consequences than in the competing game. The remaining levers are formation, mentality, width, tempo, pressing intensity and lineup. See risk 6.

---

## 5. Success criteria

Release 1 passes only if every check below passes in the project's automated test and batch-runner suite.

### A. Integrity (non-negotiable)
1. **Determinism.** The same `(engine version, Rapier version, tuning config hash, seed, MatchDefinition, command log)` yields an identical final state hash on x86_64 and aarch64.
2. **Replay.** Every recorded match replays to its recorded result. The hash covers full state, including RNG state and pending events.
3. **Command integrity.** Every accepted manager command is applied exactly once at its recorded tick, and no accepted command is lost.
4. **Scope invariants.** Every match runs 45:00 + 45:00, starts with legal kickoff positions, ends with a valid score, and never changes state after full time.

### B. Behavioural checks
Each runs on N seeded matches. Provisional N is at least 1,000 per arm. Every threshold is configuration, not code.

1. **Sanity envelope.** Across a batch of balanced-squad matches: no stuck ball (frozen for more than 30 simulated seconds while live) and no possession livelock; no match below a minimum shot count or above a maximum goal count. Provisional bounds are at least 3 shots and at most 12 goals.
2. **Directional tactics.** Change one lever, hold everything else, compare mean metrics over the batch. The shift must go in the expected direction and be statistically significant. Starting table:

   | Lever change | Shots for | Shots against | Possession |
   |---|---|---|---|
   | More attacking mentality | up | up | up |
   | More defensive mentality | down | down | down |
   | More midfielders in formation | no claim | no claim | up |
   | Higher tempo or pressing | no claim | up | no claim |

3. **Strength monotonicity.** Bucket matches by squad-rating gap. The stronger side's win rate must not decrease as the gap grows, and the draw rate must not rise with it.

### Notes
- Check B3 depends on the squad-rating function in Section 6.3.
- No human playtest gate was selected. A build can pass every check and still look wrong. An informal playtest before any public launch is recommended, and the decision is the owner's.
- Runtime of the batch runner is unmeasured. If 1,000 matches per check is too slow for CI, N or the schedule changes.
- With realism demoted, the sanity envelope catches only extremes. Average goals per match can drift unnoticed (risk 5).

---

## 6. Architecture and boundaries

### 6.1 Live match transport
- The server runs the simulation at 60 Hz and pushes snapshots over **Server-Sent Events** at about 10 to 20 Hz. The frontend interpolates between snapshots.
- Manager commands go in separate HTTP `POST` requests. The response returns the tick at which the command will apply, and that tick is what the command log records.
- Each SSE event carries an ID so a reconnecting client resumes where it dropped. A client outside the server's buffer receives a full keyframe.
- Snapshots include ball height and spin, plus discrete events (shot, post, save, goal).
- The old batch `POST /simulate` contract is retired.

### 6.2 Ball physics
- **Rapier** drives the ball only, with `enhanced-determinism`, a pinned Rapier version, and a fixed 1/60 s timestep. The Rapier version is part of the replay identity.
- Magnus force and any extra drag are custom code applied before each step.
- Players are steering agents, not rigid bodies. Kicks, saves and deflections are explicit events that set or modify ball velocity. The goalkeeper and the goal frame have colliders. The exact contact design is part of M1 and M4.
- [Likely] `enhanced-determinism` cannot be combined with Rapier's SIMD or parallel features and requires IEEE 754-2008 compliant targets.
- **Fallback rule (agreed):** if the Rapier ball fails the cross-architecture hash check in the first spike (M1), the project switches to a custom fixed-step ball model without a new vote.
- Rapier's guarantee covers its own maths only. All AI-side transcendentals (`exp`, `ln`, `hypot`) still move to `libm` (D1).

### 6.3 Player model (Top Eleven style)
- **Outfield players:** 15 attributes in three groups.
  - Defence: Tackling, Marking, Positioning, Heading, Bravery.
  - Attack: Passing, Dribbling, Crossing, Shooting, Finishing.
  - Physical and Mental: Fitness, Strength, Aggression, Speed, Creativity.
- **Goalkeepers:** 10 attributes: Reflexes, Agility, Anticipation, Rushing Out, Communication, Throwing, Kicking, Punching, Aerial Reach, Concentration.
- **Scale:** integers 1 to 100 at the engine boundary, with tunable response curves inside. (Proposed by the engine side; confirm against the game side's scale.)
- **Positions:** 14 codes (GK, DL, DC, DR, DML, DMC, DMR, ML, MC, MR, AML, AMC, AMR, ST) on a grid of rows and left/centre/right columns. Each player has one or more trained positions. Formations assign eleven slots, each labelled with a position code, with repeats allowed.
- **Position fit:** a penalty based on grid steps from the nearest trained position (zero steps costs nothing, one step costs a little, further costs a lot). The penalty table is tuning data.
- **Squad rating (used by check B3):** the sum of each starter's overall quality after position-fit adjustment. The formula deriving overall quality from attributes is tunable.

**Release 1 attribute effects:**

| Group | Attribute | Release 1 effect |
|---|---|---|
| Defence | Tackling | tackle success (no foul component) |
| | Marking | how tightly a player tracks his assigned opponent |
| | Positioning | error against the formation target position |
| | Heading | aerial contact quality |
| | Bravery | willingness to contest loose balls and aerial duels |
| Attack | Passing | pass accuracy and weight |
| | Dribbling | ball control under pressure and carrying |
| | Crossing | cross accuracy |
| | Shooting | shot power and accuracy |
| | Finishing | close-range shot quality |
| Physical and Mental | Speed | top speed, with a with-ball multiplier |
| | Strength | shielding and duel outcomes |
| | Aggression | pressing intensity and challenge frequency |
| | Creativity | breadth of options the decision logic considers |
| | Fitness | **inert** (fatigue is out of scope) |
| Goalkeeper | Reflexes, Agility, Anticipation, Rushing Out, Punching, Aerial Reach, Concentration | save and claim mechanics |
| | Kicking, Throwing | distribution |
| | Communication | **inert** (set-piece organisation is out of scope) |

Inert attributes stay in the schema, marked "not yet used", so saved squads and the frontend never need a breaking change. Excluded from release 1: special abilities, morale, physical condition and team chemistry. Aggression carries no foul risk in release 1.

### 6.4 The match interface
- **Input (`MatchDefinition`):** opaque `match_id`; `seed`; `engine_version` and `tuning_config_id`; for each side, formation, mentality, 11 starters (slot code, 1 to 100 attributes, trained positions), bench, and manager kind (Human or AI).
- **Output (`MatchResult`):** final score, aggregate stats, event timeline, command log, final state hash, replay artifact.
- **Idempotent.** Re-submitting the same `match_id` with identical inputs returns the same result and never starts a second session. A different payload under the same ID is rejected.
- **Opaque identifiers.** The server never interprets team, league or user IDs. It echoes them back.
- **Match-only for now, with a clean interface.** The server owns no leagues, schedules or persistent squads. Storage is a port: the server defines what must be stored, not where, so absorbing more responsibility later changes an implementation and not the contract.

### 6.5 Crate boundaries
- **Dependency direction:** math, components, then physics, rules and AI core, then player and manager AI, then the simulation crate, then replay and server. Telemetry is cross-cutting.
- **Server isolation:** the server reaches the simulation only through a narrow `MatchSession` API. No HTTP types exist below the server crate.
- **New crate `sim-protocol`:** snapshot, event, command and match-definition types. It is the single source of truth for the wire format, and the TypeScript types for the frontend are generated from it. ECS types never cross the wire.

### 6.6 Tuning configuration
- One immutable, versioned bundle identified by a content hash, loaded and validated when a session starts. An invalid config fails loudly and never falls back to defaults.
- It carries attribute response curves, the position-fit table, formation and mentality modifiers, the five formations, ball constants (drag, Magnus coefficient, restitution), and AI decision weights, hysteresis and cadence.
- The batch runner takes two config IDs and reports the metric diff.
- Retired configs are retained (Section 7.7).

### 6.7 Session lifecycle
- **States:** Created, Ready, Live, Finished, Archived.
- **Isolation:** one session is one Bevy `World` with no process-global state. The global pass-flow counter (D5) moves into the session.
- **Commands** from humans and the AI manager enter through one validated queue.
- **Disconnects.** When a human manager's client disconnects, the match keeps running and the manager's last instructions stand. Pausing is not allowed.
- **Durable log.** The Rust server owns the append-only command log and appends each accepted command durably before acknowledging it.
- **Crash recovery.** A crashed live match is rebuilt from `(seed, command log)` up to the last durable tick, then continues.

### 6.8 Pacing, highlights, commentary
- **Duration target.** About 10 real minutes per match, measured from kickoff to the full-time whistle, with halftime excluded. Halftime is a configurable pause that doubles as a substitution and tactics window. Only highlights are animated, at real-time pacing, alongside a stream of commentary.
- **Pace controller.** One server-side component maps simulation ticks to wall-clock emission times. It fast-forwards quiet phases and drops to about 1x when a highlight begins. The average is roughly 9x. The fast-forward ceiling, the highlight pace and the simulation lead (a few simulated seconds) are tuning values set against the 10-minute target.
- **Highlight classification** is a pure function of simulation state and events (for example a possession entering the final third, a shot, a save, a set-piece start). It is derived, never stored as input, so a replay produces identical highlights.
- **Commentary events** are structured keys plus parameters, with no prose. Wording and localisation belong to the frontend or game backend.
- **Command timing.** A command is stamped with the next unsimulated tick when the server receives it. This is the one nondeterministic input, and it is recorded.
- **Shared pacing.** All viewers of a match receive the same stream at the same pace, so two human managers see the same match time.

---

## 7. Determinism and replay

### 7.1 The determinism contract
A match outcome is a pure function of `(engine version, Rapier version, tuning config hash, seed, MatchDefinition, tick-stamped command log)`.

Never part of the contract, and never allowed to feed back into the simulation: wall-clock time, pacing, snapshot rate, connection state, thread scheduling, and highlight classification.

### 7.2 Requirements on the code
1. **Platform independence.** Bit-identical results on x86_64 and aarch64. Every transcendental goes through `libm`. No fast-math, no target-dependent behaviour.
2. **Ordering.** No iteration order that depends on a hash map, an archetype layout or an entity-spawn accident. Where order can affect output, it is fixed by stable keys or a fixed roster.
3. **Single-threaded gameplay schedule**, or an enforced ambiguity check in tests.
4. **No process-global state.** Everything lives in the session.
5. **RNG stream isolation.** Adding or removing a random draw in one subsystem must not change outcomes in unrelated subsystems. Draws happen once per discrete event, keyed by `(seed, tick, entity, purpose)`.
6. **Total ordering.** All float comparisons use total ordering, with NaN handled explicitly.
7. **Rapier** runs with `enhanced-determinism`, a pinned version, and the fixed 1/60 s step.

### 7.3 State hash
A canonical 128-bit non-cryptographic hash over the full simulation state: positions, velocities, ball state and spin, RNG state, intents, pending events and restarts, a single clock, and score. It must not have the blind spots found in the review (D2).

### 7.4 Replay artifact
- **Header:** engine version, Rapier version, protocol schema version, tuning config hash, seed, full `MatchDefinition`.
- **Command log:** per command, `(tick, sequence, command, validation outcome)`, plus the authenticated manager identity once OD-1 is decided.
- **Checkpoint hashes** every N ticks (provisional: every 10 simulated seconds) and a final hash.
- **Verification** re-runs the simulation from the artifact and reports the first mismatching checkpoint. It uses the same code path as live play.

### 7.5 Versioning rule
Any change that can alter outcomes bumps the engine version. A tuning change is a new config hash. Recorded results (score, stats, timeline) are stored as data and never depend on re-simulation. Re-simulation is for verification and rewatching.

### 7.6 Crash recovery
A crashed live match is rebuilt by replaying `(seed, command log)` to the last durable tick, then continuing. During recovery, pacing runs fast-forward.

### 7.7 Archival (decided: verifiable forever)
- **Reproducible builds.** Every released engine build is made from a pinned Rust toolchain, a committed `Cargo.lock` and a pinned Rapier version, in containers. The source revision, toolchain version and build artifact are archived, with hashes in a release manifest.
- **Archive scope.** Every engine build that has played a recorded match, and every tuning config referenced by a recorded match. Experimental configs that never reached a real match are not archived.
- **Multi-version verifier.** The verification service reads a replay's header and dispatches to the matching archived build. A verify-only mode takes an artifact and returns a pass or a first-mismatch report, with no network access.
- **Sandboxing.** Old builds keep their old dependencies and run only in verify-only mode, inside an isolated container with no network and no data beyond the artifact. Security patches go into a new version, never an archived one.
- **Schema compatibility.** Every replay-artifact and protocol schema version stays readable forever. Schema changes are additive, or the verifier carries a reader per version.
- **Architecture independence.** The cross-architecture check (Section 5, A1) is what lets archived builds run on future hardware, including under emulation.
- **Release gate.** A build is not released until a corpus of reference replays verifies on it on both architectures. Older versions' reference replays are re-checked against their archived builds periodically to catch archive rot.
- **Cost.** [Certain] "Forever" is a permanent operating commitment: storage, an archive that outlives team and infrastructure changes, and a verifier service. No storage estimate is made without build sizes and match volumes.

---

## 8. Roadmap and sequencing

One workstream, strictly sequential: M0, M1, M2, M3, M4, M5, M6, M7, M8, M9. No dates or effort estimates are given because no velocity data exists. Each milestone has an exit gate drawn from Section 5.

| # | Milestone | Delivers | Review findings closed | Exit gate |
|---|---|---|---|---|
| M0 | Safety net | Cross-arch CI (x86_64 and aarch64) with same-seed hash comparison; minimal per-checkpoint hash trace; single-threaded schedule; `libm` for all transcendentals; single `Pitch` source of truth; no process-global state; total-order comparators; foul, offside and referee systems removed; swap-symmetry and full-schedule test harness; headline review defects reproduced as failing tests | D1, D4 (partly), D5, D7, D8, I2, I3, S1 | CI green on both architectures; swap-symmetry test exists (expected to fail until M3) |
| M1 | Ball spike (go/no-go) | Rapier ball with `enhanced-determinism`: gravity, bounce, spin and Magnus, posts and crossbar | none (new) | Cross-arch hash identical; otherwise fall back to the custom ball model |
| M2 | Physics and boundaries | Timestep everywhere, units audit, 3D ball integrated, whole-ball line crossing, goals with height, restarts (kickoff, throw-in, corner, goal kick), state-gated schedule, clean clock (45:00 halves) | B1, B2, B3, B8, B9 (non-added-time part), R2, R3 | Speed-ceiling test in m/s passes; goals and restarts correct on both sides |
| M3 | Teams, orientation, formations | `AttackingDirection` per team and half, legal kickoff and end swap, goalkeeper role, five formations as data, 14-position model with position fit, attribute schema (1 to 100), bundled spawn, protocol v0 published as a mock stream | B5, B10, E3 | Swap-symmetry test passes; Law 8 kickoff invariant passes |
| M4 | Play loop | Perception without self-match, possession and contest model (ball follows carrier, swept acquisition, tackles resolved once per challenge, no fouls), passes and shots acting on the ball, goalkeeper saves, RNG stream isolation, widened state hash | B4, B6, D2, D3 | Sanity envelope (B1) passes |
| M5 | Tuning configuration and batch runner | Immutable versioned config bundle, config hash, strict loader, N-match runner, two-config diff report | A2 | A config change visibly changes a batch report; the same config reproduces identical reports |
| M6 | Tactical causality | Attribute-to-behaviour mapping (6.3), formation and mentality modifiers, hysteresis fixes, decision cadence on the monotonic tick, per-team manager mentality logic, AI manager as a command source | A1, A3, A4, E1 | Directional tactics (B2) and strength monotonicity (B3) pass |
| M7 | Live session and protocol | `sim-protocol` and generated TS types, `MatchSession` API, SSE snapshots with resume, command POST with tick assignment, durable log, working command queue, pace controller, highlight classification, commentary events, disconnect behaviour, stoppage substitution queue with entity-level swap, crash recovery | B7, R5, E5, I1, P2 | A scripted human and AI match completes live; every accepted command applies exactly once and replays identically |
| M8 | Replay, verification, archival | Replay artifact and checkpoints, verify-only mode and multi-version verifier, reproducible containerised builds, release manifest, release gate corpus | D6 | An archived build verifies reference replays on both architectures |
| M9 | Release 1 acceptance | All Section 5 checks A and B at N of at least 1,000, frontend integration against the live server, informal playtest (recommended) | | Everything in Section 5 passes |

**Rationale (inference):**
- M0 first, so later failures are attributable.
- M1 before M2, so the physics rework is built once on the chosen ball model.
- M5 after M4, because a batch runner measures nothing until the match plays coherently.
- Protocol v0 in M3 lets the frontend build against a mock stream. If the frontend is built by the same single workstream, drop it and publish the protocol at M7.

**Consequences of strictly sequential:**
- [Certain] The first human-watchable live match arrives at M7. Everything before it is headless.
- [Likely] A flaw in the SSE, command or pacing design found at M7 can force changes to state shapes fixed in M3 and M4. A walking-skeleton milestone is the standard mitigation and is available on request. It was not added.
- [Certain] M4 and M6 are open-ended tuning work. The Section 5 gates are the only brake.

---

## 9. Risks and non-goals

### Non-goals for release 1
- Offside, fouls, cards, free kicks, penalties, advantage.
- Stamina, fatigue, injuries, pitch and weather effects.
- Stoppage-driven added time, extra time, penalty shootouts, VAR, crowd effects.
- Special abilities, morale, physical condition, team chemistry.
- Leagues, schedules, transfers and persistent squads (the game backend owns them).
- A client-side simulation fallback, and the old batch `POST /simulate` contract.
- Statistical realism as a hard target.
- Physics beyond the ball: players are not rigid bodies.
- 3D rendering.

### Risks

| # | Risk | Rating | Mitigation | Where handled |
|---|---|---|---|---|
| 1 | Rapier ball is not bit-identical across architectures | [Likely] medium | Spike gate with automatic fallback to the custom model | M1 |
| 2 | Tuning is open-ended and blocks the sequential plan | [Likely] high | Section 5 gates are the stopping rule | M4, M6 |
| 3 | Late integration: first watchable live match arrives at M7 | [Certain] medium | Mock stream from protocol v0; walking-skeleton milestone available | M3, M7 |
| 4 | "Verifiable forever" is a permanent operating cost and old builds age | [Certain] medium | Reproducible builds, sandboxed verify-only mode, release gate corpus | M8 |
| 5 | Without realism targets, averages can drift to odd values unnoticed | [Likely] medium | Sanity envelope catches extremes only; consider a soft goals-per-match band later | M5, M9 |
| 6 | With fouls and offside out, pressing and defensive-line tactics are weaker levers than in the competing game | [Likely] medium | Documented limitation; first candidates for release 2 | Scope |
| 7 | 60 Hz continuous simulation is expensive at scale | accepted by owner | The batch runner may force a performance milestone if CI becomes too slow | M5 |
| 8 | Variable pacing is complex and must stay fair between two human managers | [Likely] medium | One shared pace per match; pacing never feeds back into the simulation | M7 |
| 9 | Stale documents contradict the plan (TypeScript milestone docs, frontend instructions) | [Certain] low | Mark as historical; rewrite the frontend instructions against `sim-protocol` | M3, M7 |
| 10 | The code review was static: nothing was compiled or run | [Certain] medium | M0 turns the headline defects into failing tests; a finding is treated as a bug only once its test fails | M0 |
| 11 | Command authentication is undecided | [Certain] high | See OD-1; must be decided before M7 starts | M7 |

### Open decisions

**OD-1: Command authentication (open).**
- **Option (a):** the game backend issues a short-lived signed token per `(match_id, side, manager)`. The Rust server verifies the signature and applies a command only if the token's side matches. It never stores credentials. The signing key is shared through configuration, and key rotation is a game-backend responsibility. Every logged command records the authenticated manager, which gives an audit trail.
- **Option (b):** the Rust server trusts the game backend completely, with no per-command authentication. This is only viable if browsers never reach the Rust server directly, meaning the game backend proxies every request. With SSE and POST as the transport, the React client is the natural caller, so a direct connection would leave the endpoint open to anyone who can reach it.
- **Deadline:** before M7 starts. The command-log schema in Section 7.4 depends on the answer.

**Substitution wait.** No maximum wait is set for queued substitutions. Without fouls, play stops only at goals and when the ball goes out. Revisit in M7 if watching matches shows long waits.

---

## Appendix A: Provisional parameters

These are placeholders to be fixed by tuning. None of them is a decision.

| Parameter | Provisional value | Fixed in |
|---|---|---|
| Batch size N per check | at least 1,000 per arm | M5 |
| Sanity bounds | at least 3 shots, at most 12 goals per match | M4 |
| Stuck-ball threshold | 30 simulated seconds while live | M4 |
| Checkpoint interval | every 10 simulated seconds | M8 |
| Halftime break length | configurable; no value chosen (the current code's 18 ticks is 0.3 s) | M2 |
| Fast-forward ceiling, highlight pace, simulation lead | tuning values against the ~10-minute target | M7 |
| Direction table (B2) | as in Section 5 | M6 |

## Appendix B: Assumptions made by the engine side, for owner confirmation

- Attribute scale 1 to 100 at the engine boundary (6.3).
- Halftime is excluded from the 10-minute duration target and is a configurable pause (6.8).
- Fitness and Communication stay in the schema as inert fields (6.3).
- The five-formation set (Section 4).

## Appendix C: Decision log

| Section | Decision |
|---|---|
| Canonical backend | Rust/Bevy `sim-server`; the TypeScript engine is superseded |
| 1 | Football management game, Top Eleven category; differentiator is visible tactical causality; cost risk accepted |
| 2 | Live manager commands; any human/AI pairing; substitutions requested live, executed at stoppages |
| 3 | Priorities: tactical causality, tunability, 3D ball with spin, Laws, statistical realism |
| 4 | Release 1 scope A plus stoppage substitutions; offside and fouls out; five formations |
| 5 | Checks: sanity envelope, directional tactics, strength monotonicity; integrity checks A1 to A4 |
| 6 | SSE plus HTTP POST; Rapier for the ball only with a fallback gate; Top Eleven attribute and position model; match-only server with a clean interface; ~10-minute matches with variable pacing; server owns the command log; disconnects do not pause |
| 7 | Deterministic contract; verifiable forever |
| 8 | One workstream, strictly sequential M0 to M9 |
| 9 | Risks and non-goals as listed; command authentication left open (OD-1) |
