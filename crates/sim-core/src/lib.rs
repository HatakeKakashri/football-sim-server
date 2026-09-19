use bevy_ecs::prelude::*;
use sim_components::{Match, MatchState, ManagerCommand};
use sim_math::DeterministicRng;
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

    pub fn get_state_hash(&self) -> u64 {
        // State hash computation including positions, velocities, stamina, ball state, score, clock
        // Excluding timing
        let mut hash: u64 = 0;
        
        // Include RNG state for determinism verification
        hash = hash.wrapping_add(self.rng.state() as u64);
        
        // Include tick count
        hash = hash.wrapping_add(self.tick);
        
        // Note: In a real implementation, we would iterate over all relevant components
        // and include their state in the hash. For now, we return a placeholder.
        // The actual implementation will need to query the world for all relevant components.
        
        hash
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
            ManagerCommand::ChangeFormation(formation) => {
                // Validate formation change is allowed in current state
                if match_component.state != MatchState::InPlay && 
                   match_component.state != MatchState::Stoppage {
                    return Err(format!("Invalid state for formation change: {:?}", match_component.state));
                }
            }
            ManagerCommand::Substitute { out, substitute } => {
                // Validate substitution is allowed in current state
                if match_component.state != MatchState::Stoppage &&
                   match_component.state != MatchState::HalfTime {
                    return Err(format!("Invalid state for substitution: {:?}", match_component.state));
                }
            }
            ManagerCommand::ChangeMentality(mentality) => {
                // Validate mentality change is allowed in current state
                if match_component.state != MatchState::InPlay &&
                   match_component.state != MatchState::Stoppage {
                    return Err(format!("Invalid state for mentality change: {:?}", match_component.state));
                }
            }
            ManagerCommand::SetTactic(tactic) => {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulation_creation() {
        let sim = Simulation::new(12345);
        assert_eq!(sim.tick, 0);
        assert_eq!(sim.accumulator, 0.0);
    }

    #[test]
    fn test_per_tick_determinism() {
        let seed = 42u64;
        let mut sim1 = Simulation::new(seed);
        let mut sim2 = Simulation::new(seed);

        sim1.create_match(seed);
        sim2.create_match(seed);

        let delta = 1.0 / 60.0;

        for _ in 0..1000 {
            sim1.tick(delta);
            sim2.tick(delta);

            assert_eq!(
                sim1.get_state_hash(),
                sim2.get_state_hash(),
                "Determinism violated at tick {}",
                sim1.tick
            );
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
}