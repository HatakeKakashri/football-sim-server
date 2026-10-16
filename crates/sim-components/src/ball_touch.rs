//! Ball-touch and rule-event types.
//!
//! Split out of `lib.rs` to keep that file under the 500-LoC budget
//! (`docs/CODING_STANDARDS.md` §6). All ball-state enums and rule
//! events live here.
//!
//! Touch registration flow: writers push `LastTouch` records to the
//! `PendingBallTouches` Resource, and the consumer
//! `apply_ball_touched_system` (in `sim_rules::touch`) drains the
//! buffer at the start of the Rules set and writes the final entry to
//! `Ball.last_touch`.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use sim_math::Vec2;

use crate::TeamId;

/// Stable identity for a player, carried on records that must outlive
/// the ECS entity they were captured from.
///
/// PRD §9.1: *"Players are identified by stable fixture `player_id`,
/// never by engine entity ids."* `LastTouch.player` is an `Entity`,
/// which is only meaningful while that entity is alive; a substitution
/// or despawn would leave the field dangling. `player_id` is the
/// denormalised identity a replay record or event payload can hold
/// without pinning a live world entity.
///
/// Today the id is minted from `Entity::to_bits()`, which is stable
/// and deterministic for a given world. When the PRD's roster fixture
/// lands, `PlayerId` becomes the fixture's `player_id` and every
/// existing site keeps compiling — the type is the seam.
/// Internal u64 is private (§5): access goes through accessors so the
/// invariant "the id came from a fixture or from `Entity::to_bits()`"
/// stays intact across the planned roster-fixture migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PlayerId(u64);

impl PlayerId {
    /// Mint a stable id from a live entity's bits.
    #[must_use]
    pub const fn from_entity(entity: Entity) -> Self {
        Self(entity.to_bits())
    }

    /// Mint a stable id directly from `u64` bits.
    ///
    /// Used by event writers when they have the id from a
    /// `LastTouch.player_id` (the bit-encoded form) and need to
    /// pass it through to a wire payload without re-wrapping
    /// through a live `Entity`. Today `player_id` is always
    /// minted via `from_entity`, so `bits == entity.to_bits()`;
    /// this constructor exists as the inverse for downstream
    /// sites that already have the bits.
    #[must_use]
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// Raw bits accessor for serialisation and hashing.
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }
}

/// Ball possession state — the singleton `Ball` Resource's `state`
/// field, mirrored from `Ball.possessor.is_some()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BallState {
    Free,
    Possessed,
    InFlight,
    Dead,
}

/// Phase 3: Out-of-bounds restart types (Law 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BallOutOfBoundsType {
    ThrowIn,
    GoalKick,
    Corner,
}

/// Phase 3: Foul types (Law 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FoulType {
    DangerousPlay,
}

/// Phase 3: Rule events emitted by the rules engine.
/// Used for event streaming, replay recording, and CLI output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Component)]
pub enum RuleEvent {
    OutOfBounds {
        oob_type: BallOutOfBoundsType,
        /// The full touch record at the moment the ball crossed the
        /// boundary. `Ball.last_touch` is preserved on `Ball` after the
        /// event fires (M8 stopped the post-event clear); downstream
        /// consumers can re-read `Ball.last_touch` for replay data.
        last_touch: Option<LastTouch>,
    },
    Goal {
        scorer_team: TeamId,
        score: (u8, u8),
        /// The full touch record at the moment the ball crossed the
        /// line. Scorer attribution uses ball position, not the
        /// toucher team; this field is captured for completeness and
        /// future use (e.g. own-goal attribution if introduced).
        last_touch: Option<LastTouch>,
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

/// Categorises a touch of the ball: whether the player actively played it
/// (kick / interception) or whether the ball was merely placed by a
/// restart procedure.
///
/// `Restart` touches (kickoff, throw-in, goal-kick, corner placement)
/// credit a team for Law 9 / Law 10 restart-taker resolution but are not
/// an active play of the ball; only `Play` touches are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TouchKind {
    /// Ball placed by `restart_system` or `lifecycle_system` (kickoff,
    /// throw-in, goal-kick, corner).
    Restart,
    /// Player kicked the ball (`kick_execution_system`) or gained
    /// possession by proximity (`possession_resolution_system`).
    Play,
}

/// Records who last touched the ball and how.
///
/// Every identity field is denormalised so the record stays meaningful
/// after the touching entity despawns:
///
/// - `team` — the toucher's team, copied off `Player.team_id` at
///   registration time. This is what Law 9 restart-taker resolution
///   reads, and what keeps an out-of-bounds restart signal
///   payload correct after the toucher is gone.
/// - `player` — the live `Entity`, for the same-tick readers that need
///   a world lookup. `None` when no specific player is credited.
/// - `player_id` — the stable identity paired with `player`, so a
///   replay record or event payload survives despawn. `None` whenever
///   `player` is `None`.
///
/// `kind` distinguishes active play from restart placements.
///
/// Note: `team` and `player_id` are denormalised for replay-data
/// persistence but are **not** mixed into the state hash — see
/// `sim-core/src/simulation/state_hash.rs` for the rationale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LastTouch {
    pub team: TeamId,
    pub player: Option<Entity>,
    pub player_id: Option<PlayerId>,
    pub kind: TouchKind,
    /// Global simulation tick at which the touch was registered.
    ///
    /// Writers stamp this via [`Self::with_tick`]; `0` means
    /// "unknown" (hand-built records in tests and legacy writers).
    /// Goal attribution treats `0` as within the assist window
    /// rather than inventing a cutoff (PRD §8.5).
    pub tick: u64,
}

impl LastTouch {
    /// Register a touch by a specific player. `player_id` is minted
    /// from the entity's bits so the pair stays consistent.
    #[must_use]
    pub const fn by_player(team: TeamId, player: Entity, kind: TouchKind) -> Self {
        Self {
            team,
            player: Some(player),
            player_id: Some(PlayerId::from_entity(player)),
            kind,
            tick: 0,
        }
    }

    /// Register a touch that credits a team but no individual player
    /// (a restart placement with no resolvable taker). Both identity
    /// fields are `None`.
    #[must_use]
    pub const fn by_team(team: TeamId, kind: TouchKind) -> Self {
        Self {
            team,
            player: None,
            player_id: None,
            kind,
            tick: 0,
        }
    }

    /// Stamp the registration tick onto a touch built by one of the
    /// constructors above. Writers call this with the live
    /// `CurrentTick` so downstream readers (goal attribution's
    /// 10 s assist window, PRD §8.5) can compare touch recency.
    #[must_use]
    pub const fn with_tick(mut self, tick: u64) -> Self {
        self.tick = tick;
        self
    }

    /// Register a touch from a (possibly placeholder) taker entity.
    /// `Entity::PLACEHOLDER` collapses to `by_team` (no resolvable
    /// player identity); every other entity is recorded with its
    /// `player` and minted `player_id` via `by_player`. This is the
    /// single helper the restart / kickoff / lifecycle writers all use
    /// so the placeholder check lives in exactly one place.
    #[must_use]
    pub fn from_taker(team: TeamId, taker: Entity, kind: TouchKind) -> Self {
        if taker == Entity::PLACEHOLDER {
            Self::by_team(team, kind)
        } else {
            Self::by_player(team, taker, kind)
        }
    }
}

/// Airborne state of the ball (PRD §7.4).
///
/// When the ball is in flight from a cross, `landing_point` is where it
/// will settle and `flight_ticks_remaining` counts down each tick until
/// it lands. `None` / `0` means the ball is not airborne (on the ground
/// or possessed).
///
/// Kept as a separate resource (rather than fields on [`Ball`]) so the
/// ball's core constructor stays stable across test fixtures.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct BallAirborne {
    /// Where the airborne ball will settle. `None` when not airborne.
    pub landing_point: Option<Vec2>,
    /// Ticks remaining until the ball lands. `0` when not airborne.
    pub flight_ticks_remaining: u64,
}

/// Pending ball touches registered this tick.
///
/// Writers (kick, possession acquisition, restart placements) append
/// to this buffer; the consumer `apply_ball_touched_system` drains it
/// and writes the final entry to `Ball.last_touch`.
///
/// The consumer runs at **two** sites, and both are load-bearing:
///
/// 1. First in the `Rules` set (`sim-core`'s `register_systems`), so
///    OOB / goal / foul / restart all read the same-tick
///    touch written by the `Execution` and `Possession` sets.
/// 2. In `Simulation::tick`, immediately after the post-schedule
///    `lifecycle_system` returns. `lifecycle_system` runs outside the
///    schedule, so its kickoff placement is invisible to site 1 on the
///    tick it happens; site 2 is what lands that touch on
///    `Ball.last_touch` before the tick ends. Removing site 2 leaves
///    the kickoff touch one tick stale — see
///    `sim-core/src/tests/touch_drain_tests.rs`.
///
/// Determinism: writes happen in fixed system-set order
/// (Execution → Possession → Rules → `MatchAdmin`), and Bevy
/// `ResMut` access serialises those writes. Last-touch-wins
/// semantics are equivalent to a same-tick `EventReader` ordered by
/// emission time.
///
/// This resource replaces a Bevy `Events<T>` channel by design
/// (see the design spec §B and the OOB restart bug ticket at
/// `.superpowers/sdd/oob-restart-bug.md`, which recommends
/// Resource over events).
#[derive(Resource, Debug, Default)]
pub struct PendingBallTouches(pub Vec<LastTouch>);
