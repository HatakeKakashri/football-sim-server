//! Formation reset (4-4-2 placeholder).

use bevy_ecs::prelude::*;
use sim_components::{Player, Position, Team};
use sim_math::Vec2;

use super::lifecycle::apply_player_slot;

/// Canonical 4-4-2 slot table for a team attacking toward `x = PITCH_LENGTH`
/// (`attack_direction` = +1). Index 0 = GK, 1-4 = defenders, 5-8 = midfielders,
/// 9-10 = forwards. GK is near x=5, forwards near x=88.
const ATTACK_RIGHT_SLOTS: [Vec2; 11] = [
    Vec2::new(5.0, 34.0),  // GK
    Vec2::new(20.0, 20.0), // Defender 1
    Vec2::new(35.0, 20.0), // Defender 2
    Vec2::new(50.0, 20.0), // Defender 3
    Vec2::new(65.0, 20.0), // Defender 4
    Vec2::new(25.0, 34.0), // Midfielder 1
    Vec2::new(40.0, 34.0), // Midfielder 2
    Vec2::new(55.0, 34.0), // Midfielder 3
    Vec2::new(70.0, 34.0), // Midfielder 4
    Vec2::new(80.0, 34.0), // Forward 1
    Vec2::new(88.0, 34.0), // Forward 2
];

/// Mirror of `ATTACK_RIGHT_SLOTS` for a team attacking toward `x = 0`
/// (`attack_direction` = -1). Each slot is mirrored about x = 52.5. GK is near
/// x=100, forwards near x=17.
const ATTACK_LEFT_SLOTS: [Vec2; 11] = [
    Vec2::new(100.0, 34.0), // GK (mirrored from 5)
    Vec2::new(85.0, 48.0),  // Defender 1 (mirrored x: 105-20=85, y: 68-20=48)
    Vec2::new(70.0, 48.0),  // Defender 2
    Vec2::new(55.0, 48.0),  // Defender 3
    Vec2::new(40.0, 48.0),  // Defender 4
    Vec2::new(80.0, 34.0),  // Midfielder 1 (mirrored x: 105-25=80)
    Vec2::new(65.0, 34.0),  // Midfielder 2 (105-40=65)
    Vec2::new(50.0, 34.0),  // Midfielder 3 (105-55=50)
    Vec2::new(35.0, 34.0),  // Midfielder 4 (105-70=35)
    Vec2::new(25.0, 34.0),  // Forward 1 (105-80=25)
    Vec2::new(17.0, 34.0),  // Forward 2 (105-88=17)
];

/// Phase 1 hardcoded 4-4-2 formation reset. Reads each team's `attack_direction`
/// so that the correct half's slot table is used — teams attacking right
/// (`attack_direction = +1`) use [`ATTACK_RIGHT_SLOTS`]; teams attacking left
/// (`attack_direction = -1`) use [`ATTACK_LEFT_SLOTS`]. This single slot table
/// is reused for both halves via the direction field, with no hardcoded
/// home/away distinction.
///
/// Players within each team are sorted by current x-coordinate (ascending for
/// attack-right teams, descending for attack-left teams) so the goalkeeper
/// always lands in slot 0.
pub fn reset_formation_to_4_4_2(world: &mut World) {
    // Collect (entity, team_id, attack_direction, current_x) for every player.
    // We need attack_direction from the team, which is a separate component,
    // so we read it via the Team query keyed by team id.
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

    // Sort and assign slots for team 0.
    assign_slots(
        world,
        &mut team0_players,
        attack_dir_by_team[0],
    );
    // Sort and assign slots for team 1.
    assign_slots(
        world,
        &mut team1_players,
        attack_dir_by_team[1],
    );
}

/// Sort `players` by x-coordinate according to `attack_direction` and assign
/// each one the corresponding slot from the canonical slot table.
///
/// For attack-right teams (`attack_direction = +1`) players are sorted
/// ascending so the leftmost (deepest) player gets slot 0 (the GK position).
/// For attack-left teams (`attack_direction = -1`) players are sorted
/// descending so the rightmost (deepest) player gets slot 0.
fn assign_slots(world: &mut World, players: &mut [Entity], attack_direction: i8) {
    players.sort_by(|a, b| {
        let ax = world.entity(*a).get::<Position>().map_or(0.0, |p| p.0.x);
        let bx = world.entity(*b).get::<Position>().map_or(0.0, |p| p.0.x);
        if attack_direction > 0 {
            // Attacking right: sort ascending — smallest x = deepest = GK slot 0.
            ax.partial_cmp(&bx).unwrap_or(std::cmp::Ordering::Equal)
        } else {
            // Attacking left: sort descending — largest x = deepest = GK slot 0.
            bx.partial_cmp(&ax).unwrap_or(std::cmp::Ordering::Equal)
        }
    });

    let slots = if attack_direction > 0 {
        &ATTACK_RIGHT_SLOTS
    } else {
        &ATTACK_LEFT_SLOTS
    };

    for (i, entity) in players.iter().enumerate() {
        if let Some(target) = slots.get(i) {
            apply_player_slot(world, *entity, *target);
        }
    }
}
