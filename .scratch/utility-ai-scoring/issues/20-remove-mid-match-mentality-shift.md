# 20: Remove mid-match mentality shifts (PRD 7.2)

**What to build:** Context: PRD 7.2 and the comment in construct say mentality is configured before kickoff and does not change mid-match, yet `mentality_shift_system` is registered and, for example, switches both teams to Defend in any level game inside the last 10 minutes. Unregister it, delete it and its tests if nothing else uses it, and fix the contradicting comment. Record the decision as an ADR in `docs/adr/` (next free number; create the directory if absent) so it is not re-suggested: mentality is fixed at kickoff; live mentality would require rebuilding brains mid-match and is a separate effort.

**Blocked by:** 18, 19.

**Status:** ready-for-agent

- [ ] Test (through `Simulation`): a level game advanced into the last 10 minutes still has each team's `mentality` equal to its setup (use the existing clock-advance helpers; mark the test ignored only if it takes more than a few seconds).
- [ ] Test: a team trailing by 2 goals keeps its setup mentality.
- [ ] Manager crate tests that asserted shifts are removed; remaining manager tests pass.
- [ ] ADR written and linked from the PR.
- [ ] AGENTS.md mentions of mentality shifting, if any, are updated.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] Ignored playability gate re-run in release (`cargo test --release -p sim-core -- --ignored phase_0_playability_gate --nocapture`) and its four metrics reported in the PR; no regression versus the pre-change run.
