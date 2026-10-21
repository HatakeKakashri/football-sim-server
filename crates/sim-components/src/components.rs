//! Core ECS components: per-entity state used by simulation systems.

use bevy_ecs::prelude::*;
use sim_math::Vec2;

use super::{TeamId, enums::StrengthMode};

/// Player component: team membership and current intent.
#[derive(Component, Debug, Clone)]
pub struct Player {
    pub team_id: TeamId,
    pub intent: Option<super::Intent>,
}

/// Team component: per-side roster, mentality, tactic, formation.
#[derive(Component, Debug, Clone)]
pub struct Team {
    pub id: TeamId,
    pub name: String,
    pub formation: super::Formation,
    pub mentality: super::Mentality,
    pub tactic: super::Tactic,
    pub players: Vec<Entity>,
    pub substitutes: Vec<Entity>,
    /// Per-half attack direction.
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

/// Fixture identity of a player entity (PRD §5.4 roster).
///
/// PRD §9.1: *"Players are identified by stable fixture `player_id`,
/// never by engine entity ids."* The component carries that identity
/// on the entity, so a consumer (the API's manifest and the frames it
/// publishes) can name a player without inventing an id from the spawn
/// order.
///
/// It is inserted when the match is built from fixture data
/// (`sim_core::Simulation::new_from_fixture`); a world built from
/// placeholders has no fixture players to name.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct PlayerIdentity {
    /// The fixture's `player_id` (e.g. `"p-alpha-01"`).
    pub id: String,
    /// The player's index in its side's fixture roster (0–10).
    pub roster_index: u8,
}

impl PlayerIdentity {
    /// Build the identity of a fixture player.
    ///
    /// `roster_index` is the player's index in its side's fixture roster
    /// (0–10); the fixture's `player_id` is the id itself.
    ///
    /// # Panics
    ///
    /// Panics if `roster_index` does not fit in a `u8`, which cannot
    /// happen for a validated squad of eleven players.
    #[must_use]
    pub fn from_fixture_player(
        player: &crate::fixture::ValidatedPlayer,
        roster_index: usize,
    ) -> Self {
        Self {
            id: player.id.clone(),
            roster_index: u8::try_from(roster_index).expect("invariant: a squad has 11 players"),
        }
    }
}

/// Tag component: maps an entity to a team id.
#[derive(Component, Debug, Clone)]
pub struct TeamIdComponent(pub TeamId);

/// Tag component: maps an entity to a role.
#[derive(Component, Debug, Clone, Copy)]
pub struct RoleComponent(pub super::Role);

/// Position component: world coordinates.
#[derive(Component, Debug, Clone)]
pub struct Position(pub Vec2);

/// Velocity component: world velocity in metres per tick.
#[derive(Component, Debug, Clone)]
pub struct Velocity(pub Vec2);

/// Stamina component: 0.0 = exhausted, 1.0 = full.
#[derive(Component, Debug, Clone)]
pub struct Stamina(pub f32);

/// Skill component: overall skill rating.
#[derive(Component, Debug, Clone)]
pub struct Skill(pub f32);

/// Tackle recovery component: a tackler is out of position for this many
/// ticks after a *failed* tackle (PRD §7.3).
#[derive(Component, Debug, Clone, Copy)]
pub struct TackleRecovery {
    /// Ticks remaining in the recovery window.
    pub ticks_remaining: u64,
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
        formation: super::Formation,
        mentality: super::Mentality,
        tactic: super::Tactic,
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
