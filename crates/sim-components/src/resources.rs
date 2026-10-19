//! Core resources: singletons owned by the simulation world.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use sim_math::Vec2;

use crate::{BallState, LastTouch, MatchState};

/// `MatchClock` is the simulation-time clock for the active match.
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
        super::time::ticks_to_secs_f32(self.elapsed_ticks)
    }

    /// Returns added time in seconds for display purposes.
    #[must_use]
    pub fn added_time_secs(&self) -> f32 {
        super::time::ticks_to_secs_f32(self.added_time_ticks)
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
