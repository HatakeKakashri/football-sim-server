//! Regression: `create_match` must give every player a `TeamIdComponent`,
//! otherwise `perception_system` and `player_decision_system` (which query
//! it) silently match zero entities and no AI decision ever runs.

use crate::Simulation;
use sim_ai_player::DecisionEvaluationCount;
use sim_components::{Player, TeamIdComponent};

#[test]
fn every_player_has_team_id_component_matching_player_team() {
    let mut sim = Simulation::new(42);
    let world = sim.world_mut();
    let mut query = world.query::<(&Player, Option<&TeamIdComponent>)>();
    let rows: Vec<_> = query.iter(world).collect();
    assert_eq!(rows.len(), 22, "expected 22 players");
    for (player, team_component) in rows {
        let component = team_component.expect("invariant: every player has TeamIdComponent");
        assert_eq!(component.0, player.team_id);
    }
}

#[test]
fn decision_system_evaluates_players_in_a_real_simulation() {
    let mut sim = Simulation::new(42);
    for _ in 0..60 {
        sim.tick();
    }
    let evaluations = sim.world().resource::<DecisionEvaluationCount>().get();
    assert!(evaluations > 0, "no player was ever evaluated");
}
