//! Determinism tests: per-tick hash history equivalence across seeds.

#![allow(
    clippy::float_cmp,
    reason = "tests compare outputs of identical arithmetic against the inputs they were derived from"
)]

use crate::Simulation;

/// Strengthened from the prior "tick-aligned equality" form: collect every
/// per-tick hash from each simulation and assert the full 1000-element
/// histories are equal element-wise. This catches any drift that would
/// cancel out by the time the loop finished.
#[test]
fn test_per_tick_determinism() {
    for seed in [42u64, 0xCAFE_BABE_DEAD_BEEFu64] {
        let mut sim1 = Simulation::new(seed);
        let mut sim2 = Simulation::new(seed);

        let mut hashes1 = Vec::with_capacity(1000);
        let mut hashes2 = Vec::with_capacity(1000);

        for _ in 0..1000 {
            sim1.tick();
            sim2.tick();
            hashes1.push(sim1.get_state_hash());
            hashes2.push(sim2.get_state_hash());
        }

        assert_eq!(hashes1.len(), 1000);
        assert_eq!(hashes2.len(), 1000);
        assert_eq!(
            hashes1, hashes2,
            "seed {seed:#x}: per-tick hash histories diverged — determinism violated"
        );
    }
}
