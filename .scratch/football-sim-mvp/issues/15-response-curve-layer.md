# 15: Logistic response-curve layer

**What to build:** A single logistic-curve layer that maps a normalised attribute (`0..=200` → `0..=1`) and a list of tunables to a mechanic parameter, e.g. `scaled_shot_error(shot_attr, error_scale)`. Curve parameters (one record per attribute consumer) live in the tuning struct (§11) and are the single edit surface for §13.3 calibration. The layer is the only place that converts raw attribute integers to a numeric mechanic parameter; every consumer in Phase 4 (ticket 21 onwards) reads through it. Per-attribute monotonicity tests (`A10`) assert that for each consumed attribute, raising the attribute strictly and monotonically changes the mechanic parameter in the expected direction (e.g., higher Reflexes ⇒ higher save probability). Five attributes (Shooting, Finishing, Passing, Tackling, Reflexes) get a full-match sweep test fixture in Phase 7 ticket 29; the remaining 25 attributes get unit-level monotonicity tests only (PRD §13.1 A10).

**Blocked by:** 13 (the accessor supplies the input).

**Status:** ready-for-agent

- [ ] `curve(attribute: i32, params: &CurveParams) -> f32` logistic layer at one location.
- [ ] One `CurveParams` record per consumed attribute, all fields live in the tuning struct.
- [ ] Per-attribute monotonicity unit tests for all 25 unit-tested attributes (rising attr ⇒ direction-correct mechanic change).
- [ ] Phase 4 mechanic tickets consume curves via this layer — no raw attribute-to-number conversion anywhere else.
- [ ] `cargo test --workspace` green.
