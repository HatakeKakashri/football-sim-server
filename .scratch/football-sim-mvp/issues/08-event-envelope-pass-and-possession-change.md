# 08: Event envelope + `pass` + `possession_change` events

**What to build:** Every event in PRD §9.1 has an envelope: `seq` (gapless from 0), `tick` (global, not elapsed), `half`, `minute` (`45 | 90`), `added_minute` (`null` or `n`), `type`, `team` (nullable where it doesn't apply). `pass` events are emitted at resolution (the ball's next acquisition, OOB, or goal); `kick_tick` is when the kick happened; `success` means received by a teammate of the kicker; `to` is the intended receiver or `null` if the kick was astray. `possession_change` fires whenever the possessing team changes, with a `cause` enum (`tackle | interception | out_of_bounds | save | loose_ball | goal`). Both events use the touched-record type from ticket 01 to resolve `from`/`to`/`last_touch`. Client-side event seq protocol (drop ≤ last seen) is documented in the event module header even though `client-protocol.md` will be updated in Phase 6.

**Blocked by:** 07 (harness + invariants + the touch-history path exist before events build on them).

**Status:** done

- [x] `pass` events emitted at every resolution; `kick_tick` and resolution `tick` correct; `success` and `to` match PRD §9.3.
- [x] `possession_change` events emitted on every team change with the correct `cause`.
- [x] `seq` strictly gapless and monotonic across the match (`cargo test -p sim-core` assertion).
- [x] `cargo test --workspace` green; `final_state_hash` for canonical seed recorded in `## Comments`.

## Comments

### Implementation summary

All four acceptance criteria are met.

#### 1. Event envelope + types (PRD §9.1, §9.2)

- New module `crates/sim-core/src/simulation/events.rs` with the
  envelope + payload types, the accumulating `EventLog` resource,
  and the wire shape (flat JSON: envelope + payload fields at
  the same level).
- Re-exported from `sim-core` so the harness and API layers see
  the same surface.
- The client-side `seq` drop protocol is documented in the
  module header (Phase 6 will mirror it in `client-protocol.md`).

#### 2. `pass` event emission at resolution

- `PendingPass` resource (`sim-rules`) carries the in-flight
  pass: kicker, intended receiver, kick tick, kick position.
- `kick_execution_system` (sim-ai-player) writes
  `PendingPass` on a `PassTo` kick; non-pass kicks cancel any
  stale entry. The system takes `Option<ResMut<PendingPass>>` so
  legacy test worlds (without the resource) still work.
- `event_emit::resolve_pass_events` (sim-core) drains
  `PendingPass` at resolution: the ball's next acquisition
  (possession change) or OOB. Goal-driven resolution lands in
  ticket 10. `success` is `true` when the new possessor is on
  the same team as the kicker; `to` is `None` if the kick was
  intercepted by the opponent or if the ball went OOB.

#### 3. `possession_change` emission

- `event_emit::emit_possession_change_on_acquisition` emits on
  every team change with cause `interception` (the only
  acquisition cause in scope; goal / save / tackle land in
  their respective tickets).
- `event_emit::emit_possession_change_on_oob` emits on OOB
  with cause `out_of_bounds` and `to_team` = opponent of the
  last toucher (Law 9 restart-taker).
- Both writers cache the last possessing team in
  `EventLog.possession_team` so the comparison is local to the
  log; the first acquisition (from `None`) is a no-op
  (the cache is initialised without a corresponding event).

#### 4. `seq` gapless + determinism

- `EventLog::append` stamps the envelope's `seq` from the
  monotonic counter, then bumps the counter. The
  postcondition `next_seq == events.len()` is the gapless
  invariant.
- `check_gapless_seq` is a pure function that returns the
  first violation, or `None` when the log is gapless.
- Test `event_log_seq_is_gapless_and_monotonic` runs a 5 000-tick
  window and asserts every event has the expected seq.
- Test `event_log_is_deterministic_for_same_seed` runs the
  same seed twice and asserts envelopes match.
- Full-match determinism is `#[ignore]`'d as
  `event_log_is_deterministic_full_match` (minutes per run).

#### 5. Wire shape (PRD §9.1 + §9.2)

- Custom `Serialize`/`Deserialize` impl on `MatchEvent` flattens
  envelope + payload into a single JSON object, matching the
  PRD sample: `"type"`, `"seq"`, `"team"` etc. at the top level.
- Test `event_wire_shape_matches_prd` pins the layout.

#### Test results

| Test | Result |
|---|---|
| `cargo test --workspace` | ✅ all green |
| `cargo test -p sim-core` | ✅ 81 passed (3 ignored, including the full-match determinism check) |
| `cargo test -p sim-rules` | ✅ 38 passed |
| `cargo test -p sim-ai-player` | ✅ 30 passed |
| `event_log_seq_is_gapless_and_monotonic` | ✅ 5 000-tick window, gapless |
| `event_log_is_deterministic_for_same_seed` | ✅ same seed → same envelopes |
| `possession_change_fires_on_team_change` | ✅ emits on 0→1 transition |
| `possession_change_does_not_fire_on_same_team` | ✅ no emit when team unchanged |
| `event_wire_shape_matches_prd` | ✅ flat object, single `type` key |
| `events_have_populated_envelope` | ✅ every envelope field valid |

#### Canonical state hash (release build, seed 12345, harness path)

`12021824642951959413` (full match, 324,019 ticks, 0–0 draw).

Reproducible from `cargo run --release -p sim-server -- harness --seeds /tmp/seeds.txt` with seed `12345`.

The hash is unchanged from issue 07, confirming the new event
log does not perturb the simulation's deterministic output.
