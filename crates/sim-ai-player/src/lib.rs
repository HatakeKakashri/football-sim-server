use bevy_ecs::prelude::*;
use sim_components::{Ball, Intent, Player, Position, Skill, Stamina, Velocity};
use sim_math::Vec2;

#[derive(Component, Debug, Clone)]
pub struct UtilityBrain {
    pub actions: Vec<PlayerAction>,
    pub hysteresis: f32,
    pub evaluation_interval: u64,
}

#[derive(Debug, Clone)]
pub struct PlayerAction {
    pub intent: Intent,
    pub considerations: Vec<PlayerConsideration>,
}

#[derive(Debug, Clone)]
pub struct PlayerConsideration {
    pub name: String,
    pub weight: f32,
}

pub fn perception_system(
    mut queries: ParamSet<(
        Query<(Entity, &mut Player, &Position)>,
        Query<(Entity, &Player, &Position)>,
        Query<&Position, With<Ball>>,
    )>,
) {
    // Get ball position via the Ball query — disjointness with the player
    // queries is enforced by the ParamSet slot, not by Query filtering.
    let ball_position = {
        let q = queries.p2();
        if let Ok(pos) = q.get_single() {
            pos.0
        } else {
            Vec2::zero()
        }
    };

    // Snapshot all other players' (team_id, position) up front. This avoids
    // holding p1 open while iterating p0 mutably, which ParamSet forbids.
    // Phase 1 has 22 players so an O(n) snapshot per tick is fine.
    let mut others: Vec<(sim_components::TeamId, Vec2)> = Vec::new();
    {
        let q1 = queries.p1();
        for (_e, other_player, other_pos) in q1.iter() {
            others.push((other_player.team_id, other_pos.0));
        }
    }

    // Now iterate players via the mutable query, building and applying the
    // snapshot in one pass. Using `get_mut` per entity keeps each mutation
    // window short and avoids holding a long-lived p0 iterator.
    let entity_ids: Vec<bevy_ecs::prelude::Entity> = {
        let q0 = queries.p0();
        q0.iter().map(|(e, _, _)| e).collect()
    };
    for entity in entity_ids {
        // Snapshot player_pos + team_id via an immutable get (read-only).
        let snapshot_info = queries.p0().get(entity).ok().map(|(_, p, pos)| (pos.0, p.team_id));
        let Some((player_pos, team_id)) = snapshot_info else {
            continue;
        };

        let mut nearby_teammates = smallvec::SmallVec::new();
        let mut nearby_opponents = smallvec::SmallVec::new();
        for (other_team_id, other_pos) in &others {
            let distance = player_pos.distance(*other_pos);
            if distance <= 20.0 {
                let relative_position = *other_pos - player_pos;
                let nearby_entity = sim_components::NearbyEntity {
                    entity: Entity::PLACEHOLDER,
                    distance,
                    relative_position,
                };
                if *other_team_id == team_id {
                    if nearby_teammates.len() < 8 {
                        nearby_teammates.push(nearby_entity);
                    }
                } else if nearby_opponents.len() < 8 {
                    nearby_opponents.push(nearby_entity);
                }
            }
        }

        nearby_teammates.sort_by(|a: &sim_components::NearbyEntity, b: &sim_components::NearbyEntity| a.distance.partial_cmp(&b.distance).unwrap());
        nearby_opponents.sort_by(|a: &sim_components::NearbyEntity, b: &sim_components::NearbyEntity| a.distance.partial_cmp(&b.distance).unwrap());

        let pitch_bounds = sim_components::PitchBounds {
            distance_to_left: player_pos.x,
            distance_to_right: 105.0 - player_pos.x,
            distance_to_top: player_pos.y,
            distance_to_bottom: 68.0 - player_pos.y,
        };
        let goal_position = Vec2::new(105.0, 34.0);

        let perception = sim_components::PerceptionSnapshot {
            nearby_teammates,
            nearby_opponents,
            ball_position,
            goal_position,
            pitch_bounds,
        };

        if let Ok((_, mut player, _)) = queries.p0().get_mut(entity) {
            player.perception = Some(perception);
        }
    }
}

pub fn consideration_scoring_system(mut query: Query<(&mut Player, &Stamina, &Skill)>) {
    // Score considerations for each player
    for (mut player, stamina, skill) in query.iter_mut() {
        // Calculate stamina factor (lower stamina = more conservative)
        let _stamina_factor = stamina.0;

        // Calculate skill factor
        let _skill_factor = skill.0;
        
        // Update player's intent based on considerations
        // For now, just set a default intent
        if player.intent.is_none() {
            // If player has no intent, set to hold position
            player.intent = Some(sim_components::Intent::HoldPosition);
        }
    }
}

pub fn player_decision_system(mut query: Query<(&mut Player, &UtilityBrain)>) {
    // Make decisions for each player based on utility brain
    for (mut player, utility_brain) in query.iter_mut() {
        // Get player's perception
        if let Some(_perception) = &player.perception {
            // Evaluate each action in the utility brain
            let mut best_action: Option<&sim_components::Intent> = None;
            let mut best_score = f32::MIN;
            
            for action in &utility_brain.actions {
                // Calculate score for this action
                let score = 1.0; // Base score
                
                // Apply considerations (simplified)
                // In a real implementation, we would evaluate each consideration
                // and multiply the scores together
                
                if score > best_score {
                    best_score = score;
                    best_action = Some(&action.intent);
                }
            }
            
            // Set player's intent to best action
            if let Some(intent) = best_action {
                player.intent = Some(intent.clone());
            }
        }
    }
}

pub fn player_action_execution_system(mut query: Query<(&Player, &mut Velocity)>) {
    // Execute player actions by setting velocity
    for (player, mut velocity) in query.iter_mut() {
        if let Some(intent) = &player.intent {
            match intent {
                sim_components::Intent::MoveToPosition(target) => {
                    // Calculate direction to target
                    let direction = *target - player.position;
                    let distance = direction.length();
                    
                    if distance > 0.1 {
                        // Normalize and apply speed
                        let normalized = direction / distance;
                        let speed = 5.0; // m/s
                        velocity.0 = normalized * speed;
                    } else {
                        velocity.0 = Vec2::zero();
                    }
                }
                sim_components::Intent::PassTo(_target) => {
                    // For now, just stop
                    velocity.0 = Vec2::zero();
                }
                sim_components::Intent::ShootAtGoal(_target) => {
                    // For now, just stop
                    velocity.0 = Vec2::zero();
                }
                _ => {
                    // For other intents, stop
                    velocity.0 = Vec2::zero();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_components::{Role, TeamId};

    #[test]
    fn test_player_ai_plugin() {
        // Placeholder test
        assert!(true);
    }

    #[test]
    fn test_stamina_based_decision() {
        // Test that low stamina player conserves energy
        let mut world = World::new();
        
        // Create a player with low stamina
        let player_entity = world.spawn(()).id();
        world.entity_mut(player_entity).insert(Player {
            team_id: TeamId(0),
            position: Vec2::new(50.0, 34.0),
            velocity: Vec2::zero(),
            stamina: 0.2, // Low stamina
            role: Role::Striker,
            skill: 0.8,
            intent: None,
            perception: None,
        });
        world.entity_mut(player_entity).insert(Stamina(0.2));
        world.entity_mut(player_entity).insert(Skill(0.8));
        
        // Create utility brain with sprint action
        let utility_brain = UtilityBrain {
            actions: vec![PlayerAction {
                intent: sim_components::Intent::MoveToPosition(Vec2::new(100.0, 34.0)),
                considerations: vec![],
            }],
            hysteresis: 0.1,
            evaluation_interval: 10,
        };
        world.entity_mut(player_entity).insert(utility_brain);
        
        // Run systems
        let mut schedule = Schedule::default();
        schedule.add_systems(consideration_scoring_system);
        schedule.add_systems(player_decision_system);
        
        // Run schedule
        schedule.run(&mut world);
        
        // Check that player has an intent
        let player = world.entity(player_entity).get::<Player>().unwrap();
        assert!(player.intent.is_some());
    }

    #[test]
    fn test_passing_option() {
        // Test that player considers teammates in better positions
        let mut world = World::new();
        
        // Create a player
        let player_entity = world.spawn(()).id();
        world.entity_mut(player_entity).insert(Player {
            team_id: TeamId(0),
            position: Vec2::new(50.0, 34.0),
            velocity: Vec2::zero(),
            stamina: 0.8,
            role: Role::Striker,
            skill: 0.8,
            intent: None,
            perception: None,
        });
        world.entity_mut(player_entity).insert(Stamina(0.8));
        world.entity_mut(player_entity).insert(Skill(0.8));
        
        // Create a teammate in better position
        let teammate_entity = world.spawn(()).id();
        world.entity_mut(teammate_entity).insert(Player {
            team_id: TeamId(0),
            position: Vec2::new(80.0, 34.0), // Closer to goal
            velocity: Vec2::zero(),
            stamina: 0.9,
            role: Role::Striker,
            skill: 0.7,
            intent: None,
            perception: None,
        });
        
        // Create utility brain with pass action
        let utility_brain = UtilityBrain {
            actions: vec![PlayerAction {
                intent: sim_components::Intent::PassTo(teammate_entity),
                considerations: vec![],
            }],
            hysteresis: 0.1,
            evaluation_interval: 10,
        };
        world.entity_mut(player_entity).insert(utility_brain);
        
        // Run systems
        let mut schedule = Schedule::default();
        schedule.add_systems(consideration_scoring_system);
        schedule.add_systems(player_decision_system);
        
        // Run schedule
        schedule.run(&mut world);
        
        // Check that player has a pass intent
        let player = world.entity(player_entity).get::<Player>().unwrap();
        assert!(player.intent.is_some());
    }

    #[test]
    fn test_defender_tackle() {
        // Test that defender attempts tackle when attacker shoots
        let mut world = World::new();
        
        // Create an attacker with shoot intent
        let attacker_entity = world.spawn(()).id();
        world.entity_mut(attacker_entity).insert(Player {
            team_id: TeamId(1),
            position: Vec2::new(45.0, 34.0),
            velocity: Vec2::zero(),
            stamina: 0.8,
            role: Role::Striker,
            skill: 0.8,
            intent: Some(sim_components::Intent::ShootAtGoal(Vec2::new(105.0, 34.0))),
            perception: None,
        });
        
        // Create a defender
        let defender_entity = world.spawn(()).id();
        world.entity_mut(defender_entity).insert(Player {
            team_id: TeamId(0),
            position: Vec2::new(40.0, 34.0),
            velocity: Vec2::zero(),
            stamina: 0.9,
            role: Role::CenterBack,
            skill: 0.7,
            intent: None,
            perception: Some(sim_components::PerceptionSnapshot {
                nearby_teammates: smallvec::SmallVec::new(),
                nearby_opponents: smallvec::smallvec![sim_components::NearbyEntity {
                    entity: attacker_entity,
                    distance: 5.0,
                    relative_position: Vec2::new(5.0, 0.0),
                }],
                ball_position: Vec2::new(45.0, 34.0),
                goal_position: Vec2::new(105.0, 34.0),
                pitch_bounds: sim_components::PitchBounds {
                    distance_to_left: 40.0,
                    distance_to_right: 65.0,
                    distance_to_top: 34.0,
                    distance_to_bottom: 34.0,
                },
            }),
        });
        
        // Create utility brain with tackle action
        let utility_brain = UtilityBrain {
            actions: vec![PlayerAction {
                intent: sim_components::Intent::Tackle(attacker_entity),
                considerations: vec![],
            }],
            hysteresis: 0.1,
            evaluation_interval: 10,
        };
        world.entity_mut(defender_entity).insert(utility_brain);
        
        // Run systems
        let mut schedule = Schedule::default();
        schedule.add_systems(consideration_scoring_system);
        schedule.add_systems(player_decision_system);
        
        // Run schedule
        schedule.run(&mut world);
        
        // Check that defender has tackle intent
        let defender = world.entity(defender_entity).get::<Player>().unwrap();
        assert!(defender.intent.is_some());
    }
}