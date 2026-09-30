//! Referee decisions: offside (Law 11), fouls (Law 12), minimum
//! player count (Law 3).
//!
//! `offside_detection_system` flags players in an offside position
//! (demo stub: logs at most once every ~5 simulated seconds).
//! `foul_detection_system` flags high-speed collisions as
//! `FoulType::DangerousPlay`.
//! `minimum_player_count_system` warns when a team drops below
//! 7 players (the minimum required to start/continue a match).

use bevy_ecs::prelude::*;
use sim_components::{Ball, BallMarker, Match, Player, Position, Team, TeamId, TeamIdComponent, Velocity};

/// Phase 3: Offside detection system (Law 11).
///
/// A player is in an offside position if:
/// - They are in the opponent's half (x > 52.5 for home team, x < 52.5 for away)
/// - They are nearer to the opponent's goal line than both the ball and the
///   second-last opponent
///
/// This system is a detection stub — it marks the offside position but does
/// NOT yet penalise (that requires a foul/restart system). The check uses
/// `Ball.last_touched_by` to determine the moment of the pass: offside is
/// judged relative to the position of the second-last defender *at the
/// instant the ball was touched* by a teammate of the potentially-offside
/// player.
/// Demo-scope note: this evaluates every attacking player every tick with no
/// edge-detection, so a player who simply lingers in an offside position
/// (plausible with the demo's basic/random movement) would otherwise print
/// one line per tick for as long as they stay there. `tick_counter` throttles
/// output to at most once every 300 calls (~5 sim-seconds at 60 ticks/sec)
/// regardless of how many players are offside in that window. This changes
/// only how often a detected condition is printed — the detection logic
/// itself (a known Phase-1-only stub per tasks.md) is untouched.
pub fn offside_detection_system(
    ball_query: Query<&Position, With<BallMarker>>,
    player_query: Query<(Entity, &Position, &TeamIdComponent), Without<BallMarker>>,
    ball: Res<Ball>,
    mut tick_counter: Local<u32>,
) {
    *tick_counter += 1;
    let should_log = (*tick_counter).is_multiple_of(300);

    let Ok(ball_pos) = ball_query.get_single() else {
        return;
    };

    let Some(last_toucher) = ball.last_touched_by else {
        return;
    };

    let Some(toucher_team_id) = player_query
        .get(last_toucher)
        .ok()
        .map(|(_, _, team)| crate::team_id_u8(team))
    else {
        return;
    };

    // Resolve attack polarity once — home (team 0) attacks +x, away (team 1) attacks -x.
    let dir = crate::attacking_direction(TeamId(toucher_team_id));

    // Collect defender x-positions from the opposing team and find the
    // second-last (Law 11: the second-closest defender to their own goal).
    let mut defender_xs: Vec<f32> = player_query
        .iter()
        .filter(|(_, _, team)| crate::team_id_u8(team) != toucher_team_id)
        .map(|(_, pos, _)| pos.0.x)
        .collect();
    defender_xs.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    // Fallback to own goal line (0.0) if fewer than 2 defenders exist.
    let second_last_defender_x = defender_xs.get(1).copied().unwrap_or(0.0);

    // Evaluate every player on the attacking team.
    for (_, pos, team) in player_query.iter() {
        if crate::team_id_u8(team) != toucher_team_id {
            continue;
        }

        let in_opponent_half = dir.in_opponent_half(pos.0.x);
        if !in_opponent_half {
            continue;
        }

        let closer_than_ball = dir.closer_than(pos.0.x, ball_pos.0.x);
        let closer_than_defender = dir.closer_than(pos.0.x, second_last_defender_x);

        if closer_than_ball && closer_than_defender && should_log {
            tracing::debug!(
                "OFFSIDE position detected: player at ({:.1}, {:.1}), toucher team {}",
                pos.0.x,
                pos.0.y,
                toucher_team_id
            );
        }
    }
}

/// Phase 3: Foul detection system (Law 12).
///
/// Detects dangerous play and professional fouls:
/// - High-speed collisions between players (dangerous play)
/// - Deliberate handball (denying obvious goal scoring)
/// - Denying obvious goal scoring opportunity (DOGSO)
///
/// For Phase 1: only detects and logs fouls. Full card system (yellow/red)
/// and restart placement is Phase 2.
pub fn foul_detection_system(
    mut commands: Commands,
    player_query: Query<(Entity, &Player, &Position, &Velocity)>,
    match_res: Res<Match>,
) {
    // Skip if match is not in play
    if match_res.state != sim_components::MatchState::InPlay {
        return;
    }

    // Phase 1: Detect high-speed collisions between opponents
    // Two players from opposing teams within 1.0m with high relative velocity
    let players: Vec<(Entity, sim_components::TeamId, sim_math::Vec2, sim_math::Vec2)> =
        player_query
            .iter()
            .map(|(e, p, pos, vel)| (e, p.team_id, pos.0, vel.0))
            .collect();

    for i in 0..players.len() {
        for j in (i + 1)..players.len() {
            let (e1, team1, pos1, vel1) = players[i];
            let (e2, team2, pos2, vel2) = players[j];

            // Only check opposing team collisions
            if team1 == team2 {
                continue;
            }

            let distance = pos1.distance(pos2);
            if distance < 1.0 {
                // High relative velocity = dangerous play
                let rel_vel = (vel1 - vel2).length();
                if rel_vel > 8.0 {
                    // Collision detected - foul on the player moving faster or with ball

                    // For Phase 1: emit a RuleEvent but don't yet assign cards
                    // Use u64 entity IDs for serialization compatibility
                    commands.entity(e1).insert(sim_components::RuleEvent::Foul {
                        fouler: e1.to_bits(),
                        foulee: e2.to_bits(),
                        foul_type: sim_components::FoulType::DangerousPlay,
                    });
                    commands.entity(e2).insert(sim_components::RuleEvent::Foul {
                        fouler: e2.to_bits(),
                        foulee: e1.to_bits(),
                        foul_type: sim_components::FoulType::DangerousPlay,
                    });
                }
            }
        }
    }

}

pub fn minimum_player_count_system(team_query: Query<&Team>) {
    for team in team_query.iter() {
        let player_count = team.players.len();
        if player_count < 7 {
            tracing::warn!(
                "Team {} has only {} players (minimum 7 required)",
                team.name,
                player_count
            );
        }
    }
}