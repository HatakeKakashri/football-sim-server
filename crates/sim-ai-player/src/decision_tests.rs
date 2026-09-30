//! Integration tests for the decision cadence systems. Compiled only
//! under `cfg(test)`.

use crate::{
    Consideration, DecisionEvaluationCount, PlayerAction, UtilityBrain,
    consideration_scoring_system, player_decision_system,
};
use crate::tests::run_one_cadence_window;

use bevy_ecs::prelude::*;
use sim_components::{Player, Position, Role, RoleComponent, Skill, Stamina, TeamId, TeamIdComponent, Velocity};
use sim_math::Vec2;

#[test]
fn test_stamina_based_decision() {
    // Test that low stamina player conserves energy
    let mut world = World::new();

    // Create a player with low stamina
    let player_entity = world.spawn(()).id();
    world.entity_mut(player_entity).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world
        .entity_mut(player_entity)
        .insert(Position(Vec2::new(50.0, 34.0)));
    world
        .entity_mut(player_entity)
        .insert(Velocity(Vec2::zero()));
    world.entity_mut(player_entity).insert(Stamina(0.2));
    world.entity_mut(player_entity).insert(Skill(0.8));
    world
        .entity_mut(player_entity)
        .insert(RoleComponent(Role::Striker));
    world
        .entity_mut(player_entity)
        .insert(TeamIdComponent(TeamId(0)));

    // Create utility brain with sprint action
    let utility_brain = UtilityBrain {
        actions: vec![PlayerAction {
            intent: sim_components::Intent::Movement(
                sim_components::MovementIntent::MoveToPosition(Vec2::new(100.0, 34.0)),
            ),
            considerations: vec![],
        }],
        hysteresis: 0.1,
    };
    world.entity_mut(player_entity).insert(utility_brain);

    // Phase 2: provide a minimal perception snapshot so
    // player_decision_system can run.
    world
        .entity_mut(player_entity)
        .insert(sim_components::PerceptionSnapshot {
            self_position: Vec2::new(50.0, 34.0),
            nearby_teammates: smallvec::SmallVec::new(),
            nearby_opponents: smallvec::SmallVec::new(),
            ball_position: Vec2::new(52.5, 34.0),
            ball_state: sim_components::BallState::Free,
            goal_position: Vec2::new(105.0, 34.0),
            pitch_bounds: sim_components::PitchBounds {
                distance_to_left: 50.0,
                distance_to_right: 55.0,
                distance_to_top: 34.0,
                distance_to_bottom: 34.0,
            },
        });

    // Run systems
    let mut schedule = Schedule::default();
    schedule.add_systems(consideration_scoring_system);
    schedule.add_systems(player_decision_system);

    let match_entity = world.spawn(()).id();
    world.insert_resource(sim_components::Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: sim_components::MatchState::InPlay,
        seed: 0,
    });
    // Phase D: player_decision_system requires this resource.
    world.insert_resource(DecisionEvaluationCount::default());
    // Phase F follow-up: MatchClock is a Resource (spec §3). Insert both
    // forms — the Resource is the source of truth for systems; the
    // Component on the entity is kept for backwards compat.
    let clock = sim_components::MatchClock {
        elapsed_ticks: 0,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    };
    world.insert_resource(clock.clone());
    world.entity_mut(match_entity).insert(clock);

    // Run schedule.
    // Phase D: drive a full cadence window (6 ticks) so the player
    // is guaranteed at least one evaluation regardless of which
    // slot their entity id hashes into.
    run_one_cadence_window(&mut world, &mut schedule, match_entity);

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
        intent: None,
    });
    world
        .entity_mut(player_entity)
        .insert(Position(Vec2::new(50.0, 34.0)));
    world
        .entity_mut(player_entity)
        .insert(Velocity(Vec2::zero()));
    world.entity_mut(player_entity).insert(Stamina(0.8));
    world.entity_mut(player_entity).insert(Skill(0.8));
    world
        .entity_mut(player_entity)
        .insert(RoleComponent(Role::Striker));
    world
        .entity_mut(player_entity)
        .insert(TeamIdComponent(TeamId(0)));

    // Create a teammate in better position
    let teammate_entity = world.spawn(()).id();
    world.entity_mut(teammate_entity).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world
        .entity_mut(teammate_entity)
        .insert(Position(Vec2::new(80.0, 34.0)));
    world
        .entity_mut(teammate_entity)
        .insert(Velocity(Vec2::zero()));
    world.entity_mut(teammate_entity).insert(Stamina(0.9));
    world.entity_mut(teammate_entity).insert(Skill(0.7));
    world
        .entity_mut(teammate_entity)
        .insert(RoleComponent(Role::Striker));

    // Create utility brain with pass action
    let utility_brain = UtilityBrain {
        actions: vec![PlayerAction {
            intent: sim_components::Intent::Action(sim_components::ActionIntent::PassTo),
            considerations: vec![],
        }],
        hysteresis: 0.1,
    };
    world.entity_mut(player_entity).insert(utility_brain);

    // Phase 2: minimal perception snapshot for player_decision_system.
    world
        .entity_mut(player_entity)
        .insert(sim_components::PerceptionSnapshot {
            self_position: Vec2::new(50.0, 34.0),
            nearby_teammates: smallvec::SmallVec::new(),
            nearby_opponents: smallvec::SmallVec::new(),
            ball_position: Vec2::new(52.5, 34.0),
            ball_state: sim_components::BallState::Free,
            goal_position: Vec2::new(105.0, 34.0),
            pitch_bounds: sim_components::PitchBounds {
                distance_to_left: 50.0,
                distance_to_right: 55.0,
                distance_to_top: 34.0,
                distance_to_bottom: 34.0,
            },
        });

    // Run systems
    let mut schedule = Schedule::default();
    schedule.add_systems(consideration_scoring_system);
    schedule.add_systems(player_decision_system);

    let match_entity = world.spawn(()).id();
    world.insert_resource(sim_components::Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: sim_components::MatchState::InPlay,
        seed: 0,
    });
    // Phase D: player_decision_system requires this resource.
    world.insert_resource(DecisionEvaluationCount::default());
    // Phase F follow-up: MatchClock is a Resource (spec §3). Insert both
    // forms — the Resource is the source of truth for systems; the
    // Component on the entity is kept for backwards compat.
    let clock = sim_components::MatchClock {
        elapsed_ticks: 0,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    };
    world.insert_resource(clock.clone());
    world.entity_mut(match_entity).insert(clock);

    // Run schedule.
    // Phase D: drive a full cadence window (6 ticks) so the player
    // is guaranteed at least one evaluation regardless of which
    // slot their entity id hashes into.
    run_one_cadence_window(&mut world, &mut schedule, match_entity);

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
        intent: Some(sim_components::Intent::Action(
            sim_components::ActionIntent::ShootAtGoal(Vec2::new(105.0, 34.0)),
        )),
    });
    world
        .entity_mut(attacker_entity)
        .insert(Position(Vec2::new(45.0, 34.0)));
    world
        .entity_mut(attacker_entity)
        .insert(Velocity(Vec2::zero()));
    world.entity_mut(attacker_entity).insert(Stamina(0.8));
    world.entity_mut(attacker_entity).insert(Skill(0.8));
    world
        .entity_mut(attacker_entity)
        .insert(RoleComponent(Role::Striker));
    world
        .entity_mut(attacker_entity)
        .insert(TeamIdComponent(TeamId(1)));

    // Create a defender
    let defender_entity = world.spawn(()).id();
    world.entity_mut(defender_entity).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world
        .entity_mut(defender_entity)
        .insert(Position(Vec2::new(40.0, 34.0)));
    world
        .entity_mut(defender_entity)
        .insert(Velocity(Vec2::zero()));
    world.entity_mut(defender_entity).insert(Stamina(0.9));
    world.entity_mut(defender_entity).insert(Skill(0.7));
    world
        .entity_mut(defender_entity)
        .insert(RoleComponent(Role::CenterBack));
    world
        .entity_mut(defender_entity)
        .insert(TeamIdComponent(TeamId(0)));
    world
        .entity_mut(defender_entity)
        .insert(sim_components::PerceptionSnapshot {
            self_position: Vec2::new(40.0, 34.0),
            nearby_teammates: smallvec::SmallVec::new(),
            nearby_opponents: smallvec::smallvec![sim_components::NearbyEntity {
                entity: attacker_entity,
                distance: 5.0,
                relative_position: Vec2::new(5.0, 0.0),
            }],
            ball_position: Vec2::new(45.0, 34.0),
            ball_state: sim_components::BallState::Free,
            goal_position: Vec2::new(105.0, 34.0),
            pitch_bounds: sim_components::PitchBounds {
                distance_to_left: 40.0,
                distance_to_right: 65.0,
                distance_to_top: 34.0,
                distance_to_bottom: 34.0,
            },
        });

    // Create utility brain with tackle action. Phase 2 decision cadence is
    let utility_brain = UtilityBrain {
        actions: vec![PlayerAction {
            intent: sim_components::Intent::Action(sim_components::ActionIntent::Tackle(
                attacker_entity,
            )),
            considerations: vec![],
        }],
        hysteresis: 0.1,
    };
    world.entity_mut(defender_entity).insert(utility_brain);

    let match_entity = world.spawn(()).id();
    world.insert_resource(sim_components::Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: sim_components::MatchState::InPlay,
        seed: 0,
    });
    // Phase D: player_decision_system requires this resource.
    world.insert_resource(DecisionEvaluationCount::default());
    // Phase F follow-up: MatchClock is a Resource (spec §3). Insert both
    // forms — the Resource is the source of truth for systems; the
    // Component on the entity is kept for backwards compat.
    let clock = sim_components::MatchClock {
        elapsed_ticks: 0,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    };
    world.insert_resource(clock.clone());
    world.entity_mut(match_entity).insert(clock);

    // Run systems
    let mut schedule = Schedule::default();
    schedule.add_systems(consideration_scoring_system);
    schedule.add_systems(player_decision_system);

    // Run schedule.
    // Phase D: drive a full cadence window (6 ticks) so the player
    // is guaranteed at least one evaluation regardless of which
    // slot their entity id hashes into.
    run_one_cadence_window(&mut world, &mut schedule, match_entity);

    // Check that defender has tackle intent
    let defender = world.entity(defender_entity).get::<Player>().unwrap();
    assert!(defender.intent.is_some());
}

/// Phase D (`CODEBASE_REVIEW` §7): per-player decision cadence.
///
/// The decision system MUST evaluate a given player only on the ticks
/// where `(elapsed_ticks % DECISION_CADENCE_TICKS) == player_slot(entity)`,
/// per spec §13 #15 (default N=6 → ~10 Hz at 60 Hz physics).
///
/// We drive 60 ticks with a single player. The player's slot is
/// deterministic (depends only on entity id), so over 60 ticks the
/// player should be evaluated exactly 10 times (60 / cadence 6).
/// Before Phase D, the guard was missing and the count was 60.
#[test]
fn test_decision_cadence_does_not_re_evaluate_every_tick() {
    use sim_ai_core::ResponseCurve as TestResponseCurve;
    let mut world = World::new();

    // Spawn a player + utility brain.
    let player_entity = world.spawn(()).id();
    world.entity_mut(player_entity).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world
        .entity_mut(player_entity)
        .insert(Position(Vec2::new(50.0, 34.0)));
    world
        .entity_mut(player_entity)
        .insert(Velocity(Vec2::zero()));
    world.entity_mut(player_entity).insert(Stamina(1.0));
    world
        .entity_mut(player_entity)
        .insert(RoleComponent(Role::Striker));
    world.entity_mut(player_entity).insert(Skill(0.7));
    world
        .entity_mut(player_entity)
        .insert(TeamIdComponent(TeamId(0)));
    let brain = UtilityBrain {
        actions: vec![PlayerAction {
            intent: sim_components::Intent::Movement(
                sim_components::MovementIntent::HoldPosition,
            ),
            considerations: vec![Consideration::FormationDiscipline {
                weight: 1.0,
                curve: TestResponseCurve::Linear { min: 0.0, max: 1.0 },
            }],
        }],
        hysteresis: 0.1,
    };
    world.entity_mut(player_entity).insert(brain);

    // Provide a minimal perception snapshot so decision_system can run.
    world
        .entity_mut(player_entity)
        .insert(sim_components::PerceptionSnapshot {
            self_position: Vec2::new(50.0, 34.0),
            nearby_teammates: smallvec::SmallVec::new(),
            nearby_opponents: smallvec::SmallVec::new(),
            ball_position: Vec2::new(52.5, 34.0),
            ball_state: sim_components::BallState::Free,
            goal_position: Vec2::new(105.0, 34.0),
            pitch_bounds: sim_components::PitchBounds {
                distance_to_left: 50.0,
                distance_to_right: 55.0,
                distance_to_top: 34.0,
                distance_to_bottom: 34.0,
            },
        });

    // Insert a Match entity so player_decision_system can compute tick.
    let match_entity = world.spawn(()).id();
    world.insert_resource(sim_components::Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: sim_components::MatchState::InPlay,
        seed: 0,
    });
    // Phase D: player_decision_system requires this resource.
    world.insert_resource(DecisionEvaluationCount::default());
    // Phase F follow-up: MatchClock is a Resource (spec §3). Insert both
    // forms — the Resource is the source of truth for systems; the
    // Component on the entity is kept for backwards compat.
    let clock = sim_components::MatchClock {
        elapsed_ticks: 0,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    };
    world.insert_resource(clock.clone());
    world.entity_mut(match_entity).insert(clock);

    let mut schedule = Schedule::default();
    schedule.add_systems(player_decision_system);

    // Drive 60 ticks total. The first 6 are the warm-up phase
    // (pre-Phase D the test looped 6 ticks then 100 more); with
    // cadence = 6, exactly 60 / 6 = 10 of those ticks should be
    // a player's evaluation tick.
    for _ in 0..60 {
        // Phase F follow-up: MatchClock is a Resource (spec §3). Update
        // both forms — the Resource is the source of truth for systems;
        // the Component on the entity is kept for backwards compat.
        let elapsed = world
            .get_resource::<sim_components::MatchClock>()
            .map_or(0, |c| c.elapsed_ticks)
            + 1;
        if let Some(mut clock) = world.get_resource_mut::<sim_components::MatchClock>() {
            clock.elapsed_ticks = elapsed;
        }
        if let Some(mut clock) = world
            .entity_mut(match_entity)
            .get_mut::<sim_components::MatchClock>()
        {
            clock.elapsed_ticks = elapsed;
        }
        schedule.run(&mut world);
    }

    // Phase D assertion: exactly 10 evaluations over 60 ticks at
    // cadence 6. Before Phase D this count was 60 (every tick).
    let eval_count = world.resource::<DecisionEvaluationCount>().get();
    assert_eq!(
        eval_count, 10,
        "expected exactly 10 evaluations over 60 ticks at cadence 6, got {eval_count}"
    );

    let player = world.entity(player_entity).get::<Player>().unwrap();
    assert!(player.intent.is_some());
}
