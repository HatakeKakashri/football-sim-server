//! Integration tests for the decision cadence under a full 22-player roster.
//! Compiled only under `cfg(test)`.

use crate::{
    Consideration, DecisionEvaluationCount, PlayerAction, UtilityBrain, player_decision_system,
};

use bevy_ecs::prelude::*;
use sim_components::{Player, Position, Role, RoleComponent, Skill, Stamina, TeamId, TeamIdComponent, Velocity};
use sim_math::Vec2;

/// Phase D regression: the cadence counter must not fire on ticks that
/// do not match a player's slot. Spawning 22 players (a full roster)
/// and driving 60 ticks should produce exactly 60 evaluations total
/// (60 ticks × 1 evaluation per tick, with each tick evaluating
/// roughly 22/6 ≈ 3-4 players) — and crucially NOT 22 × 60 = 1320.
#[test]
fn test_cadence_full_roster_load_distribution() {
    use sim_ai_core::ResponseCurve as TestResponseCurve;
    let mut world = World::new();

    // Insert Match as Resource (Phase C §4.3).
    world.insert_resource(sim_components::Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: sim_components::MatchState::InPlay,
        seed: 0,
    });

    // MatchClock on a single entity, as the production code does.
    // Phase F follow-up: MatchClock is a Resource (spec §3). Insert both
    // forms — the Resource is the source of truth for systems; the
    // Component on the entity is kept for backwards compat.
    let match_entity = world.spawn(()).id();
    let clock = sim_components::MatchClock {
        elapsed_ticks: 0,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    };
    world.insert_resource(clock.clone());
    world.entity_mut(match_entity).insert(clock);

    // 22-player roster (Phase 3 default), 11 per team.
    for team in 0..2u8 {
        for _ in 0..11 {
            let e = world.spawn(()).id();
            world.entity_mut(e).insert(Player {
                team_id: TeamId(team),
                intent: None,
            });
            world.entity_mut(e).insert(Position(Vec2::new(50.0, 34.0)));
            world.entity_mut(e).insert(Velocity(Vec2::zero()));
            world.entity_mut(e).insert(Stamina(1.0));
            world.entity_mut(e).insert(RoleComponent(Role::Striker));
            world.entity_mut(e).insert(Skill(0.7));
            world.entity_mut(e).insert(TeamIdComponent(TeamId(team)));
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
            world.entity_mut(e).insert(brain);
            world
                .entity_mut(e)
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
        }
    }

    world.insert_resource(DecisionEvaluationCount::default());

    let mut schedule = Schedule::default();
    schedule.add_systems(player_decision_system);

    // Drive 60 ticks.
    for _ in 0..60 {
        if let Some(mut clock) = world
            .entity_mut(match_entity)
            .get_mut::<sim_components::MatchClock>()
        {
            clock.elapsed_ticks += 1;
        }
        schedule.run(&mut world);
    }

    let eval_count = world.resource::<DecisionEvaluationCount>().get();
    // Each tick of the 60 must evaluate at least one player (since
    // slots cycle through 0..6 every 6 ticks), but never all 22.
    // Pre-Phase D count was 60 × 22 = 1320.
    assert!(
        (60..=300).contains(&eval_count),
        "expected evaluation count in [60, 300] (per-tick ≥1, far below 1320); got {eval_count}"
    );
    assert!(
        eval_count < 22 * 60,
        "expected far fewer evaluations than full every-tick roster (got {eval_count})"
    );
}
