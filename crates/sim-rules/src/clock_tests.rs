//! Tests for `match_duration_enforcement_system` (Law 7): half-time
//! transition when the half's ticks are exhausted.
//!
//! Plus tests for `added_time_calculation_system` (PRD §8.4): the
//! referee's accumulated stoppage ticks round up to whole minutes,
//! capped at 10 per half.

use bevy_ecs::prelude::*;
use sim_components::{MatchClock, Referee, StoppageEvent};

use crate::added_time_calculation_system;
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

/// World for `added_time_calculation_system`: one `Referee` entity
/// holding `stoppage_ticks` dead-ball entries plus a `MatchClock`
/// resource starting at `added_before`.
fn added_time_test_world(stoppage_ticks: u64, added_before: u64) -> World {
    let mut world = World::new();
    let events = (0..stoppage_ticks)
        .map(|tick| StoppageEvent {
            tick,
            reason: "dead_ball".to_string(),
        })
        .collect();
    world.spawn(Referee {
        stoppage_events: events,
        cards: Vec::new(),
    });
    world.insert_resource(MatchClock {
        elapsed_ticks: 0,
        half: 1,
        added_time_ticks: added_before,
        is_running: true,
    });
    world
}

fn run_added_time(world: &mut World) {
    let mut schedule = Schedule::default();
    schedule.add_systems(added_time_calculation_system);
    schedule.run(world);
}

/// No stoppages, no added time.
#[test]
fn test_added_time_zero_without_stoppages() {
    let mut world = added_time_test_world(0, 0);
    run_added_time(&mut world);
    assert_eq!(
        world.resource::<MatchClock>().added_time_ticks,
        0,
        "empty stoppage queue grants no added time"
    );
}

/// A single dead-ball tick rounds up to one whole minute (PRD §8.3: the
/// first added minute is 1, not 0).
#[test]
fn test_added_time_rounds_up_to_first_minute() {
    let mut world = added_time_test_world(1, 0);
    run_added_time(&mut world);
    assert_eq!(
        world.resource::<MatchClock>().added_time_ticks,
        3_600,
        "1 stoppage tick rounds up to 1 minute (3,600 ticks)"
    );
}

/// Exact minute boundaries stay put; one tick over rounds up.
#[test]
fn test_added_time_rounds_up_partial_minutes() {
    let mut world = added_time_test_world(3_600, 0);
    run_added_time(&mut world);
    assert_eq!(
        world.resource::<MatchClock>().added_time_ticks,
        3_600,
        "exactly 1 minute of stoppage grants exactly 1 minute"
    );

    let mut world = added_time_test_world(3_601, 0);
    run_added_time(&mut world);
    assert_eq!(
        world.resource::<MatchClock>().added_time_ticks,
        7_200,
        "1 minute + 1 tick of stoppage rounds up to 2 minutes"
    );
}

/// Added time is capped at 10 minutes per half (PRD §8.4).
#[test]
fn test_added_time_capped_at_ten_minutes() {
    let mut world = added_time_test_world(10 * 3_600, 0);
    run_added_time(&mut world);
    assert_eq!(
        world.resource::<MatchClock>().added_time_ticks,
        36_000,
        "10 minutes of stoppage grants the 10-minute cap"
    );

    let mut world = added_time_test_world(20 * 3_600, 0);
    run_added_time(&mut world);
    assert_eq!(
        world.resource::<MatchClock>().added_time_ticks,
        36_000,
        "stoppage beyond 10 minutes is capped at 36,000 ticks"
    );
}

/// Added time never shrinks within a half, and a clock seeded with more
/// than the queue implies keeps its value.
#[test]
fn test_added_time_is_monotonic() {
    let mut world = added_time_test_world(1, 3 * 3_600);
    run_added_time(&mut world);
    assert_eq!(
        world.resource::<MatchClock>().added_time_ticks,
        3 * 3_600,
        "the system must not shrink already-granted added time"
    );
}
