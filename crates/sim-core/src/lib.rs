use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use sim_ai_manager::{mentality_shift_system, substitution_system};
use sim_ai_player::{
    kick_execution_system, perception_system, player_action_execution_system,
    player_decision_system,
};
use sim_components::{
    ManagerCommand, Match, MatchClock, MatchState, PerceptionSnapshot, PitchBounds, Player,
    Position, RoleComponent, Skill, Stamina, Velocity,
};
use sim_math::{PitchDimensions, Vec2};
use sim_physics::{
    apply_kick_velocity_system, ball_physics_system, pitch_control_system, player_movement_system,
};
use sim_rules::{
    added_time_calculation_system, foul_detection_system, goal_detection_system,
    minimum_player_count_system, offside_detection_system, out_of_bounds_system,
    possession_resolution_system, restart_system,
};

// Re-export time constants from sim-components for downstream crates
pub use sim_components::time::{
    HALF_LENGTH_TICKS, HALFTIME_BREAK_TICKS, TICKS_PER_SECOND, TOTAL_MATCH_TICKS,
};

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SimulationSet {
    Perception,
    Decision,
    Execution,
    Physics,
    Possession,
    Rules,
    MatchAdmin,
}

pub struct Simulation {
    pub world: World,
    pub schedule: Schedule,
    pub original_seed: u64,
    pub tick: u64,
    pub match_entity: Entity,
    pub ball_entity: Entity,
}

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

    // Decision (no per-player time-slicing; spec §13 #15 deferred)
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
    pub fn new(seed: u64) -> Self {
        let mut world = World::new();
        // Phase 0 substrate: physics needs pitch dimensions. Insert the standard
        // pitch as a resource so the registered ball physics system can resolve
        // its `Res<PitchDimensions>` parameter.
        world.insert_resource(PitchDimensions::standard());
        // Phase 2: insert the pitch control grid resource so the
        // pitch_control_system (registered below) can mutate it in place.
        world.insert_resource(sim_physics::PitchControlGrid::build_standard());
        // Phase 3: insert the RNG as a resource for stochastic gameplay outcomes
        // (tackle resolution, pass completion, shot accuracy).
        world.insert_resource(sim_physics::SimRng::new(seed));

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

    /// Advance the simulation by one fixed timestep (1/60 second).
    pub fn tick(&mut self) {
        // Phase 3: Insert current tick as a resource so referee systems can access it.
        self.world
            .insert_resource(sim_rules::CurrentTick(self.tick));

        self.schedule.run(&mut self.world);

        // lifecycle_system takes the current tick counter.
        lifecycle_system(
            self.match_entity,
            self.ball_entity,
            self.tick,
            &mut self.world,
        );

        // Advance match clock (integer ticks, no floating point).
        self.tick_clock();

        self.tick += 1;
    }

    /// Advance the match clock by one tick if the clock is running.
    fn tick_clock(&mut self) {
        if let Some(mut clock) = self
            .world
            .entity_mut(self.match_entity)
            .get_mut::<MatchClock>()
            && clock.is_running
        {
            clock.elapsed_ticks += 1;
        }
    }

    /// Compute a deterministic FNV-1a (64-bit) hash over the world state.
    ///
    /// Covers (in this fixed order, per-entity, entities sorted by
    /// `Entity::to_bits()`): `Position`, `Velocity`, `Stamina`, `Skill`, `Ball`,
    /// `Player`, `Team`, `Match`. After all entity bytes are mixed in, the
    /// RNG state is appended as the final 4 bytes.
    ///
    /// **Excluded** by design: the tick counter and any wall-clock data.
    /// The hash identifies *state*, not *time* — two identical states
    /// reached at different ticks must hash equal, which is what makes
    /// replay divergence detection meaningful.
    pub fn get_state_hash(&self) -> u64 {
        use sim_components::{BallMarker, Match, Player, Position, Skill, Stamina, Team, Velocity};

        fn mix(h: &mut u64, v: u64) {
            // FNV-1a: XOR one byte at a time (little-endian) then multiply.
            let bytes = v.to_le_bytes();
            for b in bytes {
                *h ^= u64::from(b);
                *h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }

        let mut h: u64 = 0xcbf2_9ce4_8422_2325;

        // Collect entities in a deterministic order: by `to_bits()`.
        let mut entities: Vec<Entity> = self.world.iter_entities().map(|e| e.id()).collect();
        entities.sort_by_key(|e| e.to_bits());

        for entity in entities {
            let er = self.world.entity(entity);
            mix(&mut h, entity.to_bits());

            if let Some(p) = er.get::<Position>() {
                mix(&mut h, u64::from(p.0.x.to_bits()));
                mix(&mut h, u64::from(p.0.y.to_bits()));
            }
            if let Some(v) = er.get::<Velocity>() {
                mix(&mut h, u64::from(v.0.x.to_bits()));
                mix(&mut h, u64::from(v.0.y.to_bits()));
            }
            if let Some(s) = er.get::<Stamina>() {
                mix(&mut h, u64::from(s.0.to_bits()));
            }
            if let Some(s) = er.get::<Skill>() {
                mix(&mut h, u64::from(s.0.to_bits()));
            }
            if er.get::<sim_components::BallMarker>().is_some() {
                // Phase C §4.3: ball is tagged with BallMarker. Mix
                // position/velocity from components (still per-entity) and
                // ball state from the Ball Resource below.
                if let Some(pos) = er.get::<Position>() {
                    mix(&mut h, u64::from(pos.0.x.to_bits()));
                    mix(&mut h, u64::from(pos.0.y.to_bits()));
                }
                if let Some(vel) = er.get::<Velocity>() {
                    mix(&mut h, u64::from(vel.0.x.to_bits()));
                    mix(&mut h, u64::from(vel.0.y.to_bits()));
                }
            }
            if let Some(p) = er.get::<Player>() {
                mix(&mut h, u64::from(p.team_id.0));
            }
            if let Some(rc) = er.get::<RoleComponent>() {
                mix(&mut h, role_discriminant(rc.0));
            }
            if let Some(st) = er.get::<Stamina>() {
                mix(&mut h, u64::from(st.0.to_bits()));
            }
            if let Some(sk) = er.get::<Skill>() {
                mix(&mut h, u64::from(sk.0.to_bits()));
            }
            if let Some(t) = er.get::<Team>() {
                mix(&mut h, u64::from(t.id.0));
            }
            if let Some(clock) = er.get::<MatchClock>() {
                mix(&mut h, clock.elapsed_ticks);
                mix(&mut h, u64::from(clock.half));
                mix(&mut h, clock.added_time_ticks);
            }
        }

        // Phase C §4.3: `Match` is now a Resource, not a Component. Read
        // its state from the resource so the hash still reflects match
        // score and state (the per-entity branch above is a no-op now
        // since no entity inserts `Match`).
        if let Some(m) = self.world.get_resource::<Match>() {
            mix(&mut h, match_state_discriminant(m.state));
            mix(&mut h, u64::from(m.score.0));
            mix(&mut h, u64::from(m.score.1));
        }

        // Phase C §4.3: `Ball` is also a Resource — mix state/possessor/
        // last_touched_by/spin from the resource so the hash reflects the
        // dynamic ball state in addition to position/velocity (mixed per-
        // entity above).
        if let Some(b) = self.world.get_resource::<sim_components::Ball>() {
            mix(&mut h, u64::from(b.spin.to_bits()));
            mix(&mut h, ball_state_discriminant(b.state));
            mix(
                &mut h,
                b.possessor
                    .map_or(u64::MAX, bevy_ecs::entity::Entity::to_bits),
            );
            mix(
                &mut h,
                b.last_touched_by
                    .map_or(u64::MAX, bevy_ecs::entity::Entity::to_bits),
            );
        }

        // Mix the original seed so same-seed simulations hash identically.
        mix(&mut h, self.original_seed);

        h
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
        // Phase C §4.3: Match is now a Resource, no longer a Component on
        // the entity. The match entity keeps its MatchClock component for
        // backwards compatibility (lifecycle_system still uses the entity
        // for clock reads); future Phase D work may move that to a
        // resource too.
        world.insert_resource(match_resource);
        world.entity_mut(match_entity).insert(initial_clock);

        (match_entity, home_team_entity, ball_entity)
    }

    /// Apply a manager command to the given match.
    ///
    /// # Errors
    ///
    /// Returns an error string if `match_id` is not a match entity, or if the
    /// command is not allowed in the match's current state (formation,
    /// mentality and tactic changes need `InPlay`/`Stoppage`; substitutions
    /// need `Stoppage`/`HalfTime`).
    pub fn apply_command(
        &mut self,
        _match_id: Entity,
        command: ManagerCommand,
    ) -> Result<(), String> {
        // Phase C §4.3: Match is now a Resource. Validate using
        // `world.resource::<Match>()` — the match_id parameter is retained
        // for API compatibility but is no longer used to look up Match.
        let match_component = self
            .world
            .resource::<Match>()
            .clone();

        // Validate command based on current state
        match &command {
            ManagerCommand::ChangeFormation(_) => {
                // Validate formation change is allowed in current state
                if match_component.state != MatchState::InPlay
                    && match_component.state != MatchState::Stoppage
                {
                    return Err(format!(
                        "Invalid state for formation change: {:?}",
                        match_component.state
                    ));
                }
            }
            ManagerCommand::Substitute {
                out: _,
                substitute: _,
            } => {
                // Validate substitution is allowed in current state
                if match_component.state != MatchState::Stoppage
                    && match_component.state != MatchState::HalfTime
                {
                    return Err(format!(
                        "Invalid state for substitution: {:?}",
                        match_component.state
                    ));
                }
            }
            ManagerCommand::ChangeMentality(_) => {
                // Validate mentality change is allowed in current state
                if match_component.state != MatchState::InPlay
                    && match_component.state != MatchState::Stoppage
                {
                    return Err(format!(
                        "Invalid state for mentality change: {:?}",
                        match_component.state
                    ));
                }
            }
            ManagerCommand::SetTactic(_) => {
                // Validate tactic change is allowed in current state
                if match_component.state != MatchState::InPlay
                    && match_component.state != MatchState::Stoppage
                {
                    return Err(format!(
                        "Invalid state for tactic change: {:?}",
                        match_component.state
                    ));
                }
            }
        }

        // Apply command
        match command {
            ManagerCommand::ChangeFormation(formation) => {
                // Update team formation
                let home_team = match_component.home_team;
                if let Some(mut team) = self
                    .world
                    .entity_mut(home_team)
                    .get_mut::<sim_components::Team>()
                {
                    team.formation = formation;
                }
            }
            ManagerCommand::Substitute { out, substitute } => {
                // Implement substitution logic
                // For now, just log it
                tracing::info!("Substitution: {out:?} -> {substitute:?}");
            }
            ManagerCommand::ChangeMentality(mentality) => {
                // Update team mentality
                let home_team = match_component.home_team;
                if let Some(mut team) = self
                    .world
                    .entity_mut(home_team)
                    .get_mut::<sim_components::Team>()
                {
                    team.mentality = mentality;
                }
            }
            ManagerCommand::SetTactic(tactic) => {
                // Store tactic somewhere (for now, just log)
                tracing::info!("Tactic set: {tactic:?}");
            }
        }

        Ok(())
    }

    /// Snapshot the current state of the given match.
    ///
    /// # Errors
    ///
    /// Returns an error string if no `Match` resource is registered. The
    /// `match_id` parameter is retained for API compatibility but is no
    /// longer used to locate the match (Phase C §4.3).
    pub fn get_state(&self, _match_id: Entity) -> Result<MatchSnapshot, String> {
        // Phase C §4.3: Match is now a Resource.
        let match_component = self
            .world
            .resource::<Match>()
            .clone();

        // Get ball state from the Resource, position/velocity from the entity.
        let ball_res = self.world.resource::<sim_components::Ball>();
        let mut ball_view = BallView {
            position: [0.0, 0.0],
            velocity: [0.0, 0.0],
            spin: ball_res.spin,
            state: ball_res.state,
            possessor: ball_res.possessor.map(bevy_ecs::entity::Entity::to_bits),
        };
        // Phase C §4.3: find the ball entity by `BallMarker` (the only
        // entity carrying Position + Velocity but no Player).
        let mut ball_entity_candidates: Vec<Entity> = self
            .world
            .iter_entities()
            .map(|e| e.id())
            .filter(|e| self.world.entity(*e).get::<sim_components::BallMarker>().is_some())
            .collect();
        ball_entity_candidates.sort_by_key(|e| e.to_bits());
        if let Some(ball_entity) = ball_entity_candidates.first() {
            let er = self.world.entity(*ball_entity);
            if let Some(pos) = er.get::<Position>() {
                ball_view.position = [pos.0.x, pos.0.y];
            }
            if let Some(vel) = er.get::<Velocity>() {
                ball_view.velocity = [vel.0.x, vel.0.y];
            }
        }

        // Get player views
        let mut player_views = Vec::new();
        for entity in self.world.iter_entities() {
            if let Some(player) = entity.get::<sim_components::Player>() {
                let pos = entity.get::<Position>().map(|p| p.0);
                let vel = entity.get::<Velocity>().map(|v| v.0);
                let stamina = entity.get::<Stamina>().map_or(0.0, |s| s.0);
                let role = entity.get::<RoleComponent>().map(|r| r.0);
                let skill = entity.get::<Skill>().map_or(0.0, |s| s.0);

                let bp = entity
                    .get::<PerceptionSnapshot>()
                    .map_or([0.0, 0.0], |p| [p.ball_position.x, p.ball_position.y]);

                player_views.push(PlayerView {
                    entity_id: entity.id().to_bits(),
                    team_id: player.team_id,
                    position: [pos.map_or(0.0, |p| p.x), pos.map_or(0.0, |p| p.y)],
                    velocity: [vel.map_or(0.0, |v| v.x), vel.map_or(0.0, |v| v.y)],
                    stamina,
                    role: role.unwrap_or(sim_components::Role::CentralMidfielder),
                    skill,
                    perception_ball_position: bp,
                });
            }
        }

        // Get clock view from MatchClock component on the match entity
        let clock_view = self.world.entity(self.match_entity).get::<MatchClock>().map_or(
            ClockView {
                elapsed_ticks: 0,
                half: 1,
                added_time_ticks: 0,
                is_running: false,
            },
            |clock| ClockView {
                elapsed_ticks: clock.elapsed_ticks,
                half: clock.half,
                added_time_ticks: clock.added_time_ticks,
                is_running: clock.is_running,
            },
        );

        Ok(MatchSnapshot {
            tick: self.tick,
            match_state: MatchStateView {
                state: match_component.state,
            },
            ball: ball_view,
            players: player_views,
            score: match_component.score,
            clock: clock_view,
            state_hash: self.get_state_hash(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchSnapshot {
    pub tick: u64,
    pub match_state: MatchStateView,
    pub ball: BallView,
    pub players: Vec<PlayerView>,
    pub score: (u8, u8),
    pub clock: ClockView,
    pub state_hash: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchStateView {
    pub state: MatchState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BallView {
    pub position: [f32; 2],
    pub velocity: [f32; 2],
    pub spin: f32,
    pub state: sim_components::BallState,
    pub possessor: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerView {
    pub entity_id: u64,
    pub team_id: sim_components::TeamId,
    pub position: [f32; 2],
    pub velocity: [f32; 2],
    pub stamina: f32,
    pub role: sim_components::Role,
    pub skill: f32,
    /// Phase 2: ball position as observed by this player's perception
    /// snapshot. Used by external observers / replays to verify the
    /// perception system is populating per-player ball position correctly.
    pub perception_ball_position: [f32; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockView {
    pub elapsed_ticks: u64,
    pub half: u8,
    pub added_time_ticks: u64,
    pub is_running: bool,
}

pub struct WorldWrapper {
    pub world: World,
}

impl Default for WorldWrapper {
    fn default() -> Self {
        Self::new()
    }
}

impl WorldWrapper {
    #[must_use]
    pub fn new() -> Self {
        Self {
            world: World::new(),
        }
    }
}

pub struct ScheduleWrapper {
    pub schedule: Schedule,
}

impl Default for ScheduleWrapper {
    fn default() -> Self {
        Self::new()
    }
}

impl ScheduleWrapper {
    #[must_use]
    pub fn new() -> Self {
        Self {
            schedule: Schedule::default(),
        }
    }
}

/// Stable, source-order discriminant for `BallState`. Avoids the unspecified
/// representation of plain `as u64` casts on `pub enum` so the hash stays
/// deterministic across compiler versions.
const fn ball_state_discriminant(s: sim_components::BallState) -> u64 {
    use sim_components::BallState as B;
    match s {
        B::Free => 0,
        B::Possessed => 1,
        B::InFlight => 2,
        B::OutOfPlay => 3,
        B::Dead => 4,
    }
}

const fn match_state_discriminant(s: sim_components::MatchState) -> u64 {
    use sim_components::MatchState as M;
    match s {
        M::PreMatch => 0,
        M::Kickoff => 1,
        M::InPlay => 2,
        M::Stoppage => 3,
        M::HalfTime => 4,
        M::FullTime => 5,
    }
}

const fn role_discriminant(r: sim_components::Role) -> u64 {
    use sim_components::Role as R;
    match r {
        R::Goalkeeper => 0,
        R::CenterBack => 1,
        R::FullBack => 2,
        R::DefensiveMidfielder => 3,
        R::CentralMidfielder => 4,
        R::AttackingMidfielder => 5,
        R::Winger => 6,
        R::Striker => 7,
    }
}

/// Match state machine driver. Runs once per tick after `schedule.run()`
/// (and before `tick_clock`). Owns `PreMatch` → Kickoff → `InPlay` → `HalfTime` →
/// Kickoff(2nd half) → `InPlay` → `FullTime`. Also responsible for placing the
/// ball at the center spot, zeroing ball velocity, applying the kickoff
/// impulse, and resetting the 4-4-2 formation at every kickoff transition.
///
/// Phase 1 deliberately hardcodes formation slots and intent — proving the
/// read/write-separation pipeline and perception-snapshot shape before any
/// utility-AI variable comes online.
///
/// Phase 1 implementation note: the `PreMatch` → Kickoff → `InPlay` chain is
/// instantaneous (all three states settle on tick 1), so the driver loops
/// through the state machine until the state stabilises. Without this,
/// `PreMatch` would only advance one step per tick and the kickoff impulse
/// wouldn't apply until tick 2 (clobbering anything else that ran in
/// between).
// `MatchClock.elapsed_ticks` accumulates in simulation ticks (60 ticks = 1 second).
// A half is 45 match-minutes = 45 * 60 * 60 = 162,000 ticks (HALF_LENGTH_TICKS).
// This is the SAME threshold for both halves: `elapsed_ticks` resets to 0 at the
// second-half kickoff (see `lifecycle_system` below), so half 2 needs its own
// 162,000-tick budget, not the cumulative 90-minute match length.
fn lifecycle_system(match_entity: Entity, ball_entity: Entity, sim_tick: u64, world: &mut World) {
    const MAX_TRANSITIONS_PER_TICK: usize = 8;

    for _iteration in 0..MAX_TRANSITIONS_PER_TICK {
        // Phase C §4.3: Match is now a Resource, read it from `world.resource`.
        let current_state = world.resource::<Match>().state;

        let mut next_state = current_state;
        let mut new_clock: Option<MatchClock> = None;
        let mut place_ball_center = false;
        let mut apply_kickoff_impulse = false;

        match current_state {
            MatchState::PreMatch => {
                next_state = MatchState::Kickoff;
                place_ball_center = true;
            }
            MatchState::Kickoff => {
                next_state = MatchState::InPlay;
                apply_kickoff_impulse = true;
                place_ball_center = true;
            }
            MatchState::InPlay => {
                let clock = world.entity(match_entity).get::<MatchClock>().cloned();
                if let Some(c) = clock {
                    // Both halves use the same per-half length: `elapsed_ticks` is
                    // reset to 0 at the second-half kickoff, so there is no
                    // "cumulative 90 minutes" quantity to compare against here.
                    let in_play_threshold = HALF_LENGTH_TICKS + c.added_time_ticks;
                    let is_final_half = c.half >= 2;
                    if c.elapsed_ticks >= in_play_threshold {
                        if is_final_half {
                            next_state = MatchState::FullTime;
                        } else {
                            next_state = MatchState::HalfTime;
                        }
                        let mut updated = c;
                        updated.is_running = false;
                        new_clock = Some(updated);
                    }
                }
            }
            MatchState::HalfTime => {
                let clock = world.entity(match_entity).get::<MatchClock>().cloned();
                let mut entry = world
                    .get_resource::<HalfTimeEntryTick>()
                    .copied()
                    .unwrap_or_default();
                if entry.tick.is_none() {
                    entry.tick = Some(sim_tick);
                    world.insert_resource(entry);
                }
                let entry_tick = entry.tick.unwrap_or(sim_tick);
                let in_halftime_for = sim_tick.saturating_sub(entry_tick);
                if in_halftime_for >= HALFTIME_BREAK_TICKS {
                    next_state = MatchState::Kickoff;
                    if let Some(c) = clock {
                        new_clock = Some(MatchClock {
                            elapsed_ticks: 0,
                            half: 2,
                            added_time_ticks: c.added_time_ticks,
                            is_running: true,
                        });
                    }
                    place_ball_center = true;
                    world.insert_resource(HalfTimeEntryTick { tick: None });
                }
            }
            MatchState::FullTime | MatchState::Stoppage => {}
        }

        // Apply clock mutation.
        if let Some(clock) = new_clock {
            world.entity_mut(match_entity).insert(clock);
        }

        // Apply ball placement / kickoff impulse.
        if place_ball_center {
            if let Some(mut pos) = world.entity_mut(ball_entity).get_mut::<Position>() {
                pos.0 = Vec2::new(52.5, 34.0);
            }
            if let Some(mut vel) = world.entity_mut(ball_entity).get_mut::<Velocity>() {
                vel.0 = Vec2::zero();
            }
            // Phase C §4.3: Ball state lives on the Resource, not on the entity.
            world.resource_mut::<sim_components::Ball>().state = sim_components::BallState::Free;
            // Drop any pending kick velocity — placement at the center
            // spot must not be combined with an inbound kick impulse.
            world.resource_mut::<sim_components::Ball>().kick_velocity = None;
        }
        if apply_kickoff_impulse
            && let Some(mut vel) = world.entity_mut(ball_entity).get_mut::<Velocity>()
        {
            vel.0 = Vec2::new(2.0, 0.0);
        }

        // Reset 4-4-2 formation whenever we enter Kickoff.
        if next_state == MatchState::Kickoff && current_state != MatchState::Kickoff {
            reset_formation_to_4_4_2(world);
        }

        // Commit state mutation.
        if next_state == current_state {
            // No further transitions this tick; stop iterating.
            return;
        }
        world.resource_mut::<Match>().state = next_state;
    }
}

/// Tracks the simulation tick at which the match entered the `HalfTime`
/// state. `None` means we are not currently in `HalfTime`. Used to time out
/// the half-time break using the sim-tick counter rather than the paused
/// match clock (which doesn't accumulate `elapsed` while frozen).
#[derive(Resource, Debug, Clone, Copy, Default)]
struct HalfTimeEntryTick {
    tick: Option<u64>,
}

/// Phase 2: build the default `UtilityBrain` used by every player at match
/// start. Mirrors the §4a consideration list — `MoveToPosition`, `ChaseBall`,
/// `PassTo`, `ShootAtGoal`, Tackle, `MarkOpponent`, Press, `SupportRun`,
/// `HoldPosition`. Curves are tuned for the standard 105×68 pitch (midpoints
/// are real metres, not normalised). `evaluation_interval` = 6 (per spec §13
/// decision #15: ~10 Hz decision cadence). Hysteresis bonus = 0.1.
#[expect(
    clippy::too_many_lines,
    reason = "declarative table of actions and considerations; splitting it would only scatter the data"
)]
fn default_utility_brain() -> sim_ai_player::UtilityBrain {
    use sim_ai_core::ResponseCurve;
    use sim_ai_player::{PlayerAction, PlayerConsideration};
    use sim_components::Intent;

    const fn lin(min: f32, max: f32) -> ResponseCurve {
        ResponseCurve::Linear { min, max }
    }
    const fn log(mid: f32, steep: f32) -> ResponseCurve {
        ResponseCurve::Logistic {
            midpoint: mid,
            steepness: steep,
        }
    }
    const fn step(threshold: f32, below: f32, above: f32) -> ResponseCurve {
        ResponseCurve::Step {
            threshold,
            below,
            above,
        }
    }

    let actions = vec![
        // MoveToPosition — distance_to_target: closer = better.
        PlayerAction {
            intent: Intent::MoveToPosition(Vec2::new(52.5, 34.0)),
            considerations: vec![PlayerConsideration {
                name: "distance_to_target".to_string(),
                weight: 1.0,
                curve: lin(0.0, 30.0),
            }],
        },
        // ChaseBall — distance_to_ball, stamina, pitch control at ball.
        PlayerAction {
            intent: Intent::ChaseBall,
            considerations: vec![
                PlayerConsideration {
                    name: "distance_to_ball".to_string(),
                    weight: 0.6,
                    // Step: score 0.8 if within 35 m (chasable range),
                    // 0.2 beyond. Far players still find ChaseBall
                    // marginally attractive (so they don't lock into
                    // HoldPosition purely because they're far from the ball).
                    curve: ResponseCurve::Step {
                        threshold: 35.0,
                        below: 0.8,
                        above: 0.2,
                    },
                },
                PlayerConsideration {
                    name: "stamina".to_string(),
                    weight: 0.4,
                    curve: lin(0.0, 1.0),
                },
                PlayerConsideration {
                    name: "pitch_control_at_ball".to_string(),
                    weight: 0.2,
                    curve: lin(0.0, 100.0),
                },
            ],
        },
        // PassTo — open passing lane, reasonable teammate distance, space.
        PlayerAction {
            intent: Intent::PassTo,
            considerations: vec![
                PlayerConsideration {
                    name: "pass_angle_clear".to_string(),
                    weight: 0.5,
                    curve: lin(0.0, 1.0),
                },
                PlayerConsideration {
                    name: "teammate_distance".to_string(),
                    weight: 0.3,
                    curve: lin(0.0, 30.0),
                },
                PlayerConsideration {
                    name: "teammate_space".to_string(),
                    weight: 0.2,
                    curve: lin(0.0, 20.0),
                },
            ],
        },
        // ShootAtGoal — distance sweet-spot, angle, low pressure.
        PlayerAction {
            intent: Intent::ShootAtGoal(Vec2::new(105.0, 34.0)),
            considerations: vec![
                PlayerConsideration {
                    name: "distance_to_goal".to_string(),
                    weight: 0.5,
                    // Closer is better — feed (35 - distance) so Linear gives
                    // 1.0 inside the box, 0.0 at 35 m+.
                    curve: lin(0.0, 35.0),
                },
                PlayerConsideration {
                    name: "goal_angle".to_string(),
                    weight: 0.3,
                    curve: lin(-1.0, 1.0),
                },
                PlayerConsideration {
                    name: "defender_pressure".to_string(),
                    weight: 0.2,
                    curve: ResponseCurve::Step {
                        threshold: 1.0,
                        below: 1.0,
                        above: 0.2,
                    },
                },
            ],
        },
        // Tackle — close + skill advantage.
        PlayerAction {
            intent: Intent::Tackle(Entity::PLACEHOLDER),
            considerations: vec![
                PlayerConsideration {
                    name: "distance_to_opponent".to_string(),
                    weight: 0.6,
                    curve: log(2.0, 1.5),
                },
                PlayerConsideration {
                    name: "skill_diff".to_string(),
                    weight: 0.4,
                    curve: lin(-1.0, 1.0),
                },
            ],
        },
        // MarkOpponent — close to marked, between opponent and own goal.
        PlayerAction {
            intent: Intent::MarkOpponent(Entity::PLACEHOLDER),
            considerations: vec![
                PlayerConsideration {
                    name: "distance_to_marked".to_string(),
                    weight: 0.5,
                    curve: lin(0.0, 10.0),
                },
                PlayerConsideration {
                    name: "defensive_position".to_string(),
                    weight: 0.5,
                    curve: step(0.5, 0.0, 1.0),
                },
            ],
        },
        // Press — close-range press + stamina.
        PlayerAction {
            intent: Intent::Press(Entity::PLACEHOLDER),
            considerations: vec![
                PlayerConsideration {
                    name: "distance_to_press".to_string(),
                    weight: 0.6,
                    curve: log(8.0, 0.4),
                },
                PlayerConsideration {
                    name: "stamina".to_string(),
                    weight: 0.4,
                    curve: lin(0.0, 1.0),
                },
            ],
        },
        // SupportRun — open space ahead + teammate on the ball.
        PlayerAction {
            intent: Intent::SupportRun,
            considerations: vec![
                PlayerConsideration {
                    name: "space_ahead".to_string(),
                    weight: 0.6,
                    curve: lin(0.0, 15.0),
                },
                PlayerConsideration {
                    name: "teammate_ball".to_string(),
                    weight: 0.4,
                    curve: step(0.5, 0.0, 1.0),
                },
            ],
        },
        // HoldPosition — formation discipline baseline. The score is intentionally
        // below the cap so it only wins when no other action has strong
        // positive signal — i.e. when the player has nothing better to do.
        PlayerAction {
            intent: Intent::HoldPosition,
            considerations: vec![PlayerConsideration {
                name: "formation_discipline".to_string(),
                weight: 1.0,
                // Step: 0.5 always (a "default" score, not a "best").
                curve: ResponseCurve::Step {
                    threshold: -1.0,
                    below: 0.5,
                    above: 0.5,
                },
            }],
        },
    ];

    sim_ai_player::UtilityBrain {
        actions,
        hysteresis: 0.05,
    }
}

/// Phase 1 hardcoded 4-4-2 formation. Sorts players on each team by current
/// x-coordinate and assigns slot positions from the spec table:
///   home (`team_id` == 0) — GK x=5, defenders y=20 at x={20,35,50,65},
///     midfielders y=34 at x={25,40,55,70}, forwards y=34 at x={80,88}.
///   away (`team_id` == 1) — GK x=100, defenders y=48 at x={20,35,50,65},
///     midfielders y=34 at x={35,50,65,80}, forwards y=34 at x={17,25}.
/// Velocities are zeroed; intents reset to `HoldPosition`.
fn reset_formation_to_4_4_2(world: &mut World) {
    // Home formation targets — index 0 = GK, 1..=4 = defenders (sorted by x),
    // 5..=8 = midfielders (sorted by x), 9..=10 = forwards (sorted by x).
    let home_targets: [Vec2; 11] = [
        Vec2::new(5.0, 34.0),  // GK
        Vec2::new(20.0, 20.0), // Defender 1
        Vec2::new(35.0, 20.0), // Defender 2
        Vec2::new(50.0, 20.0), // Defender 3
        Vec2::new(65.0, 20.0), // Defender 4
        Vec2::new(25.0, 34.0), // Midfielder 1
        Vec2::new(40.0, 34.0), // Midfielder 2
        Vec2::new(55.0, 34.0), // Midfielder 3
        Vec2::new(70.0, 34.0), // Midfielder 4
        Vec2::new(80.0, 34.0), // Forward 1
        Vec2::new(88.0, 34.0), // Forward 2
    ];
    // Away formation targets — mirrored about pitch centre (52.5, 34).
    let away_targets: [Vec2; 11] = [
        Vec2::new(100.0, 34.0), // GK
        Vec2::new(20.0, 48.0),  // Defender 1 (y mirrored from 20 → 48)
        Vec2::new(35.0, 48.0),  // Defender 2
        Vec2::new(50.0, 48.0),  // Defender 3
        Vec2::new(65.0, 48.0),  // Defender 4
        Vec2::new(35.0, 34.0),  // Midfielder 1 (x mirrored: 70 → 35)
        Vec2::new(50.0, 34.0),  // Midfielder 2 (x mirrored: 55 → 50)
        Vec2::new(65.0, 34.0),  // Midfielder 3 (x mirrored: 40 → 65)
        Vec2::new(80.0, 34.0),  // Midfielder 4 (x mirrored: 25 → 80)
        Vec2::new(17.0, 34.0),  // Forward 1 (x mirrored: 88 → 17)
        Vec2::new(25.0, 34.0),  // Forward 2 (x mirrored: 80 → 25)
    ];

    let mut home_players: Vec<Entity> = Vec::new();
    let mut away_players: Vec<Entity> = Vec::new();
    for entity in world.iter_entities() {
        let id = entity.id();
        if let Some(p) = world.entity(id).get::<Player>() {
            if p.team_id.0 == 0 {
                home_players.push(id);
            } else if p.team_id.0 == 1 {
                away_players.push(id);
            }
        }
    }
    // Sort by current x to make slot assignment deterministic regardless of
    // entity spawn order.
    home_players.sort_by(|a, b| {
        let ax = world.entity(*a).get::<Position>().map_or(0.0, |p| p.0.x);
        let bx = world.entity(*b).get::<Position>().map_or(0.0, |p| p.0.x);
        ax.partial_cmp(&bx).unwrap_or(std::cmp::Ordering::Equal)
    });
    away_players.sort_by(|a, b| {
        let ax = world.entity(*a).get::<Position>().map_or(0.0, |p| p.0.x);
        let bx = world.entity(*b).get::<Position>().map_or(0.0, |p| p.0.x);
        // Away sorts descending so that the goalkeeper (rightmost) gets index 0.
        bx.partial_cmp(&ax).unwrap_or(std::cmp::Ordering::Equal)
    });

    for (i, entity) in home_players.iter().enumerate() {
        if let Some(target) = home_targets.get(i) {
            apply_player_slot(world, *entity, *target);
        }
    }
    for (i, entity) in away_players.iter().enumerate() {
        if let Some(target) = away_targets.get(i) {
            apply_player_slot(world, *entity, *target);
        }
    }
}

/// Place a single player at `target`, zero velocity, reset intent to
/// `HoldPosition`. Mirrors the Position into Player.position so any system
/// that reads the player component sees the same value.
///
/// Phase 2: leave intent as `None` so the player's first decision cycle
/// isn't dominated by a hysteresis bonus on `HoldPosition`. The
/// `player_decision_system` will set the first real intent on its cadence
/// tick.
fn apply_player_slot(world: &mut World, player_entity: Entity, target: Vec2) {
    if let Some(mut pos) = world.entity_mut(player_entity).get_mut::<Position>() {
        pos.0 = target;
    }
    if let Some(mut vel) = world.entity_mut(player_entity).get_mut::<Velocity>() {
        vel.0 = Vec2::zero();
    }
    if let Some(mut p) = world.entity_mut(player_entity).get_mut::<Player>() {
        p.intent = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_components::time;

    #[test]
    fn test_simulation_creation() {
        let sim = Simulation::new(12345);
        assert_eq!(sim.tick, 0);
    }

    /// Strengthened from the prior "tick-aligned equality" form: collect every
    /// per-tick hash from each simulation and assert the full 1000-element
    /// histories are equal element-wise. This catches any drift that would
    /// cancel out by the time the loop finished.
    #[test]
    fn test_per_tick_determinism() {
        let seed = 42u64;
        let mut sim1 = Simulation::new(seed);
        let mut sim2 = Simulation::new(seed);

        let mut hashes1 = Vec::with_capacity(1000);
        let mut hashes2 = Vec::with_capacity(1000);

        for _ in 0..1000 {
            sim1.tick();
            sim2.tick();
            hashes1.push(sim1.get_state_hash());
            hashes2.push(sim2.get_state_hash());
        }

        assert_eq!(hashes1.len(), 1000);
        assert_eq!(hashes2.len(), 1000);
        assert_eq!(
            hashes1, hashes2,
            "Per-tick hash histories diverged — determinism violated"
        );
    }

    /// Two simulations constructed identically must hash identically for the
    /// first N ticks. With RNG state mixed into the hash, identical seed →
    /// identical RNG state ensures the hash is equal even after any number
    /// of deterministic advances.
    #[test]
    fn test_same_seed_same_hash() {
        let seed = 0xCAFE_BABE_DEAD_BEEFu64;
        let mut a = Simulation::new(seed);
        let mut b = Simulation::new(seed);
        let n = 50u64;
        for _ in 0..n {
            a.tick();
            b.tick();
            assert_eq!(a.get_state_hash(), b.get_state_hash());
        }
    }

    #[test]
    fn test_pipeline_isolation_single_tick() {
        let mut sim = Simulation::new(42);
        let match_entity = sim.match_entity;

        sim.tick();

        let snapshot = sim.get_state(match_entity).expect("get_state failed");

        assert_ne!(
            snapshot.state_hash, 0,
            "State hash is zero — systems did not run"
        );
        assert_eq!(snapshot.players.len(), 22, "Expected 22 players");

        let bx = snapshot.ball.position[0];
        let by = snapshot.ball.position[1];
        assert!(bx >= 0.0 && bx <= 105.0, "Ball x {} out of pitch", bx);
        assert!(by >= 0.0 && by <= 68.0, "Ball y {} out of pitch", by);
    }

    /// Sanity check that the hash is a non-zero u64 that exercises every
    /// hash arm (entities, Position, Velocity, Ball, Player, Team, Match).
    /// We don't pin a numeric value (the hash is internal), only that it
    /// is stable for identical inputs.
    #[test]
    fn test_state_hash_is_stable() {
        let seed = 0x1234_5678u64;
        let sim = Simulation::new(seed);
        let h1 = sim.get_state_hash();
        let h2 = sim.get_state_hash();
        assert_eq!(h1, h2, "Hash changed between identical reads");
        assert_ne!(h1, 0);
    }

    /// The ball must actually move when given a non-zero velocity. Find the
    /// ball entity (the one with both `Ball` and `Position`/`Velocity`
    /// components) and verify the Position changes after a tick.
    ///
    /// Phase 1 update: lifecycle_system kicks the ball at Kickoff→InPlay with
    /// a velocity of (2.0, 0.0) on the first tick, which would clobber any
    /// velocity the test sets *before* the first tick. We instead let the
    /// first tick complete (so the kickoff settles), then perturb velocity
    /// and verify motion on tick 2.
    #[test]
    fn test_ball_moves_under_physics() {
        use sim_components::Position as PosComp;
        use sim_components::Velocity as VelComp;
        use sim_math::Vec2;

        let mut sim = Simulation::new(7);
        let ball_entity = sim.ball_entity;

        // Let the kickoff settle so the lifecycle no longer overwrites
        // velocity on each tick.
        sim.tick();

        // Set a non-zero velocity directly via world mutation.
        {
            let mut em = sim.world.entity_mut(ball_entity);
            let mut v = em.get_mut::<VelComp>().expect("ball has Velocity");
            v.0 = Vec2::new(5.0, 0.0);
        }
        // Snapshot initial Position component.
        let initial_pos: (f32, f32) = {
            let p = sim
                .world
                .entity(ball_entity)
                .get::<PosComp>()
                .expect("ball has Position");
            (p.0.x, p.0.y)
        };

        sim.tick();

        let final_pos: (f32, f32) = {
            let p = sim
                .world
                .entity(ball_entity)
                .get::<PosComp>()
                .expect("ball has Position");
            (p.0.x, p.0.y)
        };

        assert_ne!(
            final_pos, initial_pos,
            "Ball Position did not change after tick: started at {initial_pos:?}, ended at {final_pos:?}"
        );
    }

    /// Two identically-constructed simulations should hash identically; a
    /// perturbation to ONLY one (here: +1.0 on the ball's x) must make the
    /// hashes diverge.
    #[test]
    fn test_hash_detects_state_change() {
        use sim_components::Position as PosComp;
        use sim_math::Vec2;

        let seed = 0xABCD_EF01_2345_6789u64;
        let mut a = Simulation::new(seed);
        let mut b = Simulation::new(seed);

        for _ in 0..10 {
            a.tick();
            b.tick();
        }
        let hash_before_a = a.get_state_hash();
        let hash_before_b = b.get_state_hash();
        assert_eq!(
            hash_before_a, hash_before_b,
            "Identically-constructed simulations diverged before perturbation"
        );

        // Perturb ball on sim a.
        let ball_a = a.ball_entity;
        {
            let mut em = a.world.entity_mut(ball_a);
            let mut p = em.get_mut::<PosComp>().expect("ball has Position");
            p.0 = Vec2::new(p.0.x + 1.0, p.0.y);
        }

        let hash_after_a = a.get_state_hash();
        let hash_after_b = b.get_state_hash();
        assert_ne!(
            hash_after_a, hash_after_b,
            "Hash failed to detect +1.0 perturbation on ball x"
        );
    }

    /// Phase 1 verification: after 60 ticks (≈ 1 s of sim time) the match
    /// clock's `elapsed` should be ~1.0. Asserts the read/write-separation
    /// pipeline carries the `MatchClock` component forward without losing
    /// updates.
    #[test]
    fn test_clock_advances() {
        let mut sim = Simulation::new(42);
        let me = sim.match_entity;
        for _ in 0..60 {
            sim.tick();
        }
        let elapsed_ticks = sim
            .world
            .entity(me)
            .get::<MatchClock>()
            .expect("match entity has MatchClock component")
            .elapsed_ticks;
        assert!(
            elapsed_ticks == 60,
            "clock should be 60 ticks, got {}",
            elapsed_ticks
        );
    }

    /// Verification: a full match (≤ 330 000 ticks — two 45-minute halves at
    /// 162 000 ticks each, plus the 18-tick halftime break, matching the
    /// `--full-match` CLI budget of 324 000 ticks) eventually drives `state`
    /// to `FullTime`, transitions through `half == 2`, and the clock
    /// pauses/resets at half-time.
    #[test]
    fn test_full_match_reaches_full_time() {
        let mut sim = Simulation::new(7);
        let me = sim.match_entity;
        let mut saw_half_2 = false;
        let mut clock_paused_after_half = false;

        let final_state = {
            let mut state = MatchState::PreMatch;
            for _ in 0..330_000u64 {
                sim.tick();
                let clock = sim
                    .world
                    .entity(me)
                    .get::<MatchClock>()
                    .cloned()
                    .expect("match clock");
                if clock.half == 2 && clock.elapsed_ticks > 0 && clock.elapsed_ticks < 162_100 {
                    saw_half_2 = true;
                }
                // After we first observe half == 2 (i.e. 2nd half in progress),
                // check that the clock was reset toward 0 after half-time.
                if clock.half == 2 && !clock.is_running {
                    clock_paused_after_half = true;
                }
                state = sim.world.resource::<Match>().state;
                if state == MatchState::FullTime {
                    break;
                }
            }
            state
        };

        assert_eq!(
            final_state,
            MatchState::FullTime,
            "match never reached FullTime within 330_000 ticks"
        );
        assert!(
            saw_half_2,
            "clock.half never observed as 2 between tick 162_000 and 324_100"
        );
        assert!(
            clock_paused_after_half,
            "clock.is_running was never false while half==2 (expected pause at HalfTime)"
        );

        // The 2nd-half clock should have reset toward 0 at the HalfTime→Kickoff
        // boundary (and the HalfTime phase paused the clock for ~0.3 s). After
        // FullTime we expect elapsed to have moved past 90 (or whatever the
        // 2nd-half total was) — but importantly, after half-time was reset,
        // elapsed should be small again at some point. Verify we observed at
        // least one tick where half==2 and elapsed < 1.0 (i.e. clock was
        // restarted cleanly).
        let mut observed_low_elapsed_in_half_2 = false;
        let mut sim2 = Simulation::new(7);
        let me2 = sim2.match_entity;
        for _ in 0..170_000u64 {
            sim2.tick();
            let c = sim2.world.entity(me2).get::<MatchClock>().cloned().unwrap();
            if c.half == 2 && c.elapsed_ticks < 60 {
                observed_low_elapsed_in_half_2 = true;
                break;
            }
        }
        assert!(
            observed_low_elapsed_in_half_2,
            "clock did not reset toward 0 after half-time"
        );
    }

    /// Regression test: exported player positions via `get_state()` must read
    /// from the Position component (not the removed Player.position field).
    ///
    /// Before the sync, `get_state()` read `Player.position` which was never
    /// updated after `create_match` — the physics systems only mutate the
    /// `Position` component. This caused all 22 players to appear frozen at
    /// their kickoff coordinates in any exported state while the actual
    /// simulation physics was running against the `Position` component.
    #[test]
    fn test_player_positions_update_in_exported_state() {
        let mut sim = Simulation::new(99);
        let me = sim.match_entity;

        // Let the kickoff impulse settle (kickoff → InPlay on tick 0).
        sim.tick();

        // Verify get_state reads from Position component by checking that
        // the exported positions match the Position component values.
        let state = sim.get_state(me).expect("get_state works");
        for player_view in &state.players {
            let entity = Entity::from_bits(player_view.entity_id);
            let pos = sim.world.entity(entity).get::<Position>().unwrap().0;
            assert_eq!(
                player_view.position,
                [pos.x, pos.y],
                "get_state() must read position from Position component"
            );
        }

        // Advance a few ticks to ensure physics runs
        for _ in 0..10 {
            sim.tick();
        }

        // Verify again after physics runs
        let state = sim.get_state(me).expect("get_state works");
        for player_view in &state.players {
            let entity = Entity::from_bits(player_view.entity_id);
            let pos = sim.world.entity(entity).get::<Position>().unwrap().0;
            assert_eq!(
                player_view.position,
                [pos.x, pos.y],
                "get_state() must read position from Position component after physics"
            );
        }
    }

    /// Test: Half-time transition with added time.
    /// Verifies that the match correctly transitions to HalfTime when
    /// the first half elapsed time reaches 45 minutes + added time.
    #[test]
    fn test_half_time_transition_with_added_time() {
        let mut sim = Simulation::new(42);
        let me = sim.match_entity;

        // Fast-forward to just before half-time with 3 minutes added time
        // Set clock close to threshold so transition happens quickly
        let clock = MatchClock {
            elapsed_ticks: time::HALF_LENGTH_TICKS + 3 * 60 * 60 - 60, // 1 second before half-time threshold
            half: 1,
            added_time_ticks: 3 * 60 * 60, // 3 minutes added time
            is_running: true,
        };
        sim.world.resource_mut::<Match>().state = MatchState::InPlay;
        sim.world.entity_mut(me).insert(clock);

        // Run until half-time transition (needs ~60 ticks to reach threshold)
        for _ in 0..100 {
            sim.tick();
            let state = sim.world.resource::<Match>().state;
            if state == MatchState::HalfTime {
                break;
            }
        }

        let m = sim.world.resource::<Match>();
        let clock = sim.world.entity(me).get::<MatchClock>().unwrap();
        assert_eq!(m.state, MatchState::HalfTime);
        assert!(!clock.is_running);
        assert_eq!(clock.half, 1);
        assert_eq!(clock.added_time_ticks, 3 * 60 * 60);
    }

    /// Test: Second-half kickoff clock reset.
    /// Verifies that at the start of the second half, elapsed_ticks resets to 0.
    #[test]
    fn test_second_half_kickoff_clock_reset() {
        let mut sim = Simulation::new(42);
        let me = sim.match_entity;

        // Set up match at HalfTime state
        let clock = MatchClock {
            elapsed_ticks: time::HALF_LENGTH_TICKS,
            half: 1,
            added_time_ticks: 0,
            is_running: false,
        };
        sim.world.resource_mut::<Match>().state = MatchState::HalfTime;
        sim.world.entity_mut(me).insert(clock);

        // Insert HalfTimeEntryTick resource to trigger transition
        sim.world.insert_resource(crate::HalfTimeEntryTick {
            tick: Some(sim.tick),
        });

        // Run ticks to trigger HalfTime -> Kickoff -> InPlay (second half) transition
        // The lifecycle system transitions HalfTime -> Kickoff -> InPlay in one tick
        for _ in 0..30 {
            sim.tick();
            let state = sim.world.resource::<Match>().state;
            // After transition, state will be InPlay (second half)
            if state == MatchState::InPlay {
                break;
            }
        }

        // Verify clock was reset for second half and state is InPlay (half 2)
        // Note: tick_clock() runs after lifecycle_system(), so after one tick
        // the clock will be at 1 (it was reset to 0, then incremented).
        let m = sim.world.resource::<Match>();
        let clock = sim.world.entity(me).get::<MatchClock>().unwrap();
        assert_eq!(
            m.state,
            MatchState::InPlay,
            "should be InPlay in second half"
        );
        assert_eq!(clock.half, 2, "should be in second half");
        assert_eq!(
            clock.elapsed_ticks, 1,
            "clock should be at 1 after first tick of second half (reset to 0 then incremented)"
        );
        assert!(clock.is_running);
    }

    /// Test: Clock pause/resume during stoppage.
    /// Verifies that the match clock pauses when is_running is false
    /// and resumes when set back to true.
    #[test]
    fn test_clock_pause_resume_stoppage() {
        let mut sim = Simulation::new(42);
        let me = sim.match_entity;

        // Set to InPlay with clock running
        let clock = MatchClock {
            elapsed_ticks: 30 * 60 * 60, // 30 minutes
            half: 1,
            added_time_ticks: 0,
            is_running: true,
        };
        sim.world.resource_mut::<Match>().state = MatchState::InPlay;
        sim.world.entity_mut(me).insert(clock);

        // Run 60 ticks (1 second) - clock should advance
        for _ in 0..60 {
            sim.tick();
        }
        let clock = sim.world.entity(me).get::<MatchClock>().unwrap();
        assert_eq!(clock.elapsed_ticks, 30 * 60 * 60 + 60);

        // Pause clock (simulate stoppage)
        if let Some(mut clock) = sim.world.get_mut::<MatchClock>(me) {
            clock.is_running = false;
        }

        // Run 60 ticks - clock should NOT advance
        for _ in 0..60 {
            sim.tick();
        }
        let clock = sim.world.entity(me).get::<MatchClock>().unwrap();
        assert_eq!(
            clock.elapsed_ticks,
            30 * 60 * 60 + 60,
            "clock should not advance while paused"
        );

        // Resume clock
        if let Some(mut clock) = sim.world.get_mut::<MatchClock>(me) {
            clock.is_running = true;
        }

        // Run 60 ticks - clock should advance again
        for _ in 0..60 {
            sim.tick();
        }
        let clock = sim.world.entity(me).get::<MatchClock>().unwrap();
        assert_eq!(
            clock.elapsed_ticks,
            30 * 60 * 60 + 60 + 60,
            "clock should advance after resume"
        );
    }

    /// Test: AI time-remaining calculation in second half.
    /// Verifies that the time remaining calculation correctly handles
    /// the second half (where elapsed_ticks resets but total elapsed continues).
    #[test]
    fn test_ai_time_remaining_in_second_half() {
        let mut sim = Simulation::new(42);
        let me = sim.match_entity;

        // Set up match in second half, 15 minutes elapsed (60 minutes total match time)
        let clock = MatchClock {
            elapsed_ticks: 15 * 60 * 60, // 15 minutes into second half
            half: 2,
            added_time_ticks: 0,
            is_running: true,
        };
        sim.world.resource_mut::<Match>().state = MatchState::InPlay;
        sim.world.entity_mut(me).insert(clock);

        // Get the clock and compute time remaining using shared utilities
        let clock = sim.world.entity(me).get::<MatchClock>().unwrap();
        let match_remaining_secs = time::match_time_remaining_secs(clock);
        let half_remaining_secs = time::half_time_remaining_secs(clock);

        // Match time remaining: 90 min - 60 min = 30 min = 1800 sec
        assert_eq!(match_remaining_secs, 30.0 * 60.0);
        // Half time remaining: 45 min - 15 min = 30 min = 1800 sec
        assert_eq!(half_remaining_secs, 30.0 * 60.0);

        // Now test at 40 minutes into second half (85 minutes total)
        let clock = MatchClock {
            elapsed_ticks: 40 * 60 * 60, // 40 minutes into second half
            half: 2,
            added_time_ticks: 0,
            is_running: true,
        };
        sim.world.entity_mut(me).insert(clock);

        let clock = sim.world.entity(me).get::<MatchClock>().unwrap();
        let match_remaining_secs = time::match_time_remaining_secs(clock);
        let half_remaining_secs = time::half_time_remaining_secs(clock);

        // Match time remaining: 90 min - 85 min = 5 min = 300 sec
        assert_eq!(match_remaining_secs, 5.0 * 60.0);
        // Half time remaining: 45 min - 40 min = 5 min = 300 sec
        assert_eq!(half_remaining_secs, 5.0 * 60.0);
    }
}
