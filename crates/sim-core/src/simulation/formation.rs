//! Formation reset (4-4-2 placeholder).

use bevy_ecs::prelude::*;
use sim_components::{Player, Position};
use sim_math::Vec2;

use super::lifecycle::apply_player_slot;

/// Phase 1 hardcoded 4-4-2 formation. Sorts players on each team by current
/// x-coordinate and assigns slot positions from the spec table:
///   home (`team_id` == 0) — GK x=5, defenders y=20 at x={20,35,50,65},
///     midfielders y=34 at x={25,40,55,70}, forwards y=34 at x={80,88}.
///   away (`team_id` == 1) — GK x=100, defenders y=48 at x={20,35,50,65},
///     midfielders y=34 at x={35,50,65,80}, forwards y=34 at x={17,25}.
/// Velocities are zeroed; intents reset to `HoldPosition`.
pub fn reset_formation_to_4_4_2(world: &mut World) {
    // Home formation targets — index 0 = GK, 1..=4 = defenders (sorted by x),
    // 5..=8 = midfielders (sorted by x), 9..=10 = forwards (sorted by x).
    let home_targets: [Vec2; 11] = [
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
    // Away formation targets — mirrored about pitch centre (52.5, 34).
    let away_targets: [Vec2; 11] = [
        Vec2::new(100.0, 34.0), // GK
        Vec2::new(20.0, 48.0),  // Defender 1 (y mirrored from 20 → 48)
        Vec2::new(35.0, 48.0),  // Defender 2
        Vec2::new(50.0, 48.0),  // Defender 3
        Vec2::new(65.0, 48.0),  // Defender 4
        Vec2::new(35.0, 34.0),  // Midfielder 1 (x mirrored: 70 → 35)
        Vec2::new(50.0, 34.0),  // Midfielder 2 (x mirrored: 55 → 50)
        Vec2::new(65.0, 34.0),  // Midfielder 3 (x mirrored: 40 → 65)
        Vec2::new(80.0, 34.0),  // Midfielder 4 (x mirrored: 25 → 80)
        Vec2::new(17.0, 34.0),  // Forward 1 (x mirrored: 88 → 17)
        Vec2::new(25.0, 34.0),  // Forward 2 (x mirrored: 80 → 25)
    ];

    let mut home_players: Vec<Entity> = Vec::new();
    let mut away_players: Vec<Entity> = Vec::new();
    for entity in world.iter_entities() {
        let id = entity.id();
        if let Some(p) = world.entity(id).get::<Player>() {
            if p.team_id.0 == 0 {
                home_players.push(id);
            } else if p.team_id.0 == 1 {
                away_players.push(id);
            }
        }
    }
    // Sort by current x to make slot assignment deterministic regardless of
    // entity spawn order.
    home_players.sort_by(|a, b| {
        let ax = world.entity(*a).get::<Position>().map_or(0.0, |p| p.0.x);
        let bx = world.entity(*b).get::<Position>().map_or(0.0, |p| p.0.x);
        ax.partial_cmp(&bx).unwrap_or(std::cmp::Ordering::Equal)
    });
    away_players.sort_by(|a, b| {
        let ax = world.entity(*a).get::<Position>().map_or(0.0, |p| p.0.x);
        let bx = world.entity(*b).get::<Position>().map_or(0.0, |p| p.0.x);
        // Away sorts descending so that the goalkeeper (rightmost) gets index 0.
        bx.partial_cmp(&ax).unwrap_or(std::cmp::Ordering::Equal)
    });

    for (i, entity) in home_players.iter().enumerate() {
        if let Some(target) = home_targets.get(i) {
            apply_player_slot(world, *entity, *target);
        }
    }
    for (i, entity) in away_players.iter().enumerate() {
        if let Some(target) = away_targets.get(i) {
            apply_player_slot(world, *entity, *target);
        }
    }
}