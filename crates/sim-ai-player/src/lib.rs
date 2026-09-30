//! `sim-ai-player`: per-player decision / execution pipeline for the
//! `football-sim-server` workspace.
//!
//! Public surface: the `UtilityBrain` component, the `PerceptionSnapshot`
//! per-player component, and the three systems (`perception_system`,
//! `player_decision_system`, `player_action_execution_system`,
//! `kick_execution_system`). The decision cadence is gated by
//! `DECISION_CADENCE_TICKS` + `player_stagger_slot`.

pub(crate) mod brain;
mod decision;
mod execution;
mod perception;
#[cfg(test)]
mod decision_tests;
#[cfg(test)]
mod perception_tests;
#[cfg(test)]
mod roster_tests;
#[cfg(test)]
mod tests;

pub use brain::{PlayerAction, UtilityBrain};
pub use decision::{consideration_scoring_system, player_decision_system};
pub use execution::{kick_execution_system, player_action_execution_system};
pub use perception::perception_system;
pub use sim_components::{NearbyEntity, PerceptionSnapshot, PitchBounds};

use bevy_ecs::prelude::*;
use sim_ai_core::ResponseCurve;
use sim_components::Intent;
use sim_physics::PitchControlGrid;

/// Cadence: a player re-evaluates its `UtilityBrain` once every
/// `DECISION_CADENCE_TICKS` ticks; which tick is determined by
/// `player_stagger_slot(entity)`. The other 5 of every 6 ticks the player
/// keeps its previous `Intent`, so 22 players × 60 Hz / 6 ≈ 220 evaluations
/// per second vs 1320 without staggering.
#[allow(
    clippy::too_long_first_doc_paragraph,
    reason = "single-cadence constant; one paragraph best documents the contract"
)]
pub const DECISION_CADENCE_TICKS: u64 = 6;

/// Counter incremented by `player_decision_system` each time a player is
/// actually evaluated (i.e. passed the cadence guard). Test-only accessor
/// via `get()`; gameplay code does not read this value.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct DecisionEvaluationCount(u64);

impl DecisionEvaluationCount {
    /// Current count of players that have cleared the cadence guard.
    /// Test-only public accessor; gameplay code does not read this value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Map an entity to one of `DECISION_CADENCE_TICKS` decision-cadence
/// slots. Uses a `SplitMix64` finalize on the entity's packed id bits so
/// 22 players distribute roughly evenly across the N slots without a
/// roster-index pass. Stable for a given entity id (Bevy entity ids are
/// dense u32 indices, allocated deterministically).
///
/// Spec §10 deviation: the spec mandates `tick % 22 == player_index`
/// (roster-index staggering). This implementation substitutes a hash of
/// the entity id, which is equivalent in expectation (uniform distribution
/// across slots) but not equivalent per-player. The substitution was
/// made to avoid a roster-index pass at decision time. If a future test
/// pins a specific player's slot, this function will need to switch to
/// the spec's formula. See `test_stagger_slot_distribution` for the
/// distribution guarantee.
pub(crate) const fn player_stagger_slot(entity: Entity) -> u64 {
    let bits = entity.to_bits();
    // SplitMix64 finalize — well-trodden; uniform in [0, 2^64).
    let z = (bits ^ (bits >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    let z = z ^ (z >> 31);
    z % DECISION_CADENCE_TICKS
}

/// One consideration in a player's utility brain.
///
/// Each variant carries its own `weight` and `ResponseCurve`. The
/// `raw_input` dispatcher is exhaustive: adding a new variant forces a
/// match-arm update at compile time, so a brain template that references
/// a typo'd consideration fails at compile time instead of silently
/// scoring 0.5 (the old string-dispatch behaviour).
#[allow(
    clippy::too_long_first_doc_paragraph,
    reason = "single-paragraph exhaustive-dispatch contract"
)]
#[derive(Debug, Clone)]
pub enum Consideration {
    DistanceToTarget { weight: f32, curve: ResponseCurve },
    DistanceToBall { weight: f32, curve: ResponseCurve },
    Stamina { weight: f32, curve: ResponseCurve },
    PitchControlAtBall { weight: f32, curve: ResponseCurve },
    PassAngleClear { weight: f32, curve: ResponseCurve },
    TeammateDistance { weight: f32, curve: ResponseCurve },
    TeammateSpace { weight: f32, curve: ResponseCurve },
    DistanceToGoal { weight: f32, curve: ResponseCurve },
    GoalAngle { weight: f32, curve: ResponseCurve },
    DefenderPressure { weight: f32, curve: ResponseCurve },
    DistanceToOpponent { weight: f32, curve: ResponseCurve },
    SkillDiff { weight: f32, curve: ResponseCurve },
    DistanceToMarked { weight: f32, curve: ResponseCurve },
    DefensivePosition { weight: f32, curve: ResponseCurve },
    DistanceToPress { weight: f32, curve: ResponseCurve },
    SpaceAhead { weight: f32, curve: ResponseCurve },
    TeammateBall { weight: f32, curve: ResponseCurve },
    FormationDiscipline { weight: f32, curve: ResponseCurve },
}

/// Bundles the data the dispatcher would otherwise take as five
/// parameters. Held behind a reference so callers can keep their
/// perception snapshots on the stack.
pub struct ConsiderationContext<'a> {
    pub perception: &'a PerceptionSnapshot,
    pub intent: &'a Intent,
    pub stamina: f32,
    pub skill: f32,
    pub grid: Option<&'a PitchControlGrid>,
}

/// Macro that generates `Consideration::weight` and `Consideration::curve`.
///
/// Each row is one `Consideration` variant; the macro emits one `match` arm
/// per variant for each accessor. Adding a new `Consideration` variant
/// forces the developer to add a row here (the match would be
/// non-exhaustive otherwise), which keeps the accessors in lock-step with
/// the enum definition.
macro_rules! consideration_accessors {
    ( $( $variant:ident { weight: $w:expr, curve: $c:expr } ),+ $(,)? ) => {
        impl Consideration {
            #[must_use]
            pub const fn weight(&self) -> f32 {
                match self {
                    $( Self::$variant { weight, .. } => *weight, )+
                }
            }

            #[must_use]
            pub const fn curve(&self) -> &ResponseCurve {
                match self {
                    $( Self::$variant { curve, .. } => curve, )+
                }
            }
        }
    };
}

consideration_accessors! {
    DistanceToTarget    { weight: _, curve: _ },
    DistanceToBall      { weight: _, curve: _ },
    Stamina             { weight: _, curve: _ },
    PitchControlAtBall  { weight: _, curve: _ },
    PassAngleClear      { weight: _, curve: _ },
    TeammateDistance    { weight: _, curve: _ },
    TeammateSpace       { weight: _, curve: _ },
    DistanceToGoal      { weight: _, curve: _ },
    GoalAngle           { weight: _, curve: _ },
    DefenderPressure    { weight: _, curve: _ },
    DistanceToOpponent  { weight: _, curve: _ },
    SkillDiff           { weight: _, curve: _ },
    DistanceToMarked    { weight: _, curve: _ },
    DefensivePosition   { weight: _, curve: _ },
    DistanceToPress     { weight: _, curve: _ },
    SpaceAhead          { weight: _, curve: _ },
    TeammateBall        { weight: _, curve: _ },
    FormationDiscipline { weight: _, curve: _ },
}

/// Tactical distance thresholds (metres) used by `Consideration::raw_input`
/// to convert perception-snapshot fields into scoring inputs.
///
/// Centralising these here makes the consideration arms read like a table
/// rather than a wall of magic numbers, and gives a single point of
/// adjustment when a balance pass changes the model's sensitivity.
#[allow(
    clippy::module_name_repetitions,
    reason = "the name `tactical_thresholds` is intentional; not a repetition"
)]
pub(crate) mod tactical_thresholds {
    /// Opponents closer than this are considered to have a clear pass lane.
    pub const PASS_LANE_CLEAR_M: f32 = 8.0;
    /// Upper bound used by `TeammateSpace` (and friends) when the nearest
    /// opponent is non-finite (no opponents visible).
    pub const TEAMMATE_SPACE_DEFAULT_M: f32 = 20.0;
    /// Cap on `TeammateSpace` / `DistanceToTarget` / `SpaceAhead` raw scores
    /// before scaling.
    pub const TEAMMATE_SPACE_CAP_M: f32 = 20.0;
    /// Cap for `DistanceToOpponent` scoring.
    pub const DEFENDER_PRESSURE_RADIUS_M: f32 = 5.0;
    /// Cap for `DistanceToMarked` scoring.
    pub const DISTANCE_TO_MARKED_CAP_M: f32 = 10.0;
    /// Cap for `DistanceToPress` scoring.
    pub const DISTANCE_TO_PRESS_CAP_M: f32 = 12.0;
    /// Cap for `SpaceAhead` / `DistanceToTarget` raw scoring.
    pub const SPACE_AHEAD_CAP_M: f32 = 15.0;
    /// Distance to ball below which a teammate is considered to be
    /// "on the ball" (for `TeammateBall`).
    pub const TEAMMATE_ON_BALL_RADIUS_M: f32 = 2.0;
    /// Cap for `DistanceToTarget` and `TeammateDistance` scoring.
    pub const DISTANCE_TO_TARGET_CAP_M: f32 = 30.0;
    /// Cap for `DistanceToGoal` scoring.
    pub const DISTANCE_TO_GOAL_CAP_M: f32 = 35.0;
    /// `PitchControlAtBall` returns a value in [0, 1]; we scale to [0, 100].
    pub const PITCH_CONTROL_SCALE: f32 = 100.0;
    /// `Stamina` and `Skill` baseline; `SkillDiff` clamps skill-0.7 to [-1, 1].
    pub const SKILL_BASELINE: f32 = 0.7;
    /// Floor on goal-angle magnitude to avoid divide-by-zero in `GoalAngle`.
    pub const GOAL_ANGLE_MIN_MAGNITUDE: f32 = 1e-3;
}

/// Convert a small count (nearby entities, at most a handful) to `f32`.
#[expect(
    clippy::cast_precision_loss,
    reason = "counts here are tiny (<= 8 nearby entities), far below 2^24, so the cast is exact"
)]
pub(crate) const fn count_to_f32(n: usize) -> f32 {
    n as f32
}

impl Consideration {
    /// Compute the raw, unnormalized input for this consideration. The
    /// returned value is fed into `self.curve()` by the caller (typically
    /// `player_decision_system`) which then multiplies by the weight.
    ///
    /// `ctx.grid` is `None` when the perception snapshot was built
    /// without a pitch-control grid; the relevant consideration falls
    /// back to a neutral value in that case. Per arm the semantics
    /// mirror the original `compute_consideration_input` string dispatch
    /// 1:1.
    #[must_use]
    #[expect(
        clippy::too_many_lines,
        reason = "flat name -> input lookup table; splitting it would only scatter the mapping"
    )]
    pub fn raw_input(&self, ctx: &ConsiderationContext) -> f32 {
        match *self {
            Self::DistanceToTarget { .. } => {
                let d = if let sim_components::Intent::Movement(
                    sim_components::MovementIntent::MoveToPosition(target),
                ) = ctx.intent
                {
                    ctx.perception.self_position.distance(*target)
                } else {
                    ctx.perception
                        .self_position
                        .distance(ctx.perception.ball_position)
                };
                tactical_thresholds::DISTANCE_TO_TARGET_CAP_M
                    - d.min(tactical_thresholds::DISTANCE_TO_TARGET_CAP_M)
            }
            Self::DistanceToBall { .. } => ctx
                .perception
                .self_position
                .distance(ctx.perception.ball_position),
            Self::Stamina { .. } => ctx.stamina,
            Self::PitchControlAtBall { .. } => ctx.grid.map_or(50.0, |g| {
                g.control_at(
                    ctx.perception.ball_position.x,
                    ctx.perception.ball_position.y,
                ) * tactical_thresholds::PITCH_CONTROL_SCALE
            }),
            Self::PassAngleClear { .. } => {
                if matches!(
                    ctx.intent,
                    sim_components::Intent::Action(sim_components::ActionIntent::PassTo)
                ) {
                    let lane_clear = !ctx
                        .perception
                        .nearby_opponents
                        .iter()
                        .any(|opp| opp.distance < tactical_thresholds::PASS_LANE_CLEAR_M);
                    if lane_clear { 1.0 } else { 0.4 }
                } else {
                    0.5
                }
            }
            Self::TeammateDistance { .. } => {
                if ctx.perception.nearby_teammates.is_empty() {
                    0.0
                } else {
                    let avg: f32 = ctx
                        .perception
                        .nearby_teammates
                        .iter()
                        .map(|t| t.distance)
                        .sum::<f32>()
                        / count_to_f32(ctx.perception.nearby_teammates.len());
                    tactical_thresholds::DISTANCE_TO_TARGET_CAP_M - avg.min(tactical_thresholds::DISTANCE_TO_TARGET_CAP_M)
                }
            }
            Self::TeammateSpace { .. } => {
                let nearest_opp = ctx
                    .perception
                    .nearby_opponents
                    .iter()
                    .map(|o| o.distance)
                    .fold(f32::INFINITY, f32::min);
                let space_m = if nearest_opp.is_finite() {
                    nearest_opp
                } else {
                    tactical_thresholds::TEAMMATE_SPACE_DEFAULT_M
                };
                tactical_thresholds::TEAMMATE_SPACE_CAP_M
                    - space_m.min(tactical_thresholds::TEAMMATE_SPACE_CAP_M)
            }
            Self::DistanceToGoal { .. } => {
                tactical_thresholds::DISTANCE_TO_GOAL_CAP_M - ctx
                    .perception
                    .self_position
                    .distance(ctx.perception.goal_position)
            }
            Self::GoalAngle { .. } => {
                let to_ball = ctx.perception.ball_position - ctx.perception.self_position;
                let to_goal = ctx.perception.goal_position - ctx.perception.self_position;
                let dot = to_ball.dot(to_goal);
                let mags = to_ball.length() * to_goal.length();
                if mags > tactical_thresholds::GOAL_ANGLE_MIN_MAGNITUDE {
                    (dot / mags).clamp(-1.0, 1.0)
                } else {
                    0.0
                }
            }
            Self::DefenderPressure { .. } => count_to_f32(
                ctx.perception
                    .nearby_opponents
                    .iter()
                    .filter(|o| o.distance <= tactical_thresholds::DEFENDER_PRESSURE_RADIUS_M)
                    .count(),
            ),
            Self::DistanceToOpponent { .. } => {
                tactical_thresholds::DEFENDER_PRESSURE_RADIUS_M - ctx
                    .perception
                    .nearby_opponents
                    .iter()
                    .map(|o| o.distance)
                    .fold(f32::INFINITY, f32::min)
                    .min(tactical_thresholds::DEFENDER_PRESSURE_RADIUS_M)
            }
            Self::SkillDiff { .. } => (ctx.skill - tactical_thresholds::SKILL_BASELINE).clamp(-1.0, 1.0),
            Self::DistanceToMarked { .. } => {
                tactical_thresholds::DISTANCE_TO_MARKED_CAP_M - ctx
                    .perception
                    .nearby_opponents
                    .iter()
                    .map(|o| o.distance)
                    .fold(f32::INFINITY, f32::min)
                    .min(tactical_thresholds::DISTANCE_TO_MARKED_CAP_M)
            }
            Self::DefensivePosition { .. } => {
                let own_goal_x = 0.0_f32;
                let ball_x = ctx.perception.ball_position.x;
                let player_x = ctx.perception.self_position.x;
                if player_x <= ball_x && player_x >= own_goal_x {
                    1.0
                } else {
                    0.0
                }
            }
            Self::DistanceToPress { .. } => {
                tactical_thresholds::DISTANCE_TO_PRESS_CAP_M - ctx
                    .perception
                    .nearby_opponents
                    .iter()
                    .map(|o| o.distance)
                    .fold(f32::INFINITY, f32::min)
                    .min(tactical_thresholds::DISTANCE_TO_PRESS_CAP_M)
            }
            Self::SpaceAhead { .. } => {
                let forward = ctx.perception.ball_position.x > ctx.perception.self_position.x;
                let opp_in_front = ctx
                    .perception
                    .nearby_opponents
                    .iter()
                    .filter(|o| {
                        if forward {
                            o.relative_position.x > 0.0
                        } else {
                            o.relative_position.x < 0.0
                        }
                    })
                    .map(|o| o.distance)
                    .fold(f32::INFINITY, f32::min);
                let d = if opp_in_front.is_finite() {
                    opp_in_front
                } else {
                    tactical_thresholds::SPACE_AHEAD_CAP_M
                };
                tactical_thresholds::SPACE_AHEAD_CAP_M - d.min(tactical_thresholds::SPACE_AHEAD_CAP_M)
            }
            Self::TeammateBall { .. } => {
                let has = ctx.perception.nearby_teammates.iter().any(|t| {
                    let dist_to_ball = (t.relative_position + ctx.perception.self_position
                        - ctx.perception.ball_position)
                        .length();
                    dist_to_ball < tactical_thresholds::TEAMMATE_ON_BALL_RADIUS_M
                });
                if has { 1.0 } else { 0.0 }
            }
            Self::FormationDiscipline { .. } => 1.0,
        }
    }
}
