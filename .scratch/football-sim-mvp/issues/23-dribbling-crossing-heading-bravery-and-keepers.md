# 23: Dribbling, Crossing, Heading, Bravery + airborne state + goalkeeper distribution

**What to build:** The Phase 4 (d) mechanics. Outfield `Dribbling` is ball-control while carrying and retaining possession against tackles. `Crossing` is cross accuracy with a target point. `Heading` is aerial duel outcome — header accuracy and power — and contends at the airborne target point with the defender's `Heading` and `Strength`. `Bravery` is willingness to contest duels, block shots, and attempt challenges (decision weight). The airborne state (§7.4) is implemented: a cross becomes a probabilistic aerial duel at the target point; the ball has a landing point and a flight time; `cross` and `header` events are emitted. The remaining goalkeeper attributes — `Throwing`, `Kicking`, `Punching`, `Aerial Reach`, and the †-marked `Communication`, `Agility`, `Concentration` — land here: `Throwing` and `Kicking` are hand/kick distribution accuracy and range; `Punching` clears crosses by punching; `Aerial Reach` reaches for shots and crosses; `Communication` reduces defender positioning/marking error in the keeper's half; `Agility` is lateral movement / dive reach; `Concentration` raises save-lapse probability with time since last touch. All consumers read through the §15 curve and the §14 fit multiplier. Per-attribute monotonicity unit tests for each; `Dribbling`/`Crossing`/`Heading`/`Bravery` update `Reserved` list to empty if any had no honest consumer (PRD §6.6). Calibration considers these via the §11 tuning struct, not here.

**Blocked by:** 13, 15, 14, 20, 21 (mechanics foundation), 17 (formations canonicalised so aerial reach applies in correct slots).

**Status:** ready-for-agent

- [ ] Airborne state: ball has landing point and flight time; cross resolves as a probabilistic duel at the target point.
- [ ] `cross` and `header` events emitted per PRD §9.2 with `outcome`.
- [ ] `Bravery` decision weight raises contest / block / challenge probability (unit test).
- [ ] All five remaining goalkeeper attributes wired: `Throwing`, `Kicking`, `Punching`, `Aerial Reach` (tested); `Communication`, `Agility`, `Concentration` (unit-tested for direction).
- [ ] Reserved list (PRD §6.6) empty after this ticket unless a consumer cannot be made honest — any reservation documented with reason in the ticket `## Comments`.
- [ ] `cargo test --workspace` green.
