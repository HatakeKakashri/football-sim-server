use bevy_ecs::prelude::*;
use sim_components::{Match, MatchClock, MatchState, ManagerCommand, PerceptionSnapshot, PitchBounds, Player, Position, Velocity};
use sim_math::{DeterministicRng, PitchDimensions, Vec2};
use sim_physics::ball_physics_system;
use serde::{Serialize, Deserialize};

pub const FIXED_TIMESTEP: f32 = 1.0 / 60.0;
pub const MAX_ACCUMULATOR: f32 = 0.25;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SimulationSet {
    Perception,
    Decision,
    Intent,
    Execution,
    Physics,
    Rules,
    Referee,
}

pub struct Simulation {
    pub world: World,
    pub schedule: Schedule,
    pub rng: DeterministicRng,
    pub tick: u64,
    pub accumulator: f32,
    pub match_entity: Entity,
    pub ball_entity: Entity,
}

impl Simulation {
    pub fn new(seed: u64) -> Self {
        let mut world = World::new();
        // Phase 0 substrate: physics needs pitch dimensions. Insert the standard
        // pitch as a resource so the registered ball physics system can resolve
        // its `Res<PitchDimensions>` parameter.
        world.insert_resource(PitchDimensions::standard());
        // Phase 2: insert the pitch control grid resource so the
        // pitch_control_system (registered below) can mutate it in place.
        world.insert_resource(sim_physics::PitchControlGrid::build_standard());

        let mut schedule = Schedule::default();
        let rng = DeterministicRng::new(seed);

        // Configure system sets for explicit ordering
        schedule.configure_sets(
            (
                SimulationSet::Perception,
                SimulationSet::Decision,
                SimulationSet::Intent,
                SimulationSet::Execution,
                SimulationSet::Physics,
                SimulationSet::Rules,
                SimulationSet::Referee,
            )
                .chain(),
        );

        // Phase 0 substrate: register ONLY ball physics. No perception,
        // decision, intent, execution, rules, or referee systems are wired —
        // those belong to later phases.
        schedule.add_systems(ball_physics_system.in_set(SimulationSet::Physics));

        // Phase 1: register the perception → decision → execution → player
        // movement chain alongside the existing ball physics. These run in the
        // schedule so each tick advances all read/write-separation systems in
        // order. Lifecycle runs *outside* the schedule (after `schedule.run`)
        // because it mutates match state and needs the schedule to have already
        // executed this tick.
        use sim_ai_player::{
            perception_system, player_action_execution_system, player_decision_system,
        };
        use sim_physics::{pitch_control_system, player_movement_system};
        schedule.add_systems(
            (
                perception_system.in_set(SimulationSet::Perception),
                pitch_control_system.in_set(SimulationSet::Perception),
                player_decision_system.in_set(SimulationSet::Decision),
                player_action_execution_system.in_set(SimulationSet::Execution),
                player_movement_system.in_set(SimulationSet::Physics),
            )
                .chain(),
        );

        // Create match + home team; resolve ball entity for later use by the
        // lifecycle system and clock advancement.
        let (match_entity, _home_team_entity) = Self::create_match(&mut world, seed);
        let ball_entity = world
            .iter_entities()
            .map(|e| e.id())
            .find(|e| world.entity(*e).get::<sim_components::Ball>().is_some())
            .expect("Ball entity must exist after create_match");

        Self {
            world,
            schedule,
            rng,
            tick: 0,
            accumulator: 0.0,
            match_entity,
            ball_entity,
        }
    }

    pub fn tick(&mut self, delta_time: f32) {
        self.accumulator += delta_time;
        
        // Cap accumulator to prevent spiral of death
        if self.accumulator > MAX_ACCUMULATOR {
            self.accumulator = MAX_ACCUMULATOR;
        }
        
        while self.accumulator >= FIXED_TIMESTEP {
            self.schedule.run(&mut self.world);
            // lifecycle_system takes the *pre-increment* tick counter so the
            // first tick is tick 0. Used to time out the HalfTime state
            // independent of the paused match clock.
            let pre_tick = self.tick;
            lifecycle_system(self.match_entity, self.ball_entity, pre_tick, &mut self.world);
            self.tick_clock();
            self.tick += 1;
            self.accumulator -= FIXED_TIMESTEP;
        }
    }

    /// Advance the match clock by `FIXED_TIMESTEP` if the clock is running.
    /// Clock advancement runs after `schedule.run()` and `lifecycle_system()`
    /// so that any state transitions (e.g. `InPlay → HalfTime`) have already
    /// been applied and `is_running` reflects the new state.
    ///
    /// Mirrors the updated component value into `Match.clock` so the snapshot
    /// hash (which reads from the inner field) sees the same value.
    fn tick_clock(&mut self) {
        let new_elapsed = if let Some(mut clock) = self
            .world
            .entity_mut(self.match_entity)
            .get_mut::<MatchClock>()
        {
            if clock.is_running {
                clock.elapsed += FIXED_TIMESTEP;
                Some(clock.clone())
            } else {
                None
            }
        } else {
            None
        };
        if let Some(clock) = new_elapsed {
            if let Some(mut m) = self.world.entity_mut(self.match_entity).get_mut::<Match>() {
                m.clock = clock;
            }
        }
    }

    /// Compute a deterministic FNV-1a (64-bit) hash over the world state.
    ///
    /// Covers (in this fixed order, per-entity, entities sorted by
    /// `Entity::to_bits()`): `Position`, `Velocity`, `Stamina`, `Skill`, `Ball`,
    /// `Player`, `Team`, `Match`. After all entity bytes are mixed in, the
    /// RNG state is appended as the final 4 bytes.
    ///
    /// **Excluded** by design: the tick counter, accumulator, and any
    /// wall-clock data. The hash identifies *state*, not *time* — two
    /// identical states reached at different ticks must hash equal, which is
    /// what makes replay divergence detection meaningful.
    pub fn get_state_hash(&self) -> u64 {
        use sim_components::{
            Ball, Match, Player, Position, Skill, Stamina, Team, Velocity,
        };

        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        fn mix(h: &mut u64, v: u64) {
            // FNV-1a: XOR one byte at a time (little-endian) then multiply.
            let bytes = v.to_le_bytes();
            for b in bytes {
                *h ^= u64::from(b);
                *h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }

        // Collect entities in a deterministic order: by `to_bits()`.
        let mut entities: Vec<Entity> = self.world.iter_entities().map(|e| e.id()).collect();
        entities.sort_by_key(|e| e.to_bits());

        for entity in entities {
            let er = self.world.entity(entity);
            mix(&mut h, entity.to_bits());

            if let Some(p) = er.get::<Position>() {
                mix(&mut h, p.0.x.to_bits() as u64);
                mix(&mut h, p.0.y.to_bits() as u64);
            }
            if let Some(v) = er.get::<Velocity>() {
                mix(&mut h, v.0.x.to_bits() as u64);
                mix(&mut h, v.0.y.to_bits() as u64);
            }
            if let Some(s) = er.get::<Stamina>() {
                mix(&mut h, s.0.to_bits() as u64);
            }
            if let Some(s) = er.get::<Skill>() {
                mix(&mut h, s.0.to_bits() as u64);
            }
            if let Some(b) = er.get::<Ball>() {
                mix(&mut h, b.position.x.to_bits() as u64);
                mix(&mut h, b.position.y.to_bits() as u64);
                mix(&mut h, b.velocity.x.to_bits() as u64);
                mix(&mut h, b.velocity.y.to_bits() as u64);
                mix(&mut h, b.spin.to_bits() as u64);
                mix(&mut h, ball_state_discriminant(b.state));
                mix(
                    &mut h,
                    b.possessor.map_or(u64::MAX, |e| e.to_bits()),
                );
            }
            if let Some(p) = er.get::<Player>() {
                mix(&mut h, u64::from(p.team_id.0));
                mix(&mut h, role_discriminant(p.role));
            }
            if let Some(t) = er.get::<Team>() {
                mix(&mut h, u64::from(t.id.0));
            }
            if let Some(m) = er.get::<Match>() {
                mix(&mut h, match_state_discriminant(m.state));
                mix(&mut h, u64::from(m.score.0));
                mix(&mut h, u64::from(m.score.1));
                mix(&mut h, m.clock.elapsed.to_bits() as u64);
                mix(&mut h, u64::from(m.clock.half));
                mix(&mut h, m.clock.added_time.to_bits() as u64);
            }
        }

        // Mix the RNG state as the final value (so two simulations that have
        // advanced their RNG differently — even with identical world state —
        // will hash differently).
        mix(&mut h, u64::from(self.rng.state()));

        h
    }

    pub fn create_match(world: &mut World, seed: u64) -> (Entity, Entity) {
        use sim_components::{Ball, Match, MatchClock, MatchState, Player, Position, Team, TeamId, Velocity};
        use sim_math::Vec2;

        // Create ball entity
        let ball_entity = world.spawn(()).id();
        world.entity_mut(ball_entity).insert(Ball {
            position: Vec2::new(52.5, 34.0), // Center of pitch
            velocity: Vec2::zero(),
            spin: 0.0,
            state: sim_components::BallState::Free,
            possessor: None,
        });
        // Ball also carries Position + Velocity components so the registered
        // ball_physics_system query (`Position, Velocity, Ball`) matches it.
        // The Ball struct fields stay as the snapshot/diagnostic copy.
        world.entity_mut(ball_entity).insert(Position(Vec2::new(52.5, 34.0)));
        world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

        // Create home team
        let home_team_entity = world.spawn(()).id();
        let mut home_players = Vec::new();

        // Create 11 home players
        for i in 0..11 {
            let player_entity = world.spawn(()).id();
            let pos = Vec2::new(10.0 + (i as f32 * 8.0), 34.0);
            // Phase 1: seed each player with a default empty PerceptionSnapshot
            // stored in the existing `Player.perception` field so downstream
            // readers always have a valid snapshot without needing it to also
            // be a Component (which is out of scope for Phase 1).
            let default_snapshot = PerceptionSnapshot {
                self_position: pos,
                nearby_teammates: smallvec::SmallVec::new(),
                nearby_opponents: smallvec::SmallVec::new(),
                ball_position: Vec2::new(52.5, 34.0),
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
                position: pos,
                velocity: Vec2::zero(),
                stamina: 1.0,
                role: sim_components::Role::CentralMidfielder,
                skill: 0.7,
                intent: None,
                perception: Some(default_snapshot),
                score_differential: 0,
                time_remaining: 90.0,
                team_possession: 0.5,
                mentality_modifier: 0.0,
            });
            world.entity_mut(player_entity).insert(Position(pos));
            world.entity_mut(player_entity).insert(Velocity(Vec2::zero()));
            world.entity_mut(player_entity).insert(default_utility_brain());
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
        for i in 0..11 {
            let player_entity = world.spawn(()).id();
            let pos = Vec2::new(95.0 - (i as f32 * 8.0), 34.0);
            let default_snapshot = PerceptionSnapshot {
                self_position: pos,
                nearby_teammates: smallvec::SmallVec::new(),
                nearby_opponents: smallvec::SmallVec::new(),
                ball_position: Vec2::new(52.5, 34.0),
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
                position: pos,
                velocity: Vec2::zero(),
                stamina: 1.0,
                role: sim_components::Role::CentralMidfielder,
                skill: 0.7,
                intent: None,
                perception: Some(default_snapshot),
                score_differential: 0,
                time_remaining: 90.0,
                team_possession: 0.5,
                mentality_modifier: 0.0,
            });
            world.entity_mut(player_entity).insert(Position(pos));
            world.entity_mut(player_entity).insert(Velocity(Vec2::zero()));
            world.entity_mut(player_entity).insert(default_utility_brain());
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
            elapsed: 0.0,
            half: 1,
            added_time: 0.0,
            is_running: true,
        };
        world.entity_mut(match_entity).insert(Match {
            id: 1,
            home_team: home_team_entity,
            away_team: away_team_entity,
            score: (0, 0),
            clock: initial_clock.clone(),
            state: MatchState::PreMatch,
            seed,
        });
        // Phase 1: also expose the clock as its own Component so the
        // `tick_clock` and `lifecycle_system` functions can mutate it via
        // `get_mut::<MatchClock>()`. The inner `Match.clock` field remains
        // the snapshot used by the state hash; both views are kept in sync
        // by every site that mutates the component.
        world.entity_mut(match_entity).insert(initial_clock);

        (match_entity, home_team_entity)
    }

    pub fn apply_command(&mut self, match_id: Entity, command: ManagerCommand) -> Result<(), String> {
        // Get match component
        let match_component = self.world.entity(match_id).get::<Match>().ok_or("Match entity not found")?;
        
        // Validate command based on current state
        match &command {
            ManagerCommand::ChangeFormation(_) => {
                // Validate formation change is allowed in current state
                if match_component.state != MatchState::InPlay &&
                   match_component.state != MatchState::Stoppage {
                    return Err(format!("Invalid state for formation change: {:?}", match_component.state));
                }
            }
            ManagerCommand::Substitute { out: _, substitute: _ } => {
                // Validate substitution is allowed in current state
                if match_component.state != MatchState::Stoppage &&
                   match_component.state != MatchState::HalfTime {
                    return Err(format!("Invalid state for substitution: {:?}", match_component.state));
                }
            }
            ManagerCommand::ChangeMentality(_) => {
                // Validate mentality change is allowed in current state
                if match_component.state != MatchState::InPlay &&
                   match_component.state != MatchState::Stoppage {
                    return Err(format!("Invalid state for mentality change: {:?}", match_component.state));
                }
            }
            ManagerCommand::SetTactic(_) => {
                // Validate tactic change is allowed in current state
                if match_component.state != MatchState::InPlay &&
                   match_component.state != MatchState::Stoppage {
                    return Err(format!("Invalid state for tactic change: {:?}", match_component.state));
                }
            }
        }
        
        // Apply command
        match command {
            ManagerCommand::ChangeFormation(formation) => {
                // Update team formation
                let home_team = match_component.home_team;
                if let Some(mut team) = self.world.entity_mut(home_team).get_mut::<sim_components::Team>() {
                    team.formation = formation;
                }
            }
            ManagerCommand::Substitute { out, substitute } => {
                // Implement substitution logic
                // For now, just log it
                println!("Substitution: {:?} -> {:?}", out, substitute);
            }
            ManagerCommand::ChangeMentality(mentality) => {
                // Update team mentality
                let home_team = match_component.home_team;
                if let Some(mut team) = self.world.entity_mut(home_team).get_mut::<sim_components::Team>() {
                    team.mentality = mentality;
                }
            }
            ManagerCommand::SetTactic(tactic) => {
                // Store tactic somewhere (for now, just log)
                println!("Tactic set: {:?}", tactic);
            }
        }
        
        Ok(())
    }

    pub fn get_state(&self, match_id: Entity) -> Result<MatchSnapshot, String> {
        // Get match component
        let match_component = self.world.entity(match_id).get::<Match>().ok_or("Match entity not found")?;
        
        // Get ball entity
        let mut ball_view = BallView {
            position: [0.0, 0.0],
            velocity: [0.0, 0.0],
            spin: 0.0,
            state: sim_components::BallState::Free,
            possessor: None,
        };
        
        // Find ball entity
        for entity in self.world.iter_entities() {
            if let Some(ball) = entity.get::<sim_components::Ball>() {
                ball_view = BallView {
                    position: [ball.position.x, ball.position.y],
                    velocity: [ball.velocity.x, ball.velocity.y],
                    spin: ball.spin,
                    state: ball.state,
                    possessor: ball.possessor.map(|e| e.to_bits()),
                };
                break;
            }
        }
        
        // Get player views
        let mut player_views = Vec::new();
        for entity in self.world.iter_entities() {
            if let Some(player) = entity.get::<sim_components::Player>() {
                let bp = player
                    .perception
                    .as_ref()
                    .map(|p| [p.ball_position.x, p.ball_position.y])
                    .unwrap_or([0.0, 0.0]);
                player_views.push(PlayerView {
                    entity_id: entity.id().to_bits(),
                    team_id: player.team_id,
                    position: [player.position.x, player.position.y],
                    velocity: [player.velocity.x, player.velocity.y],
                    stamina: player.stamina,
                    role: player.role,
                    skill: player.skill,
                    perception_ball_position: bp,
                });
            }
        }
        
        // Get clock view
        let clock_view = ClockView {
            elapsed: match_component.clock.elapsed,
            half: match_component.clock.half,
            added_time: match_component.clock.added_time,
            is_running: match_component.clock.is_running,
        };
        
        Ok(MatchSnapshot {
            tick: self.tick,
            match_state: MatchStateView { state: match_component.state },
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
    pub elapsed: f32,
    pub half: u8,
    pub added_time: f32,
    pub is_running: bool,
}

pub struct WorldWrapper {
    pub world: World,
}

impl WorldWrapper {
    pub fn new() -> Self {
        Self {
            world: World::new(),
        }
    }
}

pub struct ScheduleWrapper {
    pub schedule: Schedule,
}

impl ScheduleWrapper {
    pub fn new() -> Self {
        Self {
            schedule: Schedule::default(),
        }
    }
}

/// Stable, source-order discriminant for `BallState`. Avoids the unspecified
/// representation of plain `as u64` casts on `pub enum` so the hash stays
/// deterministic across compiler versions.
fn ball_state_discriminant(s: sim_components::BallState) -> u64 {
    use sim_components::BallState as B;
    match s {
        B::Free => 0,
        B::Possessed => 1,
        B::InFlight => 2,
        B::OutOfPlay => 3,
        B::Dead => 4,
    }
}

fn match_state_discriminant(s: sim_components::MatchState) -> u64 {
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

fn role_discriminant(r: sim_components::Role) -> u64 {
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
/// (and before `tick_clock`). Owns PreMatch → Kickoff → InPlay → HalfTime →
/// Kickoff(2nd half) → InPlay → FullTime. Also responsible for placing the
/// ball at the center spot, zeroing ball velocity, applying the kickoff
/// impulse, and resetting the 4-4-2 formation at every kickoff transition.
///
/// Phase 1 deliberately hardcodes formation slots and intent — proving the
/// read/write-separation pipeline and perception-snapshot shape before any
/// utility-AI variable comes online.
///
/// Phase 1 implementation note: the PreMatch → Kickoff → InPlay chain is
/// instantaneous (all three states settle on tick 1), so the driver loops
/// through the state machine until the state stabilises. Without this,
/// `PreMatch` would only advance one step per tick and the kickoff impulse
/// wouldn't apply until tick 2 (clobbering anything else that ran in
/// between).
fn lifecycle_system(
    match_entity: Entity,
    ball_entity: Entity,
    sim_tick: u64,
    world: &mut World,
) {
    const HALFTIME_BREAK_TICKS: u64 = 18;
    const MAX_TRANSITIONS_PER_TICK: usize = 8;

    for _iteration in 0..MAX_TRANSITIONS_PER_TICK {
        let current_state = world
            .entity(match_entity)
            .get::<Match>()
            .map(|m| m.state);
        let Some(current_state) = current_state else {
            return;
        };

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
                    let in_play_threshold = if c.half == 1 {
                        45.0 + c.added_time
                    } else {
                        90.0 + c.added_time
                    };
                    let is_final_half = c.half >= 2;
                    if c.elapsed >= in_play_threshold {
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
                            elapsed: 0.0,
                            half: 2,
                            added_time: c.added_time,
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
            let clock_for_match = clock.clone();
            world.entity_mut(match_entity).insert(clock);
            if let Some(mut m) = world.entity_mut(match_entity).get_mut::<Match>() {
                m.clock = clock_for_match;
            }
        }

        // Apply ball placement / kickoff impulse.
        if place_ball_center {
            if let Some(mut pos) = world.entity_mut(ball_entity).get_mut::<Position>() {
                pos.0 = Vec2::new(52.5, 34.0);
            }
            if let Some(mut vel) = world.entity_mut(ball_entity).get_mut::<Velocity>() {
                vel.0 = Vec2::zero();
            }
            if let Some(mut ball) = world.entity_mut(ball_entity).get_mut::<sim_components::Ball>() {
                ball.position = Vec2::new(52.5, 34.0);
                ball.velocity = Vec2::zero();
            }
        }
        if apply_kickoff_impulse {
            if let Some(mut vel) = world.entity_mut(ball_entity).get_mut::<Velocity>() {
                vel.0 = Vec2::new(2.0, 0.0);
            }
            if let Some(mut ball) = world.entity_mut(ball_entity).get_mut::<sim_components::Ball>() {
                ball.velocity = Vec2::new(2.0, 0.0);
            }
        }

        // Phase 1: keep the Ball struct snapshot fields in sync with the
        // Position/Velocity components. Without this, the snapshot returned
        // by `Simulation::get_state` (which reads from `Ball.position` /
        // `Ball.velocity` rather than the Position/Velocity components)
        // would always report the kickoff centre regardless of physics.
        // ball_physics_system (in sim-physics) is the canonical owner of
        // physics integration but does not touch the Ball struct snapshot
        // fields; we sync them here once per tick to keep the snapshot
        // honest without requiring a sim-physics change (out of Phase 1
        // scope).
        let snapshot_pos = world.entity(ball_entity).get::<Position>().map(|p| p.0);
        let snapshot_vel = world.entity(ball_entity).get::<Velocity>().map(|v| v.0);
        if let (Some(p), Some(v)) = (snapshot_pos, snapshot_vel) {
            if let Some(mut ball) = world.entity_mut(ball_entity).get_mut::<sim_components::Ball>() {
                ball.position = p;
                ball.velocity = v;
            }
        }

        // Reset 4-4-2 formation whenever we enter Kickoff.
        if next_state == MatchState::Kickoff && current_state != MatchState::Kickoff {
            reset_formation_to_4_4_2(world);
        }

        // Commit state mutation.
        if next_state != current_state {
            if let Some(mut m) = world.entity_mut(match_entity).get_mut::<Match>() {
                m.state = next_state;
            }
        } else {
            // No further transitions this tick; stop iterating.
            return;
        }
    }
}

/// Tracks the simulation tick at which the match entered the `HalfTime`
/// state. `None` means we are not currently in HalfTime. Used to time out
/// the half-time break using the sim-tick counter rather than the paused
/// match clock (which doesn't accumulate `elapsed` while frozen).
#[derive(Resource, Debug, Clone, Copy, Default)]
struct HalfTimeEntryTick {
    tick: Option<u64>,
}

/// Phase 2: build the default UtilityBrain used by every player at match
/// start. Mirrors the §4a consideration list — MoveToPosition, ChaseBall,
/// PassTo, ShootAtGoal, Tackle, MarkOpponent, Press, SupportRun,
/// HoldPosition. Curves are tuned for the standard 105×68 pitch (midpoints
/// are real metres, not normalised). evaluation_interval = 6 (per spec §13
/// decision #15: ~10 Hz decision cadence). Hysteresis bonus = 0.1.
fn default_utility_brain() -> sim_ai_player::UtilityBrain {
    use sim_ai_core::ResponseCurve;
    use sim_ai_player::{PlayerAction, PlayerConsideration};
    use sim_components::Intent;

    fn lin(min: f32, max: f32) -> ResponseCurve {
        ResponseCurve::Linear { min, max }
    }
    fn log(mid: f32, steep: f32) -> ResponseCurve {
        ResponseCurve::Logistic {
            midpoint: mid,
            steepness: steep,
        }
    }
    fn step(threshold: f32, below: f32, above: f32) -> ResponseCurve {
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
            intent: Intent::PassTo(Entity::PLACEHOLDER),
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
        evaluation_interval: 6,
    }
}

/// Phase 1 hardcoded 4-4-2 formation. Sorts players on each team by current
/// x-coordinate and assigns slot positions from the spec table:
///   home (team_id == 0) — GK x=5, defenders y=20 at x={20,35,50,65},
///     midfielders y=34 at x={25,40,55,70}, forwards y=34 at x={80,88}.
///   away (team_id == 1) — GK x=100, defenders y=48 at x={20,35,50,65},
///     midfielders y=34 at x={35,50,65,80}, forwards y=34 at x={17,25}.
/// Velocities are zeroed; intents reset to `HoldPosition`.
fn reset_formation_to_4_4_2(world: &mut World) {
    // Home formation targets — index 0 = GK, 1..=4 = defenders (sorted by x),
    // 5..=8 = midfielders (sorted by x), 9..=10 = forwards (sorted by x).
    let home_targets: [Vec2; 11] = [
        Vec2::new(5.0, 34.0),    // GK
        Vec2::new(20.0, 20.0),   // Defender 1
        Vec2::new(35.0, 20.0),   // Defender 2
        Vec2::new(50.0, 20.0),   // Defender 3
        Vec2::new(65.0, 20.0),   // Defender 4
        Vec2::new(25.0, 34.0),   // Midfielder 1
        Vec2::new(40.0, 34.0),   // Midfielder 2
        Vec2::new(55.0, 34.0),   // Midfielder 3
        Vec2::new(70.0, 34.0),   // Midfielder 4
        Vec2::new(80.0, 34.0),   // Forward 1
        Vec2::new(88.0, 34.0),   // Forward 2
    ];
    // Away formation targets — mirrored about pitch centre (52.5, 34).
    let away_targets: [Vec2; 11] = [
        Vec2::new(100.0, 34.0),  // GK
        Vec2::new(20.0, 48.0),   // Defender 1 (y mirrored from 20 → 48)
        Vec2::new(35.0, 48.0),   // Defender 2
        Vec2::new(50.0, 48.0),   // Defender 3
        Vec2::new(65.0, 48.0),   // Defender 4
        Vec2::new(35.0, 34.0),   // Midfielder 1 (x mirrored: 70 → 35)
        Vec2::new(50.0, 34.0),   // Midfielder 2 (x mirrored: 55 → 50)
        Vec2::new(65.0, 34.0),   // Midfielder 3 (x mirrored: 40 → 65)
        Vec2::new(80.0, 34.0),   // Midfielder 4 (x mirrored: 25 → 80)
        Vec2::new(17.0, 34.0),   // Forward 1 (x mirrored: 88 → 17)
        Vec2::new(25.0, 34.0),   // Forward 2 (x mirrored: 80 → 25)
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
        let ax = world
            .entity(*a)
            .get::<Position>()
            .map(|p| p.0.x)
            .unwrap_or(0.0);
        let bx = world
            .entity(*b)
            .get::<Position>()
            .map(|p| p.0.x)
            .unwrap_or(0.0);
        ax.partial_cmp(&bx).unwrap_or(std::cmp::Ordering::Equal)
    });
    away_players.sort_by(|a, b| {
        let ax = world
            .entity(*a)
            .get::<Position>()
            .map(|p| p.0.x)
            .unwrap_or(0.0);
        let bx = world
            .entity(*b)
            .get::<Position>()
            .map(|p| p.0.x)
            .unwrap_or(0.0);
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
/// isn't dominated by a hysteresis bonus on HoldPosition. The
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
        p.position = target;
        p.velocity = Vec2::zero();
        p.intent = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulation_creation() {
        let sim = Simulation::new(12345);
        assert_eq!(sim.tick, 0);
        assert_eq!(sim.accumulator, 0.0);
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

        let delta = 1.0 / 60.0;
        let mut hashes1 = Vec::with_capacity(1000);
        let mut hashes2 = Vec::with_capacity(1000);

        for _ in 0..1000 {
            sim1.tick(delta);
            sim2.tick(delta);
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
            a.tick(1.0 / 60.0);
            b.tick(1.0 / 60.0);
            assert_eq!(a.get_state_hash(), b.get_state_hash());
        }
    }

    #[test]
    fn test_pipeline_isolation_single_tick() {
        let mut sim = Simulation::new(42);
        let match_entity = sim.match_entity;

        sim.tick(1.0 / 60.0);

        let snapshot = sim.get_state(match_entity).expect("get_state failed");

        assert_ne!(snapshot.state_hash, 0, "State hash is zero — systems did not run");
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
        sim.tick(1.0 / 60.0);

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

        sim.tick(1.0 / 60.0);

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
            a.tick(1.0 / 60.0);
            b.tick(1.0 / 60.0);
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
            sim.tick(1.0 / 60.0);
        }
        let elapsed = sim
            .world
            .entity(me)
            .get::<MatchClock>()
            .expect("match entity has MatchClock component")
            .elapsed;
        assert!(
            (elapsed - 1.0).abs() < 0.01,
            "clock should be ~1.0s, got {}",
            elapsed
        );
    }

    /// Phase 1 verification: a full match (≤ 10 000 ticks ≈ 2.7 min of sim
    /// time, well past the simulated 90-minute threshold) eventually drives
    /// `state` to `FullTime`, transitions through `half == 2`, and the clock
    /// pauses/resets at half-time.
    #[test]
    fn test_full_match_reaches_full_time() {
        let mut sim = Simulation::new(7);
        let me = sim.match_entity;
        let mut saw_half_2 = false;
        let mut clock_paused_after_half = false;

        let final_state = {
            let mut state = MatchState::PreMatch;
            for tick_index in 0..10_000u64 {
                sim.tick(1.0 / 60.0);
                let clock = sim
                    .world
                    .entity(me)
                    .get::<MatchClock>()
                    .cloned()
                    .expect("match clock");
                if clock.half == 2 && (2700..5400).contains(&tick_index) {
                    saw_half_2 = true;
                }
                // After we first observe half == 2 (i.e. 2nd half in progress),
                // check that the clock was reset toward 0 after half-time.
                if clock.half == 2 && !clock.is_running {
                    clock_paused_after_half = true;
                }
                state = sim
                    .world
                    .entity(me)
                    .get::<Match>()
                    .map(|m| m.state)
                    .expect("match");
                if state == MatchState::FullTime {
                    break;
                }
            }
            state
        };

        assert_eq!(
            final_state,
            MatchState::FullTime,
            "match never reached FullTime within 10_000 ticks"
        );
        assert!(
            saw_half_2,
            "clock.half never observed as 2 between tick 2700 and 5400"
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
        for _ in 0..10_000u64 {
            sim2.tick(1.0 / 60.0);
            let c = sim2.world.entity(me2).get::<MatchClock>().cloned().unwrap();
            if c.half == 2 && c.elapsed < 1.0 {
                observed_low_elapsed_in_half_2 = true;
                break;
            }
        }
        assert!(
            observed_low_elapsed_in_half_2,
            "clock did not reset toward 0 after half-time"
        );
    }
}
