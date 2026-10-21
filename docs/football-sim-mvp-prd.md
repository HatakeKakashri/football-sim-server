# Football Match Simulation Server: Product Specification

Status: final, decisions confirmed 17 Oct 2026. Source of truth for the Rust workspace (`sim-server` and its 10 sibling crates). Supersedes the earlier `Football Match Simulation: MVP PRD` and the provisional `client-protocol.md`; those are deleted in favour of this document. The earlier TypeScript engine is discarded and nothing from it is ported.

Product framing: this repository is a **single-match simulation server**. It produces one deterministic 11v11 football match end to end — player and ball movement, referee and clock, goals, out-of-bounds restarts, and offside *detection* — and exposes it over HTTP as a manifest plus immutable JSON segments so a separate UI repository can render the 2D pitch and consume match events. There is no manager game, no season, no transfer market, no training, no progression, no frontend, and no multiplayer in this repository; managers are static entities with predefined tactics and setups. The match engine is the product.

Behavioral reference: *Top Eleven – Be a Football Manager* (matchday experience only, not a requirement to reproduce proprietary details). Attribute reference: https://top-eleven.fandom.com/wiki/Player.

Confidence labels: **[Certain]** established from the codebase or an explicit decision; **[Likely]** strong inference, verify during implementation; **[Guess]** speculative, must be measured or confirmed.

---

## 1. Goal

Deliver a complete, believable, deterministic 11v11 football match simulation exposed as a server. The backend is authoritative for simulation state, player decisions, match rules and event generation, and serves a precomputed match to a separate frontend as a manifest and immutable JSON segments. The MVP is backend-only (no frontend in this repo).

A match must start, play through both halves with half-time and added time, and finish at a valid full time, with the final score equal to the count of goal events and the same seed always producing the same rendered bytes. The match must be *playable*: both teams hold the ball, the ball crosses halfway, shots occur, and goals are scored. A 0–0 match played in one corner is not acceptable.

## 2. Scope

### In scope
- Full match lifecycle: kickoff, first half, added time, half-time with side swap, second half, added time, full time.
- 30 player attributes across two profiles (Top Eleven wiki source of truth: 15 outfield + 15 goalkeeper, sharing 5 Physical & Mental — 25 unique attribute definitions), each with a declared consumer in the match model.
- 14 Top Eleven positions, a position-fit penalty, 5 formations, 3 mentalities, 4 tactic presets.
- 2D ball with aerial duels resolved probabilistically at the target point.
- Goals, kickoff restarts, out-of-bounds restarts (throw-in, goal kick, corner).
- Offside *detection* and its event logging (trace/signal only; see below).
- Structured, ordered event log including every pass.
- HTTP API serving a precomputed match as a manifest and immutable JSON segments.
- Fixtures in TOML, startup validation, determinism by seed, runtime invariants, a multi-seed measurement harness.

### Out of scope (this server, permanently or for the MVP)
- Fouls, cards, penalties, free kicks. Foul detection stays a trace-only signal that never stops play.
- Offside *enforcement* (stopping play, awarding a free kick, restarting). Only detection and event logging are in scope; enforcement is post-MVP.
- Substitutions, bench players, live manager commands, injuries, VAR.
- 3D ball physics.
- Frontend work (lives in a separate repository).
- Transfers, finances, training, progression, seasons, manager simulation (handled by other systems, not this server).
- CI, nightly sweeps, binary stream encoding, and any automatic full-match test tier.

`ManagerCommand`, its validators, `Substitute` (returns `NotImplemented`) and `replay --commands` stay compiled but are unreachable from the API. Replay by seed and setup is the supported reproducibility mechanism.

## 3. Baseline findings (current state)

| Finding | Confidence |
|---|---|
| `sim-server` is a CLI (`simulate`, `replay`, `benchmark`, `trace-filter`, `harness`, `serve`) with an HTTP layer. `client-protocol.md` is folded into §10 of this document. | Certain |
| Engine runs at 60 ticks/s; a full match is about 324,019 snapshots (`elapsed_ticks` resets each half, 162,000 per half). | Certain |
| Structured match event log exists (issues 08–10); every pass logged. | Certain |
| Perception hardcodes `goal_position = (105, 34)` for every player; the default brain hardcodes `ShootAtGoal(105, 34)` and `MoveToPosition(52.5, 34)`. The away team attacks −x, so its shots aim at its own goal. (Phase 0 target.) | Certain |
| Measured over 3 seeds (release build): ball in the away end (x > 88.5) about 99.8% of ticks; team 0 possession about 1.1 s per match; 3 possession changes; no tick with ball speed above 15 m/s; ball Free 58–96% of ticks; 0–0 in all three. | Certain (as measured) |
| The ball ending near x = 104.6 where the away goalkeeper holds it is explained by the goal-direction bug. A second bug leaving free balls unretrieved is suspected; decision traces should confirm. | Likely / Guess |
| Documented out-of-bounds restart bug: the OOB path marks the ball with a `RuleEvent` component the restart system does not see until the next tick, so the ball can stall at centre. Whether it is still present is unverified; the measured failure above is different. | Likely (unfixed) |
| The lifecycle resets both squads to a hardcoded 4-4-2 and does **not** swap ends at half-time, contrary to IFAB Law 7. Only one formation exists in code. (Phase 0/3 target: swap ends per IFAB.) | Certain |
| Added time is always 0: `added_time_calculation_system` consumes `Referee.stoppage_events`, but no `Referee` entity is spawned. (Issue 11 target.) | Certain |
| `Ball.last_touch` holds a single record, so there is no touch history. (Issue 10 target: two-deep touch history.) | Certain |
| Goals are credited by ball position (x ≤ 0 → away, x ≥ 105 → home), not by toucher. (Issue 10 target: scorer attribution.) | Certain |
| Tackle failure only multiplies speed by 0.3 for that tick; no recovery period and no stamina cost for `Tackle`/`Press`. There is no shot-save, aerial or goalkeeper mechanic. (Phase 4 target.) | Certain |
| Player quality is a single `Skill(f32)`. `Substitute` returns `NotImplemented`; manager commands affect only the home team. | Certain |
| Benchmark: 29,352 ticks/s, about 11 s of compute per match, over 6,000 ticks of a degenerate match. The `simulate` CLI took 63–72 s per match, likely JSON snapshot serialisation and disk writes. Re-measure under live play. | Certain / Likely |
| Workspace lints: `unsafe_code = "forbid"`, pedantic clippy; no CI exists. | Certain |
| **Calibration (issue 28):** over 50 seeds with the default `Simulation::new(seed)` profile (mid-range attributes, fixtures not wired into players), mean goals = 0.00, 100% draws, A3 and A4 FAIL. A5 (stronger team wins) not measurable. A1, A2, A6, A7, A8 PASS; A9, A11, A12 PASS; A10 PARTIAL (30/30 monotonic unit tests, five full-match sweeps). Root cause: ticket 19 produced four tiered TOML teams but did not wire those `PlayerProfile` values into `Simulation::new`. Closing this gap is a core deliverable (§13 A3/A4/A5, §15 Phase 0). | Certain |
| **Offside:** no offside detection exists today. Detection + event logging only is in scope (§2); enforcement is not. | Certain |

Implication: "50 of 50 seeds reach full time" would pass today with a 0–0 match played in one corner, so Phase 0 carries its own playability gate (§15).

## 4. Architecture and runtime

- **Stack:** `axum` + `tokio`, server code in the `sim-server` crate (and siblings), running as a Linux service.
- **Execution:** each `POST /match/{id}/start` runs that match's full simulation and render on a blocking thread (`spawn_blocking`). The simulation is CPU-bound and must not run on the async executor.
- **Concurrency cap:** default `cores − 1` simultaneous simulations, configurable. A start beyond the cap is queued FIFO and reports `202` with `state: queued`; it is never rejected for being over the cap. Expected load is 1–4 concurrent matches; this is a design assumption, not a requirement.
- **Results:** a ready match is held in memory for the life of the process. There is no TTL and no eviction. A deterministic failure is cached for the life of the process; a transient resource failure records a retry window (§10.5).
- **Restart behaviour:** all in-memory state is lost on process restart; fixtures reload. A client may re-`POST` and poll the manifest again; a segment request made before the new run is ready answers `409` (§10.7).
- **Performance target:** at most about 30 s release-mode wall time per full match with the full decision layer. The headless runner must not serialise per-tick snapshots.
- **Network defaults:** bind `127.0.0.1`. The service is request/response only: there is no long-lived stream and no response-buffering setting to make on a proxy.
- **Security:** `POST` requires a static bearer token if one is configured. `GET` is open, restricted by CORS to a configured origin list. If deployment is local-only, auth and CORS may be disabled via config. The bearer token must never be shipped in browser code: call `POST` from a server-side layer that holds the token, or leave the token unset for local use (§10.6).

### Configuration

| Key | Default | Meaning |
|---|---|---|
| `bind` | `127.0.0.1:8080` | Listen address |
| `cors_origins` | `[]` | Allowed browser origins |
| `api_token` | unset | Bearer token required for `POST` if set |
| `max_concurrent_sims` | `cores − 1` | Simulation thread cap; beyond it starts queue |
| `fixtures_dir` | `./fixtures` | TOML fixtures directory, loaded and validated at boot |
| `tick_cap` | 400,000 | Hard stop; exceeding it fails the match |

`fixtures_dir` and `tick_cap` are compiled defaults; `bind`, `cors_origins`, `api_token` and `max_concurrent_sims` are the `serve` command's runtime options.

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

### 5.5 Team input model
A match is addressed by `match_id` and resolved against the preloaded, validated fixture registry (§5). The server does not accept inline team/player data in the request body in the MVP; this keeps seeding deterministic and validation at startup. Inline submission is a post-MVP option.

## 6. Player model

### 6.1 Profiles
The Top Eleven player wiki (source of truth: https://top-eleven.fandom.com/wiki/Player) defines the player attributes. The simulation models **30 player attributes** arranged into two profiles, with the 5 Physical & Mental attributes shared between them:

```
PlayerProfile::Outfield   { defence(5), attack(5), physical_mental(5) }   // 15
PlayerProfile::Goalkeeper { goalkeeping(10), physical_mental(5) }         // 15
```

The wiki groups (25 unique attribute definitions; 30 slots across the two profiles because the 5 P&M are shared):

| Group | Count | Attributes |
|---|---|---|
| Defence | 5 | Tackling, Marking, Positioning, Heading, Bravery |
| Attack | 5 | Passing, Dribbling, Crossing, Shooting, Finishing |
| Physical & Mental (shared) | 5 | Fitness, Strength, Aggression, Speed, Creativity |
| Goalkeeping | 10 | Reflexes, Agility, Anticipation, Rushing Out, Communication, Throwing, Kicking, Punching, Aerial Reach, Concentration |

The simulation models exactly the wiki attributes; no attribute is invented. Scale: integer `1..=200`, most players at 50 or above.

### 6.2 Attribute consumers

**Target: zero reserved attributes.** Every attribute gets an honest consumer. All reads go through a single accessor enum whose variants declare a consumer system or `Reserved`. A test fails if a variant is undeclared. Any attribute still reserved after Phase 4 must be listed in §6.6 with its reason.

**[Guess]** The wiki provides a definition for every outfield attribute and for 5 of the 10 goalkeeping attributes (Reflexes, Rushing Out, Communication, Punching, Aerial Reach). The † mark on a cell flags a consumer that is a **proposed interpretation** because the wiki does not give a definition for that attribute (or, for Creativity, for that profile's use of a shared attribute). The † consumers are: **Agility, Anticipation, Throwing, Kicking, Concentration, and the goalkeeper use of Creativity**. They are confirmed during Phase 4; a consumer that cannot be made honest moves to the reserved list rather than receiving a faked effect. Every consumer below is written to match the wiki definition where one exists.

**Outfield: Defence** (all wiki-defined)

| Attribute | Wiki definition | Consumer |
|---|---|---|
| Tackling | how well a player executes a successful tackle on an opponent without committing a foul or injury | Tackle success probability (foul/injury aspect is N/A — fouls are out of scope) |
| Marking | how well a player can mark an opponent and use zonal marking | Accuracy of assigned marking and zonal cover (distance-to-mark error) |
| Positioning | how well a player positions himself to deal with attacking threats | Defensive positioning quality against attacking threats (target-position error) |
| Heading | how well a player is capable of heading the ball towards his target | Header accuracy towards target (aerial duel resolution) |
| Bravery | how much the player decides to mark and disarm the ball from the target | Willingness to engage — to mark tightly and dispossess (decision weight for duels, tackles, pressing) |

**Outfield: Attack** (all wiki-defined)

| Attribute | Wiki definition | Consumer |
|---|---|---|
| Passing | how accurately a player can pass the ball | Pass accuracy (error spread) |
| Dribbling | how well a player can control the ball when running with it | Ball control while running with it (retention against tackles) |
| Crossing | how well a player can place a well-aimed cross at the right time | Cross accuracy |
| Shooting | how well a player can place a strong, good shot | Shot power and placement/accuracy |
| Finishing | how well a player can react quickly and adapt to any offensive situation | Reaction and adaptation in attacking situations (finishing close to goal) |

**Physical & Mental** (shared by outfield and goalkeeper; all wiki-defined)

| Attribute | Wiki definition | Consumer |
|---|---|---|
| Fitness | how hard a player can exert himself on the pitch | Stamina / exertion capacity (drain and recovery) |
| Strength | how well a player exerts physical force to hold off opponents, win tackles and aerial balls | Hold-off, tackle and aerial duel outcomes; (keeper) contested claims |
| Aggression | how much a player asserts himself on the pitch (does **not** raise fouls/injuries) | Assertiveness — press frequency, tackle propensity; (keeper) rush-out and claim propensity |
| Speed | a player's speed on the pitch; faster without a ball than with it | Sprint speed (lower with the ball than without); (keeper) movement and rush-out speed |
| Creativity | how many opportunities a player will find to use his other abilities | Outfield: opportunity creation (key passes, through balls, chance creation). Goalkeeper †: weight of long / through-ball distribution options |

**Goalkeeping** (10; 5 wiki-defined, 5 †)

| Attribute | Wiki definition | Consumer |
|---|---|---|
| Reflexes | how well a goalkeeper can react to shots | Save probability on close and fast shots |
| Agility | *(not defined on the wiki)* † | Lateral movement in goal and dive reach on saves |
| Anticipation | *(not defined on the wiki)* † | Reading shots; positioning relative to the shooter |
| Rushing Out | how well a goalkeeper chooses when to rush out and closes down shooting angles | Rush-out decision and angle-closing quality |
| Communication | how well a goalkeeper organises his defenders during set pieces | Organises defenders during set pieces |
| Throwing | *(not defined on the wiki)* † | Hand-distribution accuracy and range |
| Kicking | *(not defined on the wiki)* † | Kick-distribution accuracy and range |
| Punching | how well a goalkeeper clears the ball with his hands | Clearance of the ball with the hands (punching) |
| Aerial Reach | how far a goalkeeper can stretch to get to shots on goal | Reach for shots on goal |
| Concentration | *(not defined on the wiki)* † | Probability of a lapse on a save, rising with time since the keeper's last touch |

### 6.3 Response curves
A single logistic response-curve layer maps each normalised attribute to a mechanic parameter. Curve parameters are tunable assumptions and live in the tuning struct (§11).

### 6.4 Quality
- Quality is **derived, never stored**: a position-weighted mean of the attributes relevant to the position, range `0..=200`. Weights are a code table per position group (defence, wide, midfield, attacking, striker, goalkeeper), tunable.
- Goalkeeper Quality uses only the attributes in the goalkeeper profile (the 10 Goalkeeping attributes and the 5 shared Physical & Mental), never the outfield-only Defence/Attack attributes.
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

Code and features that are reserved: `ManagerCommand`, its validators, `Substitute`, `replay --commands`, the offside stub (detection only — see §8.6), the foul-detection system (trace-only), and bench/substitution data.

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

## 8. Match rules and lifecycle

### 8.1 Phases
`PreKickoff → FirstHalf → HalfTime → SecondHalf → FullTime`, with in-play, dead-ball and restart sub-states.

### 8.2 Rules in the MVP (aligned with the IFAB Laws of the Game)

The subset implemented follows the IFAB Laws of the Game (latest edition). Laws not implemented are listed in §2 (Out of scope) and §8.7.

- **Law 10 — Scoring:** a goal is scored when the ball wholly crosses the goal line between the posts and under the crossbar (even if it touches a defender or the goalkeeper on the way). Own goals count; the conceding team is credited against. After a goal, play restarts with a kick-off taken by the team that conceded (Law 8). A goal may be scored directly from a kick-off (Law 8), a goal kick (Law 16) or a corner kick (Law 17). A goal may **not** be scored directly from a throw-in (Law 15); if the ball enters either goal directly from a throw-in, a goal kick or corner kick is awarded accordingly.
- **Law 9 — Ball in and out of play:** the ball is out of play when it has wholly crossed a boundary line (touchline or goal line) on the ground or in the air; otherwise it is in play. A goal is scored only when the ball wholly crosses the goal line between the posts under the crossbar.
- **Law 8 — Start and restart of play (kick-off):** a kick-off starts each half and restarts play after every goal; taken from the centre mark; opponents remain outside the centre circle until the ball is in play; the ball must move forward (towards the opponents' goal line) to be in play. The team that kicks off the first half is decided by the fixture (IFAB: a coin toss; here, by configuration). The team that did **not** kick off the first half kicks off the second half. After a goal, the conceding team kicks off.
- **Law 7 — Duration of the match:** two halves of 45 minutes (162,000 ticks at 60 Hz each). Half-time interval between halves. **Teams change ends at half-time** (IFAB: "The teams change ends during the half-time interval"), and attack direction is per team and per half everywhere in the code; no `(105, 34)` constants remain. Added time is indicated at the end of each half (§8.4).
- **Law 15 — Throw-in:** awarded when the ball wholly crosses the touchline; taken by the opponent of the player who last touched the ball before it crossed the line, at the point where it crossed, with both feet on or behind the touchline. The taker may not touch the ball again until another player has touched it.
- **Law 16 — Goal kick:** awarded when the ball wholly crosses the goal line (not between the posts) having last been touched by an attacker; taken from within the goal area by a defender (usually the goalkeeper); opponents remain outside the penalty area until the ball is in play.
- **Law 17 — Corner kick:** awarded when the ball wholly crosses the goal line (not between the posts) having last been touched by a defender; taken from the corner arc on the side where the ball crossed.
- Restart placement and taker: the restart type is determined by which line the ball crossed and which team last touched it (opposite team of the last toucher takes the restart). The taker and placement point follow the laws above.
- **Law 11 — Offside:** offside *position* and the offside *offence* are detected and logged as an `offside` event (§8.6, §9.2) for observability. There is no stoppage, no free kick and no change of possession; enforcement is out of scope (§8.7).
- No fouls, cards, penalties or free kicks (Laws 12–14). Foul detection stays a trace-only signal that never stops play.

### 8.3 Clock
- The display clock comes from the engine's match clock, never from raw tick division. Half-time ticks exist in the tick timeline but are not match time.
- Added time is reported as `45+n` / `90+n`: `minute` is 45 or 90 and `added_minute` is n (first minute of added time is 1).

### 8.4 Added time
There are no fouls, injuries or substitutions, so added time is derived deterministically from dead-ball time only:
- Per half, sum the ticks the ball spent in `Dead` plus restart delays (the 60-tick kickoff restart delay after goals and at half starts, and OOB restarts).
- Round up to whole minutes, capped at the existing 10 minutes.
- A `Referee` entity is spawned so the existing `added_time_calculation_system` and telemetry work unchanged.
- `full_time` reports `added_time_ticks: [h1, h2]`.

### 8.5 Goal attribution
- The scoring team is credited by goal position, so the final score always equals the count of `goal` events.
- `scorer` is the last `Play` toucher. If that player is on the conceding team, the event carries `own_goal: true` and no assister.
- `assister` is the previous `Play` toucher on the scoring team within the last 10 s of game time, if any; otherwise null.
- This requires a small two-deep touch history on `Ball`, which must also be mixed into the state hash.

### 8.6 Offside detection (detection + logging only)
- The referee detects offside positions and emits an `offside` event (§9.2) for observability and UI annotation.
- Detection does **not** stop play, award a free kick, or change possession. The match continues uninterrupted.
- Detection logic and its event payload are implemented in the rules layer; enforcement is post-MVP.

### 8.7 IFAB Laws not implemented (out of scope)
The following IFAB Laws are deliberately not implemented in this server (see also §2):
- **Law 3** — substitutions and bench players (no substitutions in the MVP).
- **Law 11** — offside *enforcement* (only detection/logging, §8.6).
- **Law 12** — fouls and misconduct (foul detection is trace-only and never stops play).
- **Law 13** — free kicks.
- **Law 14** — penalty kicks.
- **Laws 1, 2, 4, 6** — the field (Law 1, with its goal area, penalty area, corner arc and goal-post geometry), the ball (Law 2), players' equipment (Law 4) and the other match officials (Law 6) are reduced to the constants needed by the simulation (pitch 105 × 68 m, goal posts 7.32 m wide, 11 players per side, 45-minute halves). **Law 5 (referee) is implemented** for added time (§8.4) and offside detection (§8.6), but without the full range of on-field authority (no cautions, dismissals, or disciplinary action).

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
- Events are immutable and append-only. The score is a fold over `goal` events.
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
| `offside` | `player`, `team` (detection only; no stoppage) |
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

## 10. API contract

The match service has three routes. A match is simulated and rendered once, in full, before its manifest is served; the client then reads the manifest and fetches immutable segment bodies. There is no long-lived stream, no message resumption and no server push.

### 10.1 Endpoints and status matrix

- **`POST /match/{match_id}/start`** starts a match that is not started, or reports the state of one that is. It is idempotent and single-flight: a second start never runs the match twice. It requires `Authorization: Bearer <token>` when `api_token` is configured.
- **`GET /match/{match_id}/manifest`** reads a started match's manifest. It never starts work.
- **`GET /match/{match_id}/segments/{build_id}/{number}.json`** reads one immutable segment from the URL the manifest publishes. It never starts work.

| Match state | `POST /start` | `GET /manifest` | `GET /segments/{build_id}/{n}.json` |
|---|---|---|---|
| unknown `match_id` | `404` | `404` | `404` |
| known, not started | `202` queued or running | `409` `not_started` | `409` `not_ready` |
| queued | `202` + `Retry-After` | `202` + `Retry-After` | `409` `not_ready` |
| running | `202` + `Retry-After` | `202` + `Retry-After` | `409` `not_ready` |
| ready | `200` + manifest | `200` + manifest | `200` + segment |
| ready, revalidated | `200` + manifest | `304`, no body | `200` + segment |
| failed, deterministic | `500` | `500` | `409` `not_ready` |
| failed, transient | `503` + `Retry-After` | `503` + `Retry-After` | `409` `not_ready` |
| missing/invalid token on `POST` | `401` | — | — |

The segment route has one more row: a ready match answers an unknown build id or unknown segment number with `404` (`no_segment`). A stored body that cannot be decompressed answers `500` (`broken`).

One more row of the matrix is a condition on the request rather than a state of the match: a `GET /manifest` whose `If-None-Match` names the manifest's current validator is answered with `304` and no body, because the client already holds those bytes. The comparison is the weak one (RFC 9110 §13.1.2), so a weak `W/` validator and a list of validators both match; a client that names no validator, or names another one, is answered with the whole manifest. Only a ready match has a manifest to revalidate — every other state keeps its answer — and only the manifest route revalidates: `POST /start` always answers with the manifest.

Every JSON body carries a `state` field. The values are `queued`, `running`, `ready`, `failed`, `not_started`, `not_ready`, `no_segment`, `unknown`, `unauthorized` and `broken`.

| Response | Body |
|---|---|
| `200` on start/manifest | the manifest (below) flattened with `state: "ready"` |
| `304` | none; the client already holds the manifest |
| `202` | `{ state, match_id, retry_after_ms }`; `Retry-After` is 1 second and the body repeats the wait for cross-origin `fetch` |
| `409` | `{ state, match_id, message }` |
| `401` | `{ state: "unauthorized", match_id, message }` |
| `404` | `{ state: "unknown" \| "no_segment", match_id, message }` |
| `500`/`503` failure | `{ state: "failed", match_id, code, message, last_good_tick, retry_after_ms? }`; a `503` also carries `Retry-After` |

A failed match reports the runner's reason code and the last tick it reached. A deterministic failure (tick cap, invariant violation, panic) is cached for the life of the process: a later start reports `500` again and does not re-run it. A transient resource failure reports `503` with the remaining retry window; a start after the window schedules the run again.

### 10.2 Manifest fields

The manifest describes the whole rendered match. Its fields are:

| Field | Meaning |
|---|---|
| `build_id` | Content-derived SHA-256 over the fixture contents, the wire format version and every segment's uncompressed hash |
| `format_version` | Wire format version of the manifest and segment bodies (currently `1`) |
| `match_id` | Fixture match id |
| `seed` | Fixture seed |
| `teams` | Both sides' setups, home first: `{ id, formation, mentality, tactic }` |
| `roster` | 22 rows, home first: `{ id, team, name, number, position }`; `id` is the fixture `player_id` |
| `pitch` | `{ length, width, unit }`; currently `1050 × 680` decimetres |
| `ticks_per_second` | Match clock rate (60) |
| `frame_every_ticks` | Animation-frame interval (6, i.e. 10 Hz) |
| `attack_direction` | `{ home: [first_half, second_half], away: [...] }`, `+1` attacking increasing `x` |
| `result` | End-of-match `{ score: [home, away], event_count, state_hash }` |
| `segments` | Ordered segment rows (below) |
| `state` | Added to the `200` response only; always `"ready"` |

Each `segments` row is `{ segment, half, from_tick, to_tick, url, encoding, uncompressed_bytes, sha256, gzipped_bytes }`. `segment` counts from zero across the match, `url` embeds the build id, `encoding` is `gzip`, `sha256` is the hash of the uncompressed body and `gzipped_bytes` is the transfer size.

### 10.3 Segment bodies

Each segment body is `{ segment, half, from_minute, to_minute, from_tick, to_tick, events[], frames[] }`. Ticks and coordinates are absolute, so a segment can be read on its own. `events` are the §9 event envelopes whose tick falls in the segment; `frames` are the §9.4 animation frames in the same range. The end-of-match summary is in the manifest's `result`, not in a separate message.

### 10.4 Segmentation rule

A segment covers 60 seconds of match time: `SEGMENT_TICKS = 3,600`. Each half is cut from its own first tick at 3,600-tick boundaries, and the last segment of a half runs to the whistle and absorbs the remainder, including added time.

- A half yields 45 segments plus one per added minute.
- The segment count follows from the match and is never a constant. `m-0001`, whose halves each run into added time, yields 110 segments.
- No segment crosses a half boundary.
- The second half starts after the first half's last tick, so its segments are **not** globally aligned to multiples of 3,600 from tick 0.
- Events land in the segment that covers their tick. The two halves' segments cover their match time exactly once.

### 10.5 Determinism, caching and lifecycle

- The same fixture (seed, rosters, setups) on the same binary and platform produces the same `build_id`, the same manifest bytes and byte-identical uncompressed segment bodies. Gzip uses a fixed level and the pure-Rust backend, but a segment's identity and the build id are computed over the uncompressed body, so compression never changes a URL or a hash.
- A segment URL is immutable: the build id changes when the rendered bytes change, so an old URL never returns new content. Segment `200`s carry `Cache-Control: private, max-age=31536000, immutable` and `Vary: Accept-Encoding`; the body is `private`, not `public`, until a CDN exists.
- The manifest `200` carries `ETag: "<build_id>"` and `Cache-Control: no-cache`, and a client that revalidates it is answered rather than merely permitted: a `GET /manifest` whose `If-None-Match` names that validator gets `304` and no body, carrying the same `ETag` and the same `Cache-Control: no-cache`. The comparison is the weak one (RFC 9110 §13.1.2), so a weak `W/` validator and a list of validators both match. Every other answer of the start and manifest routes is `no-store`.
- The ready result lives in memory for the life of the process. There is no TTL, no eviction and no `410` path. A process restart forgets every rendered match: its manifest and segments answer `404`/`409` until the client starts it again.
- Deterministic failures are cached like successes, so retries are stable. A transient failure is retried only after its retry window.

### 10.6 Security and transport

- `POST` requires a static bearer token if one is configured (`Authorization: Bearer <token>`); missing or invalid → `401`.
- **The bearer token must never ship in browser code.** A browser bundle cannot keep a secret, and `GET` carries no token. If the browser must start matches, route `POST` through a server-side layer that holds the token. For local use, leave `api_token` unset.
- `GET` is open, restricted by CORS to the configured origin list. An empty origin list disables CORS for local-only deployment. An allowed origin is reflected in `Access-Control-Allow-Origin`; `OPTIONS` preflight is answered; every response varies on `Origin`; and `Retry-After` and `ETag` are exposed to cross-origin `fetch`.
- Segment bodies are stored gzipped and served at `Content-Type: application/json`. A client that accepts gzip receives `Content-Encoding: gzip`; every other client receives the decompressed body. Both decode to the same bytes; the manifest's `sha256` describes the decoded body.
- The default bind is `127.0.0.1:8080`. There is no stream route, so no reverse-proxy buffering exception is needed.

### 10.7 Client guidance for the unbuilt frontend

The frontend lives in a separate repository and is not built yet. The server-side contract is frozen here; the following guidance belongs with that client.

- **Sample payloads:** build and test against the real `m-0001` payloads in [`docs/api-samples/README.md`](api-samples/README.md) — the [`m-0001-manifest.json`](api-samples/m-0001-manifest.json) response body and the decoded [`m-0001-segment-0.json`](api-samples/m-0001-segment-0.json). The ticket-12 end-to-end test regenerates them.
- **Match ids come from configuration.** `m-0001`, `m-0002` and `m-0003` are the known fixtures; the client is configured with them. A fixture-listing endpoint is deferred (§10.8), so the client must not discover ids by probing the API.
- **Recovery after a server restart:** a segment request made before the new run is ready answers `409 not_ready`. The client must `POST /match/{id}/start`, poll the manifest until it is `ready` at the `Retry-After` pace (or `retry_after_ms` in the body), then retry the segment. A `404` means the id is not a fixture; a `404 no_segment` against a ready match means the build id or segment number is wrong.
- **Retention:** keep a sliding window of about three segments either side of the playhead. Parse segment JSON in a Web Worker into typed arrays; do not retain the parsed object graph for the whole match, and drop segments as they leave the window.
- **Wire-format revisit triggers:** revisit the wire format when the real gzipped total for a match exceeds 5 MB, or when parse time or retained heap breaks the client's budget. The current `m-0001` render is 6,500,009 gzipped bytes (110 segments, 6.20 MiB), so the size trigger is already met; the format has not been changed yet, and that decision is recorded rather than silently ignored.

### 10.8 Deferred until leagues

The MVP is single-match. The following are deliberately deferred until a league/season product exists: a CDN with `public` caching; TTL and eviction of rendered matches; shared storage between server instances; a fixture-listing endpoint; and brotli (gzip only today).

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

## 12. Runtime invariants and failure handling

Checked in the simulation loop **in release builds too**:
- Score equals the count of `goal` events per team; every goal has a scorer.
- Match clock is monotonic; state transitions are legal (no kickoff mid-play, no play after `FullTime`).
- No dead-ball or stoppage window exceeds the configured maximum (stall detector, 30 s of game time).
- Ball and players stay in valid states; no invalid restarts.
- `FullTime` is reached before `tick_cap`.

On any violation or cap breach: the match status becomes `failed`; the start and manifest routes report the failure's `code`, `message` and `last_good_tick` (`500` for a deterministic failure, `503` with `Retry-After` for a transient one); the seed is logged for reproduction; the failure is cached (deterministically) or given a retry window (transiently). The existing "log a warning and stop quietly" safety net in the CLI is not acceptable in server mode. Invariant violations and tick-cap breaches never silently truncate a match.

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
| A11 | Attribute declarations | every one of the 30 attributes has a declared consumer or is explicitly reserved (test fails otherwise); reserved list expected empty |
| A12 | API behaviour | status matrix, immutable segment URLs, gzip negotiation, build-id determinism and two-process byte equality |

### 13.2 Non-gating (reported)
- Shots, corners and possession per match. Mirrored squads should show possession of 50 ± 3.
- Reference values (appendix): goals 2.7, shots 25, corners 10 per match.
- Press-dominance check (§7.3).
- Time-to-full-match compute per seed.

### 13.3 Calibration status (issue 28)
Current measured state with the default `Simulation::new(seed)` profile (fixtures not yet wired into players): A1, A2, A6, A7, A8, A9, A11, A12 PASS; **A10 PARTIAL** (30/30 monotonic unit tests pass; full-match sweeps for the five leverage attributes only); **A3 FAIL (0.00 goals)**, **A4 FAIL (100% draws)**, **A5 GAP (not measurable)**. Root cause is the wiring gap, not an engine regression. Closing A3/A4/A5 with real tiered fixtures is a core deliverable (§15 Phase 0 / Phase 7). Thresholds remain provisional until the first successful calibration run.

### 13.4 Calibration method
After the event log exists, run a fixed batch (four fixture tiers plus mirrors) and record goals, shots, pass counts, possession and win rates. Adjust only the tuning struct, in one commit with the batch results.

## 14. Testing approach
- Unit and component tests (restarts, rules, attribute accessors, fixture validation, monotonicity per attribute) run in default `cargo test`.
- The server API has four test layers, described in `AGENTS.md`: the pure render step (segmentation, manifest shape, golden segment hashes, build id), the match service driven by fake runners (lifecycle, queue, retry window), the HTTP router driven by `tower::ServiceExt::oneshot` (status matrix, headers, CORS, gzip negotiation), and the ignored real-match test that spawns `football-sim serve` and drives a full `m-0001` through the real API.
- **Windowed smoke tests** run in default `cargo test`, capped at 3 game minutes per test, and are a deliberate exception to "no fast full-match tier". They assert that the ball crosses halfway, both teams touch it, and at least one shot occurs, so the goal-direction bug cannot return unnoticed.
- All full-match checks (A1–A6, full-match determinism, the five-attribute sweeps, mirrored-seed batches, and the real-match HTTP end-to-end test) are `#[ignore]`d and run manually through the harness or in `--release`.
- There is no CI and no nightly sweeps for the MVP. Consequence: no automatic run catches a full-match regression between manual runs. The §12 invariants plus the windowed smoke tests are the only automatic protection, and the invariants must be on in release builds.
- The headless harness records only aggregate metrics and does not serialise per-tick snapshots.

## 15. Build order

Each phase has an exit gate; nothing is built on a failed gate.

| Phase | Work | Exit gate |
|---|---|---|
| **0. Playable loop** | Per-team, per-half attack direction and half-time swap; formation slots replace `MoveToPosition(52.5, 34)`; free-ball retrieval fix (use decision traces to find the cause); OOB restart fix; spawn `Referee`; wire tiered fixtures into `Simulation::new` so the playability gate runs on real players; windowed smoke tests | Smoke tests pass **and** the playability gate below holds over 10 seeds |
| **1. Harness and event log** | Multi-seed headless runner with invariants (stall window, score = goal count, tick cap, determinism); structured events with stable ids; two-deep touch history on `Ball` (also hashed); `Referee`-based added time | Harness reports all §13 metrics; invariants fail matches explicitly |
| **2. API slice** | One fixture match rendered into a manifest and 60-second segments, served over the three HTTP routes with the status matrix | Contract proven end to end on a real fetched manifest and segments |
| **3. Attribute and squad model** | 30 player attributes (Top Eleven wiki source of truth) and the two profiles; 14-position grid, fit multiplier, TOML fixtures with startup validation; five formations as 11-slot sets; a single logistic response-curve layer with monotonic tests | Fixtures validate; accessor-enum test passes |
| **4. Mechanics** (ordered by statistical leverage) | (a) Shooting, Finishing, goalkeeper saves; (b) Passing, Tackling, Marking, Positioning; (c) Speed, Fitness, Strength; (d) Dribbling, Crossing, Heading, Bravery on the airborne state, plus the remaining goalkeeper attributes; (e) Aggression and Creativity, including goalkeeper variants; (f) AI ball pickup as an explicit intent replacing proximity-based acquisition (pickup refactor) | Every attribute has a consumer, unit-tested; reserved list finalised |
| **5. Tactics** | Mentality, tactic and formation parameters; tackle and press costs | A9 signature tests pass |
| **6. API hardening** | Registry, FIFO queue under the concurrency cap, transient-failure retry window, auth/CORS, failure paths, immutable segment caching, real-match end-to-end test | A12 passes |
| **7. Calibration** | Tune the tuning struct against §13, including wiring real fixtures (closes A3/A4/A5) | Gating criteria met or thresholds revised with recorded measurements |

### Phase 0 playability gate (provisional)
Over 10 seeds, measured by the harness:
- Each team holds the ball for at least 30% of in-play ticks.
- The ball crosses halfway at least 20 times per half.
- The ball spends 25–75% of in-play ticks in each half of the pitch.
- At least 5 shots per match on average.

The 3-minute smoke test asserts the weaker version: the ball crosses halfway, both teams touch it, and at least one shot occurs.

Dependency flow: Phase 0 → 1 → (2 in parallel with 3) → 4 → 5 → 6 → 7. Phases 3 and 4 may interleave with calibration loops.

## 16. Decisions log

| Topic | Decision |
|---|---|
| Fouls, cards, penalties, free kicks, offside *enforcement* | Out of MVP and out of this server; moved to post-MVP |
| Offside | Detection + event logging only; no stoppage |
| Fidelity target | Plausible, varied outcomes by vision thresholds, not Top Eleven statistical parity; shots, corners and possession are non-gating metrics |
| Product scope | Single-match simulation server only; no manager game, seasons, transfers, training, progression, or frontend in this repo |
| Manager model | Static managers with predefined tactics and setups; no live commands |
| Team input | Fixture registry by `match_id`; no inline submission in MVP |
| Test tiers | Full-match checks manual and `#[ignore]`d; windowed smoke tests (≤ 3 game minutes) in default `cargo test`; no CI or nightly sweeps |
| Wire layout | Manifest + data-driven 60-second half-aligned segments; every pass logged |
| Coordinates | Signed integer decimetres; `seq` from 0, gapless; global tick; byte-identity on the uncompressed body |
| Lifecycle | Blocking thread per match, cap `cores − 1`, FIFO queue over the cap, no TTL/eviction, `409` before ready, `404` unknown id |
| Attributes | All 30 player attributes (Top Eleven wiki source of truth) get consumers (target zero reserved); unit-level monotonicity for all, full-match sweeps for five |
| Quality | Derived, display/reporting only, never read by the simulation |
| Half-time | Teams swap ends per IFAB Law 7; attack direction explicit per team and half |
| Added time | Derived from dead-ball time, rounded up to whole minutes, capped at 10 per half |
| Pass event | Emitted at resolution with `kick_tick` |
| Goal attribution | Credited by goal position; `own_goal` flag; assister from a two-deep touch history within 10 s |
| Fixtures | TOML; no `starting_condition`; integer attributes 1–200 |
| Tuning | One committed tuning struct |
| Frames | Pure 10 Hz, no extra event-tick frames; fit multipliers 1.0 / 0.92 / 0.75 |
| Press-dominance check | Calibration check, not an acceptance gate |
| Data | Fictional, procedural players/teams; no licensing |
| Time model | Accelerated/on-demand single match; no seasons |
| Monetization | None in MVP |

## 17. Risks and assumptions

| Item | Status |
|---|---|
| Goal-direction and restart/stall bugs are fixed first; nothing else is trustworthy until the Phase 0 gate passes | Certain |
| The 0-goals gap (fixtures not wired into `Simulation::new`) must be closed before any calibration is meaningful | Certain |
| Several attribute consumers are interpretations (Marking, Bravery, Finishing, Creativity, and the goalkeeper-only attributes Communication, Agility, Concentration); some may move to the reserved list | Guess |
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

## 18. Post-MVP

Fouls leading to free kicks, penalties and yellow/red cards; offside *enforcement*; substitutions and bench; mid-match tactic changes; injuries and VAR; CI, nightly sweeps and performance regression budgets; binary stream encoding; a real database; manager AI (the `sim-ai-manager` crate stays inert); inline team submission; real-time scheduling; monetization.

## Appendix: Top Eleven reference values (non-gating)

For reporting and sanity checks only; these are not acceptance criteria.

| Metric (both teams combined, per match) | Reference |
|---|---|
| Goals | 2.7 |
| Shots | 25 |
| Corners | 10 |
| Possession, mirrored squads | 50% |
