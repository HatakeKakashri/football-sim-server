use bevy_ecs::prelude::*;
use sim_math::Vec2;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TeamId(pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    Goalkeeper,
    CenterBack,
    FullBack,
    DefensiveMidfielder,
    CentralMidfielder,
    AttackingMidfielder,
    Winger,
    Striker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mentality {
    Attack,
    Balance,
    Defend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Formation {
    FourFourTwo,
    FourThreeThree,
    ThreeFiveTwo,
    FourTwoThreeOne,
    FiveThreeTwo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MatchState {
    PreMatch,
    Kickoff,
    InPlay,
    Stoppage,
    HalfTime,
    FullTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BallState {
    Free,
    Possessed,
    InFlight,
    OutOfPlay,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Intent {
    MoveToPosition(Vec2),
    PassTo(Entity),
    ShootAtGoal(Vec2),
    Tackle(Entity),
    ChaseBall,
    MarkOpponent(Entity),
    Intercept,
    Press(Entity),
    HoldPosition,
    SupportRun,
    TrackBack,
}

#[derive(Debug, Clone)]
pub struct PerceptionSnapshot {
    pub nearby_teammates: smallvec::SmallVec<[NearbyEntity; 8]>,
    pub nearby_opponents: smallvec::SmallVec<[NearbyEntity; 8]>,
    pub ball_position: Vec2,
    pub goal_position: Vec2,
    pub pitch_bounds: PitchBounds,
}

#[derive(Debug, Clone)]
pub struct NearbyEntity {
    pub entity: Entity,
    pub distance: f32,
    pub relative_position: Vec2,
}

#[derive(Debug, Clone)]
pub struct PitchBounds {
    pub distance_to_left: f32,
    pub distance_to_right: f32,
    pub distance_to_top: f32,
    pub distance_to_bottom: f32,
}

#[derive(Component, Debug, Clone)]
pub struct Position(pub Vec2);

#[derive(Component, Debug, Clone)]
pub struct Velocity(pub Vec2);

#[derive(Component, Debug, Clone)]
pub struct Stamina(pub f32);

#[derive(Component, Debug, Clone)]
pub struct Skill(pub f32);

#[derive(Component, Debug, Clone)]
pub struct BallStateComponent(pub BallState);

#[derive(Component, Debug, Clone)]
pub struct TeamIdComponent(pub TeamId);

#[derive(Component, Debug, Clone)]
pub struct RoleComponent(pub Role);

#[derive(Component, Debug, Clone)]
pub struct MatchStateComponent(pub MatchState);

#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct MatchClock {
    pub elapsed: f32,
    pub half: u8,
    pub added_time: f32,
    pub is_running: bool,
}

#[derive(Component, Debug, Clone)]
pub struct Ball {
    pub position: Vec2,
    pub velocity: Vec2,
    pub spin: f32,
    pub state: BallState,
    pub possessor: Option<Entity>,
}

#[derive(Component, Debug, Clone)]
pub struct Player {
    pub team_id: TeamId,
    pub position: Vec2,
    pub velocity: Vec2,
    pub stamina: f32,
    pub role: Role,
    pub skill: f32,
    pub intent: Option<Intent>,
    pub perception: Option<PerceptionSnapshot>,
}

#[derive(Component, Debug, Clone)]
pub struct Team {
    pub id: TeamId,
    pub name: String,
    pub formation: Formation,
    pub mentality: Mentality,
    pub players: Vec<Entity>,
    pub substitutes: Vec<Entity>,
}

#[derive(Component, Debug, Clone)]
pub struct Match {
    pub id: u64,
    pub home_team: Entity,
    pub away_team: Entity,
    pub score: (u8, u8),
    pub clock: MatchClock,
    pub state: MatchState,
    pub seed: u64,
}

#[derive(Component, Debug, Clone)]
pub struct Manager {
    pub decision_table: WeightedDecisionTable,
    pub last_decision_tick: u64,
    pub decision_cooldown: u64,
}

#[derive(Debug, Clone)]
pub struct WeightedDecisionTable {
    pub factors: Vec<DecisionFactor>,
}

#[derive(Debug, Clone)]
pub struct DecisionFactor {
    pub name: String,
    pub weight: f32,
}

#[derive(Component, Debug, Clone)]
pub struct Referee {
    pub stoppage_events: Vec<StoppageEvent>,
    pub cards: Vec<Card>,
}

#[derive(Debug, Clone)]
pub struct StoppageEvent {
    pub tick: u64,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct Card {
    pub player: Entity,
    pub color: CardColor,
    pub tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardColor {
    Yellow,
    Red,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ManagerCommand {
    ChangeFormation(Formation),
    Substitute { out: u64, substitute: u64 },
    ChangeMentality(Mentality),
    SetTactic(Tactic),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Tactic {
    HighPress,
    CounterAttack,
    Possession,
    LongBall,
    WingPlay,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_component_creation() {
        let pos = Position(Vec2::new(10.0, 20.0));
        assert_eq!(pos.0.x, 10.0);
        assert_eq!(pos.0.y, 20.0);

        let stamina = Stamina(0.8);
        assert_eq!(stamina.0, 0.8);
    }
}