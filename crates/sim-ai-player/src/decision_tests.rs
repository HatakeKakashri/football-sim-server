//! Integration tests for the decision cadence systems. Compiled only
//! under `cfg(test)`.

use crate::tests::{ai_world, run_one_cadence_window};
use crate::{
    Consideration, DecisionEvaluationCount, PlayerAction, UtilityBrain,
    consideration_scoring_system, player_decision_system,
};

use bevy_ecs::prelude::*;
use sim_components::{
    Player, Position, Role, RoleComponent, Skill, Stamina, TeamId, TeamIdComponent, Velocity,
};
use sim_math::Vec2;

/// A player whose brain holds a single action with no considerations adopts
/// exactly that action's intent after one cadence window, even at low
/// stamina.
///
/// This pins the decision system's *selection plumbing* only. It does NOT
/// verify that stamina influences decisions: the brain has no
/// stamina-dependent consideration, so stamina is inert here. (Previously
/// named `test_stamina_based_decision` and described as "low stamina player
/// conserves energy", which the assertion never checked.)
#[test]
fn test_sole_action_is_adopted_regardless_of_low_stamina() {
    let mut world = ai_world();

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
            goal_position: sim_math::goal_position_for(1),
            home_slot: Vec2::new(50.0, 34.0),
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

    let player = world.entity(player_entity).get::<Player>().unwrap();
    assert_eq!(
        player.intent,
        Some(sim_components::Intent::Movement(
            sim_components::MovementIntent::MoveToPosition(Vec2::new(100.0, 34.0))
        )),
        "the sole action's intent must be adopted unchanged"
    );
}

/// A player whose brain holds a single `PassTo` action adopts a `PassTo`
/// intent after one cadence window.
///
/// Scope note: the perception snapshot has no nearby teammates (the
/// `teammate_entity` below is not referenced by perception), so this does
/// NOT test teammate evaluation or pass-target selection. It pins that the
/// decision system installs the pass action. (Previously named
/// `test_passing_option`, described as "considers teammates in better
/// positions".)
#[test]
fn test_sole_pass_action_is_adopted() {
    let mut world = ai_world();

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
            goal_position: sim_math::goal_position_for(1),
            home_slot: Vec2::new(50.0, 34.0),
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

    // The player must possess the ball for PassTo to be permitted.
    {
        let mut ball = world.resource_mut::<sim_components::Ball>();
        ball.state = sim_components::BallState::Possessed;
        ball.possessor = Some(player_entity);
    }

    // Run schedule.
    // Phase D: drive a full cadence window (6 ticks) so the player
    // is guaranteed at least one evaluation regardless of which
    // slot their entity id hashes into.
    run_one_cadence_window(&mut world, &mut schedule, match_entity);

    let player = world.entity(player_entity).get::<Player>().unwrap();
    assert_eq!(
        player.intent,
        Some(sim_components::Intent::Action(sim_components::ActionIntent::PassTo)),
        "the sole PassTo action must be adopted as the player's intent"
    );
}

/// A defender whose brain holds a single `Tackle(attacker)` action adopts a
/// tackle intent targeting that exact attacker after one cadence window.
///
/// Scope note: the brain has no considerations, so this does NOT verify that
/// a defender *chooses* to tackle in response to the attacker's shot intent;
/// it pins that the target entity is carried through the decision system.
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "test sets up attacker + defender with full component stacks and perception; splitting would obscure the scenario"
)]
fn test_sole_tackle_action_keeps_its_target() {
    let mut world = ai_world();

    // Create an attacker with shoot intent
    let attacker_entity = world.spawn(()).id();
    world.entity_mut(attacker_entity).insert(Player {
        team_id: TeamId(1),
        intent: Some(sim_components::Intent::Action(
            sim_components::ActionIntent::ShootAtGoal(sim_math::goal_position_for(1)),
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
            goal_position: sim_math::goal_position_for(1),
            home_slot: Vec2::new(40.0, 34.0),
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

    // The ball must be possessed by the attacker for Tackle to be permitted.
    {
        let mut ball = world.resource_mut::<sim_components::Ball>();
        ball.state = sim_components::BallState::Possessed;
        ball.possessor = Some(attacker_entity);
    }

    // Run systems
    let mut schedule = Schedule::default();
    schedule.add_systems(consideration_scoring_system);
    schedule.add_systems(player_decision_system);

    // Run schedule.
    // Phase D: drive a full cadence window (6 ticks) so the player
    // is guaranteed at least one evaluation regardless of which
    // slot their entity id hashes into.
    run_one_cadence_window(&mut world, &mut schedule, match_entity);

    let defender = world.entity(defender_entity).get::<Player>().unwrap();
    assert_eq!(
        defender.intent,
        Some(sim_components::Intent::Action(sim_components::ActionIntent::Tackle(
            attacker_entity
        ))),
        "tackle intent must target the attacker entity"
    );
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
    let mut world = ai_world();

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
            intent: sim_components::Intent::Movement(sim_components::MovementIntent::HoldPosition),
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
            goal_position: sim_math::goal_position_for(1),
            home_slot: Vec2::new(50.0, 34.0),
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

#[test]
fn test_move_to_position_sentinel_resolves_to_home_slot() {
    let mut world = ai_world();

    let player_entity = world.spawn(()).id();
    world.entity_mut(player_entity).insert(Player {
        team_id: TeamId(0),
        intent: None,
    });
    world
        .entity_mut(player_entity)
        .insert(Position(Vec2::new(10.0, 10.0)));
    world
        .entity_mut(player_entity)
        .insert(Velocity(Vec2::zero()));
    world.entity_mut(player_entity).insert(Stamina(1.0));
    world.entity_mut(player_entity).insert(Skill(0.8));
    world
        .entity_mut(player_entity)
        .insert(RoleComponent(Role::FullBack));
    world
        .entity_mut(player_entity)
        .insert(TeamIdComponent(TeamId(0)));

    let utility_brain = UtilityBrain {
        actions: vec![PlayerAction {
            intent: sim_components::Intent::Movement(
                sim_components::MovementIntent::MoveToPosition(Vec2::zero()),
            ),
            considerations: vec![],
        }],
        hysteresis: 0.1,
    };
    world.entity_mut(player_entity).insert(utility_brain);

    let home_slot = Vec2::new(20.0, 12.0);
    world
        .entity_mut(player_entity)
        .insert(sim_components::PerceptionSnapshot {
            self_position: Vec2::new(10.0, 10.0),
            nearby_teammates: smallvec::SmallVec::new(),
            nearby_opponents: smallvec::SmallVec::new(),
            ball_position: Vec2::new(52.5, 34.0),
            ball_state: sim_components::BallState::Free,
            goal_position: sim_math::goal_position_for(1),
            home_slot,
            pitch_bounds: sim_components::PitchBounds {
                distance_to_left: 10.0,
                distance_to_right: 95.0,
                distance_to_top: 10.0,
                distance_to_bottom: 58.0,
            },
        });

    let match_entity = world.spawn(()).id();
    let clock = sim_components::MatchClock {
        elapsed_ticks: 0,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    };
    world.insert_resource(clock.clone());
    world.entity_mut(match_entity).insert(clock);
    world.insert_resource(DecisionEvaluationCount::default());

    let mut schedule = Schedule::default();
    schedule.add_systems(player_decision_system);

    // Run one cadence window so player gets evaluated
    run_one_cadence_window(&mut world, &mut schedule, match_entity);

    let player = world.entity(player_entity).get::<Player>().unwrap();
    assert_eq!(
        player.intent,
        Some(sim_components::Intent::Movement(
            sim_components::MovementIntent::MoveToPosition(home_slot)
        )),
        "MoveToPosition(Vec2::zero()) must resolve to MoveToPosition(home_slot)"
    );
}

/// A chosen `ShootAtGoal` carries the goal the player's team attacks in the
/// current half (`perception.goal_position`), for either attack direction.
#[test]
fn test_shoot_at_goal_sentinel_resolves_to_directional_goal() {
    for direction in [1_i8, -1_i8] {
        let mut world = ai_world();

        let player_entity = world
            .spawn((
                Player {
                    team_id: TeamId(0),
                    intent: None,
                },
                Position(Vec2::new(60.0, 34.0)),
                Velocity(Vec2::zero()),
                Stamina(1.0),
                Skill(0.8),
                RoleComponent(Role::Striker),
                TeamIdComponent(TeamId(0)),
                UtilityBrain {
                    actions: vec![PlayerAction {
                        intent: sim_components::Intent::Action(
                            sim_components::ActionIntent::ShootAtGoal(Vec2::zero()),
                        ),
                        considerations: vec![],
                    }],
                    hysteresis: 0.1,
                },
            ))
            .id();
        let goal = sim_math::goal_position_for(direction);
        world
            .entity_mut(player_entity)
            .insert(sim_components::PerceptionSnapshot {
                self_position: Vec2::new(60.0, 34.0),
                nearby_teammates: smallvec::SmallVec::new(),
                nearby_opponents: smallvec::SmallVec::new(),
                ball_position: Vec2::new(60.0, 34.0),
                ball_state: sim_components::BallState::Possessed,
                goal_position: goal,
                home_slot: Vec2::new(60.0, 34.0),
                pitch_bounds: sim_components::PitchBounds {
                    distance_to_left: 60.0,
                    distance_to_right: 45.0,
                    distance_to_top: 34.0,
                    distance_to_bottom: 34.0,
                },
            });

        // The striker holds the ball (matching the perception snapshot
        // above), so a kicking action is permitted.
        {
            let mut ball = world.resource_mut::<sim_components::Ball>();
            ball.state = sim_components::BallState::Possessed;
            ball.possessor = Some(player_entity);
        }

        let match_entity = world.spawn(()).id();
        let clock = sim_components::MatchClock {
            elapsed_ticks: 0,
            half: 1,
            added_time_ticks: 0,
            is_running: true,
        };
        world.insert_resource(clock.clone());
        world.entity_mut(match_entity).insert(clock);
        world.insert_resource(DecisionEvaluationCount::default());

        let mut schedule = Schedule::default();
        schedule.add_systems(player_decision_system);
        run_one_cadence_window(&mut world, &mut schedule, match_entity);

        let player = world
            .entity(player_entity)
            .get::<Player>()
            .expect("invariant: player entity has a Player component");
        assert_eq!(
            player.intent,
            Some(sim_components::Intent::Action(
                sim_components::ActionIntent::ShootAtGoal(goal)
            )),
            "ShootAtGoal must target the goal for attack_direction {direction}"
        );
    }
}