# 32: Kick resolution module

**What to build:** A `KickResolution` module that owns the entire kick/shot lifecycle—accuracy roll, outcome determination, and event signaling—so outcomes are resolved once rather than inferred post-hoc from possession, and the ~33 orphaned mechanic functions become internal seams.

**Blocked by:** 20, 21, 22, 23, 24 (the Phase 4 mechanic tickets must land first so `KickResolution` has stable functions to compose).

**Status:** ready-for-agent

- [ ] `KickResolution` module consolidates roll, outcome, and event input; `PendingShot`/`PendingCross`/etc. are written here, not scattered.
- [ ] Orphaned mechanic functions are either called internally by `KickResolution` or removed if dead.
- [ ] `fit_multiplier` is no longer hard-coded to `1.0`; shot outcome is determined in one pass.
- [ ] No post-hoc possession-based inference remains in `resolve_shot_events`.
