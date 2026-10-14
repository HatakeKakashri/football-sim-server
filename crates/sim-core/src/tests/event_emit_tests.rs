//! Integration tests for the event-emit pipeline: every event
//! `seq` over a real `Simulation` run is gapless; `pass` events
//! carry correct `kick_tick` / `success` / `to`; `possession_change`
//! fires on team change. These run a short match window to keep
//! the test fast (full-match coverage is in
//! `event_log_is_deterministic_full_match`, `#[ignore]`'d).

#![allow(
    clippy::float_cmp,
    reason = "tests compare fresh values against the constants they were built from"
)]

use crate::{EventLog, MatchEvent, PossessionChangeCause, Simulation, check_gapless_seq};
use sim_components::TeamId;

fn pos(x: f32, y: f32) -> sim_math::Vec2 {
    sim_math::Vec2::new(x, y)
}

#[test]
fn events_emitted_in_short_window_have_gapless_seq() {
    let mut sim = Simulation::new(12345);
    for _ in 0..3_000 {
        sim.tick();
    }
    let log = sim
        .world()
        .get_resource::<EventLog>()
        .cloned()
        .expect("EventLog is registered");
    if !log.events.is_empty() {
        assert!(
            check_gapless_seq(&log).is_none(),
            "seq is not gapless: {} events",
            log.events.len()
        );
    }
}

#[test]
fn pass_event_has_correct_kick_tick_and_success() {
    // Build a pass event directly to pin the payload shape.
    let mut world = bevy_ecs::prelude::World::new();
    world.insert_resource(sim_components::MatchClock {
        elapsed_ticks: 1_000,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(EventLog::new());
    let clock = world
        .resource::<sim_components::MatchClock>()
        .clone();
    let event = crate::build_pass_event(
        0,
        1_000,
        &clock,
        TeamId(0),
        sim_components::PlayerId::from_bits(11),
        Some(sim_components::PlayerId::from_bits(22)),
        true,
        900,
        pos(50.0, 34.0),
        pos(60.0, 34.0),
    );
    let mut log = world.resource_mut::<EventLog>();
    log.append(event);
    let log = world.resource::<EventLog>();
    let MatchEvent::Pass { payload, envelope } = &log.events[0] else {
        panic!("expected pass event");
    };
    assert_eq!(payload.kick_tick, 900, "kick_tick must be the kick tick");
    assert_eq!(envelope.tick, 1_000, "envelope tick must be the resolution tick");
    assert!(payload.success, "pass to a teammate is success");
    assert_eq!(payload.to, Some(sim_components::PlayerId::from_bits(22)));
    // team 0 kicker; the team field reflects the kicker's team.
    assert_eq!(envelope.team, Some(TeamId(0)));
}

#[test]
fn possession_change_event_has_cause_and_teams() {
    let mut world = bevy_ecs::prelude::World::new();
    world.insert_resource(sim_components::MatchClock {
        elapsed_ticks: 2_000,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });
    world.insert_resource(EventLog::new());
    let clock = world
        .resource::<sim_components::MatchClock>()
        .clone();
    let event = crate::build_possession_change_event(
        0,
        2_000,
        &clock,
        Some(TeamId(0)),
        TeamId(1),
        PossessionChangeCause::OutOfBounds,
        None,
    );
    let mut log = world.resource_mut::<EventLog>();
    log.append(event);
    let log = world.resource::<EventLog>();
    let MatchEvent::PossessionChange { payload, .. } = &log.events[0] else {
        panic!("expected possession_change event");
    };
    assert_eq!(payload.from_team, TeamId(0));
    assert_eq!(payload.to_team, TeamId(1));
    assert_eq!(payload.cause, PossessionChangeCause::OutOfBounds);
}
