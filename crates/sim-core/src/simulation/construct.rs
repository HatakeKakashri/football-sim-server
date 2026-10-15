//! `Simulation::new`, `Simulation::create_match`, and the `register_systems`
//! schedule-wiring helper.

use bevy_ecs::prelude::*;
use sim_ai_manager::{mentality_shift_system, substitution_system};
use sim_ai_player::{
    kick_execution_system, perception_system, player_action_execution_system,
    player_decision_system,
};
use sim_components::{PerceptionSnapshot, PitchBounds, PlayerProfile, RoleComponent, Skill, Stamina};
use sim_physics::{
    apply_kick_velocity_system, ball_physics_system, pitch_control_system, player_movement_system,
};
use sim_rules::{
    added_time_calculation_system, apply_ball_touched_system, foul_detection_system,
    goal_detection_system, minimum_player_count_system, out_of_bounds_system, possession_resolution_system, restart_system,
    stoppage_tracking_system,
};

use super::Simulation;
use super::SimulationSet;
use super::brain_default::default_utility_brain;

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

    // Rules — ball-touch consumer → out-of-bounds → goal → foul → restart (spec §3).
    // The consumer drains `PendingBallTouches` first so every downstream
    // Rules system sees the latest touch in the same tick.
    schedule.add_systems(
        (
            apply_ball_touched_system,
            out_of_bounds_system,
            goal_detection_system,
            foul_detection_system,
            restart_system,
        )
            .chain()
            .in_set(SimulationSet::Rules)
            .after(SimulationSet::Possession),
    );

    // Event emission (issue 08). Runs in the same `Rules` set, after
    // the rule writers have settled their signals. The order is:
    // possession_change on OOB → possession_change on acquisition
    // → pass-event resolution. The pass resolver reads both OOB and
    // the live `Ball.possessor`, so it must run last.
    super::event_emit::register_event_systems(schedule);

    // MatchAdmin — stoppage tracking → added time → minimum players →
    // substitutions → mentality. Stoppage tracking runs first so the
    // same-tick dead-ball count feeds the added-time calculation.
    schedule.add_systems(
        (
            stoppage_tracking_system,
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
            Ball, BallMarker, HomeSlot, Match, MatchClock, MatchState,
            PendingBallTouches, Player, Position, Team, TeamId, Velocity,
        };
        use sim_math::Vec2;

        // Create ball entity. Phase C §4.3: `Ball` is now a Resource, so the
        // ball entity carries only `Position`, `Velocity`, and a `BallMarker`
        // component (so systems can find it via Query without an
        // `iter_entities` scan). `Ball` state lives in
        // `world.resource::<Ball>()` and is the single source of truth for
        // spin/state/possessor/last_touch/kick_velocity.
        let ball_entity = world.spawn(()).id();
        world.insert_resource(Ball {
            spin: 0.0,
            state: sim_components::BallState::Free,
            possessor: None,
            last_touch: None,
            touch_history: [None, None],
            kick_velocity: None,
        });
        world.insert_resource(PendingBallTouches::default());
        world.insert_resource(sim_rules::PendingOutOfBounds::default());
        world.insert_resource(sim_rules::OobRestartForEmit::default());
        world.insert_resource(sim_rules::PendingPass::default());
        world.insert_resource(sim_rules::GoalForEmit::default());
        world.insert_resource(sim_rules::KickoffForEmit::default());
        world.insert_resource(sim_rules::PendingShot::default());
        world.insert_resource(super::lifecycle::HalfTimeEntryTick { tick: None });
        world.insert_resource(super::lifecycle::FirstHalfAddedTime(0));
        world.insert_resource(crate::EventLog::new());
        world.entity_mut(ball_entity).insert(BallMarker);
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(52.5, 34.0)));
        world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

        // Create home team
        let home_team_entity = world.spawn(()).id();
        let mut home_players = Vec::new();

        // Create 11 home players, one per slot of the team's formation table.
        let formation = sim_components::Formation::FourFourTwo;
        let slots = formation.table().slots;
        for i in 0_u8..11 {
            let slot = slots[i as usize];
            let pos = slot.position_for(1);
            let player_entity = world.spawn(()).id();
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
                // Home team attacks right in the first half. Perception
                // overwrites this every tick; this is just the pre-tick seed.
                goal_position: sim_math::goal_position_for(1),
                pitch_bounds: PitchBounds {
                    distance_to_left: pos.x,
                    distance_to_right: sim_math::PITCH_LENGTH - pos.x,
                    distance_to_top: pos.y,
                    distance_to_bottom: 68.0 - pos.y,
                },
                home_slot: pos,
            };

            world.entity_mut(player_entity).insert(Player {
                team_id: TeamId(0),
                intent: None,
            });
            world
                .entity_mut(player_entity)
                .insert(sim_components::TeamIdComponent(TeamId(0)));
            world.entity_mut(player_entity).insert(Position(pos));
            world
                .entity_mut(player_entity)
                .insert(Velocity(Vec2::zero()));
            world.entity_mut(player_entity).insert(Stamina(1.0));
            world
                .entity_mut(player_entity)
                .insert(RoleComponent(slot.role()));
            world.entity_mut(player_entity).insert(slot);
            world.entity_mut(player_entity).insert(HomeSlot::new(pos));
            world.entity_mut(player_entity).insert(Skill(0.7));
            // Ticket 13: every player carries a `PlayerProfile`; the GK
            // slot (index 0 of the formation table) gets the goalkeeper profile.
            if i == 0 {
                world
                    .entity_mut(player_entity)
                    .insert(PlayerProfile::default_goalkeeper());
            } else {
                world
                    .entity_mut(player_entity)
                    .insert(PlayerProfile::default_outfield());
            }
            // Insert PerceptionSnapshot as a Component (Phase B)
            world.entity_mut(player_entity).insert(default_snapshot);
            world
                .entity_mut(player_entity)
                .insert(default_utility_brain());
            home_players.push(player_entity);
        }

        world.entity_mut(home_team_entity).insert(Team::with_attack_direction(
            TeamId(0),
            "Home".to_string(),
            sim_components::Formation::FourFourTwo,
            sim_components::Mentality::Balance,
            sim_components::Tactic::Possession,
            home_players,
            Vec::new(),
            // Home team attacks right (x = 105) in the first half.
            1,
        ));

        // Create away team
        let away_team_entity = world.spawn(()).id();
        let mut away_players = Vec::new();

        // Create 11 away players, one per slot of the team's formation table.
        let away_slots = sim_components::Formation::FourFourTwo.table().slots;
        for i in 0_u8..11 {
            let slot = away_slots[i as usize];
            let pos = slot.position_for(-1);
            let player_entity = world.spawn(()).id();
            let default_snapshot = PerceptionSnapshot {
                self_position: pos,
                nearby_teammates: smallvec::SmallVec::new(),
                nearby_opponents: smallvec::SmallVec::new(),
                ball_position: Vec2::new(52.5, 34.0),
                ball_state: sim_components::BallState::Free,
                // Away team attacks left in the first half. Perception
                // overwrites this every tick; this is just the pre-tick seed.
                goal_position: sim_math::goal_position_for(-1),
                pitch_bounds: PitchBounds {
                    distance_to_left: pos.x,
                    distance_to_right: sim_math::PITCH_LENGTH - pos.x,
                    distance_to_top: pos.y,
                    distance_to_bottom: 68.0 - pos.y,
                },
                home_slot: pos,
            };
            world.entity_mut(player_entity).insert(Player {
                team_id: TeamId(1),
                intent: None,
            });
            world
                .entity_mut(player_entity)
                .insert(sim_components::TeamIdComponent(TeamId(1)));
            world.entity_mut(player_entity).insert(Position(pos));
            world
                .entity_mut(player_entity)
                .insert(Velocity(Vec2::zero()));
            world.entity_mut(player_entity).insert(Stamina(1.0));
            world
                .entity_mut(player_entity)
                .insert(RoleComponent(slot.role()));
            world.entity_mut(player_entity).insert(slot);
            world.entity_mut(player_entity).insert(HomeSlot::new(pos));
            world.entity_mut(player_entity).insert(Skill(0.7));
            // Ticket 13: see the home-squad loop above.
            if i == 0 {
                world
                    .entity_mut(player_entity)
                    .insert(PlayerProfile::default_goalkeeper());
            } else {
                world
                    .entity_mut(player_entity)
                    .insert(PlayerProfile::default_outfield());
            }
            world.entity_mut(player_entity).insert(default_snapshot);
            world
                .entity_mut(player_entity)
                .insert(default_utility_brain());
            away_players.push(player_entity);
        }

        world.entity_mut(away_team_entity).insert(Team::with_attack_direction(
            TeamId(1),
            "Away".to_string(),
            sim_components::Formation::FourFourTwo,
            sim_components::Mentality::Balance,
            sim_components::Tactic::Possession,
            away_players,
            Vec::new(),
            // Away team attacks left (x = 0) in the first half.
            -1,
        ));

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

        // The referee lives for the whole match so `added_time_calculation_system`
        // has a stoppage queue to consume. Spawned last so every other entity
        // keeps the id it had before the referee existed.
        world.spawn(sim_components::Referee {
            stoppage_events: Vec::new(),
            cards: Vec::new(),
        });

        (match_entity, home_team_entity, ball_entity)
    }
}
