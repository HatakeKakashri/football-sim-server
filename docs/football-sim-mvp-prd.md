# Football Match Simulation: MVP PRD

Status: final, decisions confirmed 10 Oct 2026. Source of truth for the Rust workspace (`sim-server` and its 10 sibling crates). The earlier TypeScript engine is discarded and nothing from it is ported.

Behavioral reference: *Top Eleven – Be a Football Manager* (matchday experience only, not a requirement to reproduce proprietary details). Attribute reference: https://top-eleven.fandom.com/wiki/Player.

Confidence labels: **[Certain]** established from the codebase or an explicit decision; **[Likely]** strong inference, verify during implementation; **[Guess]** speculative, must be measured or confirmed.

---

## 1. Goal

Deliver a complete, believable, deterministic 11v11 football match simulation in Rust. The backend is authoritative for simulation state, player decisions, match rules and event generation, and exposes a precomputed match to a separate frontend over HTTP + SSE. The MVP is backend-only.

A match must start, play through both halves with half-time and added time, and finish at a valid full time, with the final score equal to the count of goal events and the same seed always producing the same stream.

## 2. Scope

### In scope
- Full match lifecycle: kickoff, first half, added time, half-time with side swap, second half, added time, full time.
- 30 player attributes across two profiles, each with a declared consumer in the match model.
- 14 Top Eleven positions, a position-fit penalty, 5 formations, 3 mentalities, 4 tactic presets.
- 2D ball with aerial duels resolved probabilistically at the target point.
- Goals, kickoff restarts, out-of-bounds restarts (throw-in, goal kick, corner).
- Structured, ordered event log including every pass.
- HTTP + SSE API serving a precomputed match.
- Fixtures in TOML, startup validation, determinism by seed, runtime invariants, a multi-seed measurement harness.

### Out of scope (MVP)
- Fouls, cards, penalties, free kicks, offside enforcement. Foul detection stays a trace-only signal that never stops play; the offside stub stays inert.
- Substitutions, bench players, live manager commands, injuries, VAR.
- 3D ball physics.
- Frontend work.
- Transfers, finances, training, progression, seasons.
- CI, nightly sweeps, binary stream encoding, and any automatic full-match test tier.

`ManagerCommand`, its validators, `Substitute` (returns `NotImplemented`) and `replay --commands` stay compiled but are unreachable from the API. Replay by seed and setup is the supported reproducibility mechanism.

---

## 3. Baseline findings (current state)

| Finding | Confidence |
|---|---|
| `sim-server` is a CLI (`simulate`, `replay`, `benchmark`, `trace-filter`) with no HTTP server or HTTP dependency. `client-protocol.md` is provisional and unimplemented. | Certain |
| Engine runs at 60 ticks/s; a full match is about 324,019 snapshots (`elapsed_ticks` resets each half, 162,000 per half). | Certain |
| There is no structured match event log. Only per-tick snapshots and a Chrome-trace telemetry channel. | Certain |
| Perception hardcodes `goal_position = (105, 34)` for every player; the default brain hardcodes `ShootAtGoal(105, 34)` and `MoveToPosition(52.5, 34)`. The away team attacks −x, so its shots aim at its own goal. | Certain |
| Measured over 3 seeds (release build): ball in the away end (x > 88.5) about 99.8% of ticks; team 0 possession about 1.1 s per match; 3 possession changes; no tick with ball speed above 15 m/s; ball Free 58–96% of ticks; 0–0 in all three. | Certain (as measured) |
| The ball ending near x = 104.6 where the away goalkeeper holds it is explained by the goal-direction bug. A second bug leaving free balls unretrieved is suspected; decision traces should confirm. | Likely / Guess |
| Documented out-of-bounds restart bug: the OOB path marks the ball with a `RuleEvent` component the restart system does not see until the next tick, so the ball can stall at centre. Whether it is still present is unverified; the measured failure above is different. | Likely (unfixed) |
| The lifecycle resets both squads to a hardcoded 4-4-2 and never swaps ends at half-time. Only one formation exists in code. | Certain |
| Added time is always 0: `added_time_calculation_system` consumes `Referee.stoppage_events`, but no `Referee` entity is spawned. | Certain |
| `Ball.last_touch` holds a single record, so there is no touch history. | Certain |
| Goals are credited by ball position (x ≤ 0 → away, x ≥ 105 → home), not by toucher. | Certain |
| Tackle failure only multiplies speed by 0.3 for that tick; no recovery period and no stamina cost for `Tackle`/`Press`. There is no shot-save, aerial or goalkeeper mechanic. | Certain |
| Player quality is a single `Skill(f32)`. `Substitute` returns `NotImplemented`; manager commands affect only the home team. | Certain |
| Benchmark: 29,352 ticks/s, about 11 s of compute per match, over 6,000 ticks of a degenerate match. The `simulate` CLI took 63–72 s per match, likely JSON snapshot serialisation and disk writes. Re-measure under live play. | Certain / Likely |
| Workspace lints: `unsafe_code = "forbid"`, pedantic clippy; no CI exists. | Certain |

Implication: "50 of 50 seeds reach full time" would pass today with a 0–0 match played in one corner, so Phase 0 carries its own playability gate (§12).

---

## 4. Architecture and runtime

- **Stack:** `axum` + `tokio`, new server code in the `sim-server` crate (or a sibling crate), running as a Linux service.
- **Execution:** each `POST /match/{id}` starts one dedicated OS thread running that match's full simulation up front. The simulation is CPU-bound and must not run on the async executor.
- **Concurrency cap:** default `cores − 1` simultaneous simulations, configurable. Beyond the cap, `POST` returns `503` with `Retry-After`. Expected load is 1–4 concurrent matches; this is a design assumption, not a requirement.
- **Cache:** finished or failed results are held in memory with a TTL (default 1 hour). After expiry `GET` returns `410`; a new `POST` re-simulates the same seed and produces an identical stream.
- **Restart behaviour:** all in-memory state is lost on process restart; fixtures reload. A client may re-`POST` and reconnect with `Last-Event-ID` to receive an identical stream.
- **Performance target:** at most about 30 s release-mode wall time per full match with the full decision layer. The headless runner must not serialise per-tick snapshots.
- **Network defaults:** bind `127.0.0.1`. Behind a reverse proxy, disable response buffering on the stream route (e.g. nginx `proxy_buffering off`).
- **Security:** `POST` requires a static bearer token if one is configured. `GET` is open, restricted by CORS to a configured origin list. If deployment is local-only, auth and CORS may be disabled via config. Because browser `EventSource` cannot set headers, no auth is placed on `GET`.

### Configuration

| Key | Default | Meaning |
|---|---|---|
| `bind` | `127.0.0.1:8080` | Listen address |
| `cors_origins` | `[]` | Allowed browser origins |
| `api_token` | unset | Bearer token required for `POST` if set |
| `max_concurrent_sims` | `cores − 1` | Simulation thread cap |
| `result_ttl_secs` | `3600` | Cache lifetime for finished/failed results |
| `fixtures_dir` | `./fixtures` | TOML fixtures directory |
| `tick_cap` | 400,000 | Hard stop; exceeding it fails the match |

---

## 5. Fixtures

Fixtures are checked into git as TOML, loaded and **validated at startup**. A validation failure aborts boot with a precise error.

### 5.1 Contents
- A match registry: `match_id`, `seed`, home and away `team_id`, and per side `formation`, `mentality`, `tactic`.
- Teams: 11 players each (starting XI only; no bench).
- Players: stable `player_id`, name, squad number, `kind` (`outfield` | `goalkeeper`), 1–3 trained positions, attributes (integers).
- No `starting_condition` field: every player starts at full fitness.

### 5.2 Required fixture teams

| Tier | Approx. mean Quality |
|---|---|
| Near-equal A | ~100 |
| Near-equal B | ~100 |
| Strong | ~130 |
| Weak | ~70 |

A mirror match (same roster on both sides) must be expressible. Attribute values within a team vary (about N(100, 25), clipped to 30–180) so differences matter. Include a few deliberately out-of-position starters so the fit penalty has a visible effect.

### 5.3 Validation rules
- Exactly 11 players per team; `player_id` unique across all fixtures.
- Formation slots each filled exactly once.
- The GK slot holds a `goalkeeper` profile and no other slot may; a `goalkeeper` profile may not appear outside the GK slot.
- Every attribute is an integer in `1..=200`.
- 1–3 trained positions per player, valid for that player's `kind`.
- `match_id` unique; `seed` present; referenced teams exist.

### 5.4 Example

```toml
# fixtures/teams/t-alpha.toml
[team]
id = "t-alpha"
name = "Alpha FC"

[[players]]
id = "p-alpha-05"
name = "B. Centre"
number = 5
kind = "outfield"
trained_positions = ["DC"]
[players.attrs]
tackling = 110
marking = 105
positioning = 115
heading = 110
bravery = 100
passing = 80
dribbling = 60
crossing = 40
shooting = 45
finishing = 40
fitness = 95
strength = 110
aggression = 90
speed = 75
creativity = 55

# fixtures/matches/m-0001.toml
[match]
id = "m-0001"
seed = 42
home = "t-alpha"
away = "t-bravo"

[match.home_setup]
formation = "4-4-2"
mentality = "balance"
tactic = "possession"

[match.away_setup]
formation = "4-3-3"
mentality = "attack"
tactic = "counter_attack"
```

Mentality and tactic names must match the existing `Mentality` (`Attack`, `Balance`, `Defend`) and `Tactic` (`Possession`, `CounterAttack`, `HighPress`, `ParkTheBus`) enums; the TOML spellings above are the serialised forms to be fixed in the schema.

---

## 6. Player model

### 6.1 Profiles
```
PlayerProfile::Outfield   { defence(5), attack(5), physical_mental(5) }   // 15 attributes
PlayerProfile::Goalkeeper { goalkeeping(10), physical_mental(5) }         // 15 attributes
```
Physical & Mental is shared. Scale: integer `1..=200`, most players at 50 or above.

### 6.2 Attribute consumers

**Target: zero reserved attributes.** Every attribute gets an honest consumer. All reads go through a single accessor enum whose variants declare a consumer system or `Reserved`. A test fails if a variant is undeclared. Any attribute still reserved after Phase 4 must be listed in §6.6 with its reason.

**[Guess]** Several consumers below interpret vague wiki definitions (Marking, Bravery, Finishing, Creativity, and the four goalkeeper attributes marked †). They are proposals confirmed during Phase 4; a consumer that cannot be made honest moves to the reserved list rather than receiving a faked effect.

**Outfield: Defence**

| Attribute | Consumer |
|---|---|
| Tackling | Tackle success probability |
| Marking | Accuracy of keeping an assigned opponent / zonal cover (distance-to-mark error) |
| Positioning | Defensive positioning quality relative to ball and threats (target-position error) |
| Heading | Aerial duel outcome; header accuracy and power |
| Bravery | Willingness to contest duels, block shots, attempt challenges (decision weight) |

**Outfield: Attack**

| Attribute | Consumer |
|---|---|
| Passing | Pass accuracy (error spread) |
| Dribbling | Ball control while carrying; retaining possession against tackles |
| Crossing | Cross accuracy |
| Shooting | Shot power and accuracy |
| Finishing | Conversion quality close to goal and under pressure |

**Physical & Mental (outfield and goalkeeper)**

| Attribute | Consumer |
|---|---|
| Fitness | Stamina drain and recovery |
| Strength | Hold-off, tackle and aerial duel outcomes; (keeper) contested claims |
| Aggression | Press frequency and trigger distance; tackle propensity; (keeper) rush-out and claim propensity |
| Speed | Sprint speed (lower with the ball than without); (keeper) movement and rush-out speed |
| Creativity | Outfield: weight of key-pass / through-ball options. Goalkeeper †: weight of long / through-ball distribution options |

**Goalkeeping**

| Attribute | Consumer |
|---|---|
| Reflexes | Save probability on close and fast shots |
| Anticipation | Reading shots; positioning relative to the shooter |
| Rushing Out | Rush-out decision and angle-closing quality |
| Throwing | Hand-distribution accuracy and range |
| Kicking | Kick-distribution accuracy and range |
| Punching | Clearance of crosses by punching |
| Aerial Reach | Reach for shots and crosses |
| Communication † | Reduces positioning and marking error of defenders in the keeper's own half |
| Agility † | Lateral movement in goal and dive reach on saves |
| Concentration † | Probability of a lapse on a save, rising with time since the keeper's last touch |

### 6.3 Response curves
A single logistic response-curve layer maps each normalised attribute to a mechanic parameter. Curve parameters are tunable assumptions and live in the tuning struct (§11).

### 6.4 Quality
- Quality is **derived, never stored**: a position-weighted mean of the attributes relevant to the position, range `0..=200`. Weights are a code table per position group (defence, wide, midfield, attacking, striker, goalkeeper), tunable.
- Goalkeeper Quality uses only the 15 goalkeeper-profile attributes.
- Quality is read only by reporting and tests; **the simulation never reads it**.
- **Team strength** (used by acceptance tests and reporting) is computed from the eleven players' position-weighted Qualities as they play in their slots, not from a raw attribute average across profiles.
- Top Eleven's 180 cap is not reproduced.

### 6.5 Positions and fit
14 positions: `GK, DL, DC, DR, DML, DMC, DMR, ML, MC, MR, AML, AMC, AMR, ST`. The goalkeeper slot accepts only goalkeeper profiles and outfield profiles are never placed in goal, so the fit matrix covers the 13 outfield positions.

Grid used for distance (rows back to front; `ST` is a single central cell adjacent to `AML/AMC/AMR`):

```
row 5:            ST
row 4:  AML       AMC       AMR
row 3:  ML        MC        MR
row 2:  DML       DMC       DMR
row 1:  DL        DC        DR
```

Fit distance is the minimum Chebyshev distance on this grid from the slot to any of the player's trained positions. The penalty is a multiplier on all of the player's effective attributes while in that slot, so it feeds both Quality-in-slot and every mechanic.

| Distance | Multiplier (initial assumption) |
|---|---|
| 0 (trained position) | 1.00 |
| 1 | 0.92 |
| 2 or more | 0.75 |

### 6.6 Reserved list
Single source of truth: this section, mirrored by the accessor enum's `Reserved` variants. Each entry is documented "reserved, not used in the MVP match model" with a reason, and is never given a faked effect.

Current content: empty. Entries are added only if Phase 4 finds no honest consumer for an attribute.

Code and features that are reserved: `ManagerCommand`, its validators, `Substitute`, `replay --commands`, the offside stub, the foul-detection system (trace-only), and bench/substitution data.

---

## 7. Formations, tactics and team behaviour

### 7.1 Formations
Five existing formations, each an 11-slot set from the 14 positions:

| Formation | Slots |
|---|---|
| 4-4-2 | GK, DL, DC, DC, DR, ML, MC, MC, MR, ST, ST |
| 4-3-3 | GK, DL, DC, DC, DR, MC, MC, MC, AML, ST, AMR |
| 3-5-2 | GK, DC, DC, DC, ML, MC, MC, MC, MR, ST, ST |
| 4-2-3-1 | GK, DL, DC, DC, DR, DMC, DMC, AML, AMC, AMR, ST |
| 5-3-2 | GK, DL, DC, DC, DC, DR, MC, MC, MC, ST, ST |

These mappings are confirmed against the existing slot layouts during Phase 3. Today only a hardcoded 4-4-2 exists.

### 7.2 Mentality and tactic
- Configured before kickoff; no mid-match changes.
- Formation sets each player's home slot. Mentality shifts the team line and risk appetite. Tactic presets: Possession (short passes, patient build-up), Counter-attack (fast transitions, direct passing), High press (larger press trigger distance, higher stamina cost), Park the bus (compact block, low line).
- All of these are **numeric parameters** feeding the existing utility-AI considerations, not special-case code paths. Each must change observable behaviour and have a test (§13, A9).

### 7.3 Costs without fouls
- A failed tackle leaves the tackler out of position for a recovery period and lets the carrier skip past. Initial assumption: about 1.5 s (90 ticks).
- Pressing and tackling spend stamina, scaled by Aggression and Fitness.
- All cost constants live in the tuning struct (§11).
- Check: a HighPress team against a Possession team, same squads swapped home/away, over 200 seeds should not win more than 60% of matches. This is a **calibration check, not an acceptance gate**.

### 7.4 Aerial play
The ball stays 2D. A cross becomes a probabilistic aerial duel at the target point. Attacker Heading and Strength contend with defender Heading and Positioning; goalkeeper Aerial Reach and Punching apply where relevant. Events record `cross` and `header`. Implementation adds an airborne state with a landing point and flight time.

### 7.5 Coherence
Team shape stays coherent: lines move together, attackers support, defenders recover on possession change. Behaviour responds to possession, ball position, passing options, pressure and scoring opportunities.

---

## 8. Match rules and lifecycle

### 8.1 Phases
`PreKickoff → FirstHalf → HalfTime → SecondHalf → FullTime`, with in-play, dead-ball and restart sub-states.

### 8.2 Rules in the MVP
- Goals with scorer attribution, then a kickoff restart (see §8.5).
- Out of bounds: throw-in, goal kick, corner, resolved to a placement and a taker (opposite team of the last toucher).
- Kickoff at the start of each half and after goals.
- Half-time swaps ends. Second-half kickoff goes to the away team. Attack direction is per team and per half everywhere in the code; no `(105, 34)` constants remain.
- No fouls, offside, cards, penalties or free kicks.

### 8.3 Clock
- The display clock comes from the engine's match clock, never from raw tick division. Half-time ticks exist in the stream but are not match time.
- Added time is reported as `45+n` / `90+n`: `minute` is 45 or 90 and `added_minute` is n (first minute of added time is 1).

### 8.4 Added time
There are no fouls, injuries or substitutions, so added time is derived deterministically from dead-ball time only:
- Per half, sum the ticks the ball spent in `Dead` plus restart delays (goal celebration at the existing 60-tick kickoff delay, OOB restarts).
- Round up to whole minutes, capped at the existing 10 minutes.
- A `Referee` entity is spawned so the existing `added_time_calculation_system` and telemetry work unchanged.
- `full_time` reports `added_time_ticks: [h1, h2]`.

### 8.5 Goal attribution
- The scoring team is credited by goal position, so the final score always equals the count of `goal` events.
- `scorer` is the last `Play` toucher. If that player is on the conceding team, the event carries `own_goal: true` and no assister.
- `assister` is the previous `Play` toucher on the scoring team within the last 10 s of game time, if any; otherwise null.
- This requires a small two-deep touch history on `Ball`, which must also be mixed into the state hash.

---

## 9. Event model

### 9.1 Envelope (every event)

```json
{
  "seq": 128,
  "tick": 7421,
  "half": 1,
  "minute": 2,
  "added_minute": null,
  "type": "pass",
  "team": "t-alpha"
}
```

- `seq` starts at 0 and is strictly increasing and gapless across the match.
- `tick` is the global `Simulation` tick (not `elapsed_ticks`); events never contradict frames at the same tick.
- Events are immutable and append-only. The score is a fold over `goal` events. A `goal_disallowed` type with a `ref_seq` field is reserved in the schema documentation but not implemented.
- Players are identified by stable fixture `player_id`, never by engine entity ids.
- Clients drop any event with `seq` at or below the last one seen.

### 9.2 Event types and payloads

| `type` | Payload fields |
|---|---|
| `kickoff` | `taker`, `reason` (`match_start` \| `second_half` \| `after_goal`), `attack_direction` per team (on half starts) |
| `pass` | `from`, `to` (nullable if astray), `success`, `kick_tick`, `start: [x,y]`, `end: [x,y]` |
| `shot` | `shooter`, `on_target`, `outcome` (`goal` \| `saved` \| `blocked` \| `off_target` \| `post`) |
| `save` | `keeper`, `shooter`, `kind` (`catch` \| `parry` \| `punch`) |
| `goal` | `scorer`, `assister` (nullable), `own_goal`, `team`, `score: {home, away}` (running score after the goal) |
| `tackle` | `tackler`, `carrier`, `success` |
| `cross` | `player`, `target: [x,y]`, `outcome` |
| `header` | `player`, `kind` (`pass` \| `shot` \| `clearance`), `outcome` |
| `possession_change` | `from_team`, `to_team`, `cause` (`tackle` \| `interception` \| `out_of_bounds` \| `save` \| `loose_ball` \| `goal`) |
| `out_of_bounds` | `last_touch` (player), `exit_point: [x,y]`, `restart` (`throw_in` \| `goal_kick` \| `corner`) |
| `throw_in` / `goal_kick` / `corner` | `taker`, `at: [x,y]` |
| `half_time` | `score: {home, away}` |
| `full_time` | `score: {home, away}`, `added_time_ticks: [h1, h2]` |

Coordinates in event payloads use the same signed integer decimetre encoding as frames.

### 9.3 Rules
- **Every pass is logged.** A `pass` event is emitted at resolution (the ball's next acquisition, out-of-bounds, or goal): its `tick` is the resolution tick and `kick_tick` is when it was kicked. `success` means received by a teammate of the kicker; `to` is the intended receiver, null if the kick was astray.
- `possession_change` is emitted whenever the possessing team changes, with its cause.
- The final score must equal the count of `goal` events per team, and `goal.score` must equal the running count.
- Events are the source of truth for the score and timing.

### 9.4 Frames
Frames carry positions for animation: every 6th global tick (10 Hz), with no extra frames at event ticks. Clients interpolate; events carry the exact tick.

```json
{
  "tick": 7422,
  "ball": { "x": 523, "y": 341, "state": "free", "possessor": null },
  "players": [ { "id": "p-alpha-01", "x": 60, "y": 340 } ]
}
```

- Positions are signed integer **decimetres**, rounded half away from zero (maximum error 0.05 m). Signed because the ball can leave the pitch. Pitch is x 0–1050 and y 0–680 in this unit.
- Every frame carries `ball.state` and `ball.possessor`, because possession cannot be recovered reliably from 10 Hz positions.
- Pitch dimensions and origin are published in the manifest from the engine's pitch constants.
- **[Guess]** Payload size: about 6,000 frames × 22 players per 10 game-minutes before compression. Measure once the log exists; reduce rate or fields only if measurement requires it.

---

## 10. API contract

### 10.1 Endpoints

**`POST /match/{match_id}`** (requires `Authorization: Bearer <token>` if configured). Starts the match if not already started; never starts a second computation.

| Status | Meaning |
|---|---|
| 202 | Simulation started now |
| 200 | Already running or finished |
| 401 | Missing or invalid token |
| 404 | Unknown `match_id` |
| 503 | Concurrency cap reached; `Retry-After` set |

Body: `{ "match_id": "...", "status": "running" | "finished" | "failed" }`.

**`GET /match/{match_id}/stream`** (`text/event-stream`, gzip)

| Status | Meaning |
|---|---|
| 200 | Stream. If the simulation is still running, the connection stays open (keep-alive comment every 15 s) until the first message is ready |
| 204 | `Last-Event-ID` is at or beyond the final message |
| 404 | Unknown `match_id` |
| 409 | Match not started (call `POST` first) |
| 410 | Result expired (TTL); `POST` again to regenerate |

### 10.2 SSE messages
The SSE `id` is the message index, used by `Last-Event-ID` to resume after message *n*. The whole match is simulated up front and then sent instantly in order.

| id | `event:` | Content |
|---|---|---|
| 0 | `manifest` | `match_id`, `seed`, teams, formations, setups, player roster (`player_id`, name, number, position), pitch dimensions and unit, tick rates, attack direction per team per half, and the exact `from_tick`/`to_tick` of all 10 segments |
| 1–10 | `segment` | `{ segment, half, from_minute, to_minute, from_tick, to_tick, events[], frames[] }` |
| 11 | `full_time` | `{ score, event_count, state_hash }` |
| — | `error` | Replaces the remaining messages on failure: `{ code, message, last_good_tick }` |

### 10.3 Segmentation
Segments never span a half boundary. Each half has exactly 5 segments:
- First half: 0–10, 10–20, 20–30, 30–40, **40–45+added**.
- Second half: 45–55, 55–65, 65–75, 75–85, **85–90+added**.

The last segment of each half runs to the end of that half including its added time. A match always streams as 1 manifest + 10 segments + 1 `full_time` message.

### 10.4 Determinism and caching
- The same fixture (seed, rosters, setups) on the same binary and platform produces a **byte-identical uncompressed stream body**. Gzip output is excluded because it depends on library version and level.
- Evict-and-regenerate produces identical bytes (tested).
- Reconnecting without `Last-Event-ID` replays from message 0; with it, resumes after the given message.
- Failures (§12) are cached like successes so retries are deterministic.

---

## 11. Tuning struct

All tunable constants live in one committed config struct, edited only by the calibration step, with tuned values and batch results committed together:

- Attribute response-curve parameters (§6.3)
- Position-fit multipliers (§6.5)
- Shot error scale
- Tackle success base and recovery ticks
- Press stamina drain
- Pass error scale
- Stamina drain and recovery
- Aerial duel weights

Initial values are documented as assumptions. Nothing is tuned until it is measured, and nothing is measured until the ball moves.

---

## 12. Runtime invariants and failure handling

Checked in the simulation loop **in release builds too**:
- Score equals the count of `goal` events per team; every goal has a scorer.
- Match clock is monotonic; state transitions are legal (no kickoff mid-play, no play after `FullTime`).
- No dead-ball or stoppage window exceeds the configured maximum (stall detector, 30 s of game time).
- Ball and players stay in valid states; no invalid restarts.
- `FullTime` is reached before `tick_cap`.

On any violation or cap breach: the match status becomes `failed`; the stream sends an `error` event (reason code and last good tick) and closes; the seed is logged for reproduction; the failure is cached. The existing "log a warning and stop quietly" safety net in the CLI is not acceptable in server mode. Invariant violations and tick-cap breaches never silently truncate a match.

---

## 13. Acceptance criteria

All values are **provisional**, measured with the multi-seed harness and run manually. They are revised only with recorded measurements after the first calibration run.

"Mirrored" means the same squads run with home and away swapped, per seed.

### 13.1 Gating

| # | Criterion | Threshold |
|---|---|---|
| A1 | Matches reach `FullTime` with no stall | 50 / 50 seeds |
| A2 | Longest dead-ball window | ≤ 30 s of game time |
| A3 | Mean goals per match | between 2 and 4 |
| A4 | 0–0 draws | ≤ 15% of seeds |
| A5 | Stronger team (by team strength) wins | > 55% over 200 mirrored seeds |
| A6 | Determinism | identical event log and state hash for repeated seeds |
| A7 | Goals vs score | goal events equal the final score in every match |
| A8 | Event ordering | `seq` strictly increasing and gapless; ticks non-decreasing |
| A9 | Tactic signatures | one test per preset over several seeds on its signature statistic (pass length, average line height, press count) |
| A10 | Attribute sensitivity | a pure-function monotonic unit test per consumed attribute (direction of effect); full-match sweeps for Shooting, Finishing, Passing, Tackling and Reflexes only. The other 25 attributes are proven only at unit level, not by a measurable match effect |
| A11 | Attribute declarations | every attribute has a declared consumer or is explicitly reserved (test fails otherwise); reserved list expected empty |
| A12 | API behaviour | status codes, `Last-Event-ID` resume, evict-and-regenerate byte equality |

### 13.2 Non-gating (reported)
- Shots, corners and possession per match. Mirrored squads should show possession of 50 ± 3.
- Reference values (appendix): goals 2.7, shots 25, corners 10 per match.
- Press-dominance check (§7.3).
- Time-to-full-match compute per seed.

### 13.3 Calibration method
After the event log exists, run a fixed batch (four fixture tiers plus mirrors) and record goals, shots, pass counts, possession and win rates. Adjust only the tuning struct, in one commit with the batch results.

---

## 14. Testing approach

- Unit and component tests (restarts, rules, attribute accessors, fixture validation, API handlers, monotonicity per attribute) run in default `cargo test`.
- **Windowed smoke tests** run in default `cargo test`, capped at 3 game minutes per test, and are a deliberate exception to "no fast full-match tier". They assert that the ball crosses halfway, both teams touch it, and at least one shot occurs, so the goal-direction bug cannot return unnoticed.
- All full-match checks (A1–A6, full-match determinism, the five-attribute sweeps, mirrored-seed batches) are `#[ignore]`d and run manually through the harness.
- There is no CI and no nightly sweeps for the MVP. Consequence: no automatic run catches a full-match regression between manual runs. The §12 invariants plus the windowed smoke tests are the only automatic protection, and the invariants must be on in release builds.
- The headless harness records only aggregate metrics and does not serialise per-tick snapshots.

---

## 15. Build order

Each phase has an exit gate; nothing is built on a failed gate.

| Phase | Work | Exit gate |
|---|---|---|
| **0. Playable loop** | Per-team, per-half attack direction and half-time swap; formation slots replace `MoveToPosition(52.5, 34)`; free-ball retrieval fix (use decision traces to find the cause); OOB restart fix; spawn `Referee`; windowed smoke tests | Smoke tests pass **and** the playability gate below holds over 10 seeds |
| **1. Harness and event log** | Multi-seed headless runner with invariants (stall window, score = goal count, tick cap, determinism); structured events with stable ids; two-deep touch history on `Ball` (also hashed); `Referee`-based added time | Harness reports all §13 metrics; invariants fail matches explicitly |
| **2. API slice** | One fixture match streaming real events and frames through manifest, 10 segments and `full_time`, with `Last-Event-ID` resume | Contract proven end to end on a real stream |
| **3. Attribute and squad model** | 30 attributes and profile enum; 14-position grid, fit multiplier, TOML fixtures with startup validation; five formations as 11-slot sets; a single logistic response-curve layer with monotonic tests | Fixtures validate; accessor-enum test passes |
| **4. Mechanics** (ordered by statistical leverage) | (a) Shooting, Finishing, goalkeeper saves; (b) Passing, Tackling, Marking, Positioning; (c) Speed, Fitness, Strength; (d) Dribbling, Crossing, Heading, Bravery on the airborne state, plus the remaining goalkeeper attributes; (e) Aggression and Creativity, including goalkeeper variants | Every attribute has a consumer, unit-tested; reserved list finalised |
| **5. Tactics** | Mentality, tactic and formation parameters; tackle and press costs | A9 signature tests pass |
| **6. API hardening** | Registry, concurrency cap, TTL and eviction, auth/CORS, failure paths, update `client-protocol.md` | A12 passes |
| **7. Calibration** | Tune the tuning struct against §13 | Gating criteria met or thresholds revised with recorded measurements |

### Phase 0 playability gate (provisional)
Over 10 seeds, measured by the harness:
- Each team holds the ball for at least 30% of in-play ticks.
- The ball crosses halfway at least 20 times per half.
- The ball spends 25–75% of in-play ticks in each half of the pitch.
- At least 5 shots per match on average.

The 3-minute smoke test asserts the weaker version: the ball crosses halfway, both teams touch it, and at least one shot occurs.

Dependency flow: Phase 0 → 1 → (2 in parallel with 3) → 4 → 5 → 6 → 7. Phases 3 and 4 may interleave with calibration loops.

---

## 16. Decisions log

| Topic | Decision |
|---|---|
| Fouls, cards, penalties, free kicks, offside | Out of MVP; moved to post-MVP |
| Fidelity target | Plausible, varied outcomes by vision thresholds, not Top Eleven statistical parity; shots, corners and possession are non-gating metrics |
| Test tiers | Full-match checks manual and `#[ignore]`d; windowed smoke tests (≤ 3 game minutes) in default `cargo test`; no CI or nightly sweeps |
| Stream layout | Manifest + 10 half-aligned segments + `full_time`; every pass logged |
| Coordinates | Signed integer decimetres; `seq` from 0, gapless; global tick; byte-identity on the uncompressed body |
| Lifecycle | One thread per match, cap `cores − 1`, `503` over cap, `410` after TTL until re-POST, `409` before POST, `404` unknown id |
| Attributes | All 30 get consumers (target zero reserved); unit-level monotonicity for all, full-match sweeps for five |
| Quality | Derived, display/reporting only, never read by the simulation |
| Half-time | Teams swap ends; attack direction explicit per team and half |
| Added time | Derived from dead-ball time, rounded up to whole minutes, capped at 10 per half |
| Pass event | Emitted at resolution with `kick_tick` |
| Goal attribution | Credited by goal position; `own_goal` flag; assister from a two-deep touch history within 10 s |
| Fixtures | TOML; no `starting_condition`; integer attributes 1–200 |
| Tuning | One committed tuning struct |
| Frames | Pure 10 Hz, no extra event-tick frames; fit multipliers 1.0 / 0.92 / 0.75 |
| Press-dominance check | Calibration check, not an acceptance gate |

---

## 17. Risks and assumptions

| Item | Status |
|---|---|
| Goal-direction and restart/stall bugs are fixed first; nothing else is trustworthy until the Phase 0 gate passes | Certain |
| Several attribute consumers are interpretations (Marking, Bravery, Finishing, Creativity, goalkeeper Communication, Agility, Concentration, Creativity); some may move to the reserved list | Guess |
| Free-ball non-retrieval may be a second bug, undiagnosed until decision traces are read | Guess |
| 10 Hz frame payload size is unmeasured | Guess |
| Per-tick cost under live play is unmeasured (benchmark covered 6,000 ticks of a degenerate match) | Likely |
| Cross-platform float determinism is not guaranteed; the guarantee is for the same binary and platform | Likely |
| Parallel ECS system ordering must be deterministic for byte-identical output | Likely |
| An attribute scale of 1–200 compresses real differences if all players sit at 50–100; fixture authors must use real spread | Likely |
| Without fouls, press-heavy tactics risk dominating; mitigated by missed-tackle recovery and stamina cost, checked with the mirrored press test | Likely |
| No CI means full-match regressions can slip between manual runs | Certain |
| Fixture authoring effort: 4 teams × 11 players × 15 attributes by hand | Certain |
| All acceptance thresholds are provisional until the first calibration run | Certain |

---

## 18. Post-MVP

Fouls leading to free kicks, penalties and yellow/red cards; offside enforcement; substitutions and bench; mid-match tactic changes; injuries and VAR; CI, nightly sweeps and performance regression budgets; binary stream encoding; a real database; manager AI (the `sim-ai-manager` crate stays inert).

---

## Appendix: Top Eleven reference values (non-gating)

For reporting and sanity checks only; these are not acceptance criteria.

| Metric (both teams combined, per match) | Reference |
|---|---|
| Goals | 2.7 |
| Shots | 25 |
| Corners | 10 |
| Possession, mirrored squads | 50% |
