# GLOSSARY.md — domain vocabulary for football-sim-server

Single source of truth for the domain terms used across the codebase, architecture
reviews, ADRs, and code comments. Keep this file in sync with the code: when a
concept is renamed, split, or merged, update the glossary in the same PR.

Terms are listed alphabetically. Each entry names the canonical type(s), the
owning crate/file, and a one-line definition.

---

## A

**ActionIntent** — `sim_components::Intent::Action`. A discrete tactical action a
player commits to: `PassTo`, `ShootAtGoal(Vec2)`, `Tackle(Entity)`,
`MarkOpponent(Entity)`, `Press(Entity)`. See `Intent`.

**added time** — Extra ticks granted to a half beyond its regular
`HALF_LENGTH_TICKS`, computed by `added_time_calculation_system` from the
referee's stoppage queue (PRD §8.4), capped at 10 minutes per half.

**AI-manager** — `sim-ai-manager`. Weighted-table manager (mentality shifts,
substitutions, formation changes); effectively no-op in MVP. Registered in
`SimulationSet::MatchAdmin`.

## B

**Ball** — `sim_components::Ball`, a `Resource`. The singleton source of truth
for dynamic ball state: `spin`, `state`, `possessor`, `last_touch`,
`touch_history`, `kick_velocity`. Owned by `sim-core` (`WorldInstaller`).

**BallAirborne** — `sim_components::BallAirborne`, a `Resource`. Landing point
and flight-ticks-remaining for an airborne cross (PRD §7.4).

**BallMarker** — `sim_components::BallMarker`, a zero-sized `Component` on the
ball entity so systems find it via `Query` instead of `iter_entities()`. The
ball entity's `Position`/`Velocity` are Components; its dynamic state lives on
the `Ball` Resource.

**BallOutOfBoundsType** — `sim_components::BallOutOfBoundsType`: `ThrowIn`,
`GoalKick`, `Corner` (Law 9 restart types).

**BallState** — `sim_components::BallState`: `Free`, `Possessed`, `InFlight`,
`Dead`. Mirrored from `Ball.possessor.is_some()` plus physics velocity.

**ball entity** — The single ECS entity carrying `BallMarker`, `Position`,
`Velocity`. Tracked as `Simulation.ball_entity` at construction; re-discovered
via `iter_entities()` + sort in `snapshot.rs`/`state_hash.rs` (known friction;
see Blueprint on ball discovery).

**build_id** — `sim_server::api::render::Manifest::build_id`. The
content-derived digest of a rendered match: SHA-256 over the fixture's
contents, the wire format version and every segment's hash. It changes
whenever the rendered bytes change and depends on no crate version and no
git hash (neither exists in this repository). It is embedded in every
segment URL, so a URL is immutable.

## C

**cacheable segment** — A segment response the browser may keep for a
year: `Cache-Control: private, max-age=31536000, immutable`. The body is
private to the client (`private`, not `public`, until a CDN exists) and
never changes behind its URL, because the URL embeds the `build_id`.

**clock** — See `MatchClock`.

**Component** — Per-entity ECS state (`Player`, `Position`, `Team`, …). Bevy
`Component` derive. Contrast with `Resource` singletons.

**construct.rs** — `crates/sim-core/src/simulation/construct.rs`. `Simulation::new`,
`create_match`, and `register_systems` (schedule wiring).

**consideration** — One scored input to a Utility AI action (`sim-ai-player`,
`Consideration`). Raw input from perception → `ResponseCurve` → weighted score.

**CurrentTick** — `sim_rules::CurrentTick(pub u64)`, a `Resource` inserted by
`sim-core` before each schedule run so referee systems can read the tick.

## D

**decision cadence** — `DECISION_CADENCE_TICKS` (6 ticks ≈ 10 Hz). Each player
re-evaluates only when `elapsed_ticks % 6 == player_stagger_slot(entity)`.

**determinism** — Hard invariant: same seed → byte-identical output. Enforced
via `SimRng` (LCG), no wall-clock, `f32::total_cmp` where ordering matters,
`Entity::to_bits()` stable ordering, and `state_hash` excluding tick/RNG.

**deterministic failure** — `sim_server::api::runner::FailureKind::Deterministic`.
A failure that repeats: the same fixture and the same seed fail the same way, so
the match service caches it for the life of the process and never retries it.
Contrast with **transient failure**.

## E

**ECS** — Bevy ECS 0.14 (no full Bevy). Systems take `Query`/`Res`/`ResMut` by
value. World owned by `sim-core::Simulation`.

**emit slots / for-emit resources** — Single-slot `ResMut` resources written by
rule systems and drained by `sim-core::event_emit`: `GoalForEmit`,
`KickoffForEmit`, `OobRestartForEmit`. Fractured signal; consolidation target
(Blueprint 2, `RuleOutcome`).

**EventEnvelope / MatchEvent / EventLog** — `sim-core::simulation::events`.
Per-tick structured match events (PRD §9): envelope `{seq, tick, half, minute,
added_minute, type, team}` + typed payload. `EventLog` owns the `seq` counter.

**execution system** — `sim-ai-player::execution::player_action_execution_system`
(+ `kick_execution_system`, `tackle_recovery_system`). Converts `Intent` to
velocity, stamina, stochastic tackle/pass/shot outcomes, pending signals.

## F

**foul** — `sim_components::RuleEvent::Foul` / `FoulType::DangerousPlay`
(Phase 1 Law 12 stub; `foul_detection_system` in `sim-rules`).

**FullTime** — `MatchState::FullTime`; terminal match state reached when both
halves (regular + added time) are exhausted.

## G

**goal** — `sim_components::RuleEvent::Goal`; Law 10. Ball fully crosses the
goal line inside the goal mouth. Scorer attributed by ball position × current
half attack direction, not last touch.

**GLOSSARY.md** — This file; the domain language for this codebase.

**gzip** — `sim_server::api::render::gzip`. The coding the segment
bodies are stored in: gzip at the fixed level `GZIP_LEVEL` (6), through
the pure-Rust backend, so the stored bytes never depend on the
platform's zlib. The level is a constant, not a knob.

**gzipped_body** — `sim_server::api::render::RenderedSegment::gzipped_body`.
The stored body of a segment: the gzip bytes, shared between clones
(`Arc<[u8]>`) and never mutated. The uncompressed body is not kept;
`RenderedSegment::decompress` reproduces it on demand for a client
that does not accept gzip.

**gzipped_bytes** — `sim_server::api::render::ManifestSegment::gzipped_bytes`.
The length of the stored gzip bytes, published per segment in the
manifest: the transfer cost of a match is known from the manifest
alone, before a client asks for a byte.

## H

**half-time** — `MatchState::HalfTime`. Transition driven by
`lifecycle_system` / `match_duration_enforcement_system`; clock resets to 0 for
half 2; first-half added time stashed in `FirstHalfAddedTime`.

## I

**Intent** — `sim_components::Intent`: `Movement(MovementIntent)` or
`Action(ActionIntent)`. Single source of truth for steer/kick dispatch via
`Intent::steer_target` / `Intent::kick` in `sim_components::intent_dispatch`.

**InvariantCounters / InvariantViolation** — `sim_core::simulation::invariants`.
Six PRD §12 invariants checked per tick: score == goal count, clock monotonic,
legal state transitions, dead-ball window ≤ 1800 ticks, valid entity state,
FullTime before `TICK_CAP`.

## L

**lifecycle_system** — `sim-core::simulation::lifecycle`. `MatchState`
transition driver (PreMatch → Kickoff → InPlay → HalfTime → … → FullTime), ball
placement at restart, `apply_player_slot`. Runs outside the schedule each tick.

**LastTouch** — `sim_components::LastTouch`: who last touched the ball (`team`,
`player`, `player_id`, `kind`, `tick`). Written only via `Ball::record_touch`;
buffered through `PendingBallTouches`.

## M

**manifest** — `sim_server::api::render::Manifest`. The document that
describes a rendered match: its metadata (match id, seed, teams, roster, pitch
dimensions, tick rates, attack direction), the end-of-match summary under
`result`, the wire format version, the `build_id`, and one row per segment
(number, half, tick range, immutable URL, encoding, uncompressed byte length,
gzipped byte length, SHA-256). It is the JSON body of `GET
/match/{match_id}/manifest` (with `state: ready` added) and the only document
a client needs before fetching segments.

**Match** — `sim_components::Match`, a `Resource`: id, team entities, score,
state, seed. Source of truth for match score/state.

**match service** — `sim_server::service::MatchService`. The single-match
lifecycle behind the HTTP routes: it holds each fixture in `NotStarted`,
`Queued`, `Running`, `Ready` or `Failed`, runs at most `cores − 1` matches at
once through a fair FIFO semaphore (a start beyond the cap queues, it is never
rejected), enforces single-flight per match, caches deterministic failures for
the life of the process and gives transient failures a retry window. It is
built by `sim_server::app::match_service` from the validated fixture set and a
`runner`; `sim-server serve` and the real-match end-to-end test use that same
constructor.

**match status** — `sim_server::service::MatchState`, the service's one
lifecycle type, and `MatchService::status`, the one call that answers it. The
answer is one of `UnknownFixture` (the id is not a fixture at all — not the
same as a match that was never started), `NotStarted`, `Queued`, `Running`,
`Ready` (carrying the match's `published` bytes) or `Failed` (carrying the
failure's code, message, last good tick and the time left before a retry).

**MatchClock** — `sim_components::MatchClock`: `elapsed_ticks`, `half`,
`added_time_ticks`, `is_running`. Spec §3: a `Resource` (source of truth for
systems); also kept as a `Component` on the match entity for backwards compat
(known dual-form anomaly; manual mirror in `tick_clock()`).

**MatchDriver** — Proposed unified loop wrapper (Blueprint 1) consolidating the
three duplicated per-tick loops in `simulate` / `harness` / `api/runner`.

**MatchSnapshot / BallView / PlayerView / ClockView / MatchStateView** —
`sim_core` read-only view types returned by `Simulation::get_state()`.

**Manager** — `sim_components::Manager`: per-team manager component
(weighted decision table, cooldown). `sim-ai-manager`.

**MovementIntent** — `sim_components::MovementIntent`: `MoveToPosition`,
`HoldPosition`, `ChaseBall`, `Intercept`, `SupportRun`, `TrackBack`.

## O

**OOB / out-of-bounds** — Law 9. `out_of_bounds_system` detects a settled ball
at the boundary, classifies restart type, writes `PendingOutOfBounds`.
`PendingOutOfBounds` — resource (not deferred component) carrying the same-tick
OOB signal to `restart_system`.

## P

**PerceptionSnapshot** — `sim_components::PerceptionSnapshot`, a per-player
`Component`: self position, nearby teammates/opponents, ball position/state,
goal position, pitch bounds, home slot. Built by `perception_system` each tick.

**PendingBallTouches** — `sim_components::PendingBallTouches(pub Vec<LastTouch>)`
resource. Writers push; `apply_ball_touched_system` drains onto
`Ball.last_touch` at two sites (start of Rules set; after post-schedule
lifecycle in `Simulation::tick`).

**PendingOutOfBounds / OutOfBoundsEvent** — `sim_rules` resources carrying the
same-tick OOB signal (see OOB).

**PendingRestart** — `sim_rules::PendingRestart`: optional `(event,
restart_tick, scorer_team)`; created on demand when a goal is scored, drained
by `restart_system` after 60-tick kickoff delay.

**PendingRuleOutcome** — Proposed single transactional record (Blueprint 2,
`RuleOutcome`) replacing the fractured emit slots.

**Perception / Decision / Execution / Physics / Possession / Rules /
MatchAdmin** — The seven `SimulationSet` phases, in fixed order, enforced by
`schedule.configure_sets(...).chain()` in `construct.rs`.

**PitchControlGrid** — `sim_physics::PitchControlGrid` resource: 16×12 grid of
per-cell attacker-win probability; rebuilt each tick by `pitch_control_system`.

**Player** — `sim_components::Player` component: `team_id`, `intent`.

**PlayerId** — `sim_components::PlayerId(u64)`: stable player identity minted
from `Entity::to_bits()` (the fixture `player_id` string is carried by
`PlayerIdentity`).

**PlayerIdentity** — `sim_components::PlayerIdentity`, a per-player
`Component`: the fixture's `player_id` and the player's index in its side's
fixture roster. Inserted by `sim_core::Simulation::new_from_fixture`, so the
API's manifest names players by their fixture id (PRD §9.1) instead of an id
invented from the spawn order. A captured frame names them by their **roster**
row instead, and the segment body spells the fixture id back out.

**PlayerProfile** — `sim_components::PlayerProfile`: 30 validated attribute
slots (PRD §6); goalkeeper vs outfield defaults.

**PlayerView** — `sim_core::PlayerView`: read-only per-player projection for
snapshots.

**pitch** — 105 m × 68 m (`PITCH_LENGTH` × `PITCH_WIDTH`); goal mouth
`GOAL_Y=[26.68, 41.32]`; center spot `(52.5, 34.0)`.

**Possession** — `Ball.possessor: Option<Entity>`; `BallState::Possessed`.
Resolved by `possession_resolution_system` via swept-path proximity (1.5 m
radius).

## R

**render / render_match** — `sim_server::api::render`. The pure step that
turns a fixture plus a completed match into a manifest and a list of
segments. Same input, same bytes, every time.

**revalidation** — `sim_server::http::conditional`. A client that asks whether
what it already holds is still current: it sends the validator it has in
`If-None-Match`, and the server answers `304` when that validator names the
resource's current one. The manifest is the only revalidatable resource: its
validator is the `ETag: "<build_id>"` the 200 carries, and the comparison is
the weak one (RFC 9110 §13.1.2), so a weak `W/` validator and a list of
validators both match.

**Resource** — Bevy ECS singleton (`Match`, `Ball`, `MatchClock`, `SimRng`, …).
Contrast with `Component`.

**restart** — Placement after a goal (kickoff, 60-tick delay) or OOB
(throw-in/goal-kick/corner). `restart_system` in `sim-rules`.

**Referee** — `sim_components::Referee` component: `stoppage_events`, `cards`.
Spawned once at match construction; accrues one `StoppageEvent` per dead-ball
tick.

**retry window** — `sim_server::service::DEFAULT_RETRY_WINDOW` (5 s). The time a
**transient failure** waits before a later start may try the match again. A
start inside the window reports the failure with the remaining wait; a start
after it schedules the match again, exactly like a fresh start.

**roster** — The manifest's list of the 22 players of a match: one row per
fixture player, home rows first then away rows, each in the fixture's roster
order. The row is what a captured frame names its players by
(`sim_server::api::model::RosterIndex`): the manifest's roster row `i` is the
player a frame's index `i` names, and the segment body spells the fixture id
back out of it.

**RuleEvent** — `sim_components::RuleEvent`: `OutOfBounds`, `Goal`,
`KickoffRestart`, `HalfTimeStart`, `FullTimeStart`, `Foul`. Emitted as
components on the ball entity (deferred; visible next tick) — see Coding
Standards gotcha on deferred commands.

**runner** — `sim_server::api::runner::MatchRunner`, the seam the `match
service` runs a match through. `run(fixture, tick_cap)` returns a
`MatchOutcome` (`Ready(CompletedMatch)` or `Failed { code, message,
last_good_tick, kind }`); production uses `SimulationRunner`, which plays the
match and captures frames on the blocking thread, while tests inject fakes.
Failures are classified `Deterministic` (cached forever) or `Transient` (the
resource failure `RESOURCE_FAILURE_CODE`, retried after the retry window).

## S

**segment** — `sim_server::api::render::RenderedSegment`. One 60-second
slice of match time (3,600 ticks), cut from a half's own first tick. The last
segment of a half runs to the whistle and absorbs the remainder, so no
segment crosses a half boundary. A half yields 45 segments plus one per
added minute; the count always comes from the match. Its body is stored
gzipped (`gzipped_body`); its identity is the SHA-256 of the *uncompressed*
body, never of the compressed bytes. Served at
`GET /match/{match_id}/segments/{build_id}/{number}.json`: 200 with the
bytes when the match is ready, 409 when it is not (or when the server
forgot it after a restart), 404 for an unknown match, an unknown build id
or an unknown segment number.

**segment route** — `sim_server::http::service_api::segments`. The one
route that serves rendered bytes. It negotiates the coding per request: a
client that accepts gzip gets the stored gzip bytes with
`Content-Encoding: gzip`, every other client gets the decompressed
bytes. Both decode to the same body, whose SHA-256 is the hash the
manifest publishes, so the URL means the same thing either way. The 200
carries `Vary: Accept-Encoding`, so a cache keeps the two
representations apart. It reads the request path with the **URL scheme**'s
parser, so a path that is not a segment URL names no segment.

**Simulation / SimulationSet** — `sim_core::Simulation`: owns `World` +
`Schedule` + `tick()`. `SimulationSet` = the seven ordered phases.

**SimRng** — `sim_physics::SimRng`: deterministic LCG seeded from `Match.seed`;
the only RNG source in simulation systems.

**snapshot / get_state** — `sim-core::simulation::snapshot`; builds
`MatchSnapshot` from resources + entity queries. Sorting ball candidates per
call is a known perf friction (AGENTS.md gotcha).

**state_hash** — `sim-core::simulation::state_hash`: deterministic FNV-1a-64
over entities (sorted by `to_bits()`) + `Match`/`Ball`/`MatchClock` resources +
seed. Excludes tick counter and live `SimRng`.

**stamina** — `sim_components::Stamina(pub f32)`; recovered each tick, drained
for running/press/tackle, scaled by Fitness/Aggression response curves.

**stoppage** — `sim_components::StoppageEvent { tick, reason }` on `Referee`;
feeds added-time calculation. Reasons: `dead_ball`, `oob_restart`.

## T

**Team** — `sim_components::Team` component: id, name, formation, mentality,
tactic, players/substitutes entity vecs, private `attack_direction`
(half-time-only flip via `apply_transition`).

**TeamId** — `sim_components::TeamId(pub u8)`: 0 = home, 1 = away.

**TeamSetup / TacticParameters** — `sim_components::tactic_params`:
`TeamSetup{formation, mentality, tactic}`; `TacticParameters` numeric
derivation (pass length, press trigger/stamina, line height, transition speed).

**tick** — One fixed 1/60 s simulation step. `TICKS_PER_SECOND=60`,
`HALF_LENGTH_TICKS=162000`, `TOTAL_MATCH_TICKS=324018` (+1 loop iteration to
detect `FullTime`).

**TickHook** — Proposed per-tick callback in `MatchDriver` (Blueprint 1).

**TouchKind** — `sim_components::TouchKind`: `Restart` (placement) or `Play`
(active kick/possession).

**transfer budget** — The 5 MB a whole match's gzipped segments must fit
in. Measured on m-0001 (110 segments, 110 minutes of match time): 6.5 MB
gzipped, 137.2 MB uncompressed, a 4.7% ratio — **over** the budget, so
the wire format is due for a revisit. Reproduce with
`cargo test --release -p sim-server --lib -- --ignored a_real_match_reports --nocapture`.

**transient failure** — `sim_server::api::runner::FailureKind::Transient`. A
resource failure: the run could not get a resource it needed. Today the one
catchable source is the memory for the animation frames
(`RESOURCE_FAILURE_CODE`): a full match's frame capture holds ~62 MB
resident (it held ~143 MB before a frame named its players by their **roster**
row instead of their fixture id), so a host under memory pressure cannot serve
it. The pressure belongs to the host, not to the match, so the failure carries
a **retry window** instead of being cached forever. Contrast with
**deterministic failure**.

## U

**URL scheme** — `sim_server::api::render::SegmentUrl`: the shape of the
URLs the manifest publishes for its segments,
`/match/{match_id}/segments/{build_id}/{number}.json`. One definition
owns the whole scheme — the path pattern the router routes, the URL the
manifest publishes and the parser that reads a path back into its parts
— so the manifest can only publish URLs the server serves. The `.json`
suffix belongs to the segment's value, not to the route: a router
matches whole path segments, so a route cannot pin a suffix inside one.

**UtilityBrain / Utility AI** — `sim-ai-player::brain`: per-player brain of
actions × considerations × response curves; scored via geometric mean with
hysteresis bonus.

## V

**View types** — Read-only projections: `MatchSnapshot`, `BallView`,
`PlayerView`, `ClockView`, `MatchStateView`. Used by JSONL output and traces.

## W

**WorldInstaller** — `sim-core::simulation::installer`: composes
`sim_rules`/`sim_physics`/`sim_ai_player` resource installers plus sim-core
resources (`Ball`, `PendingBallTouches`, `BallAirborne`, `PendingCross`,
`HalfTimeEntryTick`, `FirstHalfAddedTime`, `EventLog`, `Tuning`). Deliberately
excludes `TelemetryConfig`/`TraceGate` (opt-in) and `PendingRestart`
(on-demand).

---

### Conventions

- New domain concept? Add it here in the same PR, alphabetically, with the
  canonical type and owning file.
- Rename a type? Update this glossary and all doc comments referencing the old
  name.
- Do not invent a new term for an existing concept; reuse the glossary entry.
