# 28: Calibration — multi-seed runs, §13 metrics, tuning-struct commit

**What to build:** Per PRD §13.3, a fixed batch (four fixture tiers from ticket 19 plus mirrors) runs across 200 seeds for mirrored squads. The full §13 metric set is recorded: matches reaching FullTime, longest dead-ball window, mean goals per match, 0–0 rate, strong-side win rate over mirrored seeds, event ordering byte-identity on repeated seeds, goals-equal-score per match, seq tick order monotonicity, tactic signature statistics, attribute sensitivity. Five full-match sweep fixtures for `Shooting`, `Finishing`, `Passing`, `Tackling`, `Reflexes`. The §11 tuning struct is edited in one commit with batch results; results go to a committed calibration doc. §13.1 gating criteria must pass (A1–A12) or be revised with recorded measurements; the same measurements become the reference for post-MVP comparison. No feature work in this ticket — only measurement and parameter changes.

**Blocked by:** 25 (tactics land before calibration, since calibration depends on all §13 statics having variance), 27 (final §13.1 A12 acceptance test depends on hardening).

**Status:** ready-for-agent

- [ ] 200-seed mirror batch and tier batches run; metrics recorded in a committed calibration doc.
- [ ] Five-attribute (`Shooting`, `Finishing`, `Passing`, `Tackling`, `Reflexes`) full-match sweeps recorded.
- [ ] §11 tuning struct edited in one commit with batch results; no other production changes.
- [ ] All twelve §13.1 acceptance criteria (A1–A12) pass at provisional thresholds or thresholds are revised with measurements.
- [ ] `cargo test --workspace` green; `#[ignore]`d full-match checks run manually and recorded.
