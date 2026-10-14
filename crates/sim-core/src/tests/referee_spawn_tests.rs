//! Issue 06: a `Referee` entity exists for the whole match so that
//! `added_time_calculation_system` has something to consume.

use bevy_ecs::prelude::Entity;
use sim_components::{Referee, StoppageEvent};

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
fn added_time_system_consumes_the_referee_stoppage_queue() {
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

    let queued = sim
        .world()
        .entity(referee)
        .get::<Referee>()
        .expect("invariant: referee entity carries a Referee component")
        .stoppage_events
        .len();
    assert_eq!(queued, 0, "the added-time system drained the queue");
}
