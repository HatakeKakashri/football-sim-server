#![allow(
    clippy::needless_pass_by_value,
    reason = "Bevy system parameters (Query, Res, ...) must be taken by value; `&Query` is not a SystemParam"
)]

use bevy_ecs::prelude::*;
use sim_components::{Manager, Match, MatchClock, Player, Stamina, Team, time};

pub fn manager_decision_system(
    mut query: Query<(&mut Manager, &Team)>,
    match_res: Res<Match>,
    // Phase F follow-up: MatchClock is a Resource (spec §3).
    clock_res: Res<MatchClock>,
) {
    for (mut manager, _team) in &mut query {
        let current_tick = clock_res.elapsed_ticks;

        if current_tick - manager.last_decision_tick < manager.decision_cooldown {
            continue;
        }

        let score_difference = i16::from(match_res.score.0) - i16::from(match_res.score.1);

        // Use shared time utilities for consistent time remaining calculation
        let time_remaining_secs = time::match_time_remaining_secs(&clock_res);

        let mut total_score = 0.0;
        for factor in &manager.decision_table.factors {
            let factor_score = match factor.name.as_str() {
                "score_difference" => (f32::from(score_difference) / 3.0).clamp(-1.0, 1.0),
                "time_remaining" => (time_remaining_secs / (90.0 * 60.0)).clamp(0.0, 1.0),
                _ => 0.0,
            };
            total_score += factor_score * factor.weight;
        }

        if total_score > 0.5 {
            tracing::debug!("Manager considering aggressive tactics (score: {total_score})");
        } else if total_score < -0.5 {
            tracing::debug!("Manager considering defensive tactics (score: {total_score})");
        }

        manager.last_decision_tick = current_tick;
    }
}

pub fn formation_change_system(mut query: Query<(&mut Team,)>) {
    for (team,) in &mut query {
        use sim_components::Formation;

        let _required_players = match team.formation {
            Formation::FourFourTwo
            | Formation::FourThreeThree
            | Formation::ThreeFiveTwo
            | Formation::FourTwoThreeOne
            | Formation::FiveThreeTwo => 11,
        };

        tracing::debug!("Formation: {:?}", team.formation);
    }
}

pub fn substitution_system(
    mut query: Query<(&mut Team,)>,
    player_query: Query<(&Player, &Stamina)>,
) {
    for (mut team,) in &mut query {
        if team.substitutes.is_empty() {
            continue;
        }

        let mut substitution_to_make: Option<(Entity, Entity)> = None;

        for player_entity in &team.players {
            if let Ok((_player, stamina)) = player_query.get(*player_entity)
                && stamina.0 < 0.3
                && let Some(&substitute_entity) = team.substitutes.first()
            {
                substitution_to_make = Some((*player_entity, substitute_entity));
                break;
            }
        }

        if let Some((out, incoming)) = substitution_to_make {
            tracing::info!("Substituting {out:?} with {incoming:?}");
            team.players.retain(|&e| e != out);
            team.substitutes.retain(|&e| e != incoming);
            team.players.push(incoming);
            team.substitutes.push(out);
        }
    }
}

pub fn mentality_shift_system(
    mut query: Query<(&mut Team,)>,
    match_res: Res<Match>,
    // Phase F follow-up: MatchClock is a Resource (spec §3).
    clock_res: Res<MatchClock>,
) {
    for (mut team,) in &mut query {
        let score_difference = i32::from(match_res.score.0) - i32::from(match_res.score.1);

        // Use shared time utilities for consistent time remaining calculation (in minutes)
        let time_remaining_mins = time::match_time_remaining_mins(&clock_res);

        let new_mentality = if score_difference >= 2 {
            if time_remaining_mins < 15.0 {
                sim_components::Mentality::Defend
            } else {
                sim_components::Mentality::Balance
            }
        } else if score_difference <= -2 {
            sim_components::Mentality::Attack
        } else if time_remaining_mins < 10.0 {
            if score_difference < 0 {
                sim_components::Mentality::Attack
            } else {
                sim_components::Mentality::Defend
            }
        } else {
            sim_components::Mentality::Balance
        };

        if team.mentality != new_mentality {
            tracing::info!(
                "Mentality changed from {:?} to {:?}",
                team.mentality,
                new_mentality
            );
            team.mentality = new_mentality;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_components::{
        DecisionFactor, Formation, MatchClock, MatchState, Mentality, Position, Role,
        RoleComponent, Skill, Stamina, TeamId, Velocity, WeightedDecisionTable,
    };
    use sim_math::Vec2;

    #[test]
    fn test_manager_decision_aggressive_tactics() {
        let mut world = World::new();

        world.insert_resource(Match {
            id: 1,
            home_team: Entity::PLACEHOLDER,
            away_team: Entity::PLACEHOLDER,
            score: (1, 3),
            state: MatchState::InPlay,
            seed: 12345,
        });
        let match_entity = world.spawn(()).id();
        let clock = MatchClock {
            elapsed_ticks: 70 * 60,
            half: 2,
            added_time_ticks: 0,
            is_running: true,
        };
        // Phase F follow-up: MatchClock is a Resource (spec §3). Insert
        // both forms — the Resource is the source of truth for systems;
        // the Component on the entity is kept for backwards compat.
        world.insert_resource(clock.clone());
        world.entity_mut(match_entity).insert(clock);

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
            intent: None,
        });
        world
            .entity_mut(low_stamina_player)
            .insert(Position(Vec2::new(50.0, 34.0)));
        world
            .entity_mut(low_stamina_player)
            .insert(Velocity(Vec2::zero()));
        world.entity_mut(low_stamina_player).insert(Stamina(0.2));
        world
            .entity_mut(low_stamina_player)
            .insert(RoleComponent(Role::Striker));
        world.entity_mut(low_stamina_player).insert(Skill(0.8));

        world.entity_mut(substitute_player).insert(Player {
            team_id: TeamId(0),
            intent: None,
        });
        world
            .entity_mut(substitute_player)
            .insert(Position(Vec2::new(50.0, 34.0)));
        world
            .entity_mut(substitute_player)
            .insert(Velocity(Vec2::zero()));
        world.entity_mut(substitute_player).insert(Stamina(0.9));
        world
            .entity_mut(substitute_player)
            .insert(RoleComponent(Role::Striker));
        world.entity_mut(substitute_player).insert(Skill(0.7));

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

        world.insert_resource(Match {
            id: 1,
            home_team: Entity::PLACEHOLDER,
            away_team: Entity::PLACEHOLDER,
            score: (2, 1),
            state: MatchState::InPlay,
            seed: 12345,
        });
        let match_entity = world.spawn(()).id();
        // 5 minutes remaining in match = 85 minutes total elapsed = 45 min (half 1) + 40 min (half 2)
        let clock = MatchClock {
            elapsed_ticks: 40 * 60 * 60, // 40 minutes into second half
            half: 2,
            added_time_ticks: 0,
            is_running: true,
        };
        // Phase F follow-up: MatchClock is a Resource (spec §3). Insert
        // both forms — the Resource is the source of truth for systems;
        // the Component on the entity is kept for backwards compat.
        world.insert_resource(clock.clone());
        world.entity_mut(match_entity).insert(clock);

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
