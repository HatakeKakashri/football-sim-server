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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PlayerId(pub u64);

impl PlayerId {
    /// Mint a stable id from a live entity's bits.
    #[must_use]
    pub const fn from_entity(entity: Entity) -> Self {
        Self(entity.to_bits())
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
/// Law 11 (offside) is judged at the moment the ball is "played" by a
/// teammate of the potentially-offside attacker. Touches registered as
/// `Restart` (kickoff, throw-in, goal-kick, corner placement) do **not**
/// reset offside judgement — `offside_detection_system` ignores them.
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
///   reads, and what keeps an out-of-bounds `RuleEvent::OutOfBounds`
///   payload correct after the toucher is gone.
/// - `player` — the live `Entity`, for the same-tick readers that need
///   a world lookup (`offside_detection_system` reads the toucher's
///   position). `None` when no specific player is credited.
/// - `player_id` — the stable identity paired with `player`, so a
///   replay record or event payload survives despawn. `None` whenever
///   `player` is `None`.
///
/// `kind` distinguishes active play from restart placements so Law 11
/// (offside) can ignore restart touches via `offside_detection_system`'s
/// existing `kind == Play` filter.
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
        }
    }
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
///    OOB / goal / offside / foul / restart all read the same-tick
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
