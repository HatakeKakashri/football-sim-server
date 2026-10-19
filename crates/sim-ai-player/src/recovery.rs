//! Tackle recovery: decrements the recovery timer each tick.
//!
//! A tackler is out of position for `tackle_recovery_ticks` after a failed
//! tackle; while the component is present `player_action_execution_system`
//! slows them down.

use bevy_ecs::prelude::*;
use sim_components::TackleRecovery;

/// Decrements every `TackleRecovery` timer each tick.
///
/// Removes the component when the window expires. A tackler is out of
/// position for `tackle_recovery_ticks` after a failed tackle; while the
/// component is present `player_action_execution_system` slows them down.
///
/// Runs once per tick (registered in `SimulationSet::MatchAdmin`) after
/// deferred component inserts from the `Execution` set have been applied,
/// so a recovery window opened this tick begins decrementing next tick.
pub fn tackle_recovery_system(
    mut query: Query<(Entity, &mut TackleRecovery)>,
    mut commands: Commands,
) {
    for (entity, mut recovery) in &mut query {
        if recovery.ticks_remaining <= 1 {
            commands.entity(entity).remove::<TackleRecovery>();
        } else {
            recovery.ticks_remaining -= 1;
        }
    }
}
