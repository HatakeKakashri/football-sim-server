//! Integration tests for the perception system. Compiled only under
//! `cfg(test)`.

use bevy_ecs::prelude::*;
use sim_components::{Player, Position, TeamId, TeamIdComponent};

/// Regression: `PerceptionSnapshot.nearby_*[].entity` must carry the real
/// entity id, not `Entity::PLACEHOLDER`. Downstream `Tackle` /
/// `MarkOpponent` / `Press` intents reference this id.
#[test]
fn test_perception_records_real_entity_ids() {
    let mut world = World::new();

    // Phase C §4.3: Match and Ball are both Resources now.
    world.insert_resource(sim_components::Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: sim_components::MatchState::InPlay,
        seed: 0,
    });
    world.insert_resource(sim_components::Ball {
        spin: 0.0,
        state: sim_components::BallState::Free,
        possessor: None,
        last_touched_by: None,
        kick_velocity: None,
    });
    // Phase F follow-up: MatchClock is a Resource (spec §3).
    world.insert_resource(sim_components::MatchClock {
        elapsed_ticks: 0,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });

    // Two opposing players within perception range.
    let home_player = world.spawn(()).id();
    world.entity_mut(home_player).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world
        .entity_mut(home_player)
        .insert(Position(sim_math::Vec2::new(50.0, 34.0)));
    world
        .entity_mut(home_player)
        .insert(TeamIdComponent(TeamId(0)));

    let away_player = world.spawn(()).id();
    world.entity_mut(away_player).insert(Player {
        team_id: TeamId(1),
        intent: None,
    });
    world
        .entity_mut(away_player)
        .insert(Position(sim_math::Vec2::new(55.0, 34.0)));
    world
        .entity_mut(away_player)
        .insert(TeamIdComponent(TeamId(1)));

    let mut schedule = Schedule::default();
    schedule.add_systems(crate::perception_system);

    schedule.run(&mut world);

    let snapshot = world
        .entity(home_player)
        .get::<sim_components::PerceptionSnapshot>()
        .expect("perception_system must insert a snapshot");

    assert_eq!(
        snapshot.nearby_opponents.len(),
        1,
        "home player should see the away player"
    );
    assert_eq!(
        snapshot.nearby_opponents[0].entity, away_player,
        "nearby_opponents[0].entity must be the real entity, not PLACEHOLDER"
    );
    assert_ne!(
        snapshot.nearby_opponents[0].entity,
        Entity::PLACEHOLDER,
        "Entity::PLACEHOLDER must not leak into production perception snapshots"
    );
}
