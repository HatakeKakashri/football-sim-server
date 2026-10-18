# 26: Roster cadence test asserts the exact evaluation count

**What to build:** Context (TDD audit): `test_cadence_full_roster_load_distribution` advances only the `MatchClock` component on the match entity, while `player_decision_system` reads the `MatchClock` resource, so the clock the system sees appears never to advance; the test then accepts any count from 60 to 300, which hides that. Confirm this on the first run, advance the resource (as `run_one_cadence_window` does), and assert the exact count: 22 players evaluated once per 6-tick window over 60 ticks is exactly 220, independent of how slots are distributed. Also remove the duplicated loop in `test_stagger_slot_distribution` (the `(1..=6)` check already implies every slot is non-empty).

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] Roster test asserts `== 220` and fails if any player is skipped or evaluated twice (state in the PR whether the old clock handling was in fact the cause of the loose range).
- [ ] Clock advancement in the test updates the resource the system reads.
- [ ] Stagger test has no redundant assertion loop; bounds unchanged.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
