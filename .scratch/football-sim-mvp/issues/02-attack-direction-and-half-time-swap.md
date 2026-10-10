# 02: Per-team, per-half attack direction with half-time swap

**What to build:** `attack_direction` lives on each team as a per-half value (+1 for first half, −1 for second, with the second half flip enforced at the `HalfTime → SecondHalf` transition). Every site in the engine that hardcoded `(105, 34)` for "the goal" — perception's `goal_position`, the default brain's `ShootAtGoal(105, 34)`, and any other constant `105` / "home goal" reference — reads through the team's per-half attack direction so that the away team shoots at its own goal in the first half and at the opposite goal in the second. After this lands, no `(105, 34)` constant remains in match decision code, and a home team's perception at the same ball position differs from the away team's only by the attack-direction multiplier. Windowed smoke tests assert both teams cross halfway and both teams touch the ball within 3 minutes.

**Blocked by:** 01 (ball-touch refactor must land first because it changes state hashes; coupling these would muddy the diagnosis).

**Status:** ready-for-agent

- [ ] `attack_direction: Half → i8` lives on the team component and flips at `HalfTime`.
- [ ] Grep finds zero `105` or `(105, 34)` literals in match decision code (`sim-ai-player`, perception paths).
- [ ] Mirror-seed match (same squads, sides swapped) shows ball in attack-camp end of pitch matching the team's per-half direction (3-minute window).
- [ ] Windowed smoke test passes (PRD §14): ball crosses halfway, both teams touch the ball, at least one shot.
- [ ] `cargo test --workspace` green; `final_state_hash` for canonical seed recorded in `## Comments`.
