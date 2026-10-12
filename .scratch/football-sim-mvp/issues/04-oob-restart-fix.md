# 04: Out-of-bounds restart reads the same tick

**What to build:** OOB detection marks the ball and the restart system reads that mark within the same tick, eliminating the deferred-command one-tick delay that today stalls the ball at the centre circle. The fix follows the gotcha in the project's gotchas list: route the "ball is out of bounds" signal through a `Resource` (e.g., `PendingRestart`) rather than a component, so consumers in the same system-run step see the signal. The restarter then resolves to the correct restart (throw-in / goal-kick / corner) and placement+ taker using the team captured on the touched record from ticket 01. Same-tick reading is verified by a unit test that runs one rule step then one restart step on the same world.

**Blocked by:** 01 (restart taker selection depends on the team denormalised on the touched record).

**Status:** ready-for-human

- [x] OOB signal flows through a `Resource`, not a deferred component; the restarter sees it within the same tick.
- [ ] Restart selection (throw-in / goal-kick / corner) and taker selection match PRD §8.2 and the team from ticket 01.
- [ ] Stall window invariant passes (≤ 30 s of game time per §12) over 10 seeds in the Phase 1 harness when that lands; before Phase 1, a manual run over 5 seeds shows no centre-circle stalling.
- [x] `cargo test --workspace` green; `final_state_hash` for canonical seed recorded in `## Comments`.

## Comments

1. **Signal path** (done, not compiled): `out_of_bounds_system` now writes a `PendingOutOfBounds` resource (`Option<OutOfBoundsEvent { oob_type, last_touch }>`) instead of inserting `RuleEvent::OutOfBounds` on the ball via `Commands`; `restart_system` takes it. The resource is inserted by `Simulation::create_match`; any hand-built world that runs either system must insert it. The ball no longer carries a `RuleEvent::OutOfBounds` component (nothing outside the tests read it); the enum variant is kept for ticket 09's event stream.
2. **Same-run test**: `test_oob_restart_reads_signal_same_run_without_sync_point` (goals_tests.rs) chains `out_of_bounds_system -> restart_system` with `auto_insert_apply_deferred: false`, so it fails if the signal depends on deferred commands. Existing OOB tests now assert on the resource instead of the entity component.
3. **Classification/taker unchanged**: PRD §8.2 was not available when this was written, so the checkbox is left open. Taker = team opposite `last_touch.team` (correct for all three restart types). Classification is still purely positional; note that a ball last touched by the attacker that crosses the goal line outside the goal mouth is currently a `Corner` taken by the defenders, whereas Law 9/16 gives a goal kick. Needs a human decision against §8.2 before changing.
4. **Premise check**: in the registered schedule, `.chain()` already auto-inserts a sync point after `out_of_bounds_system` (it has `Commands`), so the old path may already have been same-tick there. The resource removes the dependence on that sync point; it may not by itself explain the centre-circle stall. The manual 5-seed run is still to do.
5. **Verified after a follow-up review pass.** Toolchain available; results below.

   - `cargo test --workspace --lib` — **231 passed, 0 failed, 3 ignored** (the three pre-existing `#[ignore]`'d tests; unchanged).
   - `cargo clippy --workspace --all-targets -- -D warnings` — clean. (The §5 review flagged a `pub` field on `pub struct PendingOutOfBounds`; the field is now `pub(crate)`-private with three `pub const fn` accessors — `set`, `take`, `get` — per CODING_STANDARDS §5. The struct itself stays `pub` so `Simulation::create_match` can `insert_resource(PendingOutOfBounds::default())`. All 8 test sites updated to use `.get().copied()` / `take()`.)
   - `cargo run --release -p sim-server -- simulate --seed 12345 --ticks 1000` — final `state_hash = 13289728479385266083`. Matches ticket 02/03's canonical value.
   - `cargo run --release -p sim-server -- simulate --seed 12345 --full-match` — tick 324019, `FullTime`, score `[0, 0]`, final `state_hash = 7678863417554148076`. Matches ticket 02's canonical full-match value.

   Same-seed determinism is preserved across the component→resource refactor (as predicted by the unchanged tick-by-tick ball state).
