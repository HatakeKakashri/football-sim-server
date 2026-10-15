//! Referee decisions: fouls (Law 12), minimum player count (Law 3).
//!
//! Offside (Law 11) is out of MVP scope (PRD §2) and has no system.
//! `foul_detection_system` flags high-speed collisions as
//! `FoulType::DangerousPlay`.
//! `minimum_player_count_system` warns when a team drops below
//! 7 players (the minimum required to start/continue a match).

use bevy_ecs::prelude::*;
use sim_components::{Ball, BallState, Match, MatchState, Player, Position, Referee, StoppageEvent, Team, Velocity};
use sim_telemetry::TelemetryConfig;

use crate::CurrentTick;

/// Reason recorded when the ball sits `Dead` for a tick while the match
/// is in play (PRD §8.4: dead-ball time accrues stoppage).
pub const DEAD_BALL_STOPPAGE_REASON: &str = "dead_ball";
/// Reason recorded when an out-of-bounds restart is taken (PRD §8.4: the
/// restart itself is a stoppage even though the ball is `Dead` for less
/// than a full tick — detection and restart settle in the same tick).
pub const OOB_RESTART_STOPPAGE_REASON: &str = "oob_restart";

/// Stoppage tracking (PRD §8.4, Law 7).
///
/// Pushes one [`StoppageEvent`] per tick the ball spends `Dead` while
/// the match is in play. The queue accumulates for the whole half (it is
/// cleared at the half-time transition); `added_time_calculation_system`
/// derives the half's added time from its length, so one entry equals one
/// stoppage tick. Goal celebrations accrue here tick-by-tick through the
/// 60-tick kickoff delay; OOB restarts accrue a single entry each via
/// `restart_system` (same-tick settle, no `Dead` window to observe).
/// The only signals are tick counts, so determinism is preserved.
pub fn stoppage_tracking_system(
    mut referee_query: Query<&mut Referee>,
    ball: Res<Ball>,
    match_res: Res<Match>,
    current_tick: Option<Res<CurrentTick>>,
) {
    if match_res.state != MatchState::InPlay {
        return;
    }
    if ball.state != BallState::Dead {
        return;
    }
    let tick = current_tick.as_deref().map_or(0, |t| t.0);
    for mut referee in &mut referee_query {
        referee.stoppage_events.push(StoppageEvent {
            tick,
            reason: DEAD_BALL_STOPPAGE_REASON.to_string(),
        });
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
    current_tick: Option<Res<CurrentTick>>,
    telemetry: Option<Res<TelemetryConfig>>,
) {
    // Skip if match is not in play
    if match_res.state != sim_components::MatchState::InPlay {
        return;
    }

    // Phase 1: Detect high-speed collisions between opponents
    // Two players from opposing teams within 1.0m with high relative velocity
    let players: Vec<(
        Entity,
        sim_components::TeamId,
        sim_math::Vec2,
        sim_math::Vec2,
    )> = player_query
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
                    if telemetry.is_some() {
                        sim_telemetry::emit::foul(
                            current_tick.as_deref().map_or(0, |t| t.0),
                            e1.to_bits(),
                            e2.to_bits(),
                        );
                    }
                }
            }
        }
    }
}

pub fn minimum_player_count_system(
    team_query: Query<&Team>,
    current_tick: Option<Res<CurrentTick>>,
    telemetry: Option<Res<TelemetryConfig>>,
) {
    for team in team_query.iter() {
        let player_count = team.players.len();
        if player_count < 7 {
            tracing::warn!(
                "Team {} has only {} players (minimum 7 required)",
                team.name,
                player_count
            );
            if telemetry.is_some() {
                sim_telemetry::emit::low_player_count(
                    current_tick.as_deref().map_or(0, |t| t.0),
                    &team.name,
                    player_count,
                );
            }
        }
    }
}
