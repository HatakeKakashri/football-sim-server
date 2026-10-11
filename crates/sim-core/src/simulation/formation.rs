//! Formation reset (4-4-2 placeholder).

use bevy_ecs::prelude::*;
use sim_components::formation_slot::{FormationSlot, formation_442_slot_position};
use sim_components::{Player, Position, Team};

use super::lifecycle::apply_player_slot;

/// Phase 1 hardcoded 4-4-2 formation reset. Reads each team's `attack_direction`
/// so that the correct half's slot positions are used via [`FormationSlot::position_for`].
/// Teams attacking right (`attack_direction = +1`) place GK near x=5; teams attacking left
/// (`attack_direction = -1`) place GK near x=100. This single slot table is reused
/// for both halves via the direction field, with no hardcoded home/away distinction.
///
/// Players with assigned [`FormationSlot`] components are placed at their dedicated
/// slot positions. If a player lacks a slot component, they are sorted by x-coordinate
/// and assigned by canonical index.
pub fn reset_formation_to_4_4_2(world: &mut World) {
    let mut attack_dir_by_team: [i8; 2] = [1, -1]; // safe default
    let team_entities: Vec<Entity> = world
        .iter_entities()
        .filter_map(|e_ref| {
            let id = e_ref.id();
            world.entity(id).get::<Team>().map(|_| id)
        })
        .collect();
    for team_entity in team_entities {
        if let Some(team) = world.entity(team_entity).get::<Team>()
            && team.id.0 < 2
        {
            attack_dir_by_team[team.id.0 as usize] = team.attack_direction;
        }
    }

    // Collect player entities per team.
    let mut team0_players: Vec<Entity> = Vec::new();
    let mut team1_players: Vec<Entity> = Vec::new();
    for entity in world.iter_entities() {
        let id = entity.id();
        if let Some(p) = world.entity(id).get::<Player>() {
            match p.team_id.0 {
                0 => team0_players.push(id),
                1 => team1_players.push(id),
                _ => {}
            }
        }
    }

    // Assign slots for team 0.
    assign_slots(
        world,
        &mut team0_players,
        attack_dir_by_team[0],
    );
    // Assign slots for team 1.
    assign_slots(
        world,
        &mut team1_players,
        attack_dir_by_team[1],
    );
}

/// Assign each player the corresponding slot from the canonical 4-4-2 slot table.
///
/// If players carry [`FormationSlot`] components, each player is directly placed at
/// their slot's position for `attack_direction`. Otherwise, players are sorted by
/// x-coordinate (ascending for attack-right, descending for attack-left) so the deepest
/// player gets slot 0 (GK).
fn assign_slots(world: &mut World, players: &mut [Entity], attack_direction: i8) {
    let all_have_slots = !players.is_empty()
        && players
            .iter()
            .all(|e| world.entity(*e).get::<FormationSlot>().is_some());

    if all_have_slots {
        for entity in players.iter() {
            if let Some(slot) = world.entity(*entity).get::<FormationSlot>() {
                apply_player_slot(world, *entity, slot.position_for(attack_direction));
            }
        }
    } else {
        players.sort_by(|a, b| {
            let ax = world.entity(*a).get::<Position>().map_or(0.0, |p| p.0.x);
            let bx = world.entity(*b).get::<Position>().map_or(0.0, |p| p.0.x);
            if attack_direction > 0 {
                ax.partial_cmp(&bx).unwrap_or(std::cmp::Ordering::Equal)
            } else {
                bx.partial_cmp(&ax).unwrap_or(std::cmp::Ordering::Equal)
            }
        });

        for (i, entity) in players.iter().enumerate() {
            let target = formation_442_slot_position(i, attack_direction);
            apply_player_slot(world, *entity, target);
        }
    }
}
