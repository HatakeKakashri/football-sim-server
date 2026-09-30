//! Integration tests for the `Simulation` pipeline: single-tick
//! isolation, ball motion under physics, and exported-state sync
//! (regression: `get_state` must read Position components, not the
//! removed `Player.position` field).

#![allow(
    clippy::float_cmp,
    reason = "tests compare outputs of identical arithmetic against the inputs they were derived from"
)]

use bevy_ecs::prelude::Entity;

use crate::Simulation;
use sim_components::Position;

#[test]
fn test_pipeline_isolation_single_tick() {
    let mut sim = Simulation::new(42);

    sim.tick();

    let snapshot = sim.get_state().expect("get_state failed");

    assert_ne!(
        snapshot.state_hash, 0,
        "State hash is zero — systems did not run"
    );
    assert_eq!(snapshot.players.len(), 22, "Expected 22 players");

    let bx = snapshot.ball.position[0];
    let by = snapshot.ball.position[1];
    assert!((0.0..=105.0).contains(&bx), "Ball x {bx} out of pitch");
    assert!((0.0..=68.0).contains(&by), "Ball y {by} out of pitch");
}

/// The ball must actually move when given a non-zero velocity. Find the
/// ball entity (the one with both `Ball` and `Position`/`Velocity`
/// components) and verify the Position changes after a tick.
///
/// Phase 1 update: `lifecycle_system` kicks the ball at Kickoff→InPlay with
/// a velocity of (2.0, 0.0) on the first tick, which would clobber any
/// velocity the test sets *before* the first tick. We instead let the
/// first tick complete (so the kickoff settles), then perturb velocity
/// and verify motion on tick 2.
#[test]
fn test_ball_moves_under_physics() {
    use sim_components::Position as PosComp;
    use sim_components::Velocity as VelComp;
    use sim_math::Vec2;

    let mut sim = Simulation::new(7);
    let ball_entity = sim.ball_entity();

    // Let the kickoff settle so the lifecycle no longer overwrites
    // velocity on each tick.
    sim.tick();

    // Set a non-zero velocity directly via world mutation.
    {
        let mut em = sim.world_mut().entity_mut(ball_entity);
        let mut v = em.get_mut::<VelComp>().expect("ball has Velocity");
        v.0 = Vec2::new(5.0, 0.0);
    }
    // Snapshot initial Position component.
    let initial_pos: (f32, f32) = {
        let p = sim
            .world()
            .entity(ball_entity)
            .get::<PosComp>()
            .expect("ball has Position");
        (p.0.x, p.0.y)
    };

    sim.tick();

    let final_pos: (f32, f32) = {
        let p = sim
            .world()
            .entity(ball_entity)
            .get::<PosComp>()
            .expect("ball has Position");
        (p.0.x, p.0.y)
    };

    assert_ne!(
        final_pos, initial_pos,
        "Ball Position did not change after tick: started at {initial_pos:?}, ended at {final_pos:?}"
    );
}

/// Regression test: exported player positions via `get_state()` must read
/// from the Position component (not the removed Player.position field).
///
/// Before the sync, `get_state()` read `Player.position` which was never
/// updated after `create_match` — the physics systems only mutate the
/// `Position` component. This caused all 22 players to appear frozen at
/// their kickoff coordinates in any exported state while the actual
/// simulation physics was running against the `Position` component.
#[test]
fn test_player_positions_update_in_exported_state() {
    let mut sim = Simulation::new(99);

    // Let the kickoff impulse settle (kickoff → InPlay on tick 0).
    sim.tick();

    // Verify get_state reads from Position component by checking that
    // the exported positions match the Position component values.
    let state = sim.get_state().expect("get_state works");
    for player_view in &state.players {
        let entity = Entity::from_bits(player_view.entity_id);
        let pos = sim.world().entity(entity).get::<Position>().unwrap().0;
        assert_eq!(
            player_view.position,
            [pos.x, pos.y],
            "get_state() must read position from Position component"
        );
    }

    // Advance a few ticks to ensure physics runs
    for _ in 0..10 {
        sim.tick();
    }

    // Verify again after physics runs
    let state = sim.get_state().expect("get_state works");
    for player_view in &state.players {
        let entity = Entity::from_bits(player_view.entity_id);
        let pos = sim.world().entity(entity).get::<Position>().unwrap().0;
        assert_eq!(
            player_view.position,
            [pos.x, pos.y],
            "get_state() must read position from Position component after physics"
        );
    }
}