//! Formation reset: every player is placed at their formation slot's
//! coordinate for the team's current `attack_direction`. One slot
//! table serves both halves and both teams — at half-time only the
//! direction flips, so each player's slot mirrors ends through the
//! same code path (ticket 02 / ticket 17).

use bevy_ecs::prelude::*;
use sim_components::{FormationSlot, Player, Team, TeamId};
use sim_math::Vec2;

use super::lifecycle::apply_player_slot;

/// Reset every player to their formation slot for their team's current
/// attack direction. Called when entering `Kickoff` (match start and
/// second-half kickoff), after the half-time `attack_direction` flip.
pub fn reset_formation(world: &mut World) {
    let directions: Vec<(TeamId, i8)> = world
        .query::<&Team>()
        .iter(world)
        .map(|team| (team.id, team.attack_direction()))
        .collect();

    let placements: Vec<(Entity, Vec2)> = world
        .query::<(Entity, &Player, &FormationSlot)>()
        .iter(world)
        .filter_map(|(entity, player, slot)| {
            let (_, direction) = directions.iter().find(|(id, _)| *id == player.team_id)?;
            Some((entity, slot.position_for(*direction)))
        })
        .collect();

    for (entity, target) in placements {
        apply_player_slot(world, entity, target);
    }
}
