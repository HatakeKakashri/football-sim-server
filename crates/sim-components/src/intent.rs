//! Intent types: what a player wants to do this tick.
//!
//! An `Intent` is either a movement (which steers the player towards a
//! target point or formation slot) or an action (which commits the
//! player to a discrete tactical action like passing, tackling, or
//! marking).
//!
//! The split lets `player_action_execution_system` and other dispatch
//! sites match on the coarse kind (`IntentKind`) when they don't care
//! about the specific variant.

use bevy_ecs::prelude::*;
use sim_math::Vec2;

use super::PerceptionSnapshot;

/// Top-level intent tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Intent {
    Movement(MovementIntent),
    Action(ActionIntent),
}

/// Movement-style intents: move the player somewhere or settle into a
/// formation slot. These never commit to a discrete ball-affecting action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MovementIntent {
    MoveToPosition(Vec2),
    HoldPosition,
    ChaseBall,
    Intercept,
    SupportRun,
    TrackBack,
}

impl MovementIntent {
    /// Resolve the destination of `MoveToPosition(target)`:
    /// `Vec2::zero()` is the "go to your home slot" sentinel and maps
    /// to `perception.home_slot`; every other target is passed through.
    ///
    /// Brain templates encode "go to your home slot" as `Vec2::zero()`
    /// so the intent can be written without a perception snapshot in
    /// scope; every consumer (steer target, decision scoring, distance
    /// consideration) resolves the sentinel through this single helper.
    /// See `default_utility_brain` for the only authoring site.
    #[must_use]
    pub fn resolve_target(target: Vec2, perception: &PerceptionSnapshot) -> Vec2 {
        if target == Vec2::zero() {
            perception.home_slot
        } else {
            target
        }
    }
}

/// Action-style intents: commit the player to a tactical action. Most of
/// these will resolve into a kick, tackle, or marking assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionIntent {
    PassTo,
    ShootAtGoal(Vec2),
    Tackle(Entity),
    MarkOpponent(Entity),
    Press(Entity),
}

impl Intent {
    /// Coarse two-way tag: does this intent move the player, or commit to
    /// an action? Useful for sites that don't care about the specific
    /// variant.
    #[must_use]
    pub const fn kind(&self) -> IntentKind {
        match *self {
            Self::Movement(_) => IntentKind::Movement,
            Self::Action(_) => IntentKind::Action,
        }
    }

    /// `true` when this intent is a `Movement(_)` variant.
    #[must_use]
    pub const fn is_movement(&self) -> bool {
        matches!(self, Self::Movement(_))
    }

    /// `true` when this intent is an `Action(_)` variant.
    #[must_use]
    pub const fn is_action(&self) -> bool {
        matches!(self, Self::Action(_))
    }
}

/// Coarse intent kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntentKind {
    Movement,
    Action,
}
