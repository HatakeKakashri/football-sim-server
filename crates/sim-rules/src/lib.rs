use bevy_ecs::prelude::*;
use sim_components::{
    Ball, BallOutOfBoundsType, BallState, Match, Player, Position, RuleEvent, Skill, TeamId,
    TeamIdComponent, Velocity,
};
use sim_math::Vec2;

/// Phase 3: Tick counter resource for referee systems.
/// Inserted by sim-core before each schedule run.
#[derive(Resource, Debug, Clone, Copy)]
pub struct CurrentTick(pub u64);

pub const PITCH_LENGTH: f32 = 105.0;
pub const PITCH_WIDTH: f32 = 68.0;
pub const GOAL_WIDTH: f32 = 7.32;
pub const GOAL_Y_CENTER: f32 = PITCH_WIDTH / 2.0;
pub const GOAL_Y_MIN: f32 = GOAL_Y_CENTER - GOAL_WIDTH / 2.0; // 26.68
pub const GOAL_Y_MAX: f32 = GOAL_Y_CENTER + GOAL_WIDTH / 2.0; // 41.32
/// Center spot of the pitch
pub const CENTER_SPOT: (f32, f32) = (52.5, 34.0);
pub const KICKOFF_RESTART_TICKS: u64 = 60;

pub const SKILL_TOLERANCE: f32 = 0.1;

/// Tracks pending restart state: (event, tick_when_restart)
#[derive(Resource, Debug, Clone, Default)]
pub struct PendingRestart {
    pub event: Option<RuleEvent>,
    pub restart_tick: u64,
}

/// Phase 3: Out-of-bounds detection system (Law 9).
///
/// Detects when ball has gone out of pitch boundary and needs a restart.
///
/// NOTE: Physics (ball_physics_system) clamps positions into [0, 105]×[0, 68] and
/// bounces balls that would cross. After physics runs, a ball that was "going out"
/// will be at the boundary with reduced (possibly near-zero) velocity.
///
/// This system detects OOB by checking if a ball is:
///   1. At/near the boundary (within OOB_TOLERANCE)
///   2. Has low velocity (not actively moving toward play)
///   
/// This correctly handles: ball kicked out → physics bounces it back → OOB detects
/// it settled at the boundary → classifies restart type.
///
/// Does NOT reset ball position — that belongs to restart_system.
pub fn out_of_bounds_system(
    mut commands: Commands,
    mut ball_query: Query<(Entity, &Position, &Velocity, &mut Ball)>,
    _current_tick: Res<CurrentTick>,
) {
    // Collect entities that are at the boundary and need OOB classification
    let mut oob_events: Vec<(Entity, BallOutOfBoundsType)> = Vec::new();

    // Tolerance for "at boundary" detection (physics clamps to boundary)
    const OOB_TOLERANCE: f32 = 0.15;
    // Velocity threshold below which ball is considered "settled"
    const VELOCITY_THRESHOLD: f32 = 0.5;

    for (ball_entity, position, velocity, ball) in ball_query.iter() {
        // Skip balls already dead or in flight (unless settling from flight)
        if ball.state == BallState::Dead {
            continue;
        }

        let (x, y) = (position.0.x, position.0.y);
        let speed = velocity.0.length();

        // Only detect OOB for settled balls
        if speed >= VELOCITY_THRESHOLD {
            continue;
        }

        // Check if ball is at/near any boundary
        let at_left = x <= OOB_TOLERANCE;
        let at_right = x >= PITCH_LENGTH - OOB_TOLERANCE;
        let at_bottom = y <= OOB_TOLERANCE;
        let at_top = y >= PITCH_WIDTH - OOB_TOLERANCE;
        let at_goal_line = at_left || at_right;
        let at_touchline = at_bottom || at_top;

        if !at_goal_line && !at_touchline {
            continue; // Not at any boundary
        }

        // Corner: ball at intersection of goal line and touchline
        // (both x and y boundaries simultaneously)
        if at_goal_line && at_touchline {
            // Check if in goal area - if so, it's a GoalKick, not Corner
            if y >= GOAL_Y_MIN && y <= GOAL_Y_MAX {
                oob_events.push((ball_entity, BallOutOfBoundsType::GoalKick));
            } else {
                oob_events.push((ball_entity, BallOutOfBoundsType::Corner));
            }
            continue;
        }

        // Goal line (x=0 or x=105): goal area → GoalKick, outside → Corner
        if at_goal_line {
            if y >= GOAL_Y_MIN && y <= GOAL_Y_MAX {
                oob_events.push((ball_entity, BallOutOfBoundsType::GoalKick));
            } else {
                // Ball crossed goal line outside goal area → Corner
                oob_events.push((ball_entity, BallOutOfBoundsType::Corner));
            }
            continue;
        }

        // Touchline (y=0 or y=68): always ThrowIn
        if at_touchline {
            oob_events.push((ball_entity, BallOutOfBoundsType::ThrowIn));
            continue;
        }
    }

    // Now apply the state changes (separate pass to avoid borrow conflict)
    for (ball_entity, oob_type) in oob_events {
        if let Ok((_, _, _, mut ball)) = ball_query.get_mut(ball_entity) {
            ball.state = BallState::Dead;
        }
        commands
            .entity(ball_entity)
            .insert(RuleEvent::OutOfBounds(oob_type));
    }
}

/// Phase 3: Goal detection system (Law 10).
///
/// Detects when ball fully crosses goal line within goal area,
/// increments score, emits RuleEvent::Goal, sets BallState to Dead,
/// schedules KickoffRestart after 60 ticks.
///
/// BUG FIX: Guards against repeat scoring by checking BallState::Dead first.
pub fn goal_detection_system(
    mut commands: Commands,
    mut ball_query: Query<(Entity, &Position, &mut Ball)>,
    mut match_query: Query<&mut Match>,
    current_tick: Res<CurrentTick>,
) {
    // First pass: collect goal events (avoid borrow conflicts)
    let mut goal_events: Vec<(Entity, sim_components::TeamId, (u8, u8))> = Vec::new();

    for (ball_entity, ball_pos, ball) in ball_query.iter() {
        // BUG FIX: Do NOT score if ball is already Dead
        if ball.state == BallState::Dead {
            continue;
        }

        let in_goal_y = ball_pos.0.y >= GOAL_Y_MIN && ball_pos.0.y <= GOAL_Y_MAX;

        if !in_goal_y {
            continue;
        }

        // Away goal: ball at x <= 0 (left goal)
        if ball_pos.0.x <= 0.0 {
            if let Ok(mut m) = match_query.get_single_mut() {
                m.score.1 += 1;
                let final_score = m.score;
                goal_events.push((ball_entity, sim_components::TeamId(1), final_score));
                println!(
                    "GOAL scored by away team! Score: {}-{}",
                    final_score.0, final_score.1
                );
            }
        }
        // Home goal: ball at x >= PITCH_LENGTH (right goal)
        else if ball_pos.0.x >= PITCH_LENGTH {
            if let Ok(mut m) = match_query.get_single_mut() {
                m.score.0 += 1;
                let final_score = m.score;
                goal_events.push((ball_entity, sim_components::TeamId(0), final_score));
                println!(
                    "GOAL scored by home team! Score: {}-{}",
                    final_score.0, final_score.1
                );
            }
        }
    }

    // Second pass: apply state changes and emit events
    for (ball_entity, scorer_team, final_score) in goal_events {
        if let Ok((_, _, mut ball)) = ball_query.get_mut(ball_entity) {
            ball.state = BallState::Dead;
        }
        commands.entity(ball_entity).insert(RuleEvent::Goal {
            scorer_team,
            score: final_score,
        });
        commands.insert_resource(PendingRestart {
            event: Some(RuleEvent::KickoffRestart),
            restart_tick: current_tick.0 + KICKOFF_RESTART_TICKS,
        });
    }
}

/// Phase 3: Restart system.
///
/// Handles ball placement after out-of-bounds and goal events.
/// - KickoffRestart (from goal): place at center spot after 60-tick delay
/// - OutOfBounds: place at correct restart spot immediately
///
/// Reads RuleEvent directly from entity to determine restart type.
/// PendingRestart resource tracks the delayed kickoff restart timing.
pub fn restart_system(
    mut commands: Commands,
    mut ball_query: Query<(Entity, &mut Position, &mut Velocity, &mut Ball)>,
    rule_event_query: Query<(Entity, &RuleEvent)>,
    current_tick: Res<CurrentTick>,
    pending_restart: Option<Res<PendingRestart>>,
) {
    // Handle KickoffRestart from goal (delayed by 60 ticks)
    if let Some(ref pr) = pending_restart {
        if let Some(event) = &pr.event {
            if let RuleEvent::KickoffRestart = event {
                if current_tick.0 >= pr.restart_tick {
                    // Place ball at center spot
                    for (_, mut pos, mut vel, mut ball) in ball_query.iter_mut() {
                        pos.0 = Vec2::new(CENTER_SPOT.0, CENTER_SPOT.1);
                        vel.0 = Vec2::zero();
                        ball.position = Vec2::new(CENTER_SPOT.0, CENTER_SPOT.1);
                        ball.velocity = Vec2::zero();
                        ball.state = BallState::Free;
                    }
                    // Clear pending restart
                    commands.remove_resource::<PendingRestart>();
                }
            }
        }
    }

    // Handle OutOfBounds restarts by reading RuleEvent from entity
    let mut processed_entities: Vec<Entity> = Vec::new();

    for (entity, mut pos, mut vel, mut ball) in ball_query.iter_mut() {
        // Only process Dead balls
        if ball.state != BallState::Dead {
            continue;
        }

        // Check if this entity has a RuleEvent
        if let Ok((_, rule_event)) = rule_event_query.get(entity) {
            if let RuleEvent::OutOfBounds(oob_type) = rule_event {
                // Determine restart position based on OOB type and position
                let restart_pos = calculate_oob_restart_position(ball.position, *oob_type);
                pos.0 = restart_pos;
                vel.0 = Vec2::zero();
                ball.position = restart_pos;
                ball.velocity = Vec2::zero();
                ball.state = BallState::Free;
                processed_entities.push(entity);
            }
        }
    }

    // Remove RuleEvent component from processed entities
    for entity in processed_entities {
        commands.entity(entity).remove::<RuleEvent>();
    }
}

/// Calculate restart position for out-of-bounds based on event type and ball position.
///
/// Coordinate system:
/// - x-axis (0 to PITCH_LENGTH=105): goal line axis
/// - y-axis (0 to PITCH_WIDTH=68): touchline axis
///
/// Law 9:
/// - Crossing touchline (y<0 or y>68) → Throw-in at point where ball crossed
/// - Crossing goal line in goal area (y in [26.68, 41.32]) → GoalKick
/// - Crossing goal line outside goal area → Corner from nearest corner arc
fn calculate_oob_restart_position(pos: Vec2, oob_type: BallOutOfBoundsType) -> Vec2 {
    match oob_type {
        BallOutOfBoundsType::ThrowIn => {
            // Throw-in: ball crossed touchline (y<0 or y>68)
            // Place at the touchline where ball went out (same y, x where it crossed)
            // Since physics clamped, ball is now at y=0 or y=68
            if pos.y < PITCH_WIDTH / 2.0 {
                // Bottom touchline (y=0 side)
                Vec2::new(pos.x.clamp(0.0, PITCH_LENGTH), 0.0)
            } else {
                // Top touchline (y=68 side)
                Vec2::new(pos.x.clamp(0.0, PITCH_LENGTH), PITCH_WIDTH)
            }
        }
        BallOutOfBoundsType::GoalKick => {
            // Goal kick: ball crossed goal line in goal area
            // Place at penalty spot (11m from goal line for away, 94m for home)
            if pos.x < PITCH_LENGTH / 2.0 {
                // Left goal - away team goal kick
                Vec2::new(11.0, 34.0)
            } else {
                // Right goal - home team goal kick
                Vec2::new(94.0, 34.0)
            }
        }
        BallOutOfBoundsType::Corner => {
            // Corner: ball crossed goal line outside goal area
            // Place near the appropriate corner arc
            let corner_offset = 1.0; // 1m from corner flag

            if pos.x < PITCH_LENGTH / 2.0 {
                // Left side - check if top or bottom
                if pos.y < PITCH_WIDTH / 2.0 {
                    Vec2::new(corner_offset, corner_offset)
                } else {
                    Vec2::new(corner_offset, PITCH_WIDTH - corner_offset)
                }
            } else {
                // Right side - check if top or bottom
                if pos.y < PITCH_WIDTH / 2.0 {
                    Vec2::new(PITCH_LENGTH - corner_offset, corner_offset)
                } else {
                    Vec2::new(PITCH_LENGTH - corner_offset, PITCH_WIDTH - corner_offset)
                }
            }
        }
    }
}

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
pub fn offside_detection_system(
    ball_query: Query<(&Position, &Ball)>,
    player_query: Query<(Entity, &Position, &TeamIdComponent), Without<Ball>>,
) {
    let Ok((ball_pos, ball)) = ball_query.get_single() else {
        return;
    };

    let Some(last_toucher) = ball.last_touched_by else {
        return;
    };

    let Some(toucher_team_id) = player_query
        .get(last_toucher)
        .ok()
        .map(|(_, _, team)| team_id_u8(&team))
    else {
        return;
    };

    // Resolve attack polarity once — home (team 0) attacks +x, away (team 1) attacks -x.
    let dir = attacking_direction(TeamId(toucher_team_id));

    // Collect defender x-positions from the opposing team and find the
    // second-last (Law 11: the second-closest defender to their own goal).
    let mut defender_xs: Vec<f32> = player_query
        .iter()
        .filter(|(_, _, team)| team_id_u8(team) != toucher_team_id)
        .map(|(_, pos, _)| pos.0.x)
        .collect();
    defender_xs.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    // Fallback to own goal line (0.0) if fewer than 2 defenders exist.
    let second_last_defender_x = defender_xs.get(1).copied().unwrap_or(0.0);

    // Evaluate every player on the attacking team.
    for (_, pos, team) in player_query.iter() {
        if team_id_u8(&team) != toucher_team_id {
            continue;
        }

        let in_opponent_half = dir.in_opponent_half(pos.0.x);
        if !in_opponent_half {
            continue;
        }

        let closer_than_ball = dir.closer_than(pos.0.x, ball_pos.0.x);
        let closer_than_defender = dir.closer_than(pos.0.x, second_last_defender_x);

        if closer_than_ball && closer_than_defender {
            println!(
                "OFFSIDE position detected: player at ({:.1}, {:.1}), toucher team {}",
                pos.0.x, pos.0.y, toucher_team_id
            );
        }
    }
}

/// Attack polarity for a given team. Home (team 0) attacks toward +x
/// (right goal, x = PITCH_LENGTH); away (team 1) attacks toward -x
/// (left goal, x = 0).
#[derive(Debug, Clone, Copy)]
struct AttackingDirection {
    /// True if this team attacks in the positive-x direction.
    attacks_positive_x: bool,
}

impl AttackingDirection {
    /// Returns true if `player_x` is in the opponent's half.
    fn in_opponent_half(self, player_x: f32) -> bool {
        if self.attacks_positive_x {
            player_x > PITCH_LENGTH / 2.0
        } else {
            player_x < PITCH_LENGTH / 2.0
        }
    }

    /// Returns true if `player_x` is nearer to the attacking goal line
    /// than `reference_x`.  For a team attacking +x, nearer means larger x.
    fn closer_than(self, player_x: f32, reference_x: f32) -> bool {
        if self.attacks_positive_x {
            player_x > reference_x
        } else {
            player_x < reference_x
        }
    }
}

/// Returns the `AttackingDirection` for the given team.
/// Home (team 0) → attacks +x; away (team 1) → attacks -x.
fn attacking_direction(team: TeamId) -> AttackingDirection {
    AttackingDirection {
        attacks_positive_x: team.0 == 0,
    }
}

/// Extracts the raw `u8` team-id from a `TeamIdComponent`.
fn team_id_u8(team: &TeamIdComponent) -> u8 {
    team.0.0
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
    match_query: Query<&Match>,
) {
    let Ok(match_component) = match_query.get_single() else {
        return;
    };

    // Skip if match is not in play
    if match_component.state != sim_components::MatchState::InPlay {
        return;
    }

    // Phase 1: Detect high-speed collisions between opponents
    // Two players from opposing teams within 1.0m with high relative velocity
    let players: Vec<(Entity, sim_components::TeamId, Vec2, Vec2)> = player_query
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
                    commands.entity(e1).insert(RuleEvent::Foul {
                        fouler: e1.to_bits(),
                        foulee: e2.to_bits(),
                        foul_type: sim_components::FoulType::DangerousPlay,
                    });
                    commands.entity(e2).insert(RuleEvent::Foul {
                        fouler: e2.to_bits(),
                        foulee: e1.to_bits(),
                        foul_type: sim_components::FoulType::DangerousPlay,
                    });
                }
            }
        }
    }

    // Phase 1: Detect professional fouls (deliberate handball, DOGSO)
    // A player is committing professional foul if:
    // - They have the ball within 1.5m but are not the possessor (handball)
    // - They are in possession and a opponent is about to score (DOGSO)
    for (_entity, _player, _pos, vel) in player_query.iter() {
        let speed = vel.0.length();

        // Deliberate handball: very low movement speed while near ball but not possessing
        // This is a simplified check - full implementation would need ball position
        if speed < 0.5 {
            // Could be a professional foul - player holding ball deliberately

            // Phase 1: just log, don't penalize yet
        }
    }
}

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
    mut ball_query: Query<(&Position, &mut Ball)>,
    player_query: Query<(Entity, &Player, &Position, &Skill), With<Player>>,
) {
    for (ball_pos, mut ball) in ball_query.iter_mut() {
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
            continue;
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

        match closest_player {
            Some((entity, _, _)) => {
                ball.possessor = Some(entity);
                ball.state = BallState::Possessed;
            }
            None => {
                ball.possessor = None;
                ball.state = BallState::Free;
            }
        }
    }
}

pub fn referee_advantage_system(ball_query: Query<&mut Ball>, match_query: Query<&Match>) {
    // Phase N: real advantage window logic
    let _ = (ball_query, match_query);
}

pub fn added_time_calculation_system(mut referee_query: Query<&mut sim_components::Referee>) {
    for mut referee in referee_query.iter_mut() {
        let added_time: f32 = referee.stoppage_events.len() as f32 * 0.5;
        let added_time = added_time.min(10.0);
        referee.stoppage_events.clear();
        println!("Added time calculated: {} minutes", added_time);
    }
}

pub fn match_duration_enforcement_system(mut match_query: Query<&mut Match>) {
    for mut m in match_query.iter_mut() {
        if !m.clock.is_running {
            continue;
        }

        let max_time = if m.clock.half == 1 { 45.0 } else { 90.0 };
        let effective_time = max_time + m.clock.added_time;

        if m.clock.elapsed >= effective_time {
            m.clock.is_running = false;
            if m.clock.half == 1 {
                m.state = sim_components::MatchState::HalfTime;
                println!("Half time! Score: {}-{}", m.score.0, m.score.1);
            } else {
                m.state = sim_components::MatchState::FullTime;
                println!("Full time! Score: {}-{}", m.score.0, m.score.1);
            }
        }
    }
}

pub fn minimum_player_count_system(team_query: Query<&sim_components::Team>) {
    for team in team_query.iter() {
        let player_count = team.players.len();
        if player_count < 7 {
            println!(
                "WARNING: Team {} has only {} players (minimum 7 required)",
                team.name, player_count
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_components::{Player, TeamId};

    /// Produces a test Ball at `pos` with the given `state`.
    /// All other fields are zeroed; `possessor` and `last_touched_by` are `None`.
    fn ball_fixture(pos: sim_math::Vec2, state: BallState) -> Ball {
        Ball {
            position: pos,
            velocity: Vec2::zero(),
            spin: 0.0,
            state,
            possessor: None,
            last_touched_by: None,
        }
    }

    #[test]
    fn test_out_of_bounds_detection_throw_in() {
        let mut world = World::new();

        // Ball at bottom touchline (y=0, x=20) with low velocity - physics bounced it back
        // Throw-in: ball crosses touchline (y < 0 or y > PITCH_WIDTH)
        let ball_entity = world.spawn(()).id();
        world
            .entity_mut(ball_entity)
            .insert(ball_fixture(Vec2::new(20.0, 0.0), BallState::Free));
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(20.0, 0.0)));
        world
            .entity_mut(ball_entity)
            .insert(Velocity(Vec2::new(0.0, 0.0)));

        let mut schedule = Schedule::default();
        schedule.add_systems(out_of_bounds_system);

        // Run with current_tick = 0
        world.insert_resource(CurrentTick(0));
        schedule.run(&mut world);

        let ball = world.entity(ball_entity).get::<Ball>().unwrap();
        assert_eq!(ball.state, BallState::Dead);

        let rule_event = world.entity(ball_entity).get::<RuleEvent>();
        assert!(rule_event.is_some(), "RuleEvent should be emitted");
        if let Some(RuleEvent::OutOfBounds(oob_type)) = rule_event {
            assert!(
                matches!(oob_type, BallOutOfBoundsType::ThrowIn),
                "Ball at y=0 (touchline) should be ThrowIn"
            );
        }
    }

    #[test]
    fn test_out_of_bounds_detection_corner() {
        let mut world = World::new();

        // Ball at bottom-left corner (x=0.1, y=0.1) with low velocity
        // Both x and y are at the boundary, so it's a corner
        let ball_entity = world.spawn(()).id();
        world
            .entity_mut(ball_entity)
            .insert(ball_fixture(Vec2::new(0.1, 0.1), BallState::Free));
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(0.1, 0.1)));
        world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

        let mut schedule = Schedule::default();
        schedule.add_systems(out_of_bounds_system);

        world.insert_resource(CurrentTick(0));
        schedule.run(&mut world);

        let ball = world.entity(ball_entity).get::<Ball>().unwrap();
        assert_eq!(ball.state, BallState::Dead);

        let rule_event = world.entity(ball_entity).get::<RuleEvent>();
        assert!(rule_event.is_some());
        if let Some(RuleEvent::OutOfBounds(oob_type)) = rule_event {
            assert!(
                matches!(oob_type, BallOutOfBoundsType::Corner),
                "Ball at corner position should be Corner"
            );
        }
    }

    #[test]
    fn test_goal_scored_home_team() {
        let mut world = World::new();

        // Create match entity
        let match_entity = world.spawn(()).id();
        world.entity_mut(match_entity).insert(Match {
            id: 1,
            home_team: Entity::PLACEHOLDER,
            away_team: Entity::PLACEHOLDER,
            score: (0, 0),
            clock: sim_components::MatchClock {
                elapsed: 30.0,
                half: 1,
                added_time: 0.0,
                is_running: true,
            },
            state: sim_components::MatchState::InPlay,
            seed: 12345,
        });

        // Create ball entity in home goal (x = 105.5, y = 34)
        let ball_entity = world.spawn(()).id();
        world
            .entity_mut(ball_entity)
            .insert(ball_fixture(Vec2::new(105.5, 34.0), BallState::Free));
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(105.5, 34.0)));
        world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

        let mut schedule = Schedule::default();
        schedule.add_systems(goal_detection_system);

        world.insert_resource(CurrentTick(100));
        schedule.run(&mut world);

        let m = world.entity(match_entity).get::<Match>().unwrap();
        assert_eq!(m.score.0, 1);
        assert_eq!(m.score.1, 0);

        let ball = world.entity(ball_entity).get::<Ball>().unwrap();
        assert_eq!(ball.state, BallState::Dead);

        // Verify KickoffRestart was scheduled
        let pending = world.get_resource::<PendingRestart>();
        assert!(pending.is_some());
    }

    #[test]
    fn test_goal_scored_away_team() {
        let mut world = World::new();

        // Create match entity
        let match_entity = world.spawn(()).id();
        world.entity_mut(match_entity).insert(Match {
            id: 1,
            home_team: Entity::PLACEHOLDER,
            away_team: Entity::PLACEHOLDER,
            score: (0, 0),
            clock: sim_components::MatchClock {
                elapsed: 30.0,
                half: 1,
                added_time: 0.0,
                is_running: true,
            },
            state: sim_components::MatchState::InPlay,
            seed: 12345,
        });

        // Create ball entity in away goal (x = -0.5, y = 34)
        let ball_entity = world.spawn(()).id();
        world
            .entity_mut(ball_entity)
            .insert(ball_fixture(Vec2::new(-0.5, 34.0), BallState::Free));
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(-0.5, 34.0)));
        world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

        let mut schedule = Schedule::default();
        schedule.add_systems(goal_detection_system);

        world.insert_resource(CurrentTick(100));
        schedule.run(&mut world);

        let m = world.entity(match_entity).get::<Match>().unwrap();
        assert_eq!(m.score.0, 0);
        assert_eq!(m.score.1, 1);
    }

    #[test]
    fn test_no_double_goal_when_ball_dead() {
        let mut world = World::new();

        // Create match entity
        let match_entity = world.spawn(()).id();
        world.entity_mut(match_entity).insert(Match {
            id: 1,
            home_team: Entity::PLACEHOLDER,
            away_team: Entity::PLACEHOLDER,
            score: (0, 0),
            clock: sim_components::MatchClock {
                elapsed: 30.0,
                half: 1,
                added_time: 0.0,
                is_running: true,
            },
            state: sim_components::MatchState::InPlay,
            seed: 12345,
        });

        // Create ball entity in home goal with Dead state
        let ball_entity = world.spawn(()).id();
        world
            .entity_mut(ball_entity)
            .insert(ball_fixture(Vec2::new(105.5, 34.0), BallState::Dead));
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(105.5, 34.0)));
        world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

        let mut schedule = Schedule::default();
        schedule.add_systems(goal_detection_system);

        world.insert_resource(CurrentTick(100));
        schedule.run(&mut world);

        // Score should still be 0-0 because ball was already Dead
        let m = world.entity(match_entity).get::<Match>().unwrap();
        assert_eq!(m.score.0, 0);
        assert_eq!(m.score.1, 0);
    }

    #[test]
    fn test_kickoff_restart_places_ball_at_center() {
        let mut world = World::new();

        // Create match entity
        let match_entity = world.spawn(()).id();
        world.entity_mut(match_entity).insert(Match {
            id: 1,
            home_team: Entity::PLACEHOLDER,
            away_team: Entity::PLACEHOLDER,
            score: (1, 0),
            clock: sim_components::MatchClock {
                elapsed: 30.0,
                half: 1,
                added_time: 0.0,
                is_running: true,
            },
            state: sim_components::MatchState::InPlay,
            seed: 12345,
        });

        // Create ball entity in Dead state (after a goal)
        let ball_entity = world.spawn(()).id();
        world
            .entity_mut(ball_entity)
            .insert(ball_fixture(Vec2::new(105.5, 34.0), BallState::Dead));
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(105.5, 34.0)));
        world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

        // Set up pending restart for kickoff
        world.insert_resource(PendingRestart {
            event: Some(RuleEvent::KickoffRestart),
            restart_tick: 60, // Restart at tick 60
        });

        let mut schedule = Schedule::default();
        schedule.add_systems(restart_system);

        // Run at tick 60 - should trigger restart
        world.insert_resource(CurrentTick(60));
        schedule.run(&mut world);

        let ball = world.entity(ball_entity).get::<Ball>().unwrap();
        assert_eq!(ball.state, BallState::Free);
        assert_eq!(ball.position, Vec2::new(CENTER_SPOT.0, CENTER_SPOT.1));
        assert_eq!(ball.velocity, Vec2::zero());

        // PendingRestart should be cleared
        assert!(world.get_resource::<PendingRestart>().is_none());
    }

    #[test]
    fn test_out_of_bounds_goal_kick() {
        let mut world = World::new();

        // Ball in goal area on left side
        let ball_entity = world.spawn(()).id();
        world
            .entity_mut(ball_entity)
            .insert(ball_fixture(Vec2::new(-0.5, 34.0), BallState::Free));
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(-0.5, 34.0)));
        world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

        let mut schedule = Schedule::default();
        schedule.add_systems(out_of_bounds_system);

        world.insert_resource(CurrentTick(0));
        schedule.run(&mut world);

        let rule_event = world.entity(ball_entity).get::<RuleEvent>();
        assert!(rule_event.is_some());
        if let Some(RuleEvent::OutOfBounds(oob_type)) = rule_event {
            assert!(matches!(oob_type, BallOutOfBoundsType::GoalKick));
        }
    }

    #[test]
    fn test_possession_resolution() {
        let mut world = World::new();

        let ball_entity = world.spawn(()).id();
        world.entity_mut(ball_entity).insert(Ball {
            position: Vec2::new(50.0, 34.0),
            velocity: Vec2::zero(),
            spin: 0.0,
            state: BallState::Free,
            possessor: None,
            last_touched_by: None,
        });
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(50.0, 34.0)));

        let player_entity = world.spawn(()).id();
        world.entity_mut(player_entity).insert(Player {
            team_id: TeamId(0),
            position: Vec2::new(50.5, 34.0),
            velocity: Vec2::zero(),
            stamina: 0.8,
            role: sim_components::Role::Striker,
            skill: 0.8,
            intent: None,
            perception: None,
            score_differential: 0,
            time_remaining: 90.0,
            team_possession: 0.5,
            mentality_modifier: 0.0,
        });
        world
            .entity_mut(player_entity)
            .insert(Position(Vec2::new(50.5, 34.0)));
        world
            .entity_mut(player_entity)
            .insert(TeamIdComponent(TeamId(0)));
        world.entity_mut(player_entity).insert(Skill(0.8));

        let mut schedule = Schedule::default();
        schedule.add_systems(possession_resolution_system);
        schedule.run(&mut world);

        let ball = world.entity(ball_entity).get::<Ball>().unwrap();
        assert_eq!(ball.state, BallState::Possessed);
    }

    /// Test that possession is resolved to the actual player entity, not
    /// Entity::PLACEHOLDER.
    #[test]
    fn test_possession_resolved_to_real_entity() {
        let mut world = World::new();

        let ball_entity = world.spawn(()).id();
        world.entity_mut(ball_entity).insert(Ball {
            position: Vec2::new(50.0, 34.0),
            velocity: Vec2::zero(),
            spin: 0.0,
            state: BallState::Free,
            possessor: None,
            last_touched_by: None,
        });
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(50.0, 34.0)));

        // Player very close to ball (0.3 m)
        let player_entity = world.spawn(()).id();
        world.entity_mut(player_entity).insert(Player {
            team_id: TeamId(0),
            position: Vec2::new(50.3, 34.0),
            velocity: Vec2::zero(),
            stamina: 0.8,
            role: sim_components::Role::Striker,
            skill: 0.8,
            intent: None,
            perception: None,
            score_differential: 0,
            time_remaining: 90.0,
            team_possession: 0.5,
            mentality_modifier: 0.0,
        });
        world
            .entity_mut(player_entity)
            .insert(Position(Vec2::new(50.3, 34.0)));
        world
            .entity_mut(player_entity)
            .insert(TeamIdComponent(TeamId(0)));
        world.entity_mut(player_entity).insert(Skill(0.8));

        let mut schedule = Schedule::default();
        schedule.add_systems(possession_resolution_system);
        schedule.run(&mut world);

        let ball = world.entity(ball_entity).get::<Ball>().unwrap();
        assert_eq!(ball.state, BallState::Possessed);
        assert_eq!(
            ball.possessor,
            Some(player_entity),
            "Ball.possessor must be the real player entity, not PLACEHOLDER"
        );
        // last_touched_by should also be set (player is within 1.0 m)
        assert_eq!(ball.last_touched_by, Some(player_entity));
    }

    /// Test that possession is cleared when no player is within 1.5 m.
    #[test]
    fn test_possession_cleared_when_ball_free() {
        let mut world = World::new();

        let ball_entity = world.spawn(()).id();
        world.entity_mut(ball_entity).insert(Ball {
            position: Vec2::new(50.0, 34.0),
            velocity: Vec2::zero(),
            spin: 0.0,
            state: BallState::Free,
            possessor: None,
            last_touched_by: None,
        });
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(50.0, 34.0)));

        // Player far away from ball (> 1.5 m)
        let _player_entity = world.spawn(()).id();
        world.entity_mut(_player_entity).insert(Player {
            team_id: TeamId(0),
            position: Vec2::new(55.0, 34.0), // 5 m away
            velocity: Vec2::zero(),
            stamina: 0.8,
            role: sim_components::Role::Striker,
            skill: 0.8,
            intent: None,
            perception: None,
            score_differential: 0,
            time_remaining: 90.0,
            team_possession: 0.5,
            mentality_modifier: 0.0,
        });
        world
            .entity_mut(_player_entity)
            .insert(Position(Vec2::new(55.0, 34.0)));
        world
            .entity_mut(_player_entity)
            .insert(TeamIdComponent(TeamId(0)));
        world.entity_mut(_player_entity).insert(Skill(0.8));

        let mut schedule = Schedule::default();
        schedule.add_systems(possession_resolution_system);
        schedule.run(&mut world);

        let ball = world.entity(ball_entity).get::<Ball>().unwrap();
        assert_eq!(ball.state, BallState::Free);
        assert_eq!(
            ball.possessor, None,
            "Ball should have no possessor when no player is within 1.5 m"
        );
    }

    /// Test that last_touched_by is updated when a player touches the ball
    /// (simulates a pass scenario).
    #[test]
    fn test_offside_last_touch_tracked() {
        let mut world = World::new();

        let ball_entity = world.spawn(()).id();
        world.entity_mut(ball_entity).insert(Ball {
            position: Vec2::new(50.0, 34.0),
            velocity: Vec2::zero(),
            spin: 0.0,
            state: BallState::Free,
            possessor: None,
            last_touched_by: None,
        });
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(50.0, 34.0)));

        // Player A close to ball (will touch it)
        let player_a = world.spawn(()).id();
        world.entity_mut(player_a).insert(Player {
            team_id: TeamId(0),
            position: Vec2::new(50.2, 34.0), // 0.2 m from ball
            velocity: Vec2::zero(),
            stamina: 0.8,
            role: sim_components::Role::Striker,
            skill: 0.8,
            intent: None,
            perception: None,
            score_differential: 0,
            time_remaining: 90.0,
            team_possession: 0.5,
            mentality_modifier: 0.0,
        });
        world
            .entity_mut(player_a)
            .insert(Position(Vec2::new(50.2, 34.0)));
        world
            .entity_mut(player_a)
            .insert(TeamIdComponent(TeamId(0)));
        world.entity_mut(player_a).insert(Skill(0.8));

        let mut schedule = Schedule::default();
        schedule.add_systems(possession_resolution_system);
        schedule.run(&mut world);

        let ball = world.entity(ball_entity).get::<Ball>().unwrap();
        assert_eq!(
            ball.last_touched_by,
            Some(player_a),
            "Player A (closest to ball) should be recorded as last toucher"
        );

        // Now move the ball to player B (simulating a pass)
        {
            let mut em = world.entity_mut(ball_entity);
            let mut pos = em.get_mut::<Position>().unwrap();
            pos.0 = Vec2::new(80.0, 34.0);
        }
        {
            let mut em = world.entity_mut(ball_entity);
            let mut ball = em.get_mut::<Ball>().unwrap();
            ball.position = Vec2::new(80.0, 34.0);
            ball.state = BallState::Free;
            ball.possessor = None;
        }

        // Player B is now close to the new ball position
        let player_b = world.spawn(()).id();
        world.entity_mut(player_b).insert(Player {
            team_id: TeamId(0),
            position: Vec2::new(80.3, 34.0), // 0.3 m from ball
            velocity: Vec2::zero(),
            stamina: 0.9,
            role: sim_components::Role::CentralMidfielder,
            skill: 0.7,
            intent: None,
            perception: None,
            score_differential: 0,
            time_remaining: 90.0,
            team_possession: 0.5,
            mentality_modifier: 0.0,
        });
        world
            .entity_mut(player_b)
            .insert(Position(Vec2::new(80.3, 34.0)));
        world
            .entity_mut(player_b)
            .insert(TeamIdComponent(TeamId(0)));
        world.entity_mut(player_b).insert(Skill(0.7));

        // Player A is far from the new ball position
        {
            let mut em = world.entity_mut(player_a);
            let mut pos = em.get_mut::<Position>().unwrap();
            pos.0 = Vec2::new(50.0, 34.0);
        }

        schedule.run(&mut world);

        let ball = world.entity(ball_entity).get::<Ball>().unwrap();
        assert_eq!(
            ball.last_touched_by,
            Some(player_b),
            "After pass, last_touched_by should be player B (new closest)"
        );
        assert_eq!(ball.possessor, Some(player_b));
        assert_eq!(ball.state, BallState::Possessed);
    }

    #[test]
    fn test_match_duration_enforcement() {
        let mut world = World::new();

        let match_entity = world.spawn(()).id();
        world.entity_mut(match_entity).insert(Match {
            id: 1,
            home_team: Entity::PLACEHOLDER,
            away_team: Entity::PLACEHOLDER,
            score: (1, 0),
            clock: sim_components::MatchClock {
                elapsed: 48.0, // 45 + 3 added time
                half: 1,
                added_time: 3.0,
                is_running: true,
            },
            state: sim_components::MatchState::InPlay,
            seed: 12345,
        });

        let mut schedule = Schedule::default();
        schedule.add_systems(match_duration_enforcement_system);
        schedule.run(&mut world);

        let m = world.entity(match_entity).get::<Match>().unwrap();
        assert!(!m.clock.is_running);
        assert_eq!(m.state, sim_components::MatchState::HalfTime);
    }

    #[test]
    fn test_foul_detection_dangerous_play() {
        let mut world = World::new();

        // Create match entity in play
        let match_entity = world.spawn(()).id();
        world.entity_mut(match_entity).insert(Match {
            id: 1,
            home_team: Entity::PLACEHOLDER,
            away_team: Entity::PLACEHOLDER,
            score: (0, 0),
            clock: sim_components::MatchClock {
                elapsed: 30.0,
                half: 1,
                added_time: 0.0,
                is_running: true,
            },
            state: sim_components::MatchState::InPlay,
            seed: 12345,
        });

        // Create two opposing players very close together with high relative velocity
        let home_player = world.spawn(()).id();
        world.entity_mut(home_player).insert(Player {
            team_id: TeamId(0),
            position: Vec2::new(50.0, 34.0),
            velocity: Vec2::zero(),
            stamina: 0.8,
            role: sim_components::Role::CenterBack,
            skill: 0.7,
            intent: None,
            perception: None,
            score_differential: 0,
            time_remaining: 90.0,
            team_possession: 0.5,
            mentality_modifier: 0.0,
        });
        world
            .entity_mut(home_player)
            .insert(Position(Vec2::new(50.0, 34.0)));
        world
            .entity_mut(home_player)
            .insert(Velocity(Vec2::new(5.0, 0.0))); // Moving toward opponent
        world
            .entity_mut(home_player)
            .insert(TeamIdComponent(TeamId(0)));

        let away_player = world.spawn(()).id();
        world.entity_mut(away_player).insert(Player {
            team_id: TeamId(1),
            position: Vec2::new(50.5, 34.0), // Very close to home player
            velocity: Vec2::zero(),
            stamina: 0.8,
            role: sim_components::Role::Striker,
            skill: 0.7,
            intent: None,
            perception: None,
            score_differential: 0,
            time_remaining: 90.0,
            team_possession: 0.5,
            mentality_modifier: 0.0,
        });
        world
            .entity_mut(away_player)
            .insert(Position(Vec2::new(50.5, 34.0)));
        world
            .entity_mut(away_player)
            .insert(Velocity(Vec2::new(-5.0, 0.0))); // Moving toward opponent
        world
            .entity_mut(away_player)
            .insert(TeamIdComponent(TeamId(1)));

        let mut schedule = Schedule::default();
        schedule.add_systems(foul_detection_system);
        schedule.run(&mut world);

        // Players should have RuleEvent::Foul components from high-speed collision
        let home_foul = world.entity(home_player).get::<RuleEvent>();
        let away_foul = world.entity(away_player).get::<RuleEvent>();

        assert!(
            home_foul.is_some() || away_foul.is_some(),
            "At least one player should have a foul event from dangerous play"
        );
    }
}
