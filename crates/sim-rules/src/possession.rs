//! Possession resolution.
//!
//! `possession_resolution_system` determines which player (if any) has
//! possession of the ball based on proximity and skill, and registers
//! a `LastTouch { team, player, kind: Play }` whenever the possessor
//! changes (interception / pass-receive).

use bevy_ecs::prelude::*;
use sim_components::{
    Ball, BallMarker, BallState, LastTouch, PendingBallTouches, Player, Position, Skill, TeamId,
    TouchKind, Velocity,
};
use sim_math::swept_proximity;

use crate::constants::SKILL_TOLERANCE;

/// Acquisition radius in metres.
///
/// A player acquires the ball when the minimum distance between their
/// path and the ball's path over a tick is ≤ this value.
///
/// This is a **swept test**: it accounts for the player and ball both
/// moving during the tick. A player moving at 5 m/tick (5 m/s at 60 Hz)
/// can traverse 5 m in a single tick, so a static end-of-tick radius
/// check at 1.5 m would miss the player entirely.
const ACQUISITION_RADIUS: f32 = 1.5;

/// Phase 3: Possession resolution system.
///
/// Determines which player (if any) has possession of the ball based on
/// proximity. The source of truth for possession is `Ball.possessor`
/// (an `Option<Entity>`). `BallState` is a unit variant (`Possessed` or
/// `Free`) that mirrors the presence/absence of a possessor entity.
///
/// Rules:
/// - Find the player whose swept path comes closest to the ball within
///   the acquisition radius (1.5 m).
/// - If multiple players' paths come within range, the one with the
///   higher skill value wins; ties broken by minimum distance.
/// - When a player's path intersects within range: set
///   `ball.possessor = Some(entity)` and `ball.state = BallState::Possessed`.
/// - When no player's path comes within range: set `ball.possessor = None`
///   and `ball.state = BallState::Free`.
/// - On possession change, register a `LastTouch { team, player, kind:
///   Play }` on `PendingBallTouches` (applied to `Ball.last_touch` by
///   the consumer `apply_ball_touched_system` at the start of the
///   Rules set) so Law 11 (offside) sees the new possessor as the most
///   recent "play" touch. Without this, offside would continue to be
///   evaluated for the previous kicker's team (Law 11 says it resets on
///   the next play).
///
/// # Swept Acquisition
///
/// The original implementation used a static end-of-tick radius check:
/// `distance < 1.5` at the tick boundary. This fails when a player's
/// velocity is large relative to the acquisition radius — a player moving
/// at 5 m/tick can be 5 m from the ball at tick start and 5 m from the
/// ball at tick end while crossing within 1.5 m at tick middle.
///
/// The swept test considers the full path: it reconstructs start
/// positions from `current_pos - velocity` (the position before physics
/// integrated the current tick's velocity) and checks the minimum distance
/// between the two paths over `t ∈ [0, 1]`.
pub fn possession_resolution_system(
    ball_query: Query<(&Position, &Velocity), With<BallMarker>>,
    player_query: Query<(Entity, &Player, &Position, &Velocity, &Skill), With<Player>>,
    mut ball: ResMut<Ball>,
    mut pending: ResMut<PendingBallTouches>,
) {
    let Ok((ball_pos, ball_vel)) = ball_query.get_single() else {
        return;
    };

    // --- Possession resolution ---
    // Only resolve possession when the ball is currently free.
    // Once possessed, possession is retained until the ball leaves
    // the acquisition radius or another system clears it (e.g. OOB).
    if ball.state != BallState::Free {
        return;
    }

    // Reconstruct ball start-of-tick position from velocity that was
    // applied by physics this tick: start_pos = current_pos - velocity.
    // This gives us the ball's position at the beginning of the tick,
    // before physics integrated the current velocity.
    let ball_start = ball_pos.0 - ball_vel.0;

    // `(entity, team, min_distance, skill)` — the team rides along with
    // the selection so the `LastTouch` record can be built without a
    // second lookup. `min_distance` is from the swept test.
    let mut closest_player: Option<(Entity, TeamId, f32, f32)> = None;

    for (player_entity, player, player_pos, player_vel, skill) in player_query.iter() {
        // Reconstruct player's start-of-tick position (before physics
        // integrated this tick's velocity).
        let player_start = player_pos.0 - player_vel.0;

        // Swept proximity: find minimum distance between player path and
        // ball path over the tick interval. If the minimum distance is
        // within ACQUISITION_RADIUS, the player acquires the ball.
        let proximity = swept_proximity(
            player_start,
            player_vel.0,
            ball_start,
            ball_vel.0,
            ACQUISITION_RADIUS,
        );

        if proximity.acquired {
            if let Some((_, _, best_dist, best_skill)) = closest_player {
                let skill_diff = skill.0 - best_skill;
                if skill_diff > SKILL_TOLERANCE
                    || (skill_diff <= SKILL_TOLERANCE && proximity.min_distance < best_dist)
                {
                    closest_player = Some((
                        player_entity,
                        player.team_id,
                        proximity.min_distance,
                        skill.0,
                    ));
                }
            } else {
                closest_player = Some((
                    player_entity,
                    player.team_id,
                    proximity.min_distance,
                    skill.0,
                ));
            }
        }
    }

    if let Some((entity, team, _, _)) = closest_player {
        // Register the new possessor as a Play touch: the previous
        // proximity-based `last_touched_by` would have lagged or
        // pointed at the wrong player. Law 11 resets offside at
        // this moment. The toucher's team (from `Player.team_id`,
        // carried through the selection above) is denormalised into
        // the record so it resolves even if the entity despawns.
        //
        // Push to `PendingBallTouches`; the consumer
        // `apply_ball_touched_system` applies it to `Ball.last_touch`
        // at the start of the Rules set.
        pending
            .0
            .push(LastTouch::by_player(team, entity, TouchKind::Play));
        ball.possessor = Some(entity);
        ball.state = BallState::Possessed;
    } else {
        ball.possessor = None;
        ball.state = BallState::Free;
    }
}
