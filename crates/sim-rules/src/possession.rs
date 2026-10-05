//! Possession resolution.
//!
//! `possession_resolution_system` determines which player (if any) has
//! possession of the ball based on proximity and skill, and registers
//! a `LastTouch { kind: Play }` whenever the possessor changes
//! (interception / pass-receive).

use bevy_ecs::prelude::*;
use sim_components::{Ball, BallMarker, BallState, LastTouch, Player, Position, Skill, TouchKind};

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
/// - On possession change, register `ball.last_touch = Some(LastTouch
///   { entity, kind: Play })` so Law 11 (offside) sees the new
///   possessor as the most recent "play" touch. Without this,
///   offside would continue to be evaluated for the previous
///   kicker's team (Law 11 says it resets on the next play).
pub fn possession_resolution_system(
    ball_query: Query<&Position, With<BallMarker>>,
    player_query: Query<(Entity, &Player, &Position, &Skill), With<Player>>,
    mut ball: ResMut<Ball>,
) {
    let Ok(ball_pos) = ball_query.get_single() else {
        return;
    };

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
        // Register the new possessor as a Play touch: the previous
        // proximity-based `last_touched_by` would have lagged or
        // pointed at the wrong player. Law 11 resets offside at
        // this moment.
        ball.possessor = Some(entity);
        ball.state = BallState::Possessed;
        ball.last_touch = Some(LastTouch {
            entity,
            kind: TouchKind::Play,
        });
    } else {
        ball.possessor = None;
        ball.state = BallState::Free;
    }
}