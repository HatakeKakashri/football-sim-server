# Football Match Simulation Engine — Codebase Review

*Rust · ECS · Utility AI · Server-Authoritative*

**Scope.** A working review of the current codebase across all 10 crates
(`sim-math`, `sim-components`, `sim-physics`, `sim-ai-core`, `sim-ai-player`,
`sim-ai-manager`, `sim-rules`, `sim-core`, `sim-replay`, `sim-server`),
informed by the consolidated design spec at `spec/football-sim-server-spec.md`
and the feature specs at `specs/001-…` and `specs/002-…`.

**Construction notes.**

- The codebase visibly reflects the phased construction plan in spec §12.
  Where phase markers matter to today's behaviour or determinism contract
  (e.g. Phase 0 substrate verification, Phase 1 read/write-separation
  demonstration, Phase 2 Utility AI, Phase 3 rules) I'll comment; where they
  are simply stale "Phase N: real X" comments about unimplemented features
  I will skip them.
- This is a review, not a spec. No code is proposed as final. Recommendations
  are framed as concrete steps with their trade-offs; the "Design Brainstorm"
  at the end is for us to pick a target together.
- The codebase builds clean against `cargo check` and `cargo clippy`
  (pedantic+nursery+all lints enabled at workspace level) — review notes are
  forward-looking, not blocking.

**Phase A Completion Status (as of 2026-09-28).** The following high-impact/low-effort
items from §5.1 have been implemented and tested:

- ✅ **§2.4** Removed dead `Simulation.rng` field (`sim-core/src/lib.rs`)
- ✅ **§2.7** Replaced `HashMap<TeamId, f32>` with fixed `[f32; 2]` array in `perception_system` (`sim-ai-player/src/lib.rs`)
- ✅ **§2.3** Dropped `clock` field from `Match`; single source of truth is `MatchClock` component (`sim-components`, `sim-core`, `sim-rules`, `sim-ai-player`, `sim-ai-manager`, `sim-replay`)
- ✅ **§2.11** Moved `apply_command` into `sim-core` as single authority; `sim-server` delegates
- ✅ **§4.7** Removed `sim-ai-core` dependency from `sim-ai-manager` (`Cargo.toml`)
- ✅ **§4.8** Removed `sim-rules` and `sim-physics` dependencies from `sim-server` (`Cargo.toml`)
- ✅ **§2.12** Renamed `sim_replay::MatchSnapshot` → `RecordedSnapshot` (`sim-replay/src/lib.rs`)
- ✅ Replaced `rand` crate RNG with deterministic LCG in `SimRng` (`sim-physics/src/lib.rs`); fixed RNG draw order in `player_action_execution_system` by sorting entities by ID (spec §7 rule #22)

The remaining Phase A item — **§2.8/§4.6 Replace `println!` with `tracing`** — is deferred to Phase C when spec-002 tracing infrastructure lands. All 29 `println!` calls remain in place.

All workspace tests pass (71 tests), and the CLI produces deterministic state hashes within a single process. Cross-process determinism is not yet guaranteed due to `bevy_ecs` query iteration order in `pitch_control_system` and `possession_resolution_system` (see Phase C: §4.3).

---

## 1. System Understanding

### 1.1 What the system is

A server-authoritative, deterministic, fixed-timestep football match
simulation engine. The server is the single source of truth for match
state; clients would render interpolated snapshots off a future
network protocol (deferred — §13 decision #20). One process owns one match.

The simulation pipeline runs per tick in this explicit order:

```
Perception  →  Decision  →  Execution  →  Physics  →  Possession  →  Rules  →  MatchAdmin
```

with a final `lifecycle_system` step outside the schedule (match state
machine and ball placement / kickoff impulse) and `tick_clock` to advance
the `MatchClock` resource.

### 1.2 Crate boundaries

```
                        ┌──────────────────────────┐
                        │       sim-math           │  Vec2, PitchDimensions, no logic
                        └──────────────────────────┘
                          ▲            ▲      ▲     ▲
                          │            │      │     │
              ┌──────┐ ┌─────┐  ┌────┐ ┌─────┐ ┌─────┐ ┌────────┐
              │sim-  │ │sim- │  │sim-│ │sim- │ │sim- │ │  sim-  │
              │server│ │replay│ │rules│ │physics│ │ai-...│ │ core   │
              └──────┘ └─────┘  └────┘ └─────┘ └─────┘ └────────┘
                  ▲        ▲       ▲      ▲        ▲        ▲
                  └────────┴───────┴──────┴────────┴────────┘
                              sim-components
                  (pure-data components & resources)
```

Reading the workspace's `Cargo.toml`s confirms the spec's intended shape,
with one drift to call out: the spec mandates `sim-server` depends on
`sim-core` *only* — never `sim-ai-*` internals. Current `sim-server/Cargo.toml`
depends on `sim-rules` and `sim-physics` in addition to `sim-core`, which
is a pragmatic Phase-2 expansion but a deviation from the "binary crate
sees only the seam" discipline.

### 1.3 Data and control flow

End-to-end on one tick:

1. `Simulation::tick()` inserts `CurrentTick` resource → `schedule.run()` →
   `lifecycle_system` → `tick_clock` → `self.tick += 1`.
2. `SimulationSet::Perception`:
   - `pitch_control_system` (in `sim-physics`, not the AI crate) rebuilds
     the per-cell pitch-control grid using a snapshot of the current
     `Player` and `Ball` entities.
   - `perception_system` (in `sim-ai-player`) builds a `PerceptionSnapshot`
     for each player carrying teammate/opponent lists, ball position/state,
     and match context.
3. `SimulationSet::Decision`: `player_decision_system` evaluates every
   player's `UtilityBrain` against its freshly-built snapshot and writes
   `player.intent`.
4. `SimulationSet::Execution`:
   - `player_action_execution_system` steers each player toward the target
     derived from their intent. RNG draws (tackle success, pass completion,
     shot accuracy) feed back into player velocity.
   - `kick_execution_system` (only if a `Ball.possessor` is set) writes
     ball velocity and clears possessor.
5. `SimulationSet::Physics`: `ball_physics_system` (integrate, clamp,
   bounce off walls) then `player_movement_system` (clamp + integrate).
6. `SimulationSet::Possession`: `possession_resolution_system` updates
   `Ball.possessor` and `Ball.last_touched_by` based on proximity (with
   skill-tiebreak rule from `SKILL_TOLERANCE`).
7. `SimulationSet::Rules`: OOB → goal → offside → foul → restart (all
   chained in that order).
8. `SimulationSet::MatchAdmin`: added-time calc, minimum-player warning,
   substitution, mentality shift.
9. `lifecycle_system` (outside the schedule): match state machine and
   `reset_formation_to_4_4_2`, plus a per-tick *sync* from `Position`/
   `Velocity` components back into `Ball.position`/`Ball.velocity` and
   `Player.position`/`Player.velocity` fields. (See §2.1.)

### 1.4 Design constraints I confirmed

- **Server-authoritative**: clients don't execute any match logic (spec §2;
  affirmed in `sim-server/src/lib.rs`).
- **Single-tick read/write separation**: perception/decision systems only
  write to `Intent`-shaped output and play on the previous tick's resolved
  state (spec §3, reaffirmed by every AI system reading from a
  `PerceptionSnapshot` rather than live `Query`).
- **Determinism bar**: same-build, same-machine reproducibility; `f32`
  IEEE-754 operators are stable enough; `transcendentals` (only `exp()`
  in the logistic curve and pitch-control sigmoid) are an accepted named
  exception (spec §7.1, decision #24 — `f32::exp()` for the logistic is
  *deliberately* used; I do not flag this as a violation).
- **Schedule ordering discipline**: every order-sensitive system is
  pinned via `SimulationSet` chaining and `after(...)` (no work-stealing
  parallelism on order-sensitive code). The schedule uses Bevy's default
  work-stealing only where borrow-based conflict detection is sufficient.
- **One RNG** (`SimRng` resource) drives every stochastic game-event
  outcome. A second `SmallRng` lives on `Simulation` but is **never used** —
  see §2.4.
- **`unsafe_code = "forbid"`** at the workspace level. Confirmed.

### 1.5 What is unclear or needs more investigation

These I'd flag as hypotheses rather than confirmed problems. Each one
would need a small probing change before being committed to:

- **RNG-drawing systems' iteration order.** `player_action_execution_system`
  iterates a `Query<(&mut Player, &mut Velocity)>` while calling
  `rng.gen()` / `rng.gen_range(...)` per player. Bevy 0.14 doesn't
  contractually guarantee iteration order is stable, and this system
  *does* draw from the shared RNG inside the loop. This is exactly the
  failure mode spec §7 calls out as Rule 22 ("RNG-consuming queries must
  sort entity ids first"), but I can't tell from static reading whether
  Bevy 0.14 is actually deterministic about query traversal in practice.
  **This needs a runtime probe**, not just reasoning from the spec.
- **`velocity.0 = direction * speed` ordering inside `kick_execution_system`**
  vs the `clamp_length` that runs in `ball_physics_system` immediately
  after. The clamp is documented to honour `MAX_BALL_SPEED`, but I haven't
  verified the clamp-vs-spurious-magnitude ordering for, say, a player
  trying to shoot at a target inside their own position (`length() < 0.01`
  guards this; I see the guard, that part is fine).
- **State hash coverage.** The hash mixes Position, Velocity, Stamina,
  Skill, Ball.*, Player.{team_id, role}, Team.id, Match.{state, score,
  clock} and the original seed. It does **not** mix `intents` or
  `perception` fields. Two simulations with identical positions but
  different intent choices will hash the same at that tick. This may be
  intentional (snapshots ≠ intents), but it's worth aligning with what
  callers expect.
- **Referee component is never populated.** `create_match` does not spawn
  one. Spec 002 has an explicit clarifying decision that this is fine for
  now, but several rules (`referee_advantage_system`, card state,
  `Referee.stoppage_events`) silently no-op.
- **`[Likely]` (`confidence labels from prior research are propagated into
  code-adjacent comments but not into anything programmatic).** No
  action item unless you want to formalise the labelling.

---

## 2. Anti-Patterns and Design Issues

Each item below is a confirmed or near-confirmed problem. I cite the
specific crate, function, and lines where the evidence lives, the principle
it conflicts with, a concrete improvement, and the trade-off.

### 2.1 Ball & Player struct-field duplicates kept alive by `lifecycle_system`

**Location.** `crates/sim-core/src/lib.rs`, function
`lifecycle_system` (lines ~761–935). Every tick:

```rust
// Sync Player.position/velocity from Position/Velocity components.
// Ball: same pattern with Position/Velocity → Ball.{position,velocity}.
```

**What it does.** Each tick, `lifecycle_system` reads
`world.entity(ball_entity).get::<Position>()` and writes the value back
into the `Ball.position` field on the `Ball` struct. Same for each
`Player` (sync `Player.position` and `Player.velocity` from the
canonical Position/Velocity components). The `Ball` struct field
`velocity` is not written by `ball_physics_system` — the canonical
authoritative writer of position/velocity is the Position/Velocity
components, and the struct fields only exist so legacy readers
(`Simulation::get_state`, replay `create_snapshot_from_simulation`)
can find the data without iterating.

**Why it is problematic.**

1. **Two sources of truth**, with one of them (the components) being
   authoritative and the other (struct fields) being reconciled every
   tick in a hand-rolled sync. This is the classic "single-writer,
   many-readers" fragility: any system that forgets to update the
   struct field (or returns early before the sync) is a silent bug.
2. **Phase-1 carry-over.** Spec §3 emphasises strict pipeline
   separation and the perception/decision systems are written to
   respect it. This sync doesn't — it lives *outside* the chained
   system set, which is fine for ordering but still has a hard
   semantic coupling between the canonical physics writer and the
   snapshot consumer.
3. **Replaceable with a derived view.** `MatchSnapshot.ball`,
   `PlayerView`, etc. are already the only legitimate consumers of
   the data. The struct field doesn't earn its keep.

**Relevant Rust principle.** "Make illegal states unrepresentable"
(typestate / single-source-of-truth); Bevy's ECS already gives you a
way to ask "what is this entity's current Position" without having a
parallel struct.

**Concrete improvement.** Delete `Ball.{position, velocity}` and
`Player.{position, velocity, role, skill, stamina, …}` from the legacy
struct snapshots. Make `Ball` and `Player` carry only their *behavioural*
state (state machine values, intent, last-touched-by, role/skill which
are static). Snapshot readers should query `Position`/`Velocity` from the
ECS. This collapses two sync passes and removes the "sync mistake bugs"
class entirely.

**Trade-offs.** A moderate refactor across `sim-core::get_state`,
`sim-replay::create_snapshot_from_simulation`, every `PlayerView`/
`BallView` builder. Existing CLI output shape can be preserved by the
view builder reading from components, with no behavioural change.
The hash function in `get_state_hash` already reads from components
for Position/Velocity — so the hash contract doesn't change either.

---

### 2.2 `Player.perception` as a `Option<PerceptionSnapshot>` *field* on `Player`

**Location.** `crates/sim-components/src/lib.rs:202–220` (`Player` struct
field `perception: Option<PerceptionSnapshot>`);
`crates/sim-ai-player/src/lib.rs` (`perception_system` writes this every
tick); `crates/sim-core/src/lib.rs` (`lifecycle_system` syncs `Player.position`,
`Player.velocity` back into `Player.position`/`Player.velocity`).

**What it does.** The perception system stores a freshly-built
snapshot of nearby entities, ball position, and match context directly
on the `Player` struct each tick. Downstream systems (`steer`,
`player_action_execution_system`, `kick_execution_system`, the decision
system) read from this field rather than from the ECS.

**Why it is problematic.**

1. **Memory footprint.** 22 players × ~hundreds of bytes of small
   `SmallVec`s × clone-for-every-read (e.g.
   `let Some(perception) = player.perception.clone() else { … }` in
   `player_decision_system` at line 228 of `sim-ai-player`). Each tick,
   the perception system writes one `PerceptionSnapshot`, then
   `player_decision_system` clones it once. Not catastrophic at 22
   players, but it's heap-allocated `SmallVec`s in a thing that
   otherwise stores on stack.
2. **Snapshot drift.** Because `Player.perception` is a free `Option`
   field on a free struct, nothing enforces "this field is written
   before it's read, by the perception system, this tick". Tests
   have to manually populate it (which is exactly what they do — see
   `sim-ai-player/tests` lines 820–835, 920–935, 1106–1120). This is a
   known smell when the data model needs explicit setup for tests
   that *should* be ECS-driven.
3. **Reads can diverge.** `steer()` (line ~699) reads
   `player.perception.as_ref().map(|p| p.ball_position)`. If the
   perception system ever fails silently (it's a single-pass, mutable
   system reading from a `ParamSet` of four queries), downstream
   systems read stale or absent data with no diagnostic. A `Component`
   wired through `bevy_ecs` would at least make absent-data cases
   visible at compile time via systems that need `&PerceptionSnapshot`
   taking `Without<PerceptionSnapshot>` or similar filters.
4. **Architecturally: the spec calls out `PerceptionSnapshot` as the
   unit of pure decision-making.** Storing it as a struct field on
   `Player` is the one place the "pure decision" property is violated
   (writing decisions to a struct that also holds positions/velocities
   to be synced back in `lifecycle_system`).

**Relevant Rust principle.** "Componentise" data that varies on a
different cadence than the entity it lives on. Bevy's split-storage
is exactly designed for this case.

**Concrete improvement.** Promote `PerceptionSnapshot` to a
`Component`. Insert it in `perception_system`, despawn or use a
sentinel pattern when there's no useful snapshot yet. Decision-system
takes `Query<(&Player, &PerceptionSnapshot, &UtilityBrain)>`; the
decision system fails fast if a player is missing a snapshot rather
than silently falling through to a fallback.

**Trade-offs.** Some systems need to consider "no perception yet" as
a valid state (first tick after spawn). Idiomatically that's a
`Query<&PerceptionSnapshot>` filter returning none; the existing
behaviour of `continue` is already correct.

---

### 2.3 Doubled clock state: `MatchClock` is both a `Component` and a `Match` field

**Location.** `crates/sim-components/src/lib.rs:163–184`:
`#[derive(Component, …)] pub struct MatchClock`.
`crates/sim-components/src/lib.rs:252–260`: `pub struct Match { …,
clock: MatchClock, … }`. Created in
`crates/sim-core/src/lib.rs:447` (`world.entity_mut(match_entity).insert(initial_clock);`)
alongside the `Match`. Mutated in
`Simulation::tick_clock` (lines 192–206) and `lifecycle_system`
(lines 837–841) with explicit "keep both views in sync" comments.

**What it does.** The same `MatchClock` is stored twice on the match
entity: once as a separate `Component` and once as a field of `Match`.

**Why it is problematic.**

1. **Two writes for every state change.** Every site that updates
   clock state has a hand-rolled double-write (see
   `Simulation::tick_clock` and `lifecycle_system`).
2. **Drift is latent.** If any future site touches one but forgets the
   other, you get divergence between the two mirrors with no compile-time
   detection. The existing code goes to lengths in `tick_clock` to do
   `clock.clone()` then write to both — a smell that exactly this
   fragility was already felt.
3. **It's not load-bearing.** The state hash mix uses
   `m.clock.elapsed_ticks`, `m.clock.half`, `m.clock.added_time_ticks`,
   all of which can be reached through the component via
   `world.entity(match_entity).get::<MatchClock>()`.

**Relevant Rust principle.** "Don't have two variables for the same
thing." The presence of a `clone()` in the mutation path is the usual
give-away.

**Concrete improvement.** Delete the `clock` field from `Match`.
Anywhere that needs to read/write clock state does so via
`world.entity(match_entity).get::<MatchClock>()`. The DeterminismSpec
in the spec already commits to the hash reflecting clock state in a
specific way, so the contract moves cleanly to the single source.

**Trade-offs.** None of consequence. `perception_system` (Phase 2)
already takes `m.clock` from the `Match` field for a derived view
(`time_remaining_secs`); a single-line change moves it to the
component. Behaviour-identical.

---

### 2.4 `Simulation.rng: SmallRng` is wired but never used; the *real* RNG lives in a `Resource`

**Location.** `crates/sim-core/src/lib.rs:30` (`pub rng: SmallRng` on
`Simulation`), constructed at line 52 from the same `seed` that
constructs `SimRng::new(seed)` as a Resource (line 49). Nothing in
the codebase reads or writes `simulation.rng`. Confirmed across all
crates via grep — no references. Likewise `original_seed: u64` is the
seed *also* read by `get_state_hash` (line 282) — so the field
isn't dead, but the `rng` field is.

**Why it is problematic.** Dead fields are not a defect in
isolation, but here they sit alongside the actual RNG used by the
simulation (`sim_physics::SimRng` Resource). A new contributor
trying to draw randomness for a gameplay event will look at the
`Simulation` fields, find `rng`, and use it — guaranteeing
non-determinism vs. someone drawing from `SimRng` (or the inverse).
The doubleness is exactly the failure mode spec §7 explicitly warns
about ("One seeded, single-threaded `DeterministicRng` resource,
advanced in a fixed, tick-deterministic order").

**Relevant Rust principle.** "Make incorrect code obvious." When two
RNGs exist and only one is wired, mistake-resistance suffers.

**Concrete improvement.** Delete the `Simulation.rng` field. Promote
a thin accessor like `simulation.rng_mut() -> Mut<'_, SmallRng>` only
if the field is taken back into use later.

**Trade-offs.** None visible. Pure dead-code removal.

---

### 2.5 `perception_system` and `pitch_control_system` reach into the world via `world.iter_entities()`

**Location.** `crates/sim-ai-player/src/lib.rs:117–200` — a five-slot
`ParamSet` to read (players, ball, match, teams) and iterate a prebuilt
`snapshot` into a `Vec` so the mutable query on player p0 can do work.
`crates/sim-physics/src/lib.rs:127–194` — `pitch_control_system`
*doesn't even use `Query`*; it iterates `world.iter_entities()` looking
for `Ball` and `Player` + `Position`.

**What it does.** `pitch_control_system` uses
`world.iter_entities()` + `entity.get::<T>()` direct lookups
(`sim-physics/src/lib.rs:136–143`, `146–155`) instead of a `Query`.
The stated reason (`resource_mut` rather than `ResMut` to dodge
resource conflict; lines 178–181) is real but the consequence is
hidden.

**Why it is problematic.**

1. **Hidden cost.** `EntityRef::get::<T>()` looks up components per
   call, vs the archetypal-storage fast path that `Query` uses.
   192 cells × O(n) players × per-cell iteration is the hot path —
   exactly the path spec §10 §3 marks as a future profiling target
   that should not be made harder to measure.
2. **No compile-time safety.** If `Ball`/`Position` are moved or
   renamed, this code fails at runtime, not compile time.
3. **Defeats the parallel scheduling assumption.** Bevy's scheduler
   can parallelise data-parallel system passes; opaque
   `world.iter_entities()` blocks any future re-architecture.

**Relevant Rust principle.** "Let the framework see your access
patterns." `Query` is the way Bevy recognises what you read and
write so it can schedule.

**Concrete improvement.** Reframe `pitch_control_system` to take
`Query<(Entity, &Player, &Position)>` directly via
`IntoSystem::system()` and `world.resource_mut::<PitchControlGrid>()`
for the write. Pass the grid into the schedule as a `ResMut` and let
the scheduler handle the conflict. If a future feature needs the
grid to be read by `player_decision_system` (it currently does), make
that system's read explicit (`Res<PitchControlGrid>`).

**Trade-offs.** This is a mechanical refactor with no behavioural
change. The biggest risk is correctly classifying the resource
conflict — if `pitch_control_system` needs `ResMut<PitchControlGrid>`
and `player_decision_system` reads `Res<PitchControlGrid>`, the
scheduler will serialise them in declaration order, which is already
the order they need (Perception before Decision).

---

### 2.6 Considered name lookup via string match in `compute_consideration_input`

**Location.** `crates/sim-ai-player/src/lib.rs:325–515` — a 190-line
`match name { "distance_to_target" => …, "distance_to_ball" => …, …,
_ => 0.5 }`.

**What it does.** Every player consideration is dispatched by name
string at every player evaluation. The brain templates in
`default_utility_brain` (sim-core ~970–1140) build considerations with
`String` names — so each evaluation does at least one `match`,
sometimes several.

**Why it is problematic.**

1. **Allocations on the decision path.** Each consideration's `name`
   field is a `String` — heap-allocated, cloned along with the rest
   of the brain on a `Vec`. Per-evaluation, `compute_consideration_input`
   receives a `&str` and matches — that part is zero-alloc. But
   because the brain definition uses `String`s, you can't put brains
   in `const` form or share them across players without copying.
2. **Silent fall-through.** Unknown names return `0.5`. If a brain
   has a typo (`"distance_to_gole"` instead of `"distance_to_goal"`),
   the consideration silently contributes `0.5`, the player behaves
   suboptimally, and there's no test that catches it. The spec even
   calls this out as the named-correctness concern.
3. **Not typed.** The set of valid consideration names is implicit
   in the dispatch table. Two enums or a trait would make this
   refactor-safe.

**Relevant Rust principle.** "Make illegal states unrepresentable"
+ the "parse, don't validate" idiom; also a missed opportunity for
the typestate pattern: an exhaustive `enum Consideration` is exactly
what the code is doing manually.

**Concrete improvement (small).** Move the consideration name to
`&'static str` (since all definitions are constants in
`default_utility_brain`). The match keeps its current shape but with
zero allocation.

**Concrete improvement (larger, better).** Replace the
`(name: String, weight: f32, curve: ResponseCurve)` triple with an
enum:

```rust
pub enum Consideration {
    DistanceToTarget { weight: f32, curve: ResponseCurve },
    DistanceToBall   { weight: f32, curve: ResponseCurve },
    Stamina          { weight: f32, curve: ResponseCurve },
    // …
}
```

The brain definition becomes a `const` or declarative macro.
`compute_consideration_input` becomes an exhaustive match
compile-checked, not at runtime. The "_ => 0.5" silent fall-through
disappears — adding a new consideration requires a new variant
and a new match arm, with the compiler catching misses.

**Trade-offs.** The enum refactor is the larger thing but pays off
for testing and constant-folding. The smaller `&'static str` change
is a 10-line diff and immediate. Either is a win.

---

### 2.7 The `HashMap<TeamId, f32>` per-tick in `perception_system` is rebuilt and discarded

**Location.** `crates/sim-ai-player/src/lib.rs:93–105`.

**What it does.** Allocates a new `std::collections::HashMap`, inserts
two entries (one per team), reads them, drops the map. Every tick.

**Why it is problematic.**

1. **Trivially replaceable.** Two mentalities per match. The map
   can be a `(Mentality, Mentality)` tuple or a `[Option<f32>; 2]`.
2. **HashMap iteration order is non-deterministic** — but here the
   only consumer is `mentality_by_team.get(&team_id)` so iteration
   order doesn't matter. Still, removing the randomness reduces the
   "what surfaces as nondeterminism under stress" surface.
3. **Triggers clippy nursery warnings** for `default_hash_type` and
   `disallowed_*` style.

**Relevant Rust principle.** "Don't allocate when you don't have
to" + "no unnecessary randomness in deterministic code".

**Concrete improvement.** Inline as:
`let mentality_of = |id: TeamId| match id.0 { 0 => home_m, 1 => away_m, _ => 0.0 };`
or just take a tiny `[f32; 2]` indexed by `team_id.0 as usize`.

**Trade-offs.** None.

---

### 2.8 `Ad-hoc println!` debug output scattered across game systems

**Location.** 29 `println!` calls in `crates/` (counted:
manager decisions, goal scoring, OOB restart, foul detection,
substitution logic, match duration enforcement, mentality shift,
formation change, faction listed below).

Specifically:

| File | Lines | Why it prints |
|---|---|---|
| `sim-ai-manager/src/lib.rs` | 31–41, 58, 85, 122–127 | Manager decision, formation, sub, mentality |
| `sim-rules/src/lib.rs` | 162–165, 173–177, 395–399, 635, 655, 658, 670–672 | Goal, offside, added time, half/full time, low player count |
| `sim-core/src/lib.rs` | 519, 531, 545 | `apply_command` substitution/tactic/log |

**Why it is problematic.**

1. **Fights the tracing plan.** Spec 002 is specifically about
   *replacing* ad hoc debug output with a proper trace schema. Every
   `println!` here is a refactor target the next time spec-002 lands.
2. **Cripples the determinism contract in subtle ways.** The spec
   says "deterministic run is reproducible iff two runs with same
   seed print the same final state hash" — but a `println!` race in a
   multi-threaded schedule could surface nondeterminism *outside* the
   hash, where the contract can't catch it. (No threading currently,
   but if Bevy's parallel scheduling ever falls in, this is a hidden
   trap.)
3. **No level / no channel.** No way to silence trivia without
   silencing everything.

**Concrete improvement.** Replace with:

- `tracing` events (used by the upcoming spec-002 work). Even at the
  warn level during the transition, that's an improvement.
- For gameplay-relevant events (goal, card, half/full time), push to
  an event channel or `events::EventWriter<MatchEvent>` — these
  become replayable match events that spec 003 ("replay UI") could
  consume.

**Trade-offs.** Behavioural change for stdout. If you want a softer
first step: route through `tracing::info!()` with a default level
of `info`, which is what the rest of the codebase should converge
toward anyway.

---

### 2.9 Match entity is located by linear scan

**Location.** `crates/sim-server/src/lib.rs:169–182`
(`get_match_entity` iterates `iter_entities()`, looking for
`Some(Match)`); reused in many places including `apply_command`, line
172 ff. `sim-core/src/lib.rs:153–157` does the same when looking up
the ball entity. `sim-replay/src/lib.rs:202` likewise for ball.

**What it does.** On every command application, every state query,
and every replay reconstruction, we walk the world to find the one
entity carrying `Match` or `Ball`.

**Why it is problematic.** Three independent places each implement
the same find-the-entity-by-component loop. Hot path is `apply_command`
during a live match — but the call frequency is low (manager events),
so performance isn't the concern. Maintainability is.

**Concrete improvement.** Either:

1. Take `Simulation::match_entity` and `Simulation::ball_entity` as
   the source of truth (already exposed today), and pass them in to
   `ServerSimulation` (currently it doesn't — it re-derives). The
   container already keeps them; only the consumer doesn't trust
   them. Or,
2. Promote `Match` and `Ball` to `Resource`s (singleton-resource
   pattern). This is the cleanest for one-per-match scenarios.

Both reduce the linear scan to a single `world.resource::<Match>()` or
`world.entity(match_entity).get::<Match>()` look-up.

**Trade-offs.** Either is non-breaking. The singleton-resource path
has implications for the spec's "every player is an entity" rule,
but `Match` is already singleton-data (one per match) and pushing
the same shape to `Resource` is a small change.

---

### 2.10 `lifecycle_system` is a 173-line function with eight `if let Some` chains

**Location.** `crates/sim-core/src/lib.rs:761–935`. The state machine
is one giant function. Clippy flags it (`too_many_lines`, 173/100).

**Why it is problematic.** Beyond the size warning, the function:
- has eight mutable borrows per iteration,
- keeps state across iterations (`HalfTimeEntryTick` resource),
- does ball/player sync every iteration,
- resets formations at the end.

This is one of the hardest functions in the codebase to follow — and
it's been touched in most phases. The shape of the state machine
itself (a `enum MatchState → enum MatchState` transition table with
side effects) is the natural rust-shape.

**Concrete improvement.** Replace the inner logic with a small
transition table:

```rust
fn next_state(m: &Match, clock: &MatchClock, sim_tick: u64) -> Transition

struct Transition {
    next: MatchState,
    new_clock: Option<MatchClock>,
    place_ball_center: bool,
    apply_kickoff_impulse: bool,
}
```

The post-mutation "apply" loop stays, but the "what should happen"
portion is a pure function over (state, clock, sim_tick). Plus, the
ball/player sync should be a separate system, not bundled into the
state machine — see §2.1.

**Trade-offs.** A modest refactor with a real readability payoff.
Each phase change (kickoff, halftime, full-time) becomes testable
in isolation.

---

### 2.11 `apply_command` is duplicated between `sim-server` and `sim-core`

**Location.** `crates/sim-server/src/lib.rs:45–116` and
`crates/sim-core/src/lib.rs:452–551`. Same validation rules written
twice, slight format differences.

**Why it is problematic.** Validation logic drift. Substitutions
are silent in both ("`println!("Substitution: …")` — log and move
on"). When the validation rules grow (per the spec clarifications
on "validate against both match state AND available resources"),
having two copies to update is a maintenance hazard.

**Concrete improvement.** Move `apply_command` to `sim-core` as the
single authority. `ServerSimulation::apply_command` becomes a thin
wrapper that defers to it after queueing semantics.

**Trade-offs.** None. A pure consolidation.

---

### 2.12 `sim-replay::MatchSnapshot` collides with `sim-core::MatchSnapshot`

**Location.** `crates/sim-replay/src/lib.rs:152–159` (`MatchSnapshot`
struct in sim-replay) and `crates/sim-core/src/lib.rs:629–637`
(another `MatchSnapshot`, different shape). Both name the same thing.

**Why it is problematic.** Confusing re-export semantics for any
caller that does `use sim_replay::MatchSnapshot;` expecting what
`sim_core` returns. The `Trace File Contract` in spec 002 notes
*"…deliberately independent of the other three… snapshots"*, but this
pre-existing collision was not addressed when spec-002 added a fourth.

**Concrete improvement.** Rename one. Suggestion:
- Keep `sim_core::MatchSnapshot` (the source of authority).
- Rename `sim_replay::MatchSnapshot` → `sim_replay::RecordedSnapshot`,
  or move it to a typed record format that includes the seed/commands
  (the thing you'd actually persist to disk).

**Trade-offs.** Low-risk rename within `sim-replay`'s public surface.

---

## 3. Rust Idiomaticity Review

### 3.1 Ownership & borrowing

Mostly good. The codebase uses `ParamSet` where multiple query
disjointness matters and shows the discipline to buffer snapshots into
`Vec<…>` to release borrows (`perception_system`, line 117).

Two small concerns:

- `let Some(perception) = player.perception.clone() else { … }` in
  `player_decision_system` clones a heap-allocated
  `PerceptionSnapshot` per player per tick. With `PerceptionSnapshot`
  promoted to a `Component`, the clone becomes an
  `EntityRef::get::<PerceptionSnapshot>()` read, eliminating the clone
  (see §2.2).
- `match_query.get_single()` is used in places where the system
  genuinely expects exactly one match — good idiom; but it's
  `unwrap()`-style ("or fail fast") in simulation step functions and
  silently returns `false`/`()` in some systems. Consistency would
  help: a custom error type that converts to a no-op via
  `IntoResult<(), _>` makes the contract visible at the call sites.

### 3.2 Error handling

The codebase has minimal error types today and uses `Result<String, _>`
plus `Box<dyn Error>` at the CLI boundary. This is consistent with
"no production has meaningful errors yet", but:

- `sim_server::CommandError` is the one real custom error enum in the
  codebase (good — could be `thiserror`-derived). It's not exported
  as such from `lib.rs` (it's `pub` but no impl). Adding
  `#[derive(Debug, Clone)]` and a clear conversion to `String` for
  the CLI hand-off is a small but standard piece of work.
- `apply_command` in `sim-core` returns `Result<(), String>` rather
  than a structured error type. Same observation as above.
- Bevy systems that *could* fail (player not in team, possession
  lookup miss) silently `continue;` instead of logging or returning
  an issue. For phase-by-phase work this is fine; for a final system,
  an event channel or `tracing::warn!` would catch bugs earlier.

### 3.3 Trait and generic design

- `Consideration.name` as `String` and dispatch by string is the
  weakest part of the type story; everything else is
  well-encapsulated. See §2.6 for the enum refactor.
- `ResponseCurve::evaluate(&self, normalized_input: f32) -> f32`
  (plus the spec's extension to `ConsiderationScore`) — this could be
  `fn evaluate(&self, x: f32) -> Score` with a `Score` type. Today
  it's a raw f32 returned + clamp at the call site. A newtype around
  `f32` would let the compiler enforce the bounded range and prevent
  the silent "score out of [0,1] then clamp" pattern from showing up
  inconsistently. **Minor.**

### 3.4 Enums and state modelling

- `MatchState` is small and exhaustive — good.
- `BallState` includes `OutOfPlay` and `Dead` but `OutOfPlay` is never
  actually used (no system writes it). The variant is reserved for
  future use but not actively modelled. Either remove it or alias it
  to `Dead` (they serve the same purpose in this codebase).
- `Intent` is a single enum with 10 variants covering very different
  actions (some take a target entity, some take a Vec2, some are
  unit). `player_action_execution_system::match` on `&intent`
  (lines 547–594) has 47 lines of stochastic modifiers by action
  type. Refactoring `Intent` into a small hierarchy (Action with
  optional Target / Point) would clarify which actions care about what.

### 3.5 Lifetimes

- `Vec2` is `Copy` (good). `Ball`, `Player`, `Match`, etc. are
  all `Clone` so most code reads them by value via `clone()` or
  `&entity.get::<…>()`. Lifetime annotations are conspicuously rare —
  a sign the API surface is well-shaped.
- One subtle leak: `Player.perception` is `Clone`-shaped, which
  motivates the `Option<…>` field. As a Component, it would just be
  `&PerceptionSnapshot` from a query — lifetimes disappear.

### 3.6 Concurrency and async

- None currently. Bevy's parallel scheduling is *available* (work
  stealing) but the codebase lives in single-threaded land. The
  spec §3 ordering discipline is appropriate.
- If Phase 5 or beyond ever needs async (network I/O), the natural
  place is the `sim-server` binary. The simulation itself should
  stay synchronous and deterministic. This is a guard rail worth
  documenting in an ADR.

### 3.7 API design

- Public crates expose functions, not types: `create_match`,
  `apply_command`, `get_state` are top-level `pub fn`s on the
  `Simulation` struct. Fine.
- `sim_server::ServerSimulation` is a thin wrapper; its
  `command_queue` is a `pub` field rather than a private field
  with methods. **Minor.** Make the field `private` and expose
  `pub fn enqueue_command`, etc.

### 3.8 Allocation and cloning

The biggest avoidable sources of allocation/cloning:
- `Player.perception` clones per decision cycle (§2.2).
- `Consideration.name` `String` s (§2.6).
- The `pitches::pitch_control_system` `world.iter_entities()`
  approach is slower than `Query`-based (§2.5).

### 3.9 Unsafe code

- Workspace lints set `unsafe_code = "forbid"`. No `unsafe` blocks
  in the codebase. **Excellent.**

### 3.10 Module and dependency boundaries

- Generally clean per spec §11.
- `sim-ai-manager` still depends on `sim-ai-core`
  (`sim-ai-manager/Cargo.toml`). The spec §13 decision #13 locked
  that manager AI should NOT depend on `sim-ai-core`. Confirmed
  not-yet-implemented. The current `manager_decision_system`
  does use the manager's own `decision_table.factors` and is small,
  but the spec's intent is to keep the seam absent from the binary
  crate. Worth fixing in a follow-up so the architecture matches the
  documented design.
- `sim-server` depends on `sim-rules` and `sim-physics` in
  addition to `sim-core`; spec §11 says it should depend on
  `sim-core` only. Same observation.

### 3.11 Testing and observability

- Good coverage in `sim-components` (`time.rs` has 13 dedicated
  tests for clock semantics), `sim-rules` (10+ tests across OOB,
  goals, fouls, possession), and `sim-core` (per-tick
  determinism, full match lifecycle). Heartening for a Phase 2
  codebase.
- `sim-server/Cargo.toml` declares an integration test
  `tests/determinism_test.rs` — fine, but it's actually a
  re-implementation of the `sim-core` determinism tests. Could
  be a `#[path = "..."]` reference instead of duplication. **Very
  minor.**
- What I'd add: a serialisation round-trip test for each public
  snapshot type. Today `MatchSnapshot` (sim-core) is serialised via
  `serde_json` in main.rs but no test exercises that round-trip.
  Cheap insurance against silent breakage when the snapshot shape
  changes (which it does each phase).

### 3.12 Appropriate use of Rust-specific design patterns

- `Newtype` for `TeamId(pub u8)`, `BallState`, etc. — good.
- No use of `typestate` where it would help. A candidate:
  `Phase0Substrate` / `Phase1Players` markers on `Simulation`.
  Today phase state is implicit in which systems are registered.
- `SmallVec` use in `PerceptionSnapshot` — exactly the right
  pattern (small typical, occasional large).
- Builder pattern: nowhere. The codebase doesn't need one (no
  large `init` calls).
- RAII guards: `Local<u32>` in `offside_detection_system` is a
  per-system-instance counter — good idiom, but tracked manually
  rather than via a guard. Fine.
- Type-driven state machines: the `MatchState` enum *could*
  be a proper state machine (e.g. via `state` / `statig`).
  Today it is a flat enum with hand-rolled transitions
  (`lifecycle_system`, see §2.10).

---

## 4. Architecture Improvement Opportunities

These are bigger-than-a-bug changes that would meaningfully change
the shape of the codebase for the better. Each is presented with
its trade-off so you can choose.

### 4.1 Eliminate the legacy struct-field mirrors (the Sync problem)

Combine §2.1, §2.2, §2.3.

**Move:**
- `Ball.position`, `Ball.velocity` → deleted; read from `Position`/
  `Velocity` components.
- `Player.position`, `Player.velocity`, `Player.stamina`, `Player.role`,
  `Player.skill` → deleted; read from `Position`/`Velocity`/`Stamina`/
  `RoleComponent`/`Skill` components. (Spec §9 already has them in
  component form.)
- `Player.perception` → deleted; promote `PerceptionSnapshot` to a
  `Component`.
- `Match.clock` → deleted; the `MatchClock` Component is the single source.

**Net effect.** One canonical writer per datum, no `lifecycle_system`
sync, two of the three "two writers" classes of bug eradicated. No
observable behaviour change.

**Cost.** A single PR-sized refactor across the four "data shape"
crates plus the snapshot/view consumers.

**Incremental vs. architectural.** Incremental — every individual
delete + replacement keeps the code compiling.

### 4.2 A proper `Module::register_systems()` pattern over loose `add_systems`

Today, `Simulation::new` in `sim-core/src/lib.rs:91–148` registers
*every* system directly. As systems grow, this becomes a hundred-line
registration list with ordering constraints hard to reason about.

The typical Bevy shape is per-feature `Module`s with their own
`add_systems(Schedule)` method. Each module file becomes
self-contained; ordering constraints local to its concern.

**Cost.** A couple of hours of glue. Pure organisation.

### 4.3 `Match` and `Ball` as `Resource`s, not components

The match entity is unique per simulation. Promoting
`Match` → `Resource<Match>` and `Ball` → `Resource<Ball>` removes the
linear-scan lookups in §2.9, simplifies `Simulation::tick()`,
and matches the singleton-resource pattern Bevy is well set up
for (e.g. `Commands`/`Res<Time>`).

**Trade-off.** Sacrifices the spec's "every entity lives on the ECS"
idiom, but the lookup performance and API clarity wins are real.
Players would still be entities (22 of them, no singleton data).

**Cost.** A handful of `Query`/`Res` swaps.

### 4.4 Move `apply_command`/`get_state`/`tick` to a single `sim-core::Simulation` API and thin `sim-server` to a network adapter

Currently `ServerSimulation` re-implements command validation and
state snapshotting (see §2.11). The clean split is:

- `sim_core` owns the *truth* — `Simulation::tick`,
  `Simulation::apply_command`, `Simulation::get_state`.
- `sim_server` owns the *I/O* — command queue, command validation
  *types* (e.g. "command rejected because substitutes left"),
  future wire protocol.

This is a small movement that makes the binary-crate layer
actually thin, which the spec §11 explicitly envisions.

### 4.5 `ResponseCurve::evaluate` taking a `Score` newtype

Prevents the "evaluate returns 0..1 but no type enforces it" pattern.
Cheap; pairs well with §3.3.

### 4.6 Log over println

§2.8. Replace `println!` with `tracing::info!`/`warn!`. The
spec-002 work will need this anyway.

### 4.7 A `sim-ai-manager` that *really* doesn't depend on `sim-ai-core`

Spec §13 #13 locked this; current state of
`sim-ai-manager/Cargo.toml` contradicts. Delete the line, and no
production code breaks (the manager system uses
`WeightedDecisionTable`, not `ResponseCurve`/`geometric_mean`).

### 4.8 A `sim-server` that depends on `sim-core` only

Spec §11 locked this; current state of
`sim-server/Cargo.toml` includes `sim-rules` and `sim-physics`.
What this means in practice: the binary crate should compose `Simulation`
and any I/O glue, not poke at physics or rules internals. Easy
re-arrangement of pub-export surface.

### 4.9 (Optional) `offside_detection_system` losing the print counter

Currently uses a `Local<u32>` to throttle the `println!`. Replace
with `warn!` once §4.6 lands, or move the throttle into a "diagnostic
toggle" on `OffsideDetectionCfg`. Today this is awkward.

---

## 5. Prioritised Recommendations

Reasoning at the head of each group, then the items.

### 5.1 High impact / low effort

These are mechanical refactors with no behaviour change and a
disproportionate payoff. The reasoning behind lumping them together:
each one takes hours, removes one whole *class* of bug, and is
small enough to ship without review debt.

1. **Remove dead `Simulation.rng`** (§2.4). Pure field delete;
   comment-only fix; no risk.
2. **Replace `HashMap<TeamId, f32>` with a tuple/indexed array**
   (§2.7). Allocator/clippy wins; ~10 lines.
3. **Drop the `clock` field from `Match`** (§2.3). Single source of
   truth; deletes the `tick_clock` clone-and-sync dance.
4. **Move `apply_command` into one location** (§2.11). Fix the
   duplication.
5. **`sim-ai-manager` and `sim-server` dependency alignment** with
   spec §11 (§4.7, §4.8). Two `Cargo.toml` edits.
6. **Replace `println!` with `tracing`** (§2.8, §4.6). Do it once,
   all over the codebase.
7. **Rename `sim_replay::MatchSnapshot`** to `RecordedSnapshot`
   (§2.12).

### 5.2 High impact / high effort

The reasoning: each of these touches enough code that they're better
done as a multi-PR sequence, not as a one-shot. The compounds are
what makes them high impact.

1. **Sync / snapshot cleanup** (§4.1). The largest pay-off in the
   codebase: deletes `Ball.{position, velocity}`, `Player.{position,
   velocity, stamina, role, skill, perception}` mirror fields, plus
   the sync logic, plus a chunk of the lifecycle function. The
   payoff is correctness: two-synced-state classes of bug are
   eliminated.
2. **`Module`/`add_systems` pattern** (§4.2). Pairs with #1 above
   for a tidy state-machine refactor.
3. **`lifecycle_system` decomposition** (§2.10). Material
   readability improvement. Pairs with #1.
4. **Reframe `pitch_control_system` to use `Query`** (§2.5). Pairs
   with §4.2; enables future profiling work the spec §10 §3 calls
   for.
5. **Promote `Match` and `Ball` to `Resource`** (§4.3). Pairs with
   #1; simplifies `sim-server`'s command/state plumbing.

### 5.3 Lower-priority improvements

The reasoning: nice-to-haves that don't move correctness, but tidy
up the API surface for future work.

1. **Consideration enum / dispatch-table refactor** (§2.6).
2. **`ResponseCurve::evaluate` `Score` newtype** (§4.5).
3. **Split `Intent` into a small hierarchy** (§3.4).
4. **`sim_server::ServerSimulation` field privacy** (§3.7).
5. **`BallState::OutOfPlay` reserved-variant cleanup** (§3.4).
6. **Snapshot JSON round-trip tests** (§3.11).

---

## 6. Design Brainstorm (input for next conversation)

The questions below are the seams I think are worth designing
*together* rather than me proposing a single best shape. Each
section presents the trade-off space, with my recommendation called
out. We can iterate from here.

### 6.1 Single source of truth for live state

Three places currently hold the same "live" data:

- The Bevy `Component` (authoritative).
- The `Ball`/`Player` struct field (snapshot mirror).
- The `Match`/`MatchClock` double (struct + component).

Two viable directions:

**(a) Components-only.** Delete the mirrors. Snapshot readers pull
from components. Snapshot writers (the snapshot file format) become
serialisers of the component state. **Pros:** one canonical writer
per datum, no sync. **Cons:** snapshot format is determined by the
ECS shape (serialise-everything OR serialise-only-selected), which may
be brittle to ECS refactors.

**(b) Mirror-via-resource.** Keep a `Resource` that mirrors the
relevant components, written by a single sync system. Readers consult
the resource. **Pros:** stable snapshot shape, ECS can change
internally. **Cons:** the very class of bug we want to remove.

**My recommendation is (a).** The codebase is small; the snapshot
shape is dictated by the ECS anyway, and serialisation round-trip
tests (§5.3 #6) catch any change.

### 6.2 Where does `PerceptionSnapshot` live?

Three placements:

**(a) Component on the player entity.** Inserted every tick.
Read by decision system via `&PerceptionSnapshot`.

**(b) Component on its own entity.** A "perception volume" per
player; downstream systems `Query<&PerceptionSnapshot>` don't care
which player it belongs to (they walk both queries).

**(c) Resource indexed by player entity.** Single resource holding
`HashMap<Entity, PerceptionSnapshot>`.

**My recommendation is (a).** It's the natural Bevy shape: the
perception system writes one component per player, the decision
system reads it. Big-O is identical for 22 players to any indexed
approach, and there's no separate lifecycle to maintain.

### 6.3 `Intent` shape

Ten variants today; some take a target entity, some a point, some
neither. Two directions:

**(a) Flatten or merge.** Group into `Movement(Option<TargetPoint>,
Option<Entity>)` + `Action(ActionKind, Option<TargetEntity>)`.

**(b) Validate at the decision-system boundary.** A helper builds
the intent from a `PlayerConsideration`-shaped struct; downstream
arms use `match` exhaustively, but the helper enforces what's
legal.

**My recommendation: do (b) first, defer (a).** Intent is a public
type used in many places (snapshot serialisation, replays).
Refactoring it touches too many call sites for incremental value.

### 6.4 `Match`/`Ball` as entity vs resource

Two directions:

**(a) Components on a singleton entity.** Matches spec §9 today,
costs the linear-scan lookups.

**(b) `Resource<Match>`, `Resource<Ball>`.** Survives the snapshot
serialisation, lowers friction for `sim-server`. Players stay as
entities (22 unique).

**My recommendation is (b)** unless we anticipate multi-match
support. For now, exactly one match per `Simulation`, so a Resource
fits.

### 6.5 Decision-system pacing vs scheduling

Spec §13 #15 locked the decision cadence at "per-player roster-wide
stagger, default N=6 (~10 Hz)". The codebase's
`player_decision_system` does *not* implement per-player time-slicing
(it runs every tick for every player). Confirmed in code.

Three options:

**(a) Implement the time-slicing as an early-out.** Read the
`MatchClock.elapsed_ticks` and `entity.to_bits()` in the system;
`continue;` if not this player's tick. Cheap if-match. Echoes spec
§13.

**(b) Split into per-player decision cadence at the schedule level.**
Use Bevy's `tick` mechanism on the schedule? Oracles: complex.

**(c) Leave it for the profiling pass.** Phase 5 hasn't run yet.

**My recommendation: (a)**. The hatchet is small, the spec already
commits to it, and the test in
`sim-ai-player/src/lib.rs:1070–1165`
(`test_decision_cadence_does_not_re_evaluate_every_tick`) is named
for it but doesn't actually test the cadence.

### 6.6 Offside detection: stub vs plan

Today `offside_detection_system` only prints, with a tick counter
to throttle. Spec §12 Phase 5 promises the real implementation
(penalisation, restart placement). Two questions to settle:

**(a) Does offside need a penalty system, or is "warning for
developer observation" the right MVP?**

**(b) What shape does the restart placeholder take?** A
`PendingRestart`-style resource, or attach to the ball entity as a
`RuleEvent::Offside`?

Either is fine; what matters is that the answer is consistent with
how other rule events (goal, OOB) already work, which is
`RuleEvent` *and* `PendingRestart`.

---

## 7. Suggested Evolution Path

A practical order to apply the changes without breaking current
behaviour at any step. Each phase is independently shippable.

### Phase A: Mechanical cleanups (small PRs)

**Goal:** remove dead state, eliminate duplication, align dependencies
with spec.

1. Remove unused `Simulation.rng` field (§2.4).
2. Drop the HashMap for mentality dispatch (§2.7).
3. Drop `clock` field from `Match`; access via `MatchClock` component
   (§2.3).
4. Move `apply_command` into `sim-core` only; `sim-server` delegates
   (§2.11).
5. Rename `sim_replay::MatchSnapshot` to `RecordedSnapshot` (§2.12).
6. Fix `sim-ai-manager` and `sim-server` `Cargo.toml` to match spec
   §11 (§4.7, §4.8).
7. Replace `println!` with `tracing::*` (§2.8).

### Phase B: Sync snapshot cleanup (one PR; review-intensive)

**Goal:** delete the struct-field mirrors; promote `PerceptionSnapshot`
to a Component.

1. Promote `PerceptionSnapshot` to a Component (§2.2).
2. Move snapshot writers and readers (sim-core's
   `get_state`/`get_state_hash` and `sim-replay::create_snapshot_from_simulation`)
   to query the `Position`/`Velocity`/`Stamina`/`Skill`/`RoleComponent`
   components directly.
3. Delete `Ball.position`, `Ball.velocity`, `Player.{position, velocity,
   stamina, role, skill, perception, score_differential,
   time_remaining_secs, team_possession, mentality_modifier}`
   fields. **Note**: `score_differential`, `time_remaining_secs`, etc.
   are derived data — promote them to `PerceptionSnapshot` only or
   compute on demand.
4. Delete the per-tick sync in `lifecycle_system`.
5. Decompose `lifecycle_system` into "transition decision" + "apply"
   (§2.10).

### Phase C: ECS-shape polish (small to medium PRs)

**Goal:** tighten the schedule, enable future profiling.

1. Refactor `pitch_control_system` to use `Query` (§2.5).
2. Introduce per-feature `Module::register_systems` (§4.2).
3. Singleton-resource `Match`/`Ball` (§4.3).

### Phase D: Decision-system pacing (small PR)

**Goal:** implement the spec-locked per-player time-slicing.

1. Add the per-player cadence guard to `player_decision_system` (§6.5).
2. Extend the existing test
   `test_decision_cadence_does_not_re_evaluate_every_tick` to actually
   assert cadence (currently it asserts bounded *cost*, which is
   weaker).

### Phase E: API polish (multiple small PRs)

1. Consideration enum or `&'static str` (§2.6).
2. `ResponseCurve::evaluate` `Score` newtype (§4.5).
3. `Intent` split into movement/action hierarchies (§3.4).
4. Snapshot round-trip tests (§5.3 #6).
5. Reserved-variant cleanup.

### Phase F (post-Phase 5): profiling-driven tweaks

The spec §10 §3 numbers are projections only. After Phase 5
delivers a working referee + foul system, run actual benchmarks
against §10's targets. Don't refactor speculatively before then.

---

## Appendix A: Evidence index

Where each major evidence item lives, for easy review:

- **Sync / double-clock**: `sim-core/src/lib.rs:192–206`
  (`tick_clock`), `sim-core/src/lib.rs:761–935` (`lifecycle_system`),
  `sim-core/src/lib.rs:447` (double insertion).
- **Perception as field**:
  `sim-components/src/lib.rs:202–220`; `sim-ai-player/src/lib.rs:228`
  (clone); `sim-ai-player/src/lib.rs:31–201` (perception system).
- **Dead `Simulation.rng`**:
  `sim-core/src/lib.rs:30, 52`. No read sites in any crate.
- **String-dispatch considerations**:
  `sim-ai-player/src/lib.rs:325–515`; brain templates at
  `sim-core/src/lib.rs:975–1142`.
- **`world.iter_entities()` for hot path**:
  `sim-physics/src/lib.rs:127–194` (`pitch_control_system`).
- **`HashMap<TeamId, _>` per tick**:
  `sim-ai-player/src/lib.rs:93–105`.
- **`println!` listing**: 29 occurrences. See §2.8 for the table.
- **Doubled `MatchSnapshot` types**:
  `sim-core/src/lib.rs:629` and `sim-replay/src/lib.rs:152`.
- **Lifecycle system size**: `sim-core/src/lib.rs:761`–`935`, 175
  lines.
- **`apply_command` duplication**:
  `sim-server/src/lib.rs:45–116` and
  `sim-core/src/lib.rs:452–551`.
- **Decision cadence claim vs reality**:
  spec §13 #15 vs `sim-ai-player/src/lib.rs:222–298`
  (player_decision_system has no per-player cadence guard).

## Appendix B: Out of scope for this review

- The wire protocol (spec §13 #20) — deferred.
- Phase 5+ unimplemented rules (advantage, cards, real offside penalty).
- Networking / persistence layer.
- Manager AI's deeper behaviour (current implementation matches the
  weighted-table spec but lacks momentum factor, §5.1).
- The bincode snapshot format in `sim-replay` — works; no critique.

