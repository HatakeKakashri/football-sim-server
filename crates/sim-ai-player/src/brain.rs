//! Brain types: the `UtilityBrain` component, the player's chosen
//! `PlayerAction`, and the `intent_kind` helper used by parity tests.

use sim_components::{ActionIntent, Intent, MovementIntent};

#[derive(bevy_ecs::prelude::Component, Debug, Clone)]
pub struct UtilityBrain {
    pub actions: Vec<PlayerAction>,
    pub hysteresis: f32,
}

#[derive(Debug, Clone)]
pub struct PlayerAction {
    pub intent: Intent,
    pub considerations: Vec<crate::Consideration>,
}

/// Maps an `Intent` to a stable string tag used in parity tests
/// (`test_intent_kind_string_for_all_variants`). Adding a new
/// `MovementIntent` / `ActionIntent` variant breaks this function's
/// exhaustiveness, forcing the test to be updated.
pub const fn intent_kind(intent: &Intent) -> &'static str {
    match intent {
        Intent::Movement(MovementIntent::MoveToPosition(_)) => "MoveToPosition",
        Intent::Movement(MovementIntent::HoldPosition) => "HoldPosition",
        Intent::Movement(MovementIntent::ChaseBall) => "ChaseBall",
        Intent::Movement(MovementIntent::Intercept) => "Intercept",
        Intent::Movement(MovementIntent::SupportRun) => "SupportRun",
        Intent::Movement(MovementIntent::TrackBack) => "TrackBack",
        Intent::Action(ActionIntent::PassTo) => "PassTo",
        Intent::Action(ActionIntent::ShootAtGoal(_)) => "ShootAtGoal",
        Intent::Action(ActionIntent::Tackle(_)) => "Tackle",
        Intent::Action(ActionIntent::MarkOpponent(_)) => "MarkOpponent",
        Intent::Action(ActionIntent::Press(_)) => "Press",
    }
}
