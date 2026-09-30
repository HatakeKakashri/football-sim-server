//! `Simulation::new`, `Simulation::create_match`, and the `register_systems`
//! schedule-wiring helper.

use bevy_ecs::prelude::*;
use sim_ai_manager::{mentality_shift_system, substitution_system};
use sim_ai_player::{
    kick_execution_system, perception_system, player_action_execution_system,
    player_decision_system,
};
use sim_components::{PerceptionSnapshot, PitchBounds, RoleComponent, Skill, Stamina};
use sim_physics::{
    apply_kick_velocity_system, ball_physics_system, pitch_control_system, player_movement_system,
};
use sim_rules::{
    added_time_calculation_system, foul_detection_system, goal_detection_system,
    minimum_player_count_system, offside_detection_system, out_of_bounds_system,
    possession_resolution_system, restart_system,
};

use super::brain_default::default_utility_brain;
use super::Simulation;
use super::SimulationSet;

/// Phase C ECS-shape polish (review §4.2): register every simulation system
/// in one place, in the explicit per-tick order:
/// `Perception → Decision → Execution → Physics → Possession → Rules → MatchAdmin`.
///
/// Each phase's systems live in their feature crate (`sim-ai-player`,
/// `sim-physics`, `sim-rules`, `sim-ai-manager`); this function composes
/// them into the schedule with the cross-phase ordering constraints the
/// pipeline requires (per spec §3 read/write separation).
///
/// The chained-set configuration guarantees phase ordering; the per-phase
/// `.chain()` inside each `add_systems` call enforces in-phase ordering
/// (e.g. `apply_kick_velocity_system` must run before `ball_physics_system`
/// so a kick decided this tick is integrated into position the same tick).
fn register_systems(schedule: &mut Schedule) {
    // Configure system sets for explicit ordering across phases.
    schedule.configure_sets(
        (
            SimulationSet::Perception,
            SimulationSet::Decision,
            SimulationSet::Execution,
            SimulationSet::Physics,
            SimulationSet::Possession,
            SimulationSet::Rules,
            SimulationSet::MatchAdmin,
        )
            .chain(),
    );

    // Perception
    schedule.add_systems(
        (pitch_control_system, perception_system)
            .chain()
            .in_set(SimulationSet::Perception),
    );

    // Decision — Phase D (CODEBASE_REVIEW §7): per-player decision cadence
    // is implemented inside `player_decision_system` via the
    // `DECISION_CADENCE_TICKS` (default N=6, ~10 Hz) guard. Spec §10/§13 #15.
    schedule.add_systems(
        player_decision_system
            .in_set(SimulationSet::Decision)
            .after(SimulationSet::Perception),
    );

    // Execution
    schedule.add_systems(
        (player_action_execution_system, kick_execution_system)
            .chain()
            .in_set(SimulationSet::Execution)
            .after(SimulationSet::Decision),
    );

    // Physics — kick-velocity must apply before ball_physics_system so the
    // kick is integrated into position on the same tick.
    schedule.add_systems(
        (
            apply_kick_velocity_system,
            ball_physics_system,
            player_movement_system,
        )
            .chain()
            .in_set(SimulationSet::Physics)
            .after(SimulationSet::Execution),
    );

    // Possession
    schedule.add_systems(
        possession_resolution_system
            .in_set(SimulationSet::Possession)
            .after(SimulationSet::Physics),
    );

    // Rules — out-of-bounds → goal → offside → foul → restart (spec §3).
    schedule.add_systems(
        (
            out_of_bounds_system,
            goal_detection_system,
            offside_detection_system,
            foul_detection_system,
            restart_system,
        )
            .chain()
            .in_set(SimulationSet::Rules)
            .after(SimulationSet::Possession),
    );

    // MatchAdmin — added time → minimum players → substitutions → mentality.
    schedule.add_systems(
        (
            added_time_calculation_system,
            minimum_player_count_system,
            substitution_system,
            mentality_shift_system,
        )
            .chain()
            .in_set(SimulationSet::MatchAdmin)
            .after(SimulationSet::Rules),
    );
}

impl Simulation {
    /// Build a new simulation with the standard pitch, systems and match.
    ///
    /// # Panics
    ///
    /// Panics if `create_match` does not spawn a ball entity, which would be
    /// an internal invariant violation.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let mut world = World::new();
        // Phase 0 substrate: physics needs pitch dimensions. Insert the standard
        // pitch as a resource so the registered ball physics system can resolve
        // its `Res<PitchDimensions>` parameter.
        world.insert_resource(sim_math::PitchDimensions::standard());
        // Phase 2: insert the pitch control grid resource so the
        // pitch_control_system (registered below) can mutate it in place.
        world.insert_resource(sim_physics::PitchControlGrid::build_standard());
        // Phase 3: insert the RNG as a resource for stochastic gameplay outcomes
        // (tackle resolution, pass completion, shot accuracy).
        world.insert_resource(sim_physics::SimRng::new(seed));
        // Phase D: player_decision_system requires the evaluation counter
        // (CODEBASE_REVIEW §7 — per-player decision cadence).
        world.insert_resource(sim_ai_player::DecisionEvaluationCount::default());

        let mut schedule = Schedule::default();
        let original_seed = seed;

        register_systems(&mut schedule);

        // Create match + home team; resolve ball entity for later use by the
        // lifecycle system and clock advancement. Phase C §4.3: `Ball` is a
        // Resource and the ball entity is tagged with `BallMarker`.
        let (match_entity, _home_team_entity, ball_entity) = Self::create_match(&mut world, seed);

        Self {
            world,
            schedule,
            original_seed,
            tick: 0,
            match_entity,
            ball_entity,
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one-shot match setup: ball, two 11-player squads, match entity"
    )]
    pub fn create_match(world: &mut World, seed: u64) -> (Entity, Entity, Entity) {
        use sim_components::{
            Ball, BallMarker, Match, MatchClock, MatchState, Player, Position, Team, TeamId,
            Velocity,
        };
        use sim_math::Vec2;

        // Create ball entity. Phase C §4.3: `Ball` is now a Resource, so the
        // ball entity carries only `Position`, `Velocity`, and a `BallMarker`
        // component (so systems can find it via Query without an
        // `iter_entities` scan). `Ball` state lives in
        // `world.resource::<Ball>()` and is the single source of truth for
        // spin/state/possessor/last_touched_by/kick_velocity.
        let ball_entity = world.spawn(()).id();
        world.insert_resource(Ball {
            spin: 0.0,
            state: sim_components::BallState::Free,
            possessor: None,
            last_touched_by: None,
            kick_velocity: None,
        });
        world.entity_mut(ball_entity).insert(BallMarker);
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(52.5, 34.0)));
        world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

        // Create home team
        let home_team_entity = world.spawn(()).id();
        let mut home_players = Vec::new();

        // Create 11 home players
        for i in 0_u8..11 {
            let player_entity = world.spawn(()).id();
            let pos = Vec2::new(f32::from(i).mul_add(8.0, 10.0), 34.0);
            // Phase 1: seed each player with a default empty PerceptionSnapshot
            // stored in the existing `Player.perception` field so downstream
            // readers always have a valid snapshot without needing it to also
            // be a Component (which is out of scope for Phase 1).
            let default_snapshot = PerceptionSnapshot {
                self_position: pos,
                nearby_teammates: smallvec::SmallVec::new(),
                nearby_opponents: smallvec::SmallVec::new(),
                ball_position: Vec2::new(52.5, 34.0),
                ball_state: sim_components::BallState::Free,
                goal_position: Vec2::new(105.0, 34.0),
                pitch_bounds: PitchBounds {
                    distance_to_left: pos.x,
                    distance_to_right: 105.0 - pos.x,
                    distance_to_top: pos.y,
                    distance_to_bottom: 68.0 - pos.y,
                },
            };
            world.entity_mut(player_entity).insert(Player {
                team_id: TeamId(0),
                intent: None,
            });
            world.entity_mut(player_entity).insert(Position(pos));
            world
                .entity_mut(player_entity)
                .insert(Velocity(Vec2::zero()));
            world.entity_mut(player_entity).insert(Stamina(1.0));
            world
                .entity_mut(player_entity)
                .insert(RoleComponent(sim_components::Role::CentralMidfielder));
            world.entity_mut(player_entity).insert(Skill(0.7));
            // Insert PerceptionSnapshot as a Component (Phase B)
            world.entity_mut(player_entity).insert(default_snapshot);
            world
                .entity_mut(player_entity)
                .insert(default_utility_brain());
            home_players.push(player_entity);
        }

        world.entity_mut(home_team_entity).insert(Team {
            id: TeamId(0),
            name: "Home".to_string(),
            formation: sim_components::Formation::FourFourTwo,
            mentality: sim_components::Mentality::Balance,
            players: home_players,
            substitutes: Vec::new(),
        });

        // Create away team
        let away_team_entity = world.spawn(()).id();
        let mut away_players = Vec::new();

        // Create 11 away players
        for i in 0_u8..11 {
            let player_entity = world.spawn(()).id();
            let pos = Vec2::new(f32::from(i).mul_add(-8.0, 95.0), 34.0);
            let default_snapshot = PerceptionSnapshot {
                self_position: pos,
                nearby_teammates: smallvec::SmallVec::new(),
                nearby_opponents: smallvec::SmallVec::new(),
                ball_position: Vec2::new(52.5, 34.0),
                ball_state: sim_components::BallState::Free,
                goal_position: Vec2::new(105.0, 34.0),
                pitch_bounds: PitchBounds {
                    distance_to_left: pos.x,
                    distance_to_right: 105.0 - pos.x,
                    distance_to_top: pos.y,
                    distance_to_bottom: 68.0 - pos.y,
                },
            };
            world.entity_mut(player_entity).insert(Player {
                team_id: TeamId(1),
                intent: None,
            });
            world.entity_mut(player_entity).insert(Position(pos));
            world
                .entity_mut(player_entity)
                .insert(Velocity(Vec2::zero()));
            world.entity_mut(player_entity).insert(Stamina(1.0));
            world
                .entity_mut(player_entity)
                .insert(RoleComponent(sim_components::Role::CentralMidfielder));
            world.entity_mut(player_entity).insert(Skill(0.7));
            world.entity_mut(player_entity).insert(default_snapshot);
            world
                .entity_mut(player_entity)
                .insert(default_utility_brain());
            away_players.push(player_entity);
        }

        world.entity_mut(away_team_entity).insert(Team {
            id: TeamId(1),
            name: "Away".to_string(),
            formation: sim_components::Formation::FourFourTwo,
            mentality: sim_components::Mentality::Balance,
            players: away_players,
            substitutes: Vec::new(),
        });

        // Create match entity
        let match_entity = world.spawn(()).id();
        let initial_clock = MatchClock {
            elapsed_ticks: 0,
            half: 1,
            added_time_ticks: 0,
            is_running: true,
        };
        let match_resource = Match {
            id: 1,
            home_team: home_team_entity,
            away_team: away_team_entity,
            score: (0, 0),
            state: MatchState::PreMatch,
            seed,
        };
        // Phase C §4.3: Match is a Resource, no longer a Component on
        // the entity. Phase F follow-up: MatchClock is also a Resource
        // (per spec §3) so systems can read it without scanning entities.
        // The Component form is preserved on the match entity for
        // backwards compatibility with tests + external code that builds
        // worlds by hand; the Resource form is the source of truth.
        world.insert_resource(match_resource);
        world.insert_resource(initial_clock.clone());
        world.entity_mut(match_entity).insert(initial_clock);

        (match_entity, home_team_entity, ball_entity)
    }
}