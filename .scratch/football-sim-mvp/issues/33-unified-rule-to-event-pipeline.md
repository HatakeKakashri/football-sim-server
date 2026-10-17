# 33: Unified rule-to-event pipeline

**What to build:** Replace the six single-slot resources and nine order-sensitive emitters with one per-tick `RuleOutcome` record and a single Event module, collapsing the fragile chained ordering into one explicit pipeline.

**Blocked by:** 31, 32 (the MatchDriver invariant model and the kick-resolution event model should be stable before collapsing the signal layer).

**Status:** ready-for-agent

- [ ] The six resources (`GoalForEmit`, `OobRestartForEmit`, `KickoffForEmit`, `PendingShot`, `PendingPass`, `PendingTackle`, `PendingCross`) are replaced by one `RuleOutcome` record.
- [ ] One Event module drains `RuleOutcome` and emits to `EventLog`; the nine separate emitters are deleted.
- [ ] Event ordering is verified by existing tests or a new invariant; no full-match event-sequence regressions.
