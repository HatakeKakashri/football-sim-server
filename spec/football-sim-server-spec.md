# Football Match Simulation Engine — Server-Side Technical Specification

*Rust · ECS · Utility AI · Server-Authoritative*

This document consolidates two independent research reports into a single implementable
architecture. It is not a summary of either report — every section below states what was
decided, why, and which report(s) support it. Where the reports disagreed, the
disagreement is named explicitly and resolved against the project's actual requirements;
where evidence was insufficient, the item is marked **Open** rather than guessed at.

## Source material, briefly

- **Report A** — *"Utility AI for a Server-Authoritative Football Simulation in Rust"*
  (`football-rust-utility-ai-architecture.md`). A single-pass, source-cited research
  document (🔎 finding / 🧭 recommendation / ⚠️ assumption tagging throughout). It is
  explicitly grounded in this project's own prior work — it reads the [[ninety-minutes]]
  TypeScript ECS engine's standing invariants (Mulberry32 PRNG, no transcendental math in
  hot paths, same-tick read/write separation) as established inputs to carry forward, not
  as things to re-derive from scratch.
- **Report B** — an iterative design transcript (`football-sim-memory.md`). It opens with
  the same brief, produces an initial architecture, then runs three rounds of internal
  self-critique that surface and (mostly) fix concrete bugs in its own "locked"
  specifications. It does not reference this project's prior TypeScript work at all — its
  reasoning starts from a blank slate.

These two profiles matter for how much weight each report's claims carry below: Report A
is more disciplined about labeling what's sourced versus judged, and more often checks its
recommendations against this project's actual history. Report B is weaker on that
discipline (it mislabels unmeasured performance projections as "Measured Cost," and locks
a fixed-point determinism decision without ever answering the question its own critique
raises about it) but its self-critique rounds catch real, specific implementation bugs
that Report A's higher-altitude treatment never gets close enough to the metal to find.
Both get used; neither is treated as authoritative by default.

---

## 1. Specification Overview

**Purpose.** Define one coherent server-side architecture for the match simulation engine
that both reports were asked to design, resolving their disagreements with reasons on the
record rather than picking a side arbitrarily.

**Scope.** The server-side simulation core: ECS world, Utility AI decision layer, rules
engine, determinism/testing strategy, data model, crate structure, and roadmap. Client
rendering and the wire protocol are explicitly **out of scope** (both reports agree this is
premature — see §8, §13). Numeric curve constants (distances, thresholds, weights)
throughout are illustrative placeholders, not tuned values — both reports invent them
without sourcing, and neither should be read as settled game balance.

**Authoritative design principles** (convergent across both reports, and consistent with
this project's existing ways-of-working):

1. The server is the single source of truth. Clients never execute match logic; they
   render interpolated snapshots.
2. Fixed timestep, decoupled from any render or poll rate.
3. Determinism target is **same-build, same-machine reproducibility**, not cross-platform
   bit-exactness — see §7 for why this is a real disagreement between the two reports and
   how it's resolved.
4. Strict pipeline separation: perception → decision → intent → execution/physics, with
   same-tick read/write separation, so no agent's decision this tick can observe another
   agent's not-yet-applied decision from the same tick.
5. Utility AI is used only where a genuine multi-factor, continuous trade-off exists.
   Everything else — ball motion, offside, out-of-bounds, goals — is physics or a rule
   engine. Both reports agree on this as a first principle; they disagree on where exactly
   the line falls (§4, §6).
6. Ship the smallest deterministic slice first. AI is added only after the substrate is
   proven reproducible (§12).

---

## 2. Server Simulation Architecture

**Simulation ownership.** One authoritative process per live match owns all match state.
No client-side prediction of match outcomes; clients may interpolate/extrapolate purely
for rendering smoothness between snapshots, never feed predictions back into gameplay.

**Simulation loop.** A fixed-step accumulator loop, decoupled from wall-clock render rate
(both reports agree; sourced to the "Fix Your Timestep!" pattern in Report A). Tick rate
60 Hz (`dt = 1/60s`) is adopted from Report B as the concrete default — Report A doesn't
commit to a number, and 60 Hz is a reasonable, easily-revised starting point, not a
measured requirement.

**Tick pipeline**, per tick:

```
1. Sensing      — spatial hash + perception snapshot build (every tick)
2. Decision     — Utility AI evaluation (decoupled cadence — see §5)
3. Execution    — Intent -> steering forces / action effects (every tick)
4. Physics      — integration, collision resolution (every tick)
5. Rules        — referee/rule checks against the just-resolved state (every tick)
```

Both reports converge on this five-stage shape (Report A §3 & §8; Report B §3 & §8); the
consolidated pipeline below is Report A's four-stage decision split merged with Report B's
explicit rules stage, which Report A folds into "execution" less visibly.

**Inputs.** Manager commands (formation, substitution, mentality, tactical instruction)
arrive as server-validated events, applied at tick boundaries — never mid-tick. Nothing
else is client-supplied; players and the ball are entirely server-simulated.

**Outputs.** Deterministic state snapshots/event streams, at a rate independent of the
60 Hz sim tick (delta-compression and interest management are **Open** — see §13).

**Match lifecycle.** Kickoff → in-play → stoppage (foul/out-of-bounds/goal) → restart →
… → half-time → second half → full-time. Modeled as an explicit `MatchPhase` state
machine (Report A §4.3), not as anything scored.

---

## 3. ECS Architecture

**Framework.** `bevy_ecs`, used **as a library only** — `World`/`Query`/`Component`
directly, with default (rendering) features off. Both reports independently arrive at
this same recommendation (Report A §5.1 evaluates it against `hecs`, `specs`, `legion`,
`shipyard`, and `flecs`-bindings with sourced status notes; Report B's comparison table is
thinner but lands in the same place). Independent convergence from two differently-sourced
reports is good evidence this is the right call.

**Scheduling — the one real disagreement, resolved.** Report A says: don't use `Schedule`,
`App`, or `Plugin` at all — hand-write the fixed-step loop and call system functions in an
order you specify explicitly, because Bevy's automatic parallel dependency inference is
built to maximize throughput, not to guarantee the exact ordering a determinism model
depends on. Report B's final "locked" implementation does the opposite: it builds a
`Schedule` with `SystemSet`s and `.configure_sets(...).chain()`.

**Resolution (Project Decision):** use `Schedule`, but never rely on Bevy's default
work-stealing parallelism to produce an order. Concretely:
- Every `SystemSet` is chained (`.chain()`), and every system that touches an
  order-sensitive resource — the shared `DeterministicRng`, or any accumulation whose
  result depends on summation order — must be explicitly ordered relative to every other
  such system (`.before()`/`.after()`/`.chain()`), not just left to run inside a set with
  siblings.
- Two systems with *no* declared data conflict are still not safe to leave unordered if
  their side effects are order-sensitive in a way Bevy's borrow-based conflict detection
  can't see (the clearest case: two systems both drawing from the same RNG resource with
  no overlapping `Query` access — Bevy has no reason to serialize them, but their draw
  order still needs to be pinned). This is a real gap in Report B's design that neither
  report states explicitly; it's called out here as a hard implementation rule, not
  optional guidance.
- This gets Bevy's typed system-parameter ergonomics (Report B's actual win) without
  inheriting Report A's blanket "don't trust the scheduler" position, by simply not
  giving the scheduler anything to guess about.

**Representative components/resources** (merged from both reports' data models — full
listing in §9):

| Kind | Examples |
|---|---|
| Resources | `MatchClock`, `Score`, `DeterministicRng`, `PitchDimensions`, `PitchControlGrid`, `MatchPhase`, `TacticalSettingsPerTeam` |
| Player components | `Position`, `Velocity`, `TeamId`, `Role`, `Stamina`, `PerceptionSnapshot`, `Intent`, `UtilityBrain`, `ActiveAction` |
| Ball components | `Position`, `Velocity`, `Spin`, `BallState` |
| Referee | `MatchPhase`, `PendingRestart`, `AdvantagePlayed` (resource or singleton entity) |

**Data flow.** Perception and Decision systems **only ever read** the previous tick's
fully-resolved state and **only write** to intent-shaped scratch components (`Intent`,
`ActiveAction`); a single deterministic execution pass then applies all intents in a
stable order. This eliminates order-of-iteration bugs as a nondeterminism source,
independent of the float-vs-fixed-point question in §7. Both reports state this principle
(Report A §3 calls it the direct continuation of the ninety-minutes "same-tick read/write
separation" invariant); it is **Locked**.

---

## 4. Utility AI Architecture

**Model.** Infinite Axis Utility System (IAUS) vocabulary — considerations, response
curves, aggregation, action selection — implemented as **plain data + pure functions over
ECS components**, not as an entity-per-consideration graph (both reports reject
`big-brain`'s Bevy-idiomatic-but-heavyweight per-Scorer-entity model for the same reason:
22 players × ~10 actions × ~4 considerations would mean hundreds of extra entities and
queries scheduled through machinery whose exact ordering you'd then have to fight to pin
down — Report A §5.1).

**Contexts.** A `PerceptionSnapshot` per agent, built once per decision cycle from
resolved world state: self state, ball state, a bounded set of nearby teammates/opponents,
and match context (score differential, time remaining, mentality). Decision systems read
*only* this snapshot, never live world state — this is what makes the scoring function a
pure, unit-testable `PerceptionSnapshot -> Intent` transformation (Report A §3).

**Considerations.** One axis of judgment per action; each reads one input, normalizes to
`[0,1]`, and maps it through a response curve. Curve types: linear, polynomial/power,
logistic/sigmoid, threshold/step (both reports use this same small set; adopted as-is).

**Utility aggregation — resolved disagreement.** Report A recommends the **geometric
mean** of clamped consideration scores; Report B recommends the classic Dave-Mark
**compensation factor** (product × a correction term derived from the arithmetic mean).
Both are real, sourced IAUS techniques for the same problem (raw multiplication decaying
toward zero as more considerations are added — `0.8³ ≈ 0.51`).

**Decision (Locked): geometric mean.** It solves the same N-decay problem the
compensation factor does, is monotonic and simpler to reason about, and — unlike the
compensation factor — has no extra tunable constant to mistune. Report A's argument for
this is the more disciplined one (explicitly framed as a documented alternative with a
cited source, not just a preference), and there's no evidence in either report that the
compensation factor's extra flexibility is actually needed here. A consideration scoring
`0` still short-circuits the product to `0` (a hard veto/gate), which is directly
continuous with this project's existing "gate-then-score" pattern from the TypeScript
Utility AI v1 design for the outfield player agent — this Rust engine's aggregation
function is a generalization of that pattern, not a departure from it.

```rust
pub fn aggregate_geometric_mean(scores: &[f32]) -> f32 {
    let product: f32 = scores.iter().map(|s| s.max(1e-4)).product();
    product.powf(1.0 / scores.len() as f32)
}
```

**Action selection.** Highest-aggregate-score wins, with a hysteresis bonus added to the
currently-active action's score to prevent tick-to-tick flicker between near-equal
options — both reports agree (standard IAUS practice; Mark/Dill explicitly recommend
hysteresis over pure argmax for exactly this reason).

**Response-curve evaluation & determinism.** This is where §7's determinism decision
actually bites: curves are implemented as plain `f32 -> f32` functions, computed directly
(no lookup table) for polynomial/linear/threshold shapes. `sqrt`/`sin`/`cos`/`exp` calls
inside a curve are the one thing to avoid in the default (non-fixed-point) model — see §7
for the "no transcendentals in the hot path" rule and how it interacts with the logistic
curve specifically.

**Stochastic action selection (optional, off by default).** Report B proposes an optional
Boltzmann/softmax sampling mode, seeded from the deterministic RNG, for behavioral
variability outside competitive play. This is adopted as an **optional, non-default**
feature — Report A never discusses it, so this isn't a disagreement to resolve, just an
addition worth keeping, with one correction: Report B's own third self-critique round
found that its "locked" implementation is **actually broken** — the exponential lookup
table it scores candidates against is built for `x ∈ [-5, 0]`, but the code feeds it
`score / temperature`, which is always ≥ 0, so every candidate clamps to the same table
entry and the sampler degenerates to a uniform distribution regardless of utility scores
(Report B's own shipped unit test, which asserts an 80/20-ish split, would fail against
its own shipped code). The fix — standard, numerically-stable softmax — is to subtract the
max utility before dividing by temperature so the exponent is always ≤ 0:

$$x_i' = \frac{U_i - U_{\max}}{T}$$

This is the version specified here; the un-subtracted formula from Report B is explicitly
**not** adopted.

**Hysteresis/cooldowns.** A momentum-style score bonus on the previously-chosen action,
plus a minimum-run-time lock for actions that shouldn't be interrupted mid-flight (a
committed slide tackle). Both reports agree; sourced to documented mitigations for
score-oscillation in the IAUS literature (Report A §2).

**Decision frequency.** Decoupled from the physics tick — see §5 for the pipeline and
§9/§13 for why the exact tick count is Provisional, not Locked.

---

## 5. Agent Models

| | Player | Ball | Referee | Manager |
|---|---|---|---|---|
| Pattern | Full Utility AI | **Not** an agent | Rule engine + one Utility seam | Utility AI, different cadence/population |
| Population | 22 | 1 | 1 | 1–2 |
| Trigger | Decision tick, time-sliced across the roster | N/A (continuous physics) | Rule checks every tick; the advantage call on foul-contact events | Match events / periodic check |
| Perception | Local spatial neighborhood | N/A | The single foul instant | Whole-match aggregates |
| Hysteresis | High — momentum bonus, avoid flicker | N/A | N/A (one-shot) | High but different shape — long cooldown gate, not momentum |

**Player.** The canonical Utility AI case — many candidate actions (move, chase, pass,
shoot, tackle, mark, press, support, hold) with several genuinely continuous, competing
considerations. Initial action set (deliberately small, both reports converge on a
similar list): `MoveToPosition`, `ChaseBall`, `PassTo(target)`, `ShootAtGoal`,
`Tackle(target)`, `MarkOpponent(target)`, `Press(target)`, `SupportRun`, `HoldPosition`.
Match state (score differential, time remaining, mentality) is modeled as **modifiers on
existing considerations' response curves**, not as new actions — an "attacking" mentality
shifts the risk-tolerance curve on `ShootAtGoal`/`PassTo` rather than adding a new action
(Report A §9). This keeps the action set stable and puts all tactical tuning in one place.

**Ball — full agreement, Locked.** Both reports independently reject modeling the ball as
a Utility AI agent: it has no goals or perception, and scoring "actions" like "keep
rolling" is a category error that adds indirection for no payoff. It is a pure physics
entity — `Position`, `Velocity`, `Spin`, deterministic clamped integration — plus a small
explicit state tag:

```rust
pub enum BallState {
    Free,
    Possessed(Entity),
    InFlight { from: Entity, aimed_at: Vec2 },
    OutOfPlay,
    Dead,
}
```

State transitions are triggered by simple geometric/event rules (radius-of-control check,
line-crossing, whistle) — never scored. This directly matches the bug class this
project's own [[ninety-minutes]] `BallSystem` already hit (unbounded velocity from
unclamped Euler integration); the spec requires an explicit velocity clamp and a unit
test asserting it holds under adversarial inputs, carrying that lesson forward rather than
re-discovering it in Rust.

**Referee — mostly agreement, one scope disagreement.** Both reports treat offside,
out-of-bounds, goals, and restarts as pure deterministic geometry/event checks — **Locked,
not scored**. Both also agree there's exactly one genuine trade-off worth Utility AI: the
**advantage** call (play on vs. blow the whistle), because it's a real trade-off between
continuous factors (attacking position quality, likelihood possession continues, foul
severity) rather than a fixed rule. Where they diverge: Report B also scores **card
issuance** as a continuous Utility output, mapping the same decision's score into bands
(`< 0.3` → advantage, `0.3–0.7` → whistle only, `≥ 0.7` → card) with no sourcing for those
specific thresholds. Report A treats card issuance as a small deterministic state machine
over a card-count component (second yellow → red), fed by a foul-severity classification
produced upstream by contact-resolution physics — not scored by the referee module itself.

**Resolution (Provisional):** adopt Report A's narrower scope. Real refereeing doesn't
grade card severity on a smooth continuous scale the way Report B's banding implies — a
"serious foul play" classification is close to categorical, not a threshold crossed on a
dial. Foul-severity *classification* can reasonably borrow a small Utility-style scoring
internally (Report B's considerations — severity, last-defender context, match
aggression — are reasonable inputs), but the **card decision itself** is a rule-table
lookup against that classification plus prior-card state, not a second independent Utility
"action." This is marked Provisional rather than Locked because the evidence for either
shape is thin — it's a legitimate design judgment, not something either report settles
with real football-refereeing sourcing.

**Manager — resolved: weighted decision table, not full Utility AI.** Coarse-grained,
high-latency actions (`ChangeFormation`, `Substitute`, `ShiftMentality`, `NoChange`),
evaluated on match events or a periodic check — not every decision tick, and with no
time-slicing problem since there are only 1–2 manager agents. Report A's open question
(§13 of the prior draft) — whether full Utility AI machinery is warranted for 1–2
low-frequency agents versus a simpler weighted table — is now settled in favor of the
simpler table. One architectural consequence worth stating plainly: `sim-ai-manager` no
longer needs to depend on `sim-ai-core` at all. The response-curve/geometric-mean
machinery earns its cost at high evaluation frequency across 22 agents, where subtle curve
shaping prevents flicker; at "a handful of times per match," a flat linear weighted sum
loses little and is easier to tune and debug. See §5.1 for the concrete design.

### 5.1 Manager AI — weighted-table design (confirmed, Locked)

Short research pass on how other games handle low-frequency, small-population strategic
decisions, since neither original report considered any alternative to full Utility AI
here:

- **[Likely]** A flat, linear weighted sum over normalized `[0,1]` factors — no response
  curves, no geometric-mean/compensation aggregation — is a recognized, deliberate
  simplification of Utility AI, not a different paradigm bolted on. Community discussion
  citing Dave Mark's own work (the same source Report A cites for IAUS) frames it exactly
  this way: curve-based scoring is "the next step beyond simple weights," adopted when
  linear weighting stops being good enough — implying simple weights are the correct
  starting point, not a lesser one, when that threshold hasn't been reached.
- **[Certain, as reported]** Dave Mark himself, in a public forum thread on diplomatic AI
  for 4X strategy games (a low-frequency, small-population decision shaped almost
  identically to this project's manager AI), recommends a small set of weighted indicators
  over full curve machinery — i.e., the inventor of Utility AI doesn't reach for his own
  heavier tool at this decision frequency either.
- **[Guessing — indie/hobbyist source, not peer-reviewed or industry-authoritative]** A
  documented 4X AI ("Annhexation," GDC-adjacent but not a GDC talk itself) separates
  strategy from execution and profiles opponents across ~11 normalized dimensions
  (militarization, expansionism, border tension, etc.), combining them into a weighted
  ranking — but also tracks a short **trend** (rising/flat/falling over the last several
  turns) per dimension, not just the instantaneous value. This is the one genuine gap in
  a naive weighted table worth designing around from the start: reacting only to
  instantaneous match state (current score, current stamina) misses momentum a real
  manager would notice (a team that's been dominant for the last 10 minutes despite a
  scoreline that doesn't yet show it).

**Proposed design**, for you to confirm or adjust:

```rust
// sim-ai-manager — no dependency on sim-ai-core

pub struct WeightedFactor {
    pub name: &'static str,
    pub weight: f32,
    pub value: f32, // pre-normalized 0.0..=1.0
}

pub struct ManagerDecisionTable {
    pub candidates: Vec<(ManagerAction, Vec<WeightedFactor>)>,
    pub min_trigger_score: f32,   // below this, NoChange wins regardless
    pub cooldown_ticks: u32,      // long gate, not momentum bonus
    pub last_action_tick: u64,
}

pub fn score(factors: &[WeightedFactor]) -> f32 {
    factors.iter().map(|f| f.weight * f.value).sum()
}
```

- Each macro-decision type (substitution, formation change, mentality shift) is its own
  small table — rows are candidate actions, each with its own fixed weight vector over a
  handful of normalized factors (goal-difference pressure, time-remaining pressure,
  average squad stamina, cards, and a short rolling-window **momentum** factor computed
  from the last few minutes of match events, not just current state — confirmed as part
  of the design, not just instantaneous state).
- Highest score wins only if it clears `min_trigger_score`; otherwise `NoChange`. The long
  cooldown (§5) replaces momentum-bonus hysteresis entirely — no per-tick decay logic
  needed at this frequency.
- Evaluated on match events (goal, red card, stamina crossing a threshold) plus a periodic
  fallback check, per both original reports' agreement on cadence.

This keeps the "not everything needs Utility AI" principle both reports opened with
consistent all the way down — the manager literally doesn't import the Utility AI crate.
**Confirmed**: flat weighted sum (no response curves/geometric mean) plus the momentum/
trend factor, both signed off. The momentum window length (how many minutes back, and
which raw events feed it — shots, territory, possession share) is left as an
implementation-time tuning detail, not a further open architectural question.

---

## 6. Football Simulation Systems

- **Movement/steering.** `Intent` → steering forces → semi-implicit Euler integration,
  velocity-clamped (both reports; the clamp is non-negotiable per the ninety-minutes
  `BallSystem` history above, and applies equally to players).
- **Ball interaction.** Possession = radius-of-control check against the ball handler;
  while possessed, ball position is tethered to the handler until an action (pass, shot,
  clearance) applies a physics impulse (Report B §4.2) — compatible with Report A's
  state-tag model, adopted as the concrete possession mechanic.
- **Spatial perception & pitch control.** Report A stays generic here (a "nearby
  teammates/opponents" snapshot with no specified spatial algorithm); Report B is the
  substantially more developed source — a coarse grid (illustrative `16×12` cells) with a
  per-cell attacker/defender time-to-reach comparison folded through a sigmoid dominance
  score:

  $$P_{\text{control}}(\mathbf{x}) = \frac{1}{1 + e^{-k (t_{\text{def}}(\mathbf{x}) - t_{\text{att}}(\mathbf{x}))}}$$

  Adopted (Locked in shape, Provisional on exact grid resolution), with one correction
  carried over from Report B's own second self-critique round: this is **not** a per-tick
  computation. Report B's first pass computed it every physics tick with no stated
  frequency; its own critique caught that pitch control doesn't change fast enough to
  justify 60 Hz recomputation and tied it to the same decoupled decision cadence as player
  evaluation.

  Report B's critique also caught a real correctness bug worth stating plainly, because it's
  the kind of thing that's easy to reintroduce: an earlier version staggered *which team*
  evaluates on a fresh grid (Team A on ticks 0/6/12, Team B on ticks 3/9/15), which is a
  **fixed, repeating first-mover advantage baked into the schedule itself** — not
  performance noise that averages out, a standing asymmetry for the full 90 minutes. The
  fix that's specified here: the grid recomputes once per decision cycle, and **both
  teams** evaluate against that same freshly-computed grid in the same tick window — never
  partitioned by team. This also sidesteps the bug at the root, rather than just
  symmetrizing it, by never staggering *by team* in the first place (individual **players**
  can still be time-sliced across the full 22-player roster for their own decision
  re-evaluation, per Report A §8 — that staggering has no team-level bias because it
  rotates through both rosters together).
- **Passing/shooting/tackling.** Considerations per action as listed in §5/§9. The
  RoboCup soccer-simulation literature (cited in Report A — Voronoi-based space reasoning,
  Hungarian-algorithm role assignment, fuzzy-logic passing factors) is adopted as the
  reference source for *which inputs matter*, since it's more rigorously tested for this
  exact problem than a management-sim wiki would be.
- **Match rules & referee processing.** Per §5 — deterministic rule engine for
  offside/out-of-bounds/goals/restarts; one Utility seam for advantage; rule-table mapping
  for cards.
- **Tactical/managerial systems.** Per §5 — coarse action space, event/periodic cadence.

---

## 7. Determinism & Reproducibility

**This is the central disagreement between the two reports, and it's resolved explicitly
rather than split down the middle.**

Report B locks **strict fixed-point math** (`fixed` crate, `I32F32`) everywhere, justified
by "100% bit-identical cross-platform determinism." Report A argues that requirement
doesn't actually apply here: this is a **server-authoritative** engine, not peer-to-peer
lockstep netcode — clients never re-execute the simulation, they receive it. The
determinism that actually matters (replay-for-debugging, regression testing, same-seed
re-simulation) only needs **same-binary/same-machine** reproducibility, which plain
`f32`/`f64` already gives you for the `+ - * /` operators (IEEE 754-specified, and Rust
guarantees IEEE 754 semantics for those operators specifically). The genuinely expensive,
hard-to-get-right kind of determinism — bit-identical across CPU vendors and compilers —
is what lockstep P2P netcode needs, not what this architecture needs.

**Notably, Report B's own internal critique asks this exact question** — "why does
cross-platform determinism matter at all if only the server ever executes match logic?" —
and never answers it before locking Option A anyway. That's a real, self-identified gap in
Report B's reasoning, not a reach on this document's part.

**Resolution (Locked): same-build/same-machine reproducibility, plain `f32`, not
fixed-point by default.** This also matches this project's own existing, already-proven
engineering discipline: [[ninety-minutes]] already established "no transcendental math in
hot paths" as a standing invariant, and Fulltime already uses a seeded Mulberry32 PRNG for
exactly this same class of reproducibility requirement. Report A explicitly argues for
carrying both forward rather than re-deriving them; that argument is adopted as-is.
Report B never engages with this project history at all, since its reasoning doesn't draw
on it — that's the concrete reason its recommendation loses here, not a coin flip.

**Sources of nondeterminism to guard against** (both reports agree on this list; merged):

- Hash-map/set iteration order — use `BTreeMap`/`IndexMap`, or iterate a stable `Vec<Entity>`.
- Transcendental function calls (`sin`/`cos`/`sqrt`/`exp`) inside the deterministic core —
  their last bit is allowed to differ across CPU vendors/libm versions even though this
  project doesn't need cross-platform bit-exactness, this is free insurance and matches
  existing discipline.
- Wall-clock time, thread scheduling, or OS-level randomness inside the sim core.
- Unseeded or per-thread RNG — one seeded, single-threaded `DeterministicRng` resource,
  advanced in a fixed, tick-deterministic order.
- **Query iteration order for any system that both iterates multiple entities and
  consumes a shared, order-sensitive resource** (the RNG, above all). Report B's own
  third self-critique round caught its own "locked" implementation violating a rule it had
  already written down in its own §6 — the evaluation system iterates `query.iter_mut()`
  directly with no explicit entity-ID sort, and `bevy_ecs` does not contractually
  guarantee that traversal order is stable across builds, platforms, or even minor version
  bumps. This is now stated as a hard rule, not a documentation note: **any system that
  draws from `DeterministicRng` while iterating a query must sort by a stable `EntityId`
  first** (or collect into a `Vec` in a pinned order). This is exactly the kind of thing
  that's trivial to get right up front and expensive to debug later — worth having a
  lint/test for, not just a comment.

**Resolved: the logistic curve uses `f32::exp()` directly.** The pitch-control model (§6)
and several player considerations use a sigmoid; this is now a settled exception to the
"no transcendentals in the hot path" rule, not an open question. Reasoning: that rule
exists as insurance against cross-platform bit-exactness, and §7 already establishes this
project doesn't need cross-platform bit-exactness — so the insurance isn't buying anything
in this one case, and paying for a LUT (offline generation pipeline, domain-scaling logic,
interpolation) to avoid a risk that isn't being hedged against elsewhere would be pure
overhead. The general rule still holds everywhere else it applies (avoid transcendentals
where you don't have a specific reason not to); this is a named, single exception, not a
weakening of the rule.

### 7.1 Fixed-point — resolved: not adopted, no open-ended revisit clause

You indicated no preference here and asked for a decision rather than a deferred option,
so: **[Likely]** this project never needs fixed-point, and it is specified as such rather
than left open-ended. A vague "revisit if a requirement appears" clause tends to quietly
become the default the first time someone's unsure, which defeats the point of isolating
it. Two concrete, named triggers — and only these — would justify reopening this decision;
absent one of them actually being committed to (not just discussed), the answer stays
float, indefinitely:

1. **A peer-to-peer or offline/LAN multiplayer mode is committed to**, where the
   simulation must run identically on hardware you don't control end-to-end (this is the
   lockstep-netcode case Report A's §0 distinguishes from server-authoritative — nothing
   in the current design implies this is planned).
2. **A competitive-integrity feature is committed to** where a third party must
   independently re-simulate a match from a published seed + input log to verify a result,
   rather than trusting the server's recorded snapshot stream. (If verification instead
   just replays the recorded stream — which is what §2/§8 already specify — this trigger
   doesn't apply; that's exactly the distinction Report B's own critique raised and never
   resolved.)

Given this is a solo-developer, single-process, server-authoritative project with neither
of those on the roadmap, fixed-point is not a "someday" item — it's specified as not
happening unless one of the two triggers above is explicitly decided elsewhere first. If
that ever happens, Report B's corrected LUT design (after its own self-critique fixed the
domain-mismatch and runtime-division bugs — see §4 and below) is the concrete technique to
use, not a from-scratch redesign. It stays isolated in its own `sim-math` crate (§11) for
exactly this reason:

- LUTs generated **offline** (build-script, arbitrary/`f64` precision, baked in as `const`
  arrays) — never computed on the target machine, since two platforms' `libm::exp()`
  implementations can differ by an ULP and that's enough to eventually desync a
  5,400-tick replay.
- Interpolation via precomputed reciprocal multiplication, not runtime division — and this
  needs to actually be enforced: Report B's own third critique round found its "locked"
  interpolation code violating its own "no runtime division" rule in the same file that
  states the rule (`I32F32::from_num(20.0) / I32F32::from_num(1023.0)` computed live on
  every call instead of being a precomputed `const`). Worth a standing lint or code-review
  checklist item, since documentation alone visibly didn't catch it the first time.
- Overflow behavior pinned explicitly (`saturating_*`/`wrapping_*` operations used
  uniformly), because Rust's default integer-overflow behavior differs between debug
  (panics) and release (wraps) builds — a debug-build replay test could panic while a
  release-build server run silently produces a divergent-but-non-panicking result.

**Server-authoritative implication that holds either way:** because the server is the
single source of truth, clients never need to *reproduce* the simulation — they receive
it. That's what relaxes the hardest determinism requirement down to same-machine
reproducibility for replay/regression/re-simulation, regardless of which math
representation is ultimately used.

**Replay requirements.** Same seed + same inputs → identical result, verified via a
per-tick (or end-of-run) state hash/checksum, diffable to an exact divergent tick rather
than an "it drifted somewhere" search (§10).

**Parallelism.** None assumed by default for order-sensitive systems (§3). If profiling
later justifies parallel execution for genuinely independent systems, each such system
must be re-audited against the rules in this section before being parallelized — not
assumed safe because it "doesn't look order-sensitive."

---

## 8. Server/Network Boundary

Both reports agree at the principle level and neither designs a real protocol — that's
adopted deliberately, not a gap in this document. Report A explicitly defers this to
"entirely out of scope until the core sim (Phases 0–6) exists; premature to design now"
(§13.7); Report B sketches a high-level diagram (client commands → server tick loop →
compressed snapshots) but no protocol detail. The principle-level agreement:

- **Client commands:** tactical instructions and substitutions only (validated,
  server-applied at tick boundaries). Clients issue no other input.
- **Authoritative server state:** everything else — player/ball positions, possession,
  match phase, score.
- **Snapshots/events:** the server emits either full/delta state snapshots or a discrete
  event stream (goal, card, substitution, restart) for client rendering; the choice
  between these, and any compression/interest-management scheme, is **Open** (§13).
- **Client authority:** none. The client never determines an outcome — it renders what the
  server already decided.

Designing the actual wire protocol before Phases 0–6 (§12) exist is premature per both
reports' own reasoning, and that reasoning holds up: nothing about the protocol can be
validated against a simulation core that doesn't exist yet.

---

## 9. Data Model

Representative components/resources, synthesized from both reports' data models under the
determinism decision in §7 (plain `f32`, not `I32F32`):

```rust
// sim-components — pure data, no logic

pub struct Position(pub Vec2);
pub struct Velocity(pub Vec2);
pub struct TeamId(pub u8);
pub struct Role(pub PositionalRole); // CenterBack, DefensiveMid, Winger, Striker, ...
pub struct Stamina(pub f32);         // 0.0..=1.0

pub enum BallState {
    Free,
    Possessed(Entity),
    InFlight { from: Entity, aimed_at: Vec2 },
    OutOfPlay,
    Dead,
}
pub struct Ball {
    pub position: Vec2,
    pub velocity: Vec2,
    pub spin: f32,
    pub state: BallState,
}

// sim-ai-core — considerations, curves, aggregation; zero football knowledge

pub struct ConsiderationScore(pub f32); // clamped 0.0..=1.0

pub trait ResponseCurve {
    fn evaluate(&self, normalized_input: f32) -> ConsiderationScore;
}

pub fn aggregate_geometric_mean(scores: &[ConsiderationScore]) -> f32 {
    let product: f32 = scores.iter().map(|s| s.0.max(1e-4)).product();
    product.powf(1.0 / scores.len() as f32)
}

pub struct UtilityBrain<A> {
    pub candidate_actions: SmallVec<[UtilityActionDefinition<A>; 10]>,
    pub evaluation_interval_ticks: u8,   // Provisional — see §13
    pub last_evaluated_tick: u64,
    pub hysteresis_bonus: f32,
    pub softmax_temperature: f32,        // 0.0 (default) = pure greedy argmax
}

pub struct ActiveAction<A> {
    pub action: A,
    pub score: f32,
    pub ticks_executing: u32,
    pub chosen_at_tick: u64,             // for hysteresis / momentum
}

// sim-ai-player

pub struct PerceptionSnapshot {
    pub self_state: SelfContext,          // position, stamina, role, has_ball
    pub ball: BallContext,
    pub nearby_teammates: SmallVec<[TeammateContext; 6]>,
    pub nearby_opponents: SmallVec<[OpponentContext; 6]>,
    pub match_context: MatchContextRef,   // score diff, time remaining, mentality
}

pub struct Intent {
    pub action: PlayerAction,
    pub target: Option<Entity>,
    pub target_point: Option<Vec2>,
}
```

`SmallVec` inline capacity is kept from Report A — it keeps `UtilityBrain`/`Intent`
allocation-free per agent per tick, which matters once this is evaluated for 22 players at
whatever decision-tick rate §13 eventually settles on.

**Resources:** `MatchClock`, `Score`, `DeterministicRng` (seeded Mulberry32 — §7),
`PitchDimensions`, `PitchControlGrid`, `MatchPhase`, `TacticalSettingsPerTeam`.

---

## 10. Performance & Scalability

**Expected workload.** 22 players, one ball, one referee context, 1–2 manager agents, at
60 Hz physics with a decoupled, slower decision cadence.

**AI evaluation frequency.** Decoupled decision tick + per-player time-slicing across the
full roster (`tick % 22 == player_index`), so no single tick pays the full "22 players ×
N actions × M considerations" cost at once — both reports converge on decoupling cadence
from the physics tick; the specific staggering technique is Report A's, adopted because it
has no team-partition bias (§6).

**Spatial-query strategy.** Spatial hash for neighbor queries every tick (cheap); the
coarser pitch-control grid recomputed on the decision cadence, not every physics tick
(§6).

**ECS considerations.** Archetypal storage via `bevy_ecs`; explicit ordering for
order-sensitive systems (§3, §7) takes priority over throughput-maximizing automatic
scheduling.

**Profiling requirements — flagged, not accepted as given.** Report B presents specific
numbers (grid recompute ~60–90 µs, dual-team utility evaluation ~30–50 µs, combined
~0.14 ms against a 16.66 ms budget) labeled **"Measured Cost."** Nothing has actually been
measured — Report B's own third self-critique round says exactly this ("these are
estimates ahead of Phase 5 profiling... calling projected figures 'measured' invites
treating them as validated when they're really the hypothesis the benchmark exists to
test"). **These figures are carried forward here only as order-of-magnitude projections,
explicitly not as measured facts**, and the roadmap (§12) puts real profiling before any
tuning decision that would depend on them. Concrete criteria worth keeping (relabeled):

| Projected metric | Target | Status |
|---|---|---|
| `pitch_control_recompute_time` | < 200 µs | Unvalidated — Phase 5 |
| `utility_evaluation_combined_time` | < 100 µs | Unvalidated — Phase 5 |
| `frame_time_p99` variance (decision tick vs. non-decision tick) | < 0.5 ms | Unvalidated — Phase 5 |

**Scalability risks.** None severe at this population size (22 agents is small by
game-AI standards); the larger risk is the determinism discipline in §7 eroding under
future feature pressure (someone reaches for a transcendental call in a hot path six
months from now without realizing why the rule exists) more than raw throughput.

**Priority, confirmed:** a functionally-complete server simulation outranks performance
work. The Phase-5(-equivalent) benchmarking above stays on the roadmap because the targets
are cheap to state now and expensive to retrofit test coverage for later, but nothing in
§12 blocks on hitting them, and no implementation effort should be spent chasing the
projected µs figures in the table above until the simulation is otherwise working
end-to-end.

---

## 11. Rust Project Structure

Synthesized from both reports' proposed workspace layouts — Report A's is adopted as the
base (it states an explicit, load-bearing boundary invariant Report B's layout doesn't:
`sim-ai-core` has zero knowledge of football, and `sim-server` depends on `sim-core` only,
never on `sim-ai-*` internals — which is what actually makes "shared vs. specialized
infrastructure" a property of the code rather than a design-doc aspiration). Report B's
crate contents (rules engine as its own module, `sim_net`) are folded in.

```
football-sim/                    (Cargo workspace)
├── crates/
│   ├── sim-math/                fixed-step-safe math: Vec2 ops, deterministic PRNG
│   │                             (Mulberry32 port); isolated so a future fixed-point
│   │                             migration (§7.1) is contained here if it ever happens
│   ├── sim-components/          Position, Velocity, TeamId, Role, Stamina, BallState...
│   │                             (pure data, no logic — any crate can depend on it)
│   ├── sim-physics/             ball + player movement integration, collision response
│   ├── sim-ai-core/             Consideration, ResponseCurve, geometric-mean aggregator,
│   │                             Action trait, UtilityBrain — zero football knowledge,
│   │                             shared by player and manager AI
│   ├── sim-ai-player/           concrete player considerations/actions on sim-ai-core
│   ├── sim-ai-manager/          concrete manager considerations/actions on sim-ai-core
│   ├── sim-rules/               deterministic rule engine: offside, out-of-play, goals,
│   │                             restarts, card-issuance rule table (§5)
│   ├── sim-referee/             wraps sim-rules + the one advantage Decision (built on
│   │                             sim-ai-core)
│   ├── sim-core/                World, fixed-step scheduler, explicit system ordering
│   │                             (§3), perception snapshotting
│   ├── sim-replay/              deterministic replay/record harness, state-hash checks
│   └── sim-server/               (binary) server-authoritative loop, network boundary —
│                                 depends on sim-core only, never sim-ai-* internals
└── docs/football-engine-design.md   (one canonical design doc, per this project's
                                       existing documentation convention)
```

---

## 12. Implementation Roadmap

**Resolved ordering disagreement:** Report A gates AI entirely behind a proven
deterministic substrate (Phase 0 = physics + ECS + RNG only, no football, no AI — a CLI
that runs N ticks from a seed and must hash-match itself run twice). Report B treats
"Determinism Verification & Tooling" as its *last* phase, after the full stack (spatial
grid, player AI, referee, manager) already exists. **Report A's ordering is adopted.**
Finding a nondeterminism bug when the only moving part is ball physics is a bisectable,
cheap problem; finding one after the full stack exists — which is exactly the position
Report B's own Bug 2 (unsorted query iteration feeding the shared RNG) was caught in — is
an expensive audit, not a structural guarantee. Report B's phase *content* (concrete pitch
control, referee/manager grouping) is preserved, just re-slotted into Report A's more
disciplined gating.

| Phase | Deliverable |
|---|---|
| 0 | Deterministic substrate only: fixed-step loop, ECS world, ball physics (clamped integration), seeded RNG. CLI runs N ticks from a seed and prints a state hash; two runs with the same seed must match. No football, no AI. |
| 1 | Static/rule-based players: fixed formation slots, hardcoded `MoveToPosition`, radius-check possession. Proves the read/write-separation pipeline and perception-snapshot shape before AI adds a variable. |
| 2 | Player Utility AI, minimal action set (§5), one team only — opponent stays rule-based. Isolates the riskiest new layer for testing. Logistic-curve determinism choice (§7) must be closed before this phase starts. |
| 3 | Referee minimum viable: out-of-bounds, goals, kickoff/restart as a rule engine. No fouls/offside yet. |
| 4 | Expand player considerations/actions (marking, pressing, support); tune geometric-mean aggregation and hysteresis against scenario tests. Introduce the pitch-control grid (§6), tied to the decision cadence from the start — not added un-cadenced and fixed later. |
| 5 | Offside + fouls in the referee, including the advantage Decision and the card rule table (§5). |
| 6 | Manager AI (§5) — now that real match context exists to feed it. Revisit the "full Utility AI vs. simpler weighted table" open question (§13) with an actual action repertoire in hand. |
| 7+ | Tactical-instruction wiring, real profiling against §10's targets, network snapshot/delta protocol design (§8) — only once the core sim is real enough to profile and design against. |

**Explicitly deferred, out of this document's scope entirely** (both reports agree):
reverse-engineering any real product's internal formulas; full cross-platform bit-exact
determinism unless §13's trigger conditions are met; crowd/morale depth beyond manager
inputs; weather/pitch-condition systems; anti-cheat/networking hardening beyond the basic
server-authoritative boundary.

---

## 13. Final Decision Register

| # | Decision | Final specification | Reason | Source | Status |
|---|---|---|---|---|---|
| 1 | ECS crate | `bevy_ecs` as a library | Both reports converge independently | A, B | Locked |
| 2 | ECS scheduling | `Schedule` with every order-sensitive system explicitly chained; never rely on default parallel inference for order-sensitive systems | Resolves A vs. B's contradiction — keeps B's ergonomics without A's determinism risk | A, B + project reasoning | Locked |
| 3 | Determinism bar | Same-build/same-machine reproducibility, not cross-platform bit-exactness | Server-authoritative architecture doesn't need lockstep-grade determinism; matches existing [[ninety-minutes]] invariant | A (project-grounded) | Locked |
| 4 | Numeric representation | Plain `f32`; transcendentals avoided in the hot path | Follows from #3; avoids B's unresolved fixed-point justification gap | A | Locked |
| 5 | Fixed-point math | Not adopted; isolated in `sim-math` for containment only | No current requirement forces it; no open-ended revisit clause — only 2 named triggers (§7.1) reopen this | A (B's LUT design reused if triggered) | Locked — "no" until a named trigger fires |
| 6 | PRNG | Port Mulberry32 verbatim | Matches existing Fulltime/ninety-minutes invariant; enables TS↔Rust golden-master testing | A (project-grounded) | Locked |
| 7 | Ball model | Physics entity + explicit `BallState` tag, not a Utility agent | Full agreement, independently reasoned | A, B | Locked |
| 8 | Player decision model | IAUS-style Utility AI, decoupled decision tick, roster-wide time-slicing | Full agreement | A, B | Locked |
| 9 | Aggregation formula | Geometric mean | Simpler, no tunable constant, solves same N-decay problem as B's compensation factor | A | Locked |
| 10 | Referee — advantage | One small Utility Decision (2–3 considerations) | Full agreement | A, B | Locked |
| 11 | Referee — cards | Rule-table lookup from a severity classification, not a separate Utility action; thresholds unsourced placeholders, acceptable for v1 | A's scope is better supported; confirmed sufficient for v1 | A | Locked (for v1) |
| 12 | Offside/out-of-play/goals | Deterministic rule engine, unscored | Full agreement | A, B | Locked |
| 13 | Manager AI machinery | Weighted decision table, no `sim-ai-core` dependency; long-cooldown gate; includes a rolling momentum/trend factor alongside instantaneous state | Confirmed — full Utility AI machinery unwarranted at 1–2 agents / low frequency (§5.1 research); momentum factor signed off | A (open question), project decision | Locked |
| 14 | Pitch-control model | Coarse grid + sigmoid dominance, on decision cadence, synchronized across both teams (not team-staggered) | B's mechanic, A's cadence-decoupling principle, B's own bug-fix applied | B (corrected) | Locked (shape); grid resolution Provisional |
| 15 | Decision-tick rate / time-slicing | Default N=6 (~10 Hz); per-player roster-wide stagger | Needs profiling before being final | A, B | Provisional |
| 16 | Softmax/Boltzmann selection | Optional, off by default; numerically-stable max-subtraction formula | B's Bug 1 fixed; not adopted as originally shipped | B (corrected) | Provisional |
| 17 | 50/50 physical contention | Hidden-RNG-roll vs. pure geometry | Genuine game-design call, not resolved by either report | A | Open |
| 18 | Ball possession representation | Single enum tag | Simpler; revisit only if simultaneous partial possession is needed | A | Locked (default) |
| 19 | Crate structure | 11-crate workspace (§11) | A's boundary discipline + B's crate contents merged | A, B | Locked |
| 20 | Network protocol | Deferred entirely | Both reports agree it's premature before core sim exists | A, B | Open |
| 21 | Performance figures | Carried as projections only, not measured facts; not a near-term priority | B's own critique flags its "Measured" label as false; confirmed working sim outranks optimization | B (relabeled) | Open — deprioritized, needs Phase-5(-equiv.) benchmarking eventually |
| 22 | Query iteration order (RNG-consuming systems) | Explicit stable sort/`Vec<Entity>` iteration required | Closes B's self-identified Bug 2 | A (principle), B (bug) | Locked |
| 23 | Roadmap ordering | Determinism substrate proven first (Phase 0), before any AI/rules content | A's discipline over B's "verify determinism last" ordering | A | Locked |
| 24 | Logistic-curve determinism | `f32::exp()` directly | The cross-platform risk it would hedge against isn't a requirement here (#3, #4) | — (project decision) | Locked |

---

## 14. Unresolved Issues

All six original open items are now resolved (logistic-curve method, card-issuance scope,
manager AI machinery + momentum factor, network-protocol deferral, performance-target
priority, and the fixed-point trigger condition are all Locked or explicitly
deprioritized — §13). The manager's exact per-action factor list/weights (substitution vs.
formation change vs. mentality shift) and the momentum window's tuning parameters (how
many minutes back, which raw events feed it) remain implementation-time detail to fill in
during Phase 6, not architectural questions. What's genuinely still open:

- **50/50 physical contention resolution (#17).** Hidden-RNG-roll vs. pure deterministic
  geometry for contested loose-ball situations. Neither report resolves this, and it
  wasn't part of the six items you closed out — it's a genuine game-design call (how much
  visible randomness the football model should have), not an architecture question this
  document can settle. Risk of leaving unresolved: low near-term (it doesn't block Phases
  0–4), but it touches player-tackle considerations directly once Phase 4/5 need it.

- **Network/snapshot protocol (#20).** Confirmed correct to defer. No change — still
  needs Phases 0–6 to exist first before it can be designed against anything real.
