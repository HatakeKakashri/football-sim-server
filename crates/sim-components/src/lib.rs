use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use sim_math::Vec2;

pub mod attribute;
pub mod attribute_groups;
pub mod ball_touch;
pub mod event_kinds;
pub mod fixture;
pub mod formation_slot;
pub mod intent_dispatch;
pub mod outfield_mechanics;
pub mod physical_mechanics;
pub mod position;
pub mod profile;
pub mod quality;
pub mod shot_pipeline;
pub mod time;

#[cfg(test)]
mod attribute_tests;

pub use attribute::Attribute;
pub use attribute_groups::{Goalkeeping, OutfieldAttack, OutfieldDefence, PhysicalMental};
pub use profile::{
    AttributeError, PlayerProfile, ProfileKind, ATTRIBUTE_COUNT, ATTRIBUTE_MAX, ATTRIBUTE_MIN,
};
pub use position::{PosSlot, PositionError};
pub use quality::quality;
pub use shot_pipeline::{
    finishing_conversion, keeper_positioning_error, keeper_save_chance, resolve_shot_outcome,
    rush_out_quality, save_kind_from_speed, save_probability, shot_accuracy, shot_power,
    SAVE_CATCH_MAX_SPEED_MPS, ShotResolution,
};
pub use outfield_mechanics::{
    mark_distance_error, pass_error_spread, target_position_error, tackle_success_probability,
};
pub use physical_mechanics::{
    aggression_stamina_factor, fitness_drain_factor, held_ball_retention_probability, sprint_speed,
};
pub use sim_math::{effective_attribute, fit_multiplier, QualityGroup, QualityWeights};

pub use ball_touch::{
    BallOutOfBoundsType, BallState, FoulType, LastTouch, PendingBallTouches, PlayerId, RuleEvent,
    TouchKind,
};
pub use event_kinds::{KickoffReason, SaveKind, ShotOutcome};
pub use formation_slot::{FormationSlot, HomeSlot};

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
    pub home_slot: Vec2,
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

impl Mentality {
    /// The serialised name used in match fixtures (PRD §5.4).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Attack => "attack",
            Self::Balance => "balance",
            Self::Defend => "defend",
        }
    }

    /// Look up a mentality by its serialised name. Returns `None` for
    /// unknown names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "attack" => Some(Self::Attack),
            "balance" => Some(Self::Balance),
            "defend" => Some(Self::Defend),
            _ => None,
        }
    }
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
pub enum Tactic {
    Possession,
    CounterAttack,
    HighPress,
    ParkTheBus,
}

impl Tactic {
    /// The serialised name used in match fixtures (PRD §5.4).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Possession => "possession",
            Self::CounterAttack => "counter_attack",
            Self::HighPress => "high_press",
            Self::ParkTheBus => "park_the_bus",
        }
    }

    /// Look up a tactic by its serialised name. Returns `None` for
    /// unknown names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "possession" => Some(Self::Possession),
            "counter_attack" => Some(Self::CounterAttack),
            "high_press" => Some(Self::HighPress),
            "park_the_bus" => Some(Self::ParkTheBus),
            _ => None,
        }
    }
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

impl MovementIntent {
    /// Resolve the destination of `MoveToPosition(target)`:
    /// `Vec2::zero()` is the "go to your home slot" sentinel and maps
    /// to `perception.home_slot`; every other target is passed through.
    ///
    /// Brain templates encode "go to your home slot" as `Vec2::zero()`
    /// so the intent can be written without a perception snapshot in
    /// scope; every consumer (steer target, decision scoring, distance
    /// consideration) resolves the sentinel through this single helper.
    /// See `default_utility_brain` for the only authoring site.
    #[must_use]
    pub fn resolve_target(target: Vec2, perception: &PerceptionSnapshot) -> Vec2 {
        if target == Vec2::zero() {
            perception.home_slot
        } else {
            target
        }
    }
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

/// A tackler is out of position for this many ticks after a *failed*
/// tackle (PRD §7.3).
///
/// Initial value: `tackle_recovery_ticks` (90) from
/// [`sim_math::Tuning`]; the recovery system decrements it each tick
/// and removes the component when it reaches zero. While present, the
/// tackler moves slower and cannot re-engage, so the carrier skips
/// past.
#[derive(Component, Debug, Clone, Copy)]
pub struct TackleRecovery {
    /// Ticks remaining in the recovery window.
    pub ticks_remaining: u64,
}

#[derive(Component, Debug, Clone)]
pub struct TeamIdComponent(pub TeamId);

#[derive(Component, Debug, Clone, Copy)]
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
/// only writer for `state/possessor/last_touch/kick_velocity/spin`,
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
    /// Last registered touch of the ball — pushed to the buffer by
    /// `kick_execution_system` (kick), `possession_resolution_system`
    /// (proximity interception), `restart_system` (throw-in / corner /
    /// goal-kick placement), and `lifecycle_system` (kickoff placement).
    /// Those are the only four write paths; no system assigns this field
    /// directly.
    ///
    /// `apply_ball_touched_system` drains the buffer onto this field from
    /// two sites: first in the `Rules` set (so the rules writers read the
    /// same-tick touch), and again in `Simulation::tick` after the
    /// post-schedule `lifecycle_system` (which runs outside the
    /// schedule and so needs its own drain). See
    /// `PendingBallTouches` for the full contract.
    ///
    /// The record is preserved across OOB and Goal events (M8 stopped
    /// the clears) so downstream replay data can re-read it. Its
    /// identity fields (`team`, `player_id`) are denormalised, so it
    /// stays correct after the touching entity despawns.
    pub last_touch: Option<LastTouch>,
    /// Two-deep sliding window of recent touches (newest first).
    ///
    /// PRD §8.5 calls for a two-deep touch history on `Ball` so the
    /// goal event can attribute `scorer` (current `last_touch`) and
    /// `assister` (previous touch by the scoring team within the last
    /// 10 s of game time) from a stable, denormalised window. The
    /// window is updated whenever `Ball.last_touch` is set:
    /// `[0]` is the previous touch, `[1]` is the one before that.
    /// Index 0 is the immediately preceding touch; index 1 is the
    /// touch before that. Empty slots stay `None`.
    ///
    /// The history is mixed into `Simulation::get_state_hash` so that
    /// any change to the touch path (writer swap, new touch kind,
    /// extra push) changes the hash deterministically — the seam
    /// future Phase 4 work needs.
    pub touch_history: [Option<LastTouch>; 2],
    /// Pending velocity from a kick, to be applied in the Physics set.
    /// This avoids static query conflicts between `kick_execution_system`
    /// (Execution set) and `ball_physics_system` (Physics set).
    pub kick_velocity: Option<Vec2>,
}

impl Ball {
    /// Push `touch` onto `Ball`: store it as `last_touch` and slide the
    /// previous `last_touch` into `touch_history[0]`, the previous
    /// `touch_history[0]` into `touch_history[1]`.
    ///
    /// The sliding-window order is part of the contract: it gives
    /// `assister` lookup a stable "previous touch by a different (or
    /// same) team" and keeps `Simulation::get_state_hash` deterministic
    /// when callers mix in only one of the three slots.
    ///
    /// This is the **only** function that should assign
    /// `Ball.last_touch`. Other call sites must go through
    /// [`Self::record_touch`].
    pub const fn record_touch(&mut self, touch: LastTouch) {
        // Slide: prev history[0] → history[1], prev last_touch → history[0].
        self.touch_history[1] = self.touch_history[0];
        self.touch_history[0] = self.last_touch;
        self.last_touch = Some(touch);
    }
}

/// Tag component on the ball entity so systems can find it via Query
/// without scanning `iter_entities()`.
///
/// Phase C §4.3: there is exactly one entity carrying `BallMarker` per
/// match; its `Position`/`Velocity` components are written by
/// `ball_physics_system`, while `Ball` (the Resource) holds
/// `state/possessor/last_touch/kick_velocity/spin`.
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
    pub tactic: Tactic,
    pub players: Vec<Entity>,
    pub substitutes: Vec<Entity>,
    /// Per-half attack direction for this team.
    ///
    /// `+1` means the team attacks toward `x = PITCH_LENGTH` (right goal)
    /// in the current half; `-1` means they attack toward `x = 0` (left goal).
    ///
    /// Convention: the home team starts with `+1` in the first half;
    /// the away team starts with `-1`. At half-time the values flip in
    /// `apply_transition` (`sim_core::simulation::lifecycle`). Field is
    /// private (§5) so the half-time-only flip invariant cannot be
    /// bypassed by other systems; readers go through
    /// `attack_direction()`, and the only writer is `apply_transition`.
    attack_direction: i8,
}

/// How the eleven slot Qualities aggregate into a team strength
/// (PRD §6.4). `Sum` returns the total; `Mean` returns the average.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrengthMode {
    #[default]
    Sum,
    Mean,
}

impl Team {
    /// Per-half attack direction. `+1` = attacking right (`PITCH_LENGTH`),
    /// `-1` = attacking left (0.0).
    #[must_use]
    pub const fn attack_direction(&self) -> i8 {
        self.attack_direction
    }

    /// Set the per-half attack direction. Only the half-time state
    /// machine (`apply_transition` in `sim_core::simulation::lifecycle`)
    /// calls this; production callers outside that function should be a
    /// smell. `pub` because the workspace spans multiple crates; the
    /// doc comment is the load-bearing guard, not the visibility.
    pub const fn set_attack_direction(&mut self, dir: i8) {
        self.attack_direction = dir;
    }

    /// Aggregate `slot_qualities` (one per slot, in formation table
    /// order) into a team strength.
    ///
    /// # Parameters
    ///
    /// - `slot_qualities` — the per-slot [`quality`] values, one per
    ///   slot in canonical formation order.
    /// - `mode` — `StrengthMode::Sum` for the total, `StrengthMode::Mean`
    ///   for the average.
    #[expect(
        clippy::cast_precision_loss,
        reason = "slot counts are tiny (<= 11), so the usize -> f32 cast is exact"
    )]
    #[must_use]
    pub fn strength(slot_qualities: &[f32], mode: StrengthMode) -> f32 {
        let total: f32 = slot_qualities.iter().sum();
        match mode {
            StrengthMode::Sum => total,
            StrengthMode::Mean => total / slot_qualities.len().max(1) as f32,
        }
    }

    /// Constructor. Migrate struct-literal sites to this method so
    /// `attack_direction` can stay private (§5) and the half-time-only
    /// flip invariant cannot be bypassed by other systems.
    #[allow(
        clippy::too_many_arguments,
        reason = "struct constructor mirrors Team's field-for-field shape; a builder would scatter the field set across crates and obscure the migration away from struct literals"
    )]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "constructor allocates a String and (via) takes Vec<Entity>; not eligible for const fn"
    )]
    #[must_use]
    pub fn with_attack_direction(
        id: TeamId,
        name: String,
        formation: Formation,
        mentality: Mentality,
        tactic: Tactic,
        players: Vec<Entity>,
        substitutes: Vec<Entity>,
        attack_direction: i8,
    ) -> Self {
        Self {
            id,
            name,
            formation,
            mentality,
            tactic,
            players,
            substitutes,
            attack_direction,
        }
    }
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
    SetTactic(Tactic),
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
    /// The command is recognized but not yet implemented.
    NotImplemented,
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
