//! Formation reset (4-4-2 placeholder).

use bevy_ecs::prelude::*;
use sim_components::{FormationSlot, Player, Team, TeamId};
use sim_math::Vec2;

use super::lifecycle::apply_player_slot;

/// Phase 1 hardcoded 4-4-2 formation reset. Every player carries a
/// [`FormationSlot`]; each is placed at [`FormationSlot::position_for`] the
/// team's current `attack_direction`. Teams attacking right
/// (`attack_direction = +1`) place their goalkeeper near x=5; teams attacking
/// left (`attack_direction = -1`) place it near x=100. One slot table serves
/// both halves through the direction field, with no home/away distinction.
pub fn reset_formation_to_4_4_2(world: &mut World) {
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
