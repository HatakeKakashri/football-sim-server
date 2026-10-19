//! Resource installer for the rules layer.
//!
//! Every rule system takes its cross-system hand-off slots as required
//! `Res`/`ResMut` params, so any world that runs them must hold these
//! resources. This is the single place that lists them.
//!
//! `PendingRestart` is deliberately not installed: it is created on
//! demand by `goal_detection_system`, and its absence means "no restart
//! pending".
//!
//! All emit-related signals are consolidated into `PendingRuleOutcome`
//! (see `crate::outcome::RuleOutcome`), reducing the resource surface
//! from seven single-slot resources to one transactional record.

use bevy_ecs::prelude::World;

use crate::{CurrentTick, PendingOutOfBounds, PendingRuleOutcome};

/// Insert every resource the rule systems require, at its initial value.
pub fn install_rules_resources(world: &mut World) {
    world.insert_resource(CurrentTick(0));
    world.insert_resource(PendingOutOfBounds::default());
    world.insert_resource(PendingRuleOutcome::default());
}
