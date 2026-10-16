//! Formation reset: every player is placed at their formation slot's
//! coordinate for the team's current `attack_direction`. One slot
//! table serves both halves and both teams — at half-time only the
//! direction flips, so each player's slot mirrors ends through the
//! same code path (ticket 02 / ticket 17).

use bevy_ecs::prelude::*;
use sim_components::{
    FormationSlot, Player, Team, TeamId, TacticParameters,
};
use sim_math::Vec2;

use super::lifecycle::apply_player_slot;

/// Resolve a formation slot's home coordinate for the team's attack
/// direction, then apply the team's line-height shift along the attacking
/// axis (PRD §7.2 mentality/tactic): positive shift moves toward the
/// opponent goal, negative pulls back toward the own goal.
#[must_use]
pub fn slot_position_for(slot: FormationSlot, attack_direction: i8, line_shift: f32) -> Vec2 {
    let mut pos = slot.position_for(attack_direction);
    pos.x = f32::from(attack_direction).mul_add(line_shift, pos.x);
    pos
}

/// Reset every player to their formation slot for their team's current
/// attack direction. Called when entering `Kickoff` (match start and
/// second-half kickoff), after the half-time `attack_direction` flip.
///
/// The formation table drives each player's `home_slot` (ticket 17), and
/// the team's mentality+tactic line-height shift (ticket 25) is applied
/// along the attacking axis so `Attack`/`Defend` and `ParkTheBus` move the
/// whole block up/down the pitch.
pub fn reset_formation(world: &mut World) {
    let team_data: Vec<(TeamId, i8, TacticParameters)> = world
        .query::<(&Team, &TacticParameters)>()
        .iter(world)
        .map(|(team, params)| (team.id, team.attack_direction(), *params))
        .collect();

    let placements: Vec<(Entity, Vec2)> = world
        .query::<(Entity, &Player, &FormationSlot)>()
        .iter(world)
        .filter_map(|(entity, player, slot)| {
            let (_, direction, params) = team_data
                .iter()
                .find(|(id, _, _)| *id == player.team_id)?;
            Some((
                entity,
                slot_position_for(*slot, *direction, params.line_height_shift),
            ))
        })
        .collect();

    for (entity, target) in placements {
        apply_player_slot(world, entity, target);
    }
}
