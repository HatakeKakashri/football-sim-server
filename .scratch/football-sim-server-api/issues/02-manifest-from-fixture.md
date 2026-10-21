# 02: Build the manifest from the fixture, not hardcoded values

**What to build:** The manifest describes the fixture that was actually run. Today every team gets one hardcoded setup (4-4-2 / balance / possession) and roster ids are invented from spawn order ("provisional until Phase 3"), so fixture m-0001's away side (4-3-3 / attack / counter_attack) and m-0002's (defend / park_the_bus) are misreported. Because every frame also carries player id strings, a wrong roster id corrupts every frame, not only the manifest.

After this ticket, teams carry their real setups, and roster ids, names and numbers come from the fixture's players by roster index.

**Blocked by:** None (can start immediately)

**Status:** done

- [x] Each team's formation, mentality and tactic in the manifest comes from the fixture, individually for home and away.
- [x] Roster ids, names and numbers come from the fixture players by roster index; no invented ids remain.
- [x] Player ids in frames equal the roster ids in the manifest.
- [x] Tests pin manifest content for fixtures m-0001, m-0002 and m-0003, including at least one non-default setup per side.
- [x] A guard test fails if the world's spawn order and the fixture's player order ever diverge.
- [x] Segment layout is unchanged by this ticket.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Make the changes this ticket requires, and commit them as a single commit.
4. Provide that commit as a downloadable patch file (`git format-patch -1`).

## Comments

Implemented in commit `d6739bd` ("feat(sim-server): build the manifest from the fixture, not hardcoded values").

### What landed

- **New `PlayerIdentity` component** (`crates/sim-components/src/components.rs`):
  carries the fixture's `player_id` and the player's index in its side's
  roster. `sim-core`'s `create_match_from_fixture` stamps it on every player
  entity, so the world knows who each player is.
- **The manifest publishes the fixture's own setups**: `manifest_teams` reads
  `ValidatedMatch.home_setup` / `away_setup` per side, so m-0001's away side
  (4-3-3 / attack / counter_attack) and m-0002's home side
  (defend / park_the_bus) are reported correctly.
- **The roster rows are the fixture's own players** — id, name, squad number —
  in roster order, with the formation slot each occupies.
- **The frames name the same players**: the entity → id map comes from the
  world's `PlayerIdentity` values, so the invented `e-<entity bits>` fallback
  is gone.

### Evidence per criterion

| Criterion | Test | File |
|---|---|---|
| Each side's setup comes from the fixture | `manifest_publishes_both_sides_setups_from_the_fixture` (pins all three fixtures, each with a non-default side) | `crates/sim-server/src/api/manifest_tests.rs:134` |
| Roster ids, names and numbers come from the fixture by roster index | `manifest_roster_lists_the_fixture_players_by_roster_index` (all 22 rows of m-0001 pinned, m-0002 and m-0003 checked) | `crates/sim-server/src/api/manifest_tests.rs:202` |
| Player ids in frames equal the manifest roster ids | `frame_player_ids_are_the_manifest_roster_ids` (also asserts no id starts with `e-`) | `crates/sim-server/src/api/manifest_tests.rs:262` |
| Tests pin m-0001, m-0002 and m-0003, with a non-default setup per side | the three tests above cover all three fixtures | `crates/sim-server/src/api/manifest_tests.rs` |
| A guard test fails if the spawn order and the fixture order diverge | `world_spawn_order_matches_the_fixture_player_order` — compares the world's spawn order (by entity id) against the fixture's player order, per side, per fixture | `crates/sim-server/src/api/manifest_tests.rs:316` |
| Segment layout is unchanged | `git show --stat d6739bd` touches no render or segment file | — |

The guard test reads the world's spawn order from the entities themselves
(`spawned_players`, sorted by entity id) and compares it to the fixture's
player order, so a divergence in `create_match_from_fixture` fails it.

### Test results

```
cargo test -p sim-server --lib    →  136 passed; 0 failed; 8 ignored
```

The `sim-components` failure (`outfield_mechanics::tests::pass_error_spread_stays_within_curve_bounds`)
is pre-existing: it fails identically at `d6739bd~1`, and this ticket did not
touch `outfield_mechanics.rs`.
