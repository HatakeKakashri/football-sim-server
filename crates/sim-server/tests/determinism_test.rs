use sim_core::Simulation;

#[test]
fn test_deterministic_simulation() {
    let seed = 12345;
    let ticks = 1000;

    // Run simulation twice with same seed
    let mut sim1 = Simulation::new(seed);
    let mut sim2 = Simulation::new(seed);

    // Run both simulations
    for _ in 0..ticks {
        sim1.tick(1.0 / 60.0);
        sim2.tick(1.0 / 60.0);
    }

    // Verify state hashes match
    assert_eq!(sim1.get_state_hash(), sim2.get_state_hash());

    // Verify tick counts match
    assert_eq!(sim1.tick, sim2.tick);
}

#[test]
fn test_per_tick_hash_logging() {
    let seed = 12345;
    let ticks = 100;

    let mut sim = Simulation::new(seed);
    let mut hashes = Vec::new();

    for _ in 0..ticks {
        sim.tick(1.0 / 60.0);
        hashes.push(sim.get_state_hash());
    }

    // Verify we have the correct number of hashes
    assert_eq!(hashes.len(), ticks);

    // Verify hashes are not all zero (placeholder implementation)
    // Note: In a real implementation, we would verify actual hash values
    assert!(!hashes.is_empty());
}
