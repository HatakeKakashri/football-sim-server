# 02: Build the manifest from the fixture, not hardcoded values

**What to build:** The manifest describes the fixture that was actually run. Today every team gets one hardcoded setup (4-4-2 / balance / possession) and roster ids are invented from spawn order ("provisional until Phase 3"), so fixture m-0001's away side (4-3-3 / attack / counter_attack) and m-0002's (defend / park_the_bus) are misreported. Because every frame also carries player id strings, a wrong roster id corrupts every frame, not only the manifest.

After this ticket, teams carry their real setups, and roster ids, names and numbers come from the fixture's players by roster index.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Each team's formation, mentality and tactic in the manifest comes from the fixture, individually for home and away.
- [ ] Roster ids, names and numbers come from the fixture players by roster index; no invented ids remain.
- [ ] Player ids in frames equal the roster ids in the manifest.
- [ ] Tests pin manifest content for fixtures m-0001, m-0002 and m-0003, including at least one non-default setup per side.
- [ ] A guard test fails if the world's spawn order and the fixture's player order ever diverge.
- [ ] Segment layout is unchanged by this ticket.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Make the changes this ticket requires, and commit them as a single commit.
4. Provide that commit as a downloadable patch file (`git format-patch -1`).
