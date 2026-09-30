use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use sim_math::Vec2;

pub mod intent_dispatch;
pub mod time;

#[derive(Component, Debug, Clone)]
pub struct PerceptionSnapshot {
    pub self_position: Vec2,
    pub nearby_teammates: smallvec::SmallVec<[NearbyEntity; 8]>,
    pub nearby_opponents: smallvec::SmallVec<[NearbyEntity; 8]>,
    pub ball_position: Vec2,
    /// Ball state as observed by the perception system. This is a
    /// `BallState` enum value (Free / Possessed / etc.), **not** an
    /// `Entity`.  The actual possessor entity lives on `Ball.possessor`
    /// and is read by systems that need the real entity reference.
    pub ball_state: BallState,
    pub goal_position: Vec2,
    pub pitch_bounds: PitchBounds,
}

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
    Dead,
}

/// Phase 3: Out-of-bounds restart types (Law 9)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BallOutOfBoundsType {
    ThrowIn,
    GoalKick,
    Corner,
}

/// Phase 3: Foul types (Law 12)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FoulType {
    DangerousPlay,
}

/// Phase 3: Rule events emitted by the rules engine.
/// Used for event streaming, replay recording, and CLI output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Component)]
pub enum RuleEvent {
    OutOfBounds(BallOutOfBoundsType),
    Goal {
        scorer_team: TeamId,
        score: (u8, u8),
    },
    KickoffRestart,
    HalfTimeStart,
    FullTimeStart,
    Foul {
        fouler: u64,
        foulee: u64,
        foul_type: FoulType,
    },
}

/// Top-level intent tag.
///
/// An `Intent` is either a movement (which steers the player towards a
/// target point or formation slot) or an action (which commits the
/// player to a discrete tactical action like passing, tackling, or
/// marking).
///
/// The split lets `player_action_execution_system` and other dispatch
/// sites match on the coarse kind (`IntentKind`) when they don't care
/// about the specific variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Intent {
    Movement(MovementIntent),
    Action(ActionIntent),
}

/// Movement-style intents: move the player somewhere or settle into a
/// formation slot. These never commit to a discrete ball-affecting action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MovementIntent {
    MoveToPosition(Vec2),
    HoldPosition,
    ChaseBall,
    Intercept,
    SupportRun,
    TrackBack,
}

/// Action-style intents: commit the player to a tactical action. Most of
/// these will resolve into a kick, tackle, or marking assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionIntent {
    PassTo,
    ShootAtGoal(Vec2),
    Tackle(Entity),
    MarkOpponent(Entity),
    Press(Entity),
}

impl Intent {
    /// Coarse two-way tag: does this intent move the player, or commit to
    /// an action? Useful for sites that don't care about the specific
    /// variant.
    #[must_use]
    pub const fn kind(&self) -> IntentKind {
        match *self {
            Self::Movement(_) => IntentKind::Movement,
            Self::Action(_) => IntentKind::Action,
        }
    }

    /// `true` when this intent is a `Movement(_)` variant.
    #[must_use]
    pub const fn is_movement(&self) -> bool {
        matches!(self, Self::Movement(_))
    }

    /// `true` when this intent is an `Action(_)` variant.
    #[must_use]
    pub const fn is_action(&self) -> bool {
        matches!(self, Self::Action(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntentKind {
    Movement,
    Action,
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
pub struct TeamIdComponent(pub TeamId);

#[derive(Component, Debug, Clone)]
pub struct RoleComponent(pub Role);

/// Simulation-time clock for the active match.
///
/// Spec axis: per spec §3, `MatchClock` is a Resource (singleton). It is
/// also derived as a Component for backwards compatibility with tests and
/// any external code that builds a `World` by hand; the Resource form is
/// the source of truth for systems (lifecycle, perception, decision).
///
/// Phase F follow-up: the Component form will be removed in a later PR
/// once all consumers migrate. Until then, `sim-core::create_match`
/// inserts both forms and keeps them in sync.
#[derive(Component, Resource, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchClock {
    /// Elapsed time in simulation ticks (60 ticks = 1 second).
    /// Only advances when `is_running == true`.
    pub elapsed_ticks: u64,
    pub half: u8,
    /// Added time in simulation ticks.
    pub added_time_ticks: u64,
    pub is_running: bool,
}

impl MatchClock {
    /// Returns elapsed time in seconds for display purposes.
    #[must_use]
    pub fn elapsed_secs(&self) -> f32 {
        time::ticks_to_secs_f32(self.elapsed_ticks)
    }

    /// Returns added time in seconds for display purposes.
    #[must_use]
    pub fn added_time_secs(&self) -> f32 {
        time::ticks_to_secs_f32(self.added_time_ticks)
    }
}

/// `Ball` is the singleton Resource holding all dynamic ball state.
///
/// Phase C §4.3 (ECS-shape polish, full): `Ball` is now a Resource, not
/// a Component on an entity. There is exactly one ball per simulation,
/// so the singleton-resource pattern removes the linear-scan ball
/// lookups. The ball's `Position` and `Velocity` still live as Components
/// on the dedicated ball entity — `Simulation::ball_entity` exposes the
/// entity id, and systems that need both the ball state and its
/// position read `Res<Ball>` for state and `world.entity_mut(ball_entity)`
/// for position. This avoids the dual-writer problem (`Ball` is now the
/// only writer for state/possessor/last-touched-by/kick_velocity/spin,
/// while `Position`/`Velocity` are written exclusively by the physics
/// systems on the entity).
#[allow(
    clippy::too_long_first_doc_paragraph,
    reason = "Phase C history block; splitting it would scatter load-bearing context"
)]
#[derive(Resource, Debug, Clone)]
pub struct Ball {
    pub spin: f32,
    pub state: BallState,
    pub possessor: Option<Entity>,
    /// The entity of the player nearest to the ball (within 1.0 m).
    /// This is a **proximity predicate**, not a contact event — any player
    /// within 1.0 m is considered to have "touched" the ball.  Used for
    /// offside Law 11 enforcement: an offside offence is judged relative to
    /// the position of the second-to-last defender at the instant this
    /// player was the nearest to the ball.
    pub last_touched_by: Option<Entity>,
    /// Pending velocity from a kick, to be applied in the Physics set.
    /// This avoids static query conflicts between `kick_execution_system`
    /// (Execution set) and `ball_physics_system` (Physics set).
    pub kick_velocity: Option<Vec2>,
}

/// Tag component on the ball entity so systems can find it via Query
/// without scanning `iter_entities()`.
///
/// Phase C §4.3: there is exactly one entity carrying `BallMarker` per
/// match; its `Position`/`Velocity` components are written by
/// `ball_physics_system`, while `Ball` (the Resource) holds
/// `state/possessor/last_touched_by/kick_velocity/spin`.
#[allow(
    clippy::too_long_first_doc_paragraph,
    reason = "Phase C history block; splitting it would scatter load-bearing context"
)]
#[derive(Component, Debug, Clone, Copy)]
pub struct BallMarker;

#[derive(Component, Debug, Clone)]
pub struct Player {
    pub team_id: TeamId,
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

/// `Match` is the singleton Resource holding all match-level state.
///
/// Phase C ECS-shape polish (review §4.3): `Match` is now a Resource
/// rather than a Component. There is exactly one match per simulation,
/// and the singleton-resource pattern removes the linear-scan lookups in
/// `apply_command` (§2.9). `Match` is no longer a
/// `Component` — anywhere a system needs match state, take
/// `Res<Match>` / `ResMut<Match>`. The match-entity slot on
/// `Simulation::match_entity` is retained for compatibility with consumers
/// that still need the entity (e.g. lifecycle commands), but no system
/// reads match state via `Query<&Match>` after this change.
#[allow(
    clippy::too_long_first_doc_paragraph,
    reason = "Phase C history block; splitting it would scatter load-bearing context"
)]
#[derive(Resource, Debug, Clone)]
pub struct Match {
    pub id: u64,
    pub home_team: Entity,
    pub away_team: Entity,
    pub score: (u8, u8),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CardColor {
    Yellow,
    Red,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ManagerCommand {
    ChangeFormation(Formation),
    Substitute { out: u64, substitute: u64 },
    ChangeMentality(Mentality),
}

/// Error returned by `sim_core::Simulation::apply_command` and the
/// `sim_server` wrappers.
///
/// `sim_components` owns the type because the same variants are
/// meaningful at both the simulation and the network boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// The command is not legal in the match's current state
    /// (e.g. formation change during `PreMatch`).
    InvalidForState {
        current_state: crate::MatchState,
        required_state: crate::MatchState,
    },
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests compare freshly-constructed values against the literals they were built from; equality is exact"
)]
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
