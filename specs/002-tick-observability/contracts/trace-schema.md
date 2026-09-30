# Trace File Contract: Chrome JSON Trace Format (Perfetto-compatible)

**Feature**: [002-tick-observability](../spec.md)

**Format**: [Chrome JSON Trace Event Format](https://docs.google.com/document/d/1CvAClvFfyA5R-PhYUmn5OOQtYMH4h6I0nSsKchNAySU), the same format Perfetto UI (`ui.perfetto.dev`) opens natively. Produced via the `tracing-chrome` crate; this document specifies the process/thread/track/event/arg naming this feature commits to, since those names are free-form in the format itself and need to stay consistent for the trace to be readable.

This is a **new, fourth snapshot concept** in this codebase, deliberately independent of the other three (`specs/001-football-sim-engine/contracts/state-snapshot.md`, `sim-replay::MatchSnapshot`, `sim-core::MatchSnapshot`). It shares no types or serialization code with them. See `data-model.md` for why.

## Process / Thread Naming

| Process name | Threads | Present when |
|---|---|---|
| `"<Team> #<squad_number> — <Role>"` (e.g. `"Home #9 — ST"`) | `Position`, `Decision` | Always, one process per player entity |
| `"Ball"` | `State` | Always |
| `"Referee"` | `Events` | Only when a `Referee` component exists on an entity in the world (does not exist in the current codebase — see spec.md Assumptions) |

## Player Process — `Position` Thread

Counter tracks (Chrome trace `ph: "C"` events), one per field, emitted at every recorded tick:

| Track name | Value | Source |
|---|---|---|
| `x` | f32 | `Position.x` |
| `y` | f32 | `Position.y` |
| `vx` | f32 | `Velocity.x` |
| `vy` | f32 | `Velocity.y` |
| `stamina` | f32 (0.0–1.0) | `Stamina` |

## Player Process — `Decision` Thread

One **duration span pair** (`ph: "B"` begin + `ph: "E"` end) per recorded tick, named `"decision @ tick <n>"`. Nested inside it, one duration span pair per action evaluated that tick, named for the action (`"Shoot"`, `"Pass"`, `"Dribble"`, `"Cross"`, `"HoldPossession"`). **Implementation note**: Action names are derived from the `Intent` variant of each `PlayerAction` (e.g., `Action(ShootAtGoal(_))` → `"Shoot"`, `Action(PassTo)` → `"Pass"`, `Movement(HoldPosition)` → `"HoldPossession"`). A mapping function will be added to `sim-ai-player` during implementation. Each action span carries:

> **Note**: `tracing-chrome` emits `B`/`E` pairs for `tracing::span!` by default. This is the canonical Perfetto-compatible representation. The schema accepts `B`/`E` pairs; `"X"` complete events are also valid if emitted manually, but `B`/`E` is the expected output from the `span!`-based implementation.

| Arg | Type | Description |
|---|---|---|
| `chosen` | bool | Whether this action was the one ultimately selected |
| `aggregate_score` | f32 | The action's final aggregated score |

Nested inside each action span, one instant event (`ph: "i"`) per consideration evaluated for that action, named for the consideration (e.g. `"distance_to_goal"`, `"teammate_openness"`), carrying:

| Arg | Type | Description |
|---|---|---|
| `raw` | f32 | Raw input value before curve/weight applied |
| `curve` | string | Response curve identifier (from `sim-ai-core::ResponseCurve`) |
| `weight` | f32 | Weight applied to this consideration |
| `score` | f32 | Resulting score contributed to the action's aggregate |

**No consideration or action evaluated by the decision system on a recorded tick is omitted** — this is the direct implementation of FR-006/FR-010 and SC-002; every action the loop iterates over gets a span, every consideration within it gets an event, whether or not it was the winner.

## Ball Process — `State` Thread

Counter tracks, one per recorded tick:

| Track name | Value | Source |
|---|---|---|
| `x`, `y` | f32 | `Ball.position` |
| `vx`, `vy` | f32 | `Ball.velocity` |
| `spin` | f32 | `Ball.spin` |

Instant events (`ph: "i"`), emitted only on the tick a change occurs (not every recorded tick, to avoid a flood of identical no-op events):

| Event name | Args | Emitted when |
|---|---|---|
| `"state_change"` | `from`, `to` (`Free \| Possessed \| InFlight \| Dead`) | `Ball.state` changes |
| `"possession_change"` | `from` (entity id or `none`), `to` (entity id or `none`) | `Ball.possessor` changes |

## Referee Process — `Events` Thread

Only emitted when `Query<&Referee>` returns a result (currently: never, in this codebase — see spec.md). Instant events:

| Event name | Args | Emitted when |
|---|---|---|
| `"card"` | `player` (entity id), `color` (`Yellow \| Red`), `tick` | An entry is present in `Referee.cards` at a tick not previously seen |
| `"stoppage"` | fields dependent on `Referee.stoppage_events`'s shape at implementation time | An entry is present in `Referee.stoppage_events` at a tick not previously seen |

## Full Trace File Shape

A single JSON file (`tracing-chrome`'s default array-of-events format) using **`B`/`E` duration pairs** (canonical `tracing-chrome` output):

```json
[
  {"name": "x", "ph": "C", "pid": 12, "tid": 1, "ts": 1000000, "args": {"value": 34.2}},
  {"name": "decision @ tick 60", "ph": "B", "pid": 12, "tid": 2, "ts": 1000000, "args": {}},
  {"name": "decision @ tick 60", "ph": "E", "pid": 12, "tid": 2, "ts": 1016667, "args": {}},
  {"name": "Shoot", "ph": "B", "pid": 12, "tid": 2, "ts": 1000010, "args": {}},
  {"name": "Shoot", "ph": "E", "pid": 12, "tid": 2, "ts": 1005010,
    "args": {"chosen": false, "aggregate_score": 0.31}},
  {"name": "distance_to_goal", "ph": "i", "pid": 12, "tid": 2, "ts": 1000012,
    "args": {"raw": 18.4, "curve": "logistic", "weight": 0.8, "score": 0.29}},
  {"name": "x", "ph": "C", "pid": 99, "tid": 1, "ts": 1000000, "args": {"value": 52.0}},
  {"name": "possession_change", "ph": "i", "pid": 99, "tid": 1, "ts": 1000000,
    "args": {"from": "none", "to": "12"}}
]
```

(`pid` 12 here is a player's process; `pid` 99 is the Ball process. Exact `pid` assignment is an implementation detail — stable per-run, not necessarily stable across runs — since Perfetto identifies processes by their declared `process_name` metadata event, not by numeric `pid` value.)

> **Encoding note**: `tracing-chrome` produces `B`/`E` pairs for `span!`. Nested spans are represented by `ts`/`ts_end` containment (child `ts` ≥ parent `ts`, child `ts_end` ≤ parent `ts_end`). Perfetto UI renders this as a proper nested hierarchy. No manual `X` complete events required.

## Explicit Non-Goals of This Contract

- No versioning scheme — this is a debug artifact regenerated per run, not a persisted/migrated format like `state-snapshot.md`.
- No checksum/integrity guarantee — unlike the recovery snapshot contract, a partially-written trace file (e.g. from a crash) is an accepted, undefended-against failure mode for this iteration.
- No schema for the Referee section beyond "whatever `Referee.cards`/`stoppage_events` currently contain" — since that component is never populated in the current codebase, this section is aspirational and should be revisited once/if that gap is closed.
