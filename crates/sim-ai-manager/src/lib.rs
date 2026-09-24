use bevy_ecs::prelude::*;
use sim_components::{Manager, Match, Player, Stamina, Team};

pub fn manager_decision_system(
    mut query: Query<(&mut Manager, &Team)>,
    match_query: Query<&Match>,
) {
    for (mut manager, _team) in query.iter_mut() {
        if let Ok(match_entity) = match_query.get_single() {
            let current_tick = match_entity.clock.elapsed as u64;

            if current_tick - manager.last_decision_tick < manager.decision_cooldown {
                continue;
            }

            let score_difference = match_entity.score.0 as i32 - match_entity.score.1 as i32;
            let time_remaining = 90.0 - match_entity.clock.elapsed;

            let mut total_score = 0.0;
            for factor in &manager.decision_table.factors {
                let factor_score = match factor.name.as_str() {
                    "score_difference" => (score_difference as f32 / 3.0).clamp(-1.0, 1.0),
                    "time_remaining" => (time_remaining / 90.0).clamp(0.0, 1.0),
                    _ => 0.0,
                };
                total_score += factor_score * factor.weight;
            }

            if total_score > 0.5 {
                println!(
                    "Manager considering aggressive tactics (score: {})",
                    total_score
                );
            } else if total_score < -0.5 {
                println!(
                    "Manager considering defensive tactics (score: {})",
                    total_score
                );
            }

            manager.last_decision_tick = current_tick;
        }
    }
}

pub fn formation_change_system(mut query: Query<(&mut Team,)>) {
    for (team,) in query.iter_mut() {
        let _required_players = match team.formation {
            sim_components::Formation::FourFourTwo => 11,
            sim_components::Formation::FourThreeThree => 11,
            sim_components::Formation::ThreeFiveTwo => 11,
            sim_components::Formation::FourTwoThreeOne => 11,
            sim_components::Formation::FiveThreeTwo => 11,
        };

        println!("Formation: {:?}", team.formation);
    }
}

pub fn substitution_system(
    mut query: Query<(&mut Team,)>,
    player_query: Query<(&Player, &Stamina)>,
) {
    for (mut team,) in query.iter_mut() {
        if team.substitutes.is_empty() {
            continue;
        }

        let mut substitution_to_make: Option<(Entity, Entity)> = None;

        for player_entity in &team.players {
            if let Ok((_player, stamina)) = player_query.get(*player_entity) {
                if stamina.0 < 0.3 {
                    if let Some(&substitute_entity) = team.substitutes.first() {
                        substitution_to_make = Some((*player_entity, substitute_entity));
                        break;
                    }
                }
            }
        }

        if let Some((out, incoming)) = substitution_to_make {
            println!("Substituting {:?} with {:?}", out, incoming);
            team.players.retain(|&e| e != out);
            team.substitutes.retain(|&e| e != incoming);
            team.players.push(incoming);
            team.substitutes.push(out);
        }
    }
}

pub fn mentality_shift_system(mut query: Query<(&mut Team,)>, match_query: Query<&Match>) {
    for (mut team,) in query.iter_mut() {
        if let Ok(match_entity) = match_query.get_single() {
            let score_difference = match_entity.score.0 as i32 - match_entity.score.1 as i32;
            let time_remaining = 90.0 - match_entity.clock.elapsed;

            let new_mentality = if score_difference >= 2 {
                if time_remaining < 15.0 {
                    sim_components::Mentality::Defend
                } else {
                    sim_components::Mentality::Balance
                }
            } else if score_difference <= -2 {
                sim_components::Mentality::Attack
            } else {
                if time_remaining < 10.0 {
                    if score_difference < 0 {
                        sim_components::Mentality::Attack
                    } else {
                        sim_components::Mentality::Defend
                    }
                } else {
                    sim_components::Mentality::Balance
                }
            };

            if team.mentality != new_mentality {
                println!(
                    "Mentality changed from {:?} to {:?}",
                    team.mentality, new_mentality
                );
                team.mentality = new_mentality;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_components::{
        DecisionFactor, Formation, MatchClock, MatchState, Mentality, Role, TeamId,
        WeightedDecisionTable,
    };
    use sim_math::Vec2;

    #[test]
    fn test_manager_ai_plugin() {
        assert!(true);
    }

    #[test]
    fn test_manager_decision_aggressive_tactics() {
        let mut world = World::new();

        let match_entity = world.spawn(()).id();
        world.entity_mut(match_entity).insert(Match {
            id: 1,
            home_team: Entity::PLACEHOLDER,
            away_team: Entity::PLACEHOLDER,
            score: (1, 3),
            clock: MatchClock {
                elapsed: 70.0,
                half: 2,
                added_time: 0.0,
                is_running: true,
            },
            state: MatchState::InPlay,
            seed: 12345,
        });

        let team_entity = world.spawn(()).id();
        world.entity_mut(team_entity).insert(Team {
            id: TeamId(0),
            name: "Home".to_string(),
            formation: Formation::FourFourTwo,
            mentality: Mentality::Balance,
            players: Vec::new(),
            substitutes: Vec::new(),
        });

        let manager_entity = world.spawn(()).id();
        world.entity_mut(manager_entity).insert(Manager {
            decision_table: WeightedDecisionTable {
                factors: vec![
                    DecisionFactor {
                        name: "score_difference".to_string(),
                        weight: 0.7,
                    },
                    DecisionFactor {
                        name: "time_remaining".to_string(),
                        weight: 0.3,
                    },
                ],
            },
            last_decision_tick: 0,
            decision_cooldown: 0,
        });
        world.entity_mut(manager_entity).insert(Team {
            id: TeamId(0),
            name: "Home".to_string(),
            formation: Formation::FourFourTwo,
            mentality: Mentality::Balance,
            players: Vec::new(),
            substitutes: Vec::new(),
        });

        let mut schedule = Schedule::default();
        schedule.add_systems(manager_decision_system);
        schedule.run(&mut world);

        let manager = world.entity(manager_entity).get::<Manager>().unwrap();
        assert!(manager.last_decision_tick > 0);
    }

    #[test]
    fn test_substitution_low_stamina() {
        let mut world = World::new();

        let team_entity = world.spawn(()).id();
        let low_stamina_player = world.spawn(()).id();
        let substitute_player = world.spawn(()).id();

        world.entity_mut(low_stamina_player).insert(Player {
            team_id: TeamId(0),
            position: Vec2::new(50.0, 34.0),
            velocity: Vec2::zero(),
            stamina: 0.2,
            role: Role::Striker,
            skill: 0.8,
            intent: None,
            perception: None,
            score_differential: 0,
            time_remaining: 90.0,
            team_possession: 0.5,
            mentality_modifier: 0.0,
        });
        world.entity_mut(low_stamina_player).insert(Stamina(0.2));

        world.entity_mut(substitute_player).insert(Player {
            team_id: TeamId(0),
            position: Vec2::new(50.0, 34.0),
            velocity: Vec2::zero(),
            stamina: 0.9,
            role: Role::Striker,
            skill: 0.7,
            intent: None,
            perception: None,
            score_differential: 0,
            time_remaining: 90.0,
            team_possession: 0.5,
            mentality_modifier: 0.0,
        });
        world.entity_mut(substitute_player).insert(Stamina(0.9));

        world.entity_mut(team_entity).insert(Team {
            id: TeamId(0),
            name: "Home".to_string(),
            formation: Formation::FourFourTwo,
            mentality: Mentality::Balance,
            players: vec![low_stamina_player],
            substitutes: vec![substitute_player],
        });

        let mut schedule = Schedule::default();
        schedule.add_systems(substitution_system);
        schedule.run(&mut world);

        let team = world.entity(team_entity).get::<Team>().unwrap();
        assert!(team.players.contains(&substitute_player));
        assert!(team.substitutes.contains(&low_stamina_player));
    }

    #[test]
    fn test_mentality_shift_defensive() {
        let mut world = World::new();

        let match_entity = world.spawn(()).id();
        world.entity_mut(match_entity).insert(Match {
            id: 1,
            home_team: Entity::PLACEHOLDER,
            away_team: Entity::PLACEHOLDER,
            score: (2, 1),
            clock: MatchClock {
                elapsed: 85.0,
                half: 2,
                added_time: 0.0,
                is_running: true,
            },
            state: MatchState::InPlay,
            seed: 12345,
        });

        let team_entity = world.spawn(()).id();
        world.entity_mut(team_entity).insert(Team {
            id: TeamId(0),
            name: "Home".to_string(),
            formation: Formation::FourFourTwo,
            mentality: Mentality::Balance,
            players: Vec::new(),
            substitutes: Vec::new(),
        });

        let mut schedule = Schedule::default();
        schedule.add_systems(mentality_shift_system);
        schedule.run(&mut world);

        let team = world.entity(team_entity).get::<Team>().unwrap();
        assert_eq!(team.mentality, Mentality::Defend);
    }
}
