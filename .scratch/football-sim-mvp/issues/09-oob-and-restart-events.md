# 09: `out_of_bounds`, `throw_in`, `goal_kick`, `corner` events

**What to build:** The OOB family of events: `out_of_bounds` (carries `last_touch` player and `exit_point`); the appropriate restart event (`throw_in`, `goal_kick`, or `corner`) is emitted with the `taker` and `at` placement. Taker selection uses the team denormalised on the touched record (tickets 01 + 04); placement uses the same logic the restart system already computes. The `cause` of the corresponding `possession_change` (from ticket 08) is `out_of_bounds`. Ordering invariant: `out_of_bounds` precedes its restart event by exactly one `seq` and the restart event's `tick` is the same as the out-of-bounds tick (single-tick resolution now that the OOB deferred-component bug is fixed in ticket 04).

**Blocked by:** 08 (envelope + `possession_change` with `out_of_bounds` cause must already exist).

**Status:** done

- [x] OOB family emitted in the correct sequence across every restart scenario in §8.2.
- [x] Restart `taker` is on the team opposite to `last_touch`'s team per the touched record.
- [x] `seq` and `tick` invariants hold across the OOB family (`cargo test` assertion).
- [x] `cargo test --workspace` green; `final_state_hash` for canonical seed recorded in `## Comments`.

## Comments

### Implementation summary

#### 1. Wire shape (PRD §9.1, §9.2)

- Two new `MatchEvent` variants in `sim-core/src/simulation/events.rs`:
  - `OutOfBounds { envelope, payload: OutOfBoundsEventPayload }` —
    payload carries `last_touch: Option<LastTouch>` (the toucher), the
    `restart` discriminator (`ThrowIn` / `GoalKick` / `Corner`), and
    `exit_point: [f32; 2]` (the ball position the OOB detector
    observed, i.e. the clamped boundary coord).
  - `Restart { envelope, payload: RestartEventPayload }` — payload
    carries `taker: Option<PlayerId>` (the chosen taker on the
    opposite team), `at: [f32; 2]` (the placement from
    `calculate_oob_restart_position`), and `kind: RestartKind`.
- New shared type `RestartKind` (`ThrowIn` / `GoalKick` / `Corner`)
  with `from_oob(BallOutOfBoundsType) -> Self` for the 1:1 mapping
  from the rule-side enum to the event-side enum.
- Custom `Serialize`/`Deserialize` on `MatchEvent` extended to
  flatten both new variants to the PRD §9.1 wire shape (envelope
  fields + payload fields at the same JSON level). Envelope `type`
  is `out_of_bounds` and `restart` respectively; payload `kind`
  distinguishes the three restart flavors on the wire.

#### 2. `OobRestartForEmit` resource (sim-rules)

- New `Resource` carries the resolved OOB data across the
  `restart_system` → `emit_oob_and_restart_events` boundary.
- `restart_system` writes it after computing the taker team (Law 9:
  opposite of `last_touch.team`) and the restart placement
  (existing `calculate_oob_restart_position`). The ball's clamped
  position at OOB detection time is captured as `exit_point` before
  `restart_system` moves the ball to its restart position.
- Inserted by `Simulation::create_match` (hand-built test worlds
  insert it explicitly; `restart_system`/`emit_oob_and_restart_events`
  take it as `Option<ResMut<…>>` to tolerate worlds that omit it).

#### 3. Event emission

- New system `emit_oob_and_restart_events` in
  `sim-core/src/simulation/event_emit.rs`. Runs in the `Rules` set
  after `restart_system` has resolved the restart and before
  `emit_possession_change_on_oob`. Emits the `out_of_bounds` event
  first (gets the next seq, e.g. N), then the `restart` event
  (gets seq N+1) — two `EventLog::append` calls in immediate
  succession, so the ticket-09 ordering invariant
  (`out_of_bounds.seq + 1 == restart.seq`) is structurally
  guaranteed. Then clears `OobRestartForEmit`.
- The two events share the same envelope `tick` (read from
  `CurrentTick` in the emitter).

#### 4. Wire payload mapping

- `out_of_bounds.last_touch` carries the `LastTouch` the OOB
  detector captured (matches `possession_change.last_touch`).
- `out_of_bounds.team` (envelope) = the last-touch team.
- `out_of_bounds.exit_point` = ball position at OOB detection
  (the clamped boundary coord).
- `out_of_bounds.restart` = `throw_in` / `goal_kick` / `corner`.
- `restart.taker` = `PlayerId::from_entity(taker_entity)`; `None`
  only when the taker entity is `Entity::PLACEHOLDER` (sparse test
  worlds).
- `restart.team` (envelope) = taker team (Law 9: opposite of the
  last-touch team).
- `restart.at` = placement from `calculate_oob_restart_position`.
- `restart.kind` = `throw_in` / `goal_kick` / `corner` (mirrors the
  `out_of_bounds.restart` field).

#### 5. Tests

| Test | Result |
|---|---|
| `out_of_bounds_event_payload_round_trip` | ✅ wire shape (`type=out_of_bounds`, `restart=throw_in`, `exit_point`, `last_touch`) |
| `restart_event_payload_round_trip` | ✅ wire shape (`type=restart`, `kind=throw_in`, `at`, `taker`) |
| `oob_then_restart_have_consecutive_seqs_and_same_tick` | ✅ `restart.seq = out_of_bounds.seq + 1`, same tick |
| `restart_kind_from_oob_is_identity_mapping` | ✅ throw_in/goal_kick/corner round-trip |
| `oob_emits_out_of_bounds_and_restart_pair` | ✅ back-to-back in a real `Bevy` `Schedule`, throw-in scenario |
| `restart_taker_is_on_team_opposite_last_toucher` | ✅ all three restart types: taker_team = opposite of last_touch.team |
| `oob_and_restart_events_keep_seq_gapless` | ✅ corner scenario: log remains gapless |

### `cargo test` results

- `cargo test --workspace --lib` — **278 passed, 0 failed, 4 ignored**
  (the same pre-existing `#[ignore]`'d tests; none added by this
  ticket).
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo test --release -p sim-core --lib -- --ignored
  event_log_is_deterministic_full_match` — passes; two full matches
  with seed 12345 produce identical event envelopes.
- `cargo test --release -p sim-server -- harness --seeds /tmp/seeds.txt`
  for seed 12345: full match reaches `FullTime` at tick 324,019,
  score `[0, 0]`, no invariant violations, determinism agreed.

### Canonical `final_state_hash` (release build, seed 12345)

- Full match (324,019 ticks, `FullTime`, score `[0, 0]`):
  `12021824642951959413` — matches ticket 08's canonical value;
  the new event log does not perturb the simulation's
  deterministic output.
- 1,000-tick window: `17116307821950037976`.

The phase-0 playability gate (`phase_0_playability_gate`) was
already failing before this ticket (per PRD §3 baseline: team 0
possession ≈1.1 s per match, ball in away end 99.8% of ticks). It
remains failing in the same way after this ticket — out of scope
for 09.
