//! State-hash stability and perturbation-detection tests for `Simulation`.

#![allow(
    clippy::float_cmp,
    reason = "tests compare outputs of identical arithmetic against the inputs they were derived from"
)]

use crate::Simulation;

/// Sanity check that the hash is a non-zero u64 that exercises every
/// hash arm (entities, Position, Velocity, Ball, Player, Team, Match).
/// We don't pin a numeric value (the hash is internal), only that it
/// is stable for identical inputs.
#[test]
fn test_state_hash_is_stable() {
    let seed = 0x1234_5678u64;
    let sim = Simulation::new(seed);
    let h1 = sim.get_state_hash();
    let h2 = sim.get_state_hash();
    assert_eq!(h1, h2, "Hash changed between identical reads");
    assert_ne!(h1, 0);
}

/// Two identically-constructed simulations should hash identically; a
/// perturbation to ONLY one (here: +1.0 on the ball's x) must make the
/// hashes diverge.
#[test]
fn test_hash_detects_state_change() {
    use sim_components::Position as PosComp;
    use sim_math::Vec2;

    let seed = 0xABCD_EF01_2345_6789u64;
    let mut a = Simulation::new(seed);
    let mut b = Simulation::new(seed);

    for _ in 0..10 {
        a.tick();
        b.tick();
    }
    let hash_before_a = a.get_state_hash();
    let hash_before_b = b.get_state_hash();
    assert_eq!(
        hash_before_a, hash_before_b,
        "Identically-constructed simulations diverged before perturbation"
    );

    // Perturb ball on sim a.
    let ball_a = a.ball_entity();
    {
        let mut em = a.world_mut().entity_mut(ball_a);
        let mut p = em.get_mut::<PosComp>().expect("ball has Position");
        p.0 = Vec2::new(p.0.x + 1.0, p.0.y);
    }

    let hash_after_a = a.get_state_hash();
    let hash_after_b = b.get_state_hash();
    assert_ne!(
        hash_after_a, hash_after_b,
        "Hash failed to detect +1.0 perturbation on ball x"
    );
}

/// PRD §8.5: the two-deep touch history on `Ball` is folded into the
/// state hash. A change to `touch_history` (a different previous touch
/// recorded) must change the hash deterministically.
#[test]
fn test_hash_detects_touch_history_change() {
    use bevy_ecs::prelude::Entity;
    use sim_components::{Ball, LastTouch, TeamId, TouchKind};

    let seed = 0xABCD_EF01_2345_6789u64;
    let mut a = Simulation::new(seed);
    let mut b = Simulation::new(seed);

    for _ in 0..10 {
        a.tick();
        b.tick();
    }
    let hash_before_a = a.get_state_hash();
    let hash_before_b = b.get_state_hash();
    assert_eq!(
        hash_before_a, hash_before_b,
        "Identically-constructed simulations diverged before perturbation"
    );

    // Replace sim `a`'s touch history slot 0 with a concrete touch that
    // sim `b` cannot have produced (different entity bits).
    let a_ball = a
        .world()
        .resource::<Ball>()
        .last_touch
        .and_then(|t| t.player)
        .unwrap_or(Entity::PLACEHOLDER);
    {
        let mut ball = a.world_mut().resource_mut::<Ball>();
        ball.touch_history[0] = Some(LastTouch::by_player(
            TeamId(0),
            Entity::from_bits(a_ball.to_bits().wrapping_add(0xDEAD_BEEF)),
            TouchKind::Play,
        ));
    }

    let hash_after_a = a.get_state_hash();
    let hash_after_b = b.get_state_hash();
    assert_ne!(
        hash_after_a, hash_after_b,
        "Hash failed to detect touch_history perturbation"
    );
}
