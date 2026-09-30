//! Tests for `foul_detection_system` (Law 12): dangerous play on
//! high-speed opposing-team collisions.

use bevy_ecs::prelude::*;
use sim_components::{
    MatchClock, Player, Position, RoleComponent, RuleEvent, Skill, Stamina, TeamId,
    TeamIdComponent, Velocity,
};
use sim_math::Vec2;

use crate::foul_detection_system;

#[test]
fn test_foul_detection_dangerous_play() {
    let mut world = World::new();

    world.insert_resource(sim_components::Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: sim_components::MatchState::InPlay,
        seed: 12345,
    });
    let match_entity = world.spawn(()).id();
    world.entity_mut(match_entity).insert(MatchClock {
        elapsed_ticks: 30 * 60,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });

    // Create two opposing players very close together with high relative velocity
    let home_player = world.spawn(()).id();
    world.entity_mut(home_player).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world
        .entity_mut(home_player)
        .insert(Position(Vec2::new(50.0, 34.0)));
    world
        .entity_mut(home_player)
        .insert(Velocity(Vec2::new(5.0, 0.0)));
    world.entity_mut(home_player).insert(Stamina(0.8));
    world
        .entity_mut(home_player)
        .insert(RoleComponent(sim_components::Role::CenterBack));
    world.entity_mut(home_player).insert(Skill(0.7));
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
        .insert(Position(Vec2::new(50.5, 34.0)));
    world
        .entity_mut(away_player)
        .insert(Velocity(Vec2::new(-5.0, 0.0)));
    world.entity_mut(away_player).insert(Stamina(0.8));
    world
        .entity_mut(away_player)
        .insert(RoleComponent(sim_components::Role::Striker));
    world.entity_mut(away_player).insert(Skill(0.7));
    world
        .entity_mut(away_player)
        .insert(TeamIdComponent(TeamId(1)));

    let mut schedule = Schedule::default();
    schedule.add_systems(foul_detection_system);
    schedule.run(&mut world);

    // Players should have RuleEvent::Foul components from high-speed collision
    let home_foul = world.entity(home_player).get::<RuleEvent>();
    let away_foul = world.entity(away_player).get::<RuleEvent>();

    assert!(
        home_foul.is_some() || away_foul.is_some(),
        "At least one player should have a foul event from dangerous play"
    );
}