//! Issue 06: a `Referee` entity exists for the whole match so that
//! `added_time_calculation_system` has something to consume.
//!
//! Issue 11: the stoppage queue accumulates per half (PRD §8.4) — one
//! entry per stoppage tick — and the added-time system derives whole
//! minutes from its length instead of draining it.

use bevy_ecs::prelude::Entity;
use sim_components::{MatchClock, Referee, StoppageEvent};

use crate::Simulation;

fn referees(sim: &mut Simulation) -> Vec<Entity> {
    sim.world_mut()
        .query::<(Entity, &Referee)>()
        .iter(sim.world())
        .map(|(entity, _)| entity)
        .collect()
}

#[test]
fn new_simulation_spawns_exactly_one_referee() {
    let mut sim = Simulation::new(12345);
    assert_eq!(referees(&mut sim).len(), 1);
}

#[test]
fn referee_survives_ticking() {
    let mut sim = Simulation::new(12345);
    let before = referees(&mut sim);
    for _ in 0..300 {
        sim.tick();
    }
    assert_eq!(referees(&mut sim), before, "same single referee throughout");
}

#[test]
fn added_time_system_accumulates_the_referee_stoppage_queue() {
    let mut sim = Simulation::new(12345);
    let referee = referees(&mut sim)[0];
    sim.world_mut()
        .entity_mut(referee)
        .get_mut::<Referee>()
        .expect("invariant: referee entity carries a Referee component")
        .stoppage_events
        .push(StoppageEvent {
            tick: 0,
            reason: "injury".to_string(),
        });

    sim.tick();

    // The queue is retained (it accumulates for the whole half); the ball
    // is live on tick 0 so nothing new accrues.
    let queued = sim
        .world()
        .entity(referee)
        .get::<Referee>()
        .expect("invariant: referee entity carries a Referee component")
        .stoppage_events
        .len();
    assert_eq!(queued, 1, "the stoppage queue accumulates, not drains");
    // One stoppage tick rounds up to one whole added minute (PRD §8.4),
    // on both the Resource (source of truth) and the Component mirror.
    assert_eq!(
        sim.world().resource::<MatchClock>().added_time_ticks,
        3_600,
        "1 stoppage tick grants 1 minute of added time"
    );
    assert_eq!(
        sim.world()
            .entity(sim.match_entity())
            .get::<MatchClock>()
            .expect("invariant: match entity carries a MatchClock component")
            .added_time_ticks,
        3_600,
        "the Component mirror follows the Resource"
    );
}
