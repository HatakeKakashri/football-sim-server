//! `Simulation::validate_command` and `Simulation::apply_validated_command`:
//! the F5 split-API for `ManagerCommand`. Validation is read-only and lives
//! in `validate_command`; mutation lives in `apply_validated_command`.

use bevy_ecs::prelude::*;
use sim_components::{CommandError, ManagerCommand, Match, MatchState};

use super::Simulation;

impl Simulation {
    /// Validate a `ManagerCommand` against the current match state.
    ///
    /// # Errors
    ///
    /// Returns a [`sim_components::CommandError`] if the command is not
    /// allowed in the match's current state (formation, mentality and
    /// tactic changes need `InPlay`/`Stoppage`; substitutions need
    /// `Stoppage`/`HalfTime`).
    pub fn validate_command(
        &self,
        _match_id: Entity,
        command: &ManagerCommand,
    ) -> Result<(), CommandError> {
        // Phase C §4.3: Match is now a Resource. Validate using
        // `world.resource::<Match>()` — the match_id parameter is retained
        // for API compatibility but is no longer used to look up Match.
        let match_component = self.world.resource::<Match>();

        match command {
            ManagerCommand::Substitute {
                out: _,
                substitute: _,
            } => {
                if match_component.state != MatchState::Stoppage
                    && match_component.state != MatchState::HalfTime
                {
                    return Err(CommandError::InvalidForState {
                        current_state: match_component.state,
                        required_state: MatchState::Stoppage,
                    });
                }
            }
            ManagerCommand::ChangeFormation(_)
            | ManagerCommand::ChangeMentality(_)
            | ManagerCommand::SetTactic(_) => {
                if match_component.state != MatchState::InPlay
                    && match_component.state != MatchState::Stoppage
                {
                    return Err(CommandError::InvalidForState {
                        current_state: match_component.state,
                        required_state: MatchState::InPlay,
                    });
                }
            }
        }

        Ok(())
    }

    /// Apply a previously-validated `ManagerCommand` to the home team.
    ///
    /// Callers must have validated the command via [`Self::validate_command`]
    /// first; this function does not re-check state.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError::NotImplemented`] for commands that are
    /// recognized but not yet implemented (e.g. `Substitute`).
    pub fn apply_validated_command(
        &mut self,
        _match_id: Entity,
        command: ManagerCommand,
    ) -> Result<(), CommandError> {
        // Phase C §4.3: Match is now a Resource, so the match-id parameter
        // is retained for API compatibility but is no longer used to look
        // up Match. The apply always targets the home team; side-aware
        // application is not currently supported.
        let match_component = self.world.resource::<Match>().clone();

        match command {
            ManagerCommand::ChangeFormation(formation) => {
                let home_team = match_component.home_team;
                if let Some(mut team) = self
                    .world
                    .entity_mut(home_team)
                    .get_mut::<sim_components::Team>()
                {
                    team.formation = formation;
                }
                Ok(())
            }
            ManagerCommand::Substitute { out, substitute } => {
                tracing::info!("Substitution: {out:?} -> {substitute:?}");
                Err(CommandError::NotImplemented)
            }
            ManagerCommand::ChangeMentality(mentality) => {
                let home_team = match_component.home_team;
                if let Some(mut team) = self
                    .world
                    .entity_mut(home_team)
                    .get_mut::<sim_components::Team>()
                {
                    team.mentality = mentality;
                }
                Ok(())
            }
            ManagerCommand::SetTactic(tactic) => {
                let home_team = match_component.home_team;
                if let Some(mut team) = self
                    .world
                    .entity_mut(home_team)
                    .get_mut::<sim_components::Team>()
                {
                    team.tactic = tactic;
                }
                Ok(())
            }
        }
    }
}