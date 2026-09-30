//! Tests for `sim-core`.
//!
//! Test files (sibling modules, all `#[cfg(test)]`):
//! - this file: F5 `validate_command` / `apply_validated_command` tests
//!   plus the small `test_simulation_creation` smoke test.
//! - `state_hash_tests.rs`: state-hash stability and perturbation-detection.
//! - `determinism_tests.rs`: per-tick determinism and same-seed equivalence.
//! - `lifecycle_tests.rs`: clock, half-time, kickoff, pause/resume.
//! - `integration_tests.rs`: pipeline isolation, ball motion under physics,
//!   exported-state sync.
//!
//! The `HalfTimeEntryTick` resource is reachable through
//! `crate::simulation::lifecycle::HalfTimeEntryTick`.

#![allow(
    clippy::float_cmp,
    reason = "tests compare outputs of identical arithmetic against the inputs they were derived from"
)]

#[cfg(test)]
mod determinism_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod state_hash_tests;

use crate::Simulation;
use sim_components::{Match, MatchState};

/// Phase F §F5: pin the typed error variant returned when a formation
/// change is attempted during `PreMatch`. This is the load-bearing
/// guarantee that lets `sim_server::apply_command` delegate validation
/// to sim-core without re-implementing the rule.
#[test]
fn validate_command_returns_typed_error_for_prematch() {
    use sim_components::{CommandError, Formation, ManagerCommand};
    let sim = Simulation::new(7);
    // Default state is PreMatch; ChangeFormation should fail.
    let result = sim.validate_command(
        sim.match_entity,
        &ManagerCommand::ChangeFormation(Formation::FourThreeThree),
    );
    assert!(matches!(
        result,
        Err(CommandError::InvalidForState {
            current_state: MatchState::PreMatch,
            required_state: MatchState::InPlay,
        })
    ));
}

/// Phase F §F5 fix: `validate_command` must not mutate state, and
/// `apply_validated_command` is the path that actually applies the
/// change. This pins the validate-on-submit / apply-on-tick split.
#[test]
fn validate_command_does_not_apply_but_apply_validated_command_does() {
    use sim_components::{Formation, ManagerCommand};
    let mut sim = Simulation::new(7);
    // Move to InPlay so ChangeFormation passes validation.
    sim.world.resource_mut::<Match>().state = MatchState::InPlay;
    let home_team = sim.world.resource::<Match>().home_team;

    // Read default formation (FourFourTwo from Simulation::new).
    let default_formation = sim
        .world
        .entity(home_team)
        .get::<sim_components::Team>()
        .expect("invariant: home team has Team component")
        .formation;
    assert_eq!(default_formation, Formation::FourFourTwo);

    let cmd = ManagerCommand::ChangeFormation(Formation::FourThreeThree);

    // validate_command must NOT mutate.
    sim.validate_command(sim.match_entity, &cmd)
        .expect("invariant: InPlay state permits ChangeFormation");
    let formation_after_validate = sim
        .world
        .entity(home_team)
        .get::<sim_components::Team>()
        .expect("invariant: home team has Team component")
        .formation;
    assert_eq!(
        formation_after_validate, default_formation,
        "validate_command must not mutate state"
    );

    // apply_validated_command DOES mutate.
    sim.apply_validated_command(sim.match_entity, cmd);
    let formation_after_apply = sim
        .world
        .entity(home_team)
        .get::<sim_components::Team>()
        .expect("invariant: home team has Team component")
        .formation;
    assert_eq!(
        formation_after_apply,
        Formation::FourThreeThree,
        "apply_validated_command must mutate state"
    );
}

#[test]
fn test_simulation_creation() {
    let sim = Simulation::new(12345);
    assert_eq!(sim.tick, 0);
}