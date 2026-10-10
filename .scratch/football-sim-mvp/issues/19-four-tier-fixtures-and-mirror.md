# 19: Four required-tier fixtures (Near-equal A/B, Strong, Weak) + mirror

**What to build:** Four team TOML files of 11 players × 15 attributes each, with the tier profile from PRD §5.2 (means ~100 / ~100 / ~130 / ~70; within-team variation ~N(100, 25) clipped to 30–180; integer `1..=200` so real differences matter). A mirror match expressible by giving the same two teams on both sides. Each team has exactly one goalkeeper profile in the GK slot; a couple of deliberately out-of-position starters per team so the §6.5 fit penalty has visible effect. One match file per fixture pair, named per `m-XXXX.toml` convention, pointing at two of the four teams with formation/mentality/tactic setup from PRD §5.4 example. Each fixture passes the ticket 18 validator; each tier produces a team-strength value within ±10 % of its target (unit test).

**Blocked by:** 18 (the validator must exist to check the fixtures).

**Status:** ready-for-agent

- [ ] `t-alpha.toml`, `t-bravo.toml`, `t-delta.toml`, `t-echo.toml` (or equivalent) committed; each carries 11 players with the tier spread and 1–3 trained positions.
- [ ] Mirror match fixture committed and loadable.
- [ ] Each fixture passes the ticket 18 validator; team-strength values within ±10 % of tier target.
- [ ] At least 2 out-of-position starters per team; the fixture's validity proves the §6.5 fit penalty will have a target to bite.
- [ ] `cargo test --workspace` green.
