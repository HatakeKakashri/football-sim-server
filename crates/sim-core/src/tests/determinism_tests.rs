//! Determinism tests: per-tick hash history equivalence, same-seed
//! equivalence over N ticks.

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
    let seed = 42u64;
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
        "Per-tick hash histories diverged — determinism violated"
    );
}

/// Two simulations constructed identically must hash identically for the
/// first N ticks. With RNG state mixed into the hash, identical seed →
/// identical RNG state ensures the hash is equal even after any number
/// of deterministic advances.
#[test]
fn test_same_seed_same_hash() {
    let seed = 0xCAFE_BABE_DEAD_BEEFu64;
    let mut a = Simulation::new(seed);
    let mut b = Simulation::new(seed);
    let n = 50u64;
    for _ in 0..n {
        a.tick();
        b.tick();
        assert_eq!(a.get_state_hash(), b.get_state_hash());
    }
}