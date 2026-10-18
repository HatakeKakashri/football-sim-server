# 28: Perception behaviour tests

**What to build:** Context (TDD audit): `perception_system` (about 200 lines: nearby teammates and opponents with radius and limits, nearest-first ordering, pitch bounds, home slot, goal position per team and half) has one test, which checks entity ids are real. Add behaviour tests at the `perception_system` seam in an installed world. Read the radius and limit constants in `perception.rs` and test at their boundaries rather than restating them.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] An opponent just inside the radius appears in `nearby_opponents`, one just outside does not.
- [ ] With more entities than the limit, exactly the limit is kept and they are the nearest, in ascending distance order (ties resolved deterministically).
- [ ] Teammates never appear in `nearby_opponents` and vice versa; the player is not its own teammate.
- [ ] `goal_position` is the right goal for the home team and the left goal for the away team in the first half, and swaps after the half-time swap.
- [ ] Pitch bounds reflect the player's position (a player on the touchline reports zero distance to that side).
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
