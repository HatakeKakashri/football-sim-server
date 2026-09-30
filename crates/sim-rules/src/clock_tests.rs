//! Tests for `match_duration_enforcement_system` (Law 7): half-time
//! transition when the half's ticks are exhausted.

use bevy_ecs::prelude::*;
use sim_components::MatchClock;

use crate::match_duration_enforcement_system;

#[test]
fn test_match_duration_enforcement() {
    let mut world = World::new();

    world.insert_resource(sim_components::Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (1, 0),
        state: sim_components::MatchState::InPlay,
        seed: 12345,
    });
    let match_entity = world.spawn(()).id();
    world.entity_mut(match_entity).insert(MatchClock {
        // 48 minutes = 45 min half + 3 min added time = 162,180 ticks
        elapsed_ticks: 45 * 60 * 60 + 3 * 60 * 60,
        half: 1,
        added_time_ticks: 3 * 60 * 60,
        is_running: true,
    });

    let mut schedule = Schedule::default();
    schedule.add_systems(match_duration_enforcement_system);
    schedule.run(&mut world);

    let m = world.resource::<sim_components::Match>();
    let clock = world.entity(match_entity).get::<MatchClock>().unwrap();
    assert!(!clock.is_running);
    assert_eq!(m.state, sim_components::MatchState::HalfTime);
}