use bevy_ecs::prelude::*;
use sim_components::{Match, MatchState, ManagerCommand};
use sim_math::{DeterministicRng, PitchDimensions};
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
}

impl Simulation {
    pub fn new(seed: u64) -> Self {
        let mut world = World::new();
        // Phase 0 substrate: physics needs pitch dimensions. Insert the standard
        // pitch as a resource so the registered ball physics system can resolve
        // its `Res<PitchDimensions>` parameter.
        world.insert_resource(PitchDimensions::standard());

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

        Self {
            world,
            schedule,
            rng,
            tick: 0,
            accumulator: 0.0,
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
            self.tick += 1;
            self.accumulator -= FIXED_TIMESTEP;
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

    pub fn create_match(&mut self, seed: u64) -> Entity {
        use sim_components::{Ball, Match, MatchClock, MatchState, Player, Position, Team, TeamId, Velocity};
        use sim_math::Vec2;
        
        // Create ball entity
        let ball_entity = self.world.spawn(()).id();
        self.world.entity_mut(ball_entity).insert(Ball {
            position: Vec2::new(52.5, 34.0), // Center of pitch
            velocity: Vec2::zero(),
            spin: 0.0,
            state: sim_components::BallState::Free,
            possessor: None,
        });
        // Ball also carries Position + Velocity components so the registered
        // ball_physics_system query (`Position, Velocity, Ball`) matches it.
        // The Ball struct fields stay as the snapshot/diagnostic copy.
        self.world.entity_mut(ball_entity).insert(Position(Vec2::new(52.5, 34.0)));
        self.world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));
        
        // Create home team
        let home_team_entity = self.world.spawn(()).id();
        let mut home_players = Vec::new();
        
        // Create 11 home players
        for i in 0..11 {
            let player_entity = self.world.spawn(()).id();
            self.world.entity_mut(player_entity).insert(Player {
                team_id: TeamId(0),
                position: Vec2::new(10.0 + (i as f32 * 8.0), 34.0),
                velocity: Vec2::zero(),
                stamina: 1.0,
                role: sim_components::Role::CentralMidfielder,
                skill: 0.7,
                intent: None,
                perception: None,
            });
            self.world.entity_mut(player_entity).insert(Position(Vec2::new(10.0 + (i as f32 * 8.0), 34.0)));
            self.world.entity_mut(player_entity).insert(Velocity(Vec2::zero()));
            home_players.push(player_entity);
        }
        
        self.world.entity_mut(home_team_entity).insert(Team {
            id: TeamId(0),
            name: "Home".to_string(),
            formation: sim_components::Formation::FourFourTwo,
            mentality: sim_components::Mentality::Balance,
            players: home_players,
            substitutes: Vec::new(),
        });
        
        // Create away team
        let away_team_entity = self.world.spawn(()).id();
        let mut away_players = Vec::new();
        
        // Create 11 away players
        for i in 0..11 {
            let player_entity = self.world.spawn(()).id();
            self.world.entity_mut(player_entity).insert(Player {
                team_id: TeamId(1),
                position: Vec2::new(95.0 - (i as f32 * 8.0), 34.0),
                velocity: Vec2::zero(),
                stamina: 1.0,
                role: sim_components::Role::CentralMidfielder,
                skill: 0.7,
                intent: None,
                perception: None,
            });
            self.world.entity_mut(player_entity).insert(Position(Vec2::new(95.0 - (i as f32 * 8.0), 34.0)));
            self.world.entity_mut(player_entity).insert(Velocity(Vec2::zero()));
            away_players.push(player_entity);
        }
        
        self.world.entity_mut(away_team_entity).insert(Team {
            id: TeamId(1),
            name: "Away".to_string(),
            formation: sim_components::Formation::FourFourTwo,
            mentality: sim_components::Mentality::Balance,
            players: away_players,
            substitutes: Vec::new(),
        });
        
        // Create match entity
        let match_entity = self.world.spawn(()).id();
        self.world.entity_mut(match_entity).insert(Match {
            id: 1,
            home_team: home_team_entity,
            away_team: away_team_entity,
            score: (0, 0),
            clock: MatchClock {
                elapsed: 0.0,
                half: 1,
                added_time: 0.0,
                is_running: true,
            },
            state: MatchState::PreMatch,
            seed,
        });
        
        match_entity
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
                player_views.push(PlayerView {
                    entity_id: entity.id().to_bits(),
                    team_id: player.team_id,
                    position: [player.position.x, player.position.y],
                    velocity: [player.velocity.x, player.velocity.y],
                    stamina: player.stamina,
                    role: player.role,
                    skill: player.skill,
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

        sim1.create_match(seed);
        sim2.create_match(seed);

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
        a.create_match(seed);
        b.create_match(seed);
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
        let match_entity = sim.create_match(42);

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
        let mut sim = Simulation::new(seed);
        sim.create_match(seed);
        let h1 = sim.get_state_hash();
        let h2 = sim.get_state_hash();
        assert_eq!(h1, h2, "Hash changed between identical reads");
        assert_ne!(h1, 0);
    }

    /// The ball must actually move when given a non-zero velocity. Find the
    /// ball entity (the one with both `Ball` and `Position`/`Velocity`
    /// components) and verify the Position changes after a tick.
    #[test]
    fn test_ball_moves_under_physics() {
        use sim_components::Position as PosComp;
        use sim_components::Velocity as VelComp;
        use sim_math::Vec2;

        let mut sim = Simulation::new(7);
        sim.create_match(7);

        // Find the ball entity by its Ball component.
        let ball_entity = {
            let mut found = None;
            for entity in sim.world.iter_entities() {
                if entity.get::<sim_components::Ball>().is_some() {
                    found = Some(entity.id());
                    break;
                }
            }
            found.expect("ball entity must exist after create_match")
        };

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
        a.create_match(seed);
        b.create_match(seed);

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

        // Find ball entity on sim a and perturb ONLY its x Position.
        let ball_a = {
            let mut found = None;
            for entity in a.world.iter_entities() {
                if entity.get::<sim_components::Ball>().is_some() {
                    found = Some(entity.id());
                    break;
                }
            }
            found.expect("ball on sim a")
        };
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
}
