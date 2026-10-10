# 18: TOML fixture schema + startup validator

**What to build:** A TOML loader for the PRD §5 shape: `fixtures/teams/<id>.toml` (team, players with attributes, trained positions, kind) and `fixtures/matches/<id>.toml` (match_id, seed, sides, per-side formation/mentality/tactic using the serialised names from §5.4). Loading happens at server startup; validation per §5.3 rules (exactly 11 per team, `player_id` unique across all fixtures, formation slots filled exactly once, GK slot holds GK-kind and only GK-kind, every attribute integer `1..=200`, 1–3 trained positions per player valid for the player's kind, `match_id` unique, `seed` present, referenced teams exist) — any failure aborts boot with a precise error pointing at the file and rule. The fix that corrected the away-team shooting-into-its-own-goal bug must not regress: the per-team per-half attack direction wires through the fixture load. The `client-protocol.md` is not updated in this ticket (Phase 6 ticket 28).

**Blocked by:** 13, 14, 17 (Profile enum, position grid, formation registry).

**Status:** ready-for-agent

- [ ] Fixture loader loads and validates every file in `fixtures/` on startup.
- [ ] Each §5.3 rule has a unit test with a fixture that fails that rule alone.
- [ ] A validator failure aborts boot with file path, rule, and offending value; no match starts against a partial fixture set.
- [ ] `match_id` and `player_id` uniqueness enforced across all loaded fixtures.
- [ ] `cargo test --workspace` green.
