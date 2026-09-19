use bevy_ecs::prelude::*;
use sim_math::Vec2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TeamId(pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mentality {
    Attack,
    Balance,
    Defend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Formation {
    FourFourTwo,
    FourThreeThree,
    ThreeFiveTwo,
    FourTwoThreeOne,
    FiveThreeTwo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MatchState {
    PreMatch,
    Kickoff,
    InPlay,
    Stoppage,
    HalfTime,
    FullTime,
    PenaltyShootout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BallState {
    Free,
    Possessed,
    InFlight,
    OutOfPlay,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Intent {
    MoveTo(Vec2),
    PassTo(Entity),
    Shoot(Vec2),
    Tackle(Entity),
    Intercept,
    Press,
    HoldPosition,
    SupportAttack,
    TrackBack,
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

#[derive(Component, Debug, Clone)]
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
    pub last_decision_tick: u64,
    pub decision_cooldown: u64,
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