use bevy_ecs::prelude::*;
use sim_components::{Ball, Player, Position, Skill, TeamId, TeamIdComponent, Match, BallState, Referee};
use sim_math::Vec2;

pub const PITCH_LENGTH: f32 = 105.0;
pub const PITCH_WIDTH: f32 = 68.0;
pub const GOAL_WIDTH: f32 = 7.32;
pub const GOAL_Y_CENTER: f32 = PITCH_WIDTH / 2.0;
pub const SKILL_TOLERANCE: f32 = 0.1;

pub fn offside_detection_system(
    ball_query: Query<&Position, With<Ball>>,
    player_query: Query<(&Position, &TeamIdComponent), Without<Ball>>,
) {
    let _ball_pos = ball_query.single();
    let _players: Vec<(&Position, &TeamIdComponent)> = player_query.iter().collect();
}

pub fn out_of_bounds_detection_system(
    mut ball_query: Query<(&mut Position, &mut Ball)>,
) {
    for (mut position, mut ball) in ball_query.iter_mut() {
        if position.0.x < 0.0 || position.0.x > PITCH_LENGTH ||
           position.0.y < 0.0 || position.0.y > PITCH_WIDTH {
            ball.state = BallState::OutOfPlay;
            position.0.x = position.0.x.clamp(0.0, PITCH_LENGTH);
            position.0.y = position.0.y.clamp(0.0, PITCH_WIDTH);
            ball.velocity = Vec2::zero();
        }
    }
}

pub fn goal_detection_system(
    ball_query: Query<(&Position, &Ball)>,
    mut match_query: Query<&mut Match>,
) {
    for (ball_pos, ball) in ball_query.iter() {
        if ball.state == BallState::OutOfPlay || ball.state == BallState::Dead {
            continue;
        }

        let in_goal_y = ball_pos.0.y >= GOAL_Y_CENTER - GOAL_WIDTH / 2.0
            && ball_pos.0.y <= GOAL_Y_CENTER + GOAL_WIDTH / 2.0;

        if in_goal_y {
            if ball_pos.0.x <= 0.0 {
                if let Ok(mut m) = match_query.get_single_mut() {
                    m.score.1 += 1;
                    println!("GOAL scored by away team! Score: {}-{}", m.score.0, m.score.1);
                }
            } else if ball_pos.0.x >= PITCH_LENGTH {
                if let Ok(mut m) = match_query.get_single_mut() {
                    m.score.0 += 1;
                    println!("GOAL scored by home team! Score: {}-{}", m.score.0, m.score.1);
                }
            }
        }
    }
}

pub fn foul_detection_system(
    player_query: Query<(&Position, &TeamIdComponent)>,
) {
    let _players: Vec<(&Position, &TeamIdComponent)> = player_query.iter().collect();
}

pub fn possession_resolution_system(
    mut ball_query: Query<(&Position, &mut Ball)>,
    player_query: Query<(&Position, &TeamIdComponent, &Skill)>,
) {
    for (ball_pos, mut ball) in ball_query.iter_mut() {
        if ball.state != BallState::Free {
            continue;
        }

        let mut closest_player: Option<(Entity, f32, f32)> = None;

        for (player_pos, _team_id, skill) in player_query.iter() {
            let distance = ball_pos.0.distance(player_pos.0);
            if distance < 1.5 {
                if let Some((_, best_dist, best_skill)) = closest_player {
                    let skill_diff = skill.0 - best_skill;
                    if skill_diff > SKILL_TOLERANCE || (skill_diff <= SKILL_TOLERANCE && distance < best_dist) {
                        closest_player = Some((Entity::PLACEHOLDER, distance, skill.0));
                    }
                } else {
                    closest_player = Some((Entity::PLACEHOLDER, distance, skill.0));
                }
            }
        }

        if let Some((_, _, _)) = closest_player {
            ball.state = BallState::Possessed;
        }
    }
}

pub fn referee_advantage_system(
    _referee_query: Query<&Referee>,
) {
    // Placeholder for referee advantage system
}

pub fn added_time_calculation_system(
    mut referee_query: Query<&mut Referee>,
) {
    for mut referee in referee_query.iter_mut() {
        let added_time: f32 = referee.stoppage_events.len() as f32 * 0.5;
        let added_time = added_time.min(10.0);
        referee.stoppage_events.clear();
        println!("Added time calculated: {} minutes", added_time);
    }
}

pub fn match_duration_enforcement_system(
    mut match_query: Query<&mut Match>,
) {
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

pub fn minimum_player_count_system(
    team_query: Query<&sim_components::Team>,
) {
    for team in team_query.iter() {
        let player_count = team.players.len();
        if player_count < 7 {
            println!("WARNING: Team {} has only {} players (minimum 7 required)", team.name, player_count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_out_of_bounds_detection() {
        let mut world = World::new();

        let ball_entity = world.spawn(()).id();
        world.entity_mut(ball_entity).insert(Ball {
            position: Vec2::new(-1.0, 34.0),
            velocity: Vec2::new(-5.0, 0.0),
            spin: 0.0,
            state: BallState::Free,
            possessor: None,
        });
        world.entity_mut(ball_entity).insert(Position(Vec2::new(-1.0, 34.0)));

        let mut schedule = Schedule::default();
        schedule.add_systems(out_of_bounds_detection_system);
        schedule.run(&mut world);

        let ball = world.entity(ball_entity).get::<Ball>().unwrap();
        assert_eq!(ball.state, BallState::OutOfPlay);
        assert_eq!(ball.velocity, Vec2::zero());
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
        });
        world.entity_mut(ball_entity).insert(Position(Vec2::new(50.0, 34.0)));

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
        });
        world.entity_mut(player_entity).insert(Position(Vec2::new(50.5, 34.0)));
        world.entity_mut(player_entity).insert(TeamIdComponent(TeamId(0)));
        world.entity_mut(player_entity).insert(Skill(0.8));

        let mut schedule = Schedule::default();
        schedule.add_systems(possession_resolution_system);
        schedule.run(&mut world);

        let ball = world.entity(ball_entity).get::<Ball>().unwrap();
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
}
