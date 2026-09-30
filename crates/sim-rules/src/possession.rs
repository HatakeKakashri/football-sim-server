//! Possession resolution.
//!
//! `possession_resolution_system` determines which player (if any) has
//! possession of the ball based on proximity and skill. It also tracks
//! `Ball.last_touched_by` for offside judgement.

use bevy_ecs::prelude::*;
use sim_components::{Ball, BallMarker, BallState, Player, Position, Skill};

use crate::constants::SKILL_TOLERANCE;

/// Phase 3: Possession resolution system.
///
/// Determines which player (if any) has possession of the ball based on
/// proximity. The source of truth for possession is `Ball.possessor`
/// (an `Option<Entity>`). `BallState` is a unit variant (`Possessed` or
/// `Free`) that mirrors the presence/absence of a possessor entity.
///
/// Rules:
/// - Find the player closest to the ball within 1.5 m.
/// - If multiple players are within range, the one with the higher skill
///   value wins; ties broken by distance.
/// - When a player is within range: set `ball.possessor = Some(entity)`
///   and `ball.state = BallState::Possessed`.
/// - When no player is within range: set `ball.possessor = None` and
///   `ball.state = BallState::Free`.
/// - Track `last_touched_by`: any player within 1.0 m of the ball is
///   considered to have touched it. Updated unconditionally (not gated
///   on `BallState::Free`) so that the last toucher is always current.
pub fn possession_resolution_system(
    ball_query: Query<&Position, With<BallMarker>>,
    player_query: Query<(Entity, &Player, &Position, &Skill), With<Player>>,
    mut ball: ResMut<Ball>,
) {
    let Ok(ball_pos) = ball_query.get_single() else {
        return;
    };

    // --- Last-touch tracking (Law 11) ---
    // Any player within 1.0 m of the ball is considered to have
    // touched it. We update unconditionally so `last_touched_by`
    // always reflects the most recent toucher.
    let mut last_toucher: Option<(Entity, f32)> = None;
    for (player_entity, _player, player_pos, _skill) in player_query.iter() {
        let dist = ball_pos.0.distance(player_pos.0);
        if dist <= 1.0 {
            match last_toucher {
                Some((_, best_dist)) => {
                    if dist < best_dist {
                        last_toucher = Some((player_entity, dist));
                    }
                }
                None => {
                    last_toucher = Some((player_entity, dist));
                }
            }
        }
    }
    if let Some((toucher, _)) = last_toucher {
        ball.last_touched_by = Some(toucher);
    }

    // --- Possession resolution ---
    // Only resolve possession when the ball is currently free.
    // Once possessed, possession is retained until the ball leaves
    // the 1.5 m radius or another system clears it (e.g. OOB).
    if ball.state != BallState::Free {
        return;
    }

    let mut closest_player: Option<(Entity, f32, f32)> = None;

    for (player_entity, _player, player_pos, skill) in player_query.iter() {
        let distance = ball_pos.0.distance(player_pos.0);
        if distance < 1.5 {
            if let Some((_, best_dist, best_skill)) = closest_player {
                let skill_diff = skill.0 - best_skill;
                if skill_diff > SKILL_TOLERANCE
                    || (skill_diff <= SKILL_TOLERANCE && distance < best_dist)
                {
                    closest_player = Some((player_entity, distance, skill.0));
                }
            } else {
                closest_player = Some((player_entity, distance, skill.0));
            }
        }
    }

    if let Some((entity, _, _)) = closest_player {
        ball.possessor = Some(entity);
        ball.state = BallState::Possessed;
    } else {
        ball.possessor = None;
        ball.state = BallState::Free;
    }
}