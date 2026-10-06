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
/// `team` is denormalised so the record resolves even if the entity
/// despawns. `player` is the player who touched the ball, or `None`
/// when no specific player is credited (a kickoff placement today;
/// future restarts may also use `None`). `kind` distinguishes active
/// play from restart placements so Law 11 (offside) can ignore
/// restart touches via `offside_detection_system`'s existing
/// `kind == Play` filter.
///
/// Note: `team` is denormalised for replay-data persistence but is
/// **not** mixed into the state hash — see
/// `sim-core/src/simulation/state_hash.rs:163,184` for the rationale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LastTouch {
    pub team: TeamId,
    pub player: Option<Entity>,
    pub kind: TouchKind,
}

/// Pending ball touches registered this tick.
///
/// Writers (kick, possession acquisition, restart placements) append
/// to this buffer; the consumer `apply_ball_touched_system` drains it
/// at the start of the Rules set and writes the final entry to
/// `Ball.last_touch`.
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
