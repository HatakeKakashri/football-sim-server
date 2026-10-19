//! `Simulation::new`, `Simulation::create_match`, and the `register_systems`
//! schedule-wiring helper.

use bevy_ecs::prelude::*;
use sim_ai_manager::{mentality_shift_system, substitution_system};
use sim_ai_player::{
    kick_execution_system, perception_system, player_action_execution_system,
    player_decision_system, tackle_recovery_system,
};
use sim_components::{
    PerceptionSnapshot, PitchBounds, PlayerProfile, RoleComponent, Skill, Stamina, TacticParameters,
};
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
use super::brain_default::default_utility_brain_with;

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
            tackle_recovery_system,
        )
            .chain()
            .in_set(SimulationSet::MatchAdmin)
            .after(SimulationSet::Rules),
    );
}

impl Simulation {
    /// Build a new simulation with the standard pitch, systems and match,
    /// using the canonical default setup for both teams (4-4-2, Balance,
    /// Possession). This keeps `Simulation::new(seed)` — used by the CLI,
    /// the replay path, the benchmark and every existing test — unchanged
    /// in behaviour.
    ///
    /// # Panics
    ///
    /// Panics if `create_match` does not spawn a ball entity, which would be
    /// an internal invariant violation.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self::new_with_setup(
            seed,
            sim_components::TeamSetup::default(),
            sim_components::TeamSetup::default(),
        )
    }

    /// Build a new simulation with per-side `TeamSetup`s (formation,
    /// mentality, tactic). The formation's slot table drives each
    /// player's `home_slot`; mentality and tactic feed numeric
    /// `TacticParameters` that shift line heights and risk appetites
    /// (PRD §7.2).
    #[must_use]
    pub fn new_with_setup(
        seed: u64,
        home_setup: sim_components::TeamSetup,
        away_setup: sim_components::TeamSetup,
    ) -> Self {
        let mut world = World::new();

        let mut schedule = Schedule::default();
        let original_seed = seed;

        register_systems(&mut schedule);

        // Create match + home team; resolve ball entity for later use by the
        // lifecycle system and clock advancement. Phase C §4.3: `Ball` is a
        // Resource and the ball entity is tagged with `BallMarker`.
        let (match_entity, _home_team_entity, ball_entity) = Self::create_match(
            &mut world,
            seed,
            home_setup,
            away_setup,
        );

        Self {
            world,
            schedule,
            original_seed,
            tick: 0,
            match_entity,
            ball_entity,
        }
    }

    /// Build a new simulation from validated fixture data (PRD §5).
    ///
    /// Uses the match's seed, the two teams' players (with their real
    /// `PlayerProfile`s, trained positions, names, and numbers), and the
    /// per-side formation/mentality/tactic from the fixture.
    ///
    /// This is the primary entry point for the server API and the harness
    /// when running with real fixture data. The placeholder-based
    /// `Simulation::new(seed)` remains available for tests and benchmarks.
    ///
    /// # Panics
    ///
    /// Panics if the fixture data is inconsistent (e.g., wrong player count,
    /// missing GK) — validation should have caught this at load time.
    #[must_use]
    pub fn new_from_fixture(
        validated_match: &sim_components::fixture::ValidatedMatch,
        home_team: &sim_components::fixture::ValidatedTeam,
        away_team: &sim_components::fixture::ValidatedTeam,
    ) -> Self {
        let mut world = World::new();

        let mut schedule = Schedule::default();
        let original_seed = validated_match.seed;

        register_systems(&mut schedule);

        let (match_entity, _home_team_entity, ball_entity) = Self::create_match_from_fixture(
            &mut world,
            validated_match,
            home_team,
            away_team,
        );

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
    pub fn create_match(
        world: &mut World,
        seed: u64,
        home_setup: sim_components::TeamSetup,
        away_setup: sim_components::TeamSetup,
    ) -> (Entity, Entity, Entity) {
        use sim_components::{
            BallMarker, HomeSlot, Match, MatchClock, MatchState, Player, Position, Team, TeamId,
            Velocity,
        };
        use sim_math::Vec2;

        // Every resource the systems require comes from the installer;
        // only `Match` / `MatchClock` (which refer to the entities spawned
        // below) are inserted here.
        super::installer::WorldInstaller::new(seed).install_resources(world);

        // Create ball entity. Phase C §4.3: `Ball` is a Resource, so the
        // ball entity carries only `Position`, `Velocity`, and a
        // `BallMarker` component (so systems can find it via Query without
        // an `iter_entities` scan).
        let ball_entity = world.spawn(()).id();
        world.entity_mut(ball_entity).insert(BallMarker);
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(52.5, 34.0)));
        world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

        // Home team parameters (PRD §7.2: mentality + tactic are numeric
        // parameters that shift line heights and risk appetites).
        let home_params = TacticParameters::from_setup(home_setup.mentality, home_setup.tactic);
        let home_direction = 1; // Home team attacks right (x = 105) in the first half.

        // Create home team
        let home_team_entity = world.spawn(()).id();
        let mut home_players = Vec::new();

        // Create 11 home players, one per slot of the team's formation table.
        let slots = home_setup.formation.table().slots;
        for i in 0_u8..11 {
            let slot = slots[i as usize];
            // Apply the line-height shift along the attacking axis (positive
            // = toward the opponent goal). See `formation::slot_position_for`.
            let pos = super::formation::slot_position_for(slot, home_direction, home_params.line_height_shift);
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
            // Ticket 25: per-team brain so the press trigger distance
            // (HighPress vs others) feeds the DistanceToPress consideration.
            world
                .entity_mut(player_entity)
                .insert(default_utility_brain_with(&home_params));
            home_players.push(player_entity);
        }

        world.entity_mut(home_team_entity).insert(Team::with_attack_direction(
            TeamId(0),
            "Home".to_string(),
            home_setup.formation,
            home_setup.mentality,
            home_setup.tactic,
            home_players,
            Vec::new(),
            // Home team attacks right (x = 105) in the first half.
            home_direction,
        ));
        // Ticket 25: snapshot the configured tactic parameters on the team
        // so `reset_formation` applies the configured line-height shift even
        // if `mentality_shift_system` later mutates `Team.mentality`
        // (PRD §7.2: mentality is configured before kickoff and does not
        // change mid-match).
        world.entity_mut(home_team_entity).insert(home_params);

        // Away team parameters (PRD §7.2: mentality + tactic are numeric
        // parameters that shift line heights and risk appetites).
        let away_params =
            TacticParameters::from_setup(away_setup.mentality, away_setup.tactic);
        let away_direction = -1; // Away team attacks left (x = 0) in the first half.

        // Create away team
        let away_team_entity = world.spawn(()).id();
        let mut away_players = Vec::new();

        // Create 11 away players, one per slot of the team's formation table.
        let away_slots = away_setup.formation.table().slots;
        for i in 0_u8..11 {
            let slot = away_slots[i as usize];
            // Apply the line-height shift along the attacking axis (positive
            // = toward the opponent goal). See `formation::slot_position_for`.
            let pos = super::formation::slot_position_for(slot, away_direction, away_params.line_height_shift);
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
            // Ticket 25: per-team brain so the press trigger distance
            // (HighPress vs others) feeds the DistanceToPress consideration.
            world
                .entity_mut(player_entity)
                .insert(default_utility_brain_with(&away_params));
            away_players.push(player_entity);
        }

        world.entity_mut(away_team_entity).insert(Team::with_attack_direction(
            TeamId(1),
            "Away".to_string(),
            away_setup.formation,
            away_setup.mentality,
            away_setup.tactic,
            away_players,
            Vec::new(),
            // Away team attacks left (x = 0) in the first half.
            away_direction,
        ));
        world.entity_mut(away_team_entity).insert(away_params);

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

    /// Create a match from validated fixture data (PRD §5).
    ///
    /// This is the fixture-wired version of `create_match`. It uses real
    /// player profiles, trained positions, names, and numbers from the
    /// TOML fixtures instead of placeholder profiles.
    #[expect(
        clippy::too_many_lines,
        reason = "one-shot match setup: ball, two 11-player squads with fixture data, match entity"
    )]
    fn create_match_from_fixture(
        world: &mut World,
        validated_match: &sim_components::fixture::ValidatedMatch,
        home_team: &sim_components::fixture::ValidatedTeam,
        away_team: &sim_components::fixture::ValidatedTeam,
    ) -> (Entity, Entity, Entity) {
        use sim_components::{
            BallMarker, HomeSlot, Match, MatchClock, MatchState, Player, Position, Team, TeamId,
            Velocity,
        };
        use sim_math::Vec2;

        // Every resource the systems require comes from the installer;
        // only `Match` / `MatchClock` (which refer to the entities spawned
        // below) are inserted here.
        super::installer::WorldInstaller::new(validated_match.seed).install_resources(world);

        // Create ball entity. Phase C §4.3: `Ball` is a Resource, so the
        // ball entity carries only `Position`, `Velocity`, and a
        // `BallMarker` component (so systems can find it via Query without
        // an `iter_entities` scan).
        let ball_entity = world.spawn(()).id();
        world.entity_mut(ball_entity).insert(BallMarker);
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(52.5, 34.0)));
        world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

        // Home team parameters (PRD §7.2: mentality + tactic are numeric
        // parameters that shift line heights and risk appetites).
        let home_params =
            TacticParameters::from_setup(validated_match.home_setup.mentality, validated_match.home_setup.tactic);
        let home_direction = 1; // Home team attacks right (x = 105) in the first half.

        // Create home team
        let home_team_entity = world.spawn(()).id();
        let mut home_players = Vec::new();

        // Create 11 home players, one per slot of the team's formation table.
        // The fixture's validated team has players in the correct order for the
        // formation slots (validated by `fill_formation_slots`).
        let slots = validated_match.home_setup.formation.table().slots;
        for (slot_index, slot) in slots.iter().enumerate() {
            // Get the corresponding fixture player for this slot.
            // The fixture validation ensures the team's players fill the
            // formation slots exactly once in order.
            let fixture_player = &home_team.players[slot_index];

            // Apply the line-height shift along the attacking axis (positive
            // = toward the opponent goal). See `formation::slot_position_for`.
            let pos = super::formation::slot_position_for(*slot, home_direction, home_params.line_height_shift);
            let player_entity = world.spawn(()).id();

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
            world.entity_mut(player_entity).insert(*slot);
            world.entity_mut(player_entity).insert(HomeSlot::new(pos));
            world.entity_mut(player_entity).insert(Skill(0.7));
            // Use the fixture player's real profile (built from their attributes).
            world
                .entity_mut(player_entity)
                .insert(fixture_player.profile);
            // Store the fixture's trained positions so the execution hot
            // path can compute the per-player fit multiplier (PRD §6.5).
            world
                .entity_mut(player_entity)
                .insert(sim_components::TrainedPositions::new(
                    fixture_player.trained_positions.clone(),
                ));
            // Insert PerceptionSnapshot as a Component (Phase B)
            world.entity_mut(player_entity).insert(default_snapshot);
            // Ticket 25: per-team brain so the press trigger distance
            // (HighPress vs others) feeds the DistanceToPress consideration.
            world
                .entity_mut(player_entity)
                .insert(default_utility_brain_with(&home_params));
            home_players.push(player_entity);
        }

        world.entity_mut(home_team_entity).insert(Team::with_attack_direction(
            TeamId(0),
            home_team.name.clone(),
            validated_match.home_setup.formation,
            validated_match.home_setup.mentality,
            validated_match.home_setup.tactic,
            home_players,
            Vec::new(),
            // Home team attacks right (x = 105) in the first half.
            home_direction,
        ));
        // Ticket 25: snapshot the configured tactic parameters on the team
        // so `reset_formation` applies the configured line-height shift even
        // if `mentality_shift_system` later mutates `Team.mentality`
        // (PRD §7.2: mentality is configured before kickoff and does not
        // change mid-match).
        world.entity_mut(home_team_entity).insert(home_params);

        // Away team parameters (PRD §7.2: mentality + tactic are numeric
        // parameters that shift line heights and risk appetites).
        let away_params =
            TacticParameters::from_setup(validated_match.away_setup.mentality, validated_match.away_setup.tactic);
        let away_direction = -1; // Away team attacks left (x = 0) in the first half.

        // Create away team
        let away_team_entity = world.spawn(()).id();
        let mut away_players = Vec::new();

        // Create 11 away players, one per slot of the team's formation table.
        let away_slots = validated_match.away_setup.formation.table().slots;
        for (slot_index, slot) in away_slots.iter().enumerate() {
            let fixture_player = &away_team.players[slot_index];

            // Apply the line-height shift along the attacking axis (positive
            // = toward the opponent goal). See `formation::slot_position_for`.
            let pos = super::formation::slot_position_for(*slot, away_direction, away_params.line_height_shift);
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
            world.entity_mut(player_entity).insert(*slot);
            world.entity_mut(player_entity).insert(HomeSlot::new(pos));
            world.entity_mut(player_entity).insert(Skill(0.7));
            // Use the fixture player's real profile (built from their attributes).
            world
                .entity_mut(player_entity)
                .insert(fixture_player.profile);
            // Store the fixture's trained positions so the execution hot
            // path can compute the per-player fit multiplier (PRD §6.5).
            world
                .entity_mut(player_entity)
                .insert(sim_components::TrainedPositions::new(
                    fixture_player.trained_positions.clone(),
                ));
            world.entity_mut(player_entity).insert(default_snapshot);
            // Ticket 25: per-team brain so the press trigger distance
            // (HighPress vs others) feeds the DistanceToPress consideration.
            world
                .entity_mut(player_entity)
                .insert(default_utility_brain_with(&away_params));
            away_players.push(player_entity);
        }

        world.entity_mut(away_team_entity).insert(Team::with_attack_direction(
            TeamId(1),
            away_team.name.clone(),
            validated_match.away_setup.formation,
            validated_match.away_setup.mentality,
            validated_match.away_setup.tactic,
            away_players,
            Vec::new(),
            // Away team attacks left (x = 0) in the first half.
            away_direction,
        ));
        world.entity_mut(away_team_entity).insert(away_params);

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
            seed: validated_match.seed,
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
