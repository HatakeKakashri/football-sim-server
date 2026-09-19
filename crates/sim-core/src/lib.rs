use bevy_ecs::prelude::*;
use sim_components::{Match, MatchClock, MatchState};
use sim_math::{DeterministicRng, Vec2};

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
}