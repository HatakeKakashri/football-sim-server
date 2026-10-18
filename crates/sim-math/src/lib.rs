use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[must_use]
    pub const fn zero() -> Self {
        Self { x: 0.0, y: 0.0 }
    }

    #[must_use]
    pub fn length_squared(self) -> f32 {
        self.y.mul_add(self.y, self.x * self.x)
    }

    #[must_use]
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    #[must_use]
    pub fn normalized(self) -> Self {
        let len = self.length();
        if len < f32::EPSILON {
            Self::zero()
        } else {
            self / len
        }
    }

    #[must_use]
    pub fn dot(self, other: Self) -> f32 {
        self.y.mul_add(other.y, self.x * other.x)
    }

    #[must_use]
    pub fn distance_squared(self, other: Self) -> f32 {
        (self - other).length_squared()
    }

    #[must_use]
    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }

    #[must_use]
    pub fn clamp_length(self, max_length: f32) -> Self {
        let len = self.length();
        if len > max_length {
            self * (max_length / len)
        } else {
            self
        }
    }
}

/// Standard pitch length in metres (along the x-axis).
pub const PITCH_LENGTH: f32 = 105.0;

/// The 14 football positions (PRD §6.5).
///
/// The 13 outfield positions (GK excluded from the fit grid) form a
/// 5×3 grid:
/// ```text
/// row 5:            ST
/// row 4:  AML       AMC       AMR
/// row 3:  ML        MC        MR
/// row 2:  DML       DMC       DMR
/// row 1:  DL        DC        DR
/// ```
///
/// `GK` is never on the grid; it accepts only goalkeeper profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PositionSlot {
    /// Goalkeeper.
    GK,
    /// Defensive left (full back).
    DL,
    /// Defensive center (left).
    DC,
    /// Defensive center (right).
    DR,
    /// Defensive midfield left.
    DML,
    /// Defensive midfield center.
    DMC,
    /// Defensive midfield right.
    DMR,
    /// Midfield left.
    ML,
    /// Midfield center.
    MC,
    /// Midfield right.
    MR,
    /// Attacking midfield left.
    AML,
    /// Attacking midfield center.
    AMC,
    /// Attacking midfield right.
    AMR,
    /// Striker.
    ST,
}
/// Standard pitch width in metres (along the y-axis).
pub const PITCH_WIDTH: f32 = 68.0;
/// Standard goal centre y-coordinate (half of pitch width).
pub const GOAL_Y_CENTER: f32 = 34.0;

/// Returns the goal position `Vec2` for a team attacking in `attack_direction`.
///
/// - `attack_direction > 0` (+1): team attacks toward `x = PITCH_LENGTH` (right goal).
/// - `attack_direction <= 0` (-1): team attacks toward `x = 0.0` (left goal).
#[must_use]
pub const fn goal_position_for(attack_direction: i8) -> Vec2 {
    if attack_direction > 0 {
        Vec2::new(PITCH_LENGTH, GOAL_Y_CENTER)
    } else {
        Vec2::new(0.0, GOAL_Y_CENTER)
    }
}

/// Result of a swept proximity test between two moving points over a unit
/// time interval.
///
/// The swept test checks whether two objects moving at constant velocity
/// come within `radius` meters of each other at any point during the tick.
/// This is critical for acquisition: a player moving at 5 m/tick (5 m/s at
/// 60 ticks/s) can traverse 5 m in a single tick, so a static end-of-tick
/// radius check at 1.5 m would miss the player entirely.
///
/// # Mathematical basis
///
/// For two points A and B moving at velocities `v_a` and `v_b` over
/// `t ∈ [0, 1]`:
/// - `A(t) = a_start + t * v_a`
/// - `B(t) = b_start + t * v_b`
///
/// The squared distance is `|A(t) - B(t)|² = a*t² + 2*b*t + c` where:
/// - `a = |v_a - v_b|²`  (relative speed squared)
/// - `b = (a_start - b_start) · (v_a - v_b)`  (initial separation · relative velocity)
/// - `c = |a_start - b_start|²`  (initial separation squared)
///
/// The minimum occurs at `t* = -b/a` (when `a > 0`). If `t* ∈ [0, 1]`,
/// the minimum distance is `sqrt(c - b²/a)`; otherwise the minimum is at
/// an endpoint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SweptProximity {
    /// The minimum distance between the two paths over the interval.
    pub min_distance: f32,
    /// `true` if the paths come within `radius` at any point.
    pub acquired: bool,
}

/// Compute the minimum distance between two moving points over a unit time
/// interval `[0, 1]` (one simulation tick), and report whether acquisition
/// (within `radius`) occurred.
///
/// This is a **swept collision** test: it considers the entire path each
/// object travels during the tick, not just the start or end positions.
///
/// # Parameters
///
/// - `a_start`: position of object A (e.g., player) at tick start.
/// - `a_vel`: velocity of object A (displacement per tick).
/// - `b_start`: position of object B (e.g., ball) at tick start.
/// - `b_vel`: velocity of object B (displacement per tick).
/// - `radius`: acquisition radius in metres.
///
/// # Returns
///
/// `SweptProximity { min_distance, acquired }` where `acquired` is
/// `min_distance <= radius`.
#[must_use]
pub fn swept_proximity(
    a_start: Vec2,
    a_vel: Vec2,
    b_start: Vec2,
    b_vel: Vec2,
    radius: f32,
) -> SweptProximity {
    let rel_vel = a_vel - b_vel;
    let rel_start = a_start - b_start;

    // Coefficients of |rel_start + t * rel_vel|² = a*t² + 2*b*t + c
    let a = rel_vel.length_squared();
    let b = rel_start.dot(rel_vel);
    let c = rel_start.length_squared();

    let min_distance;

    if a < f32::EPSILON {
        // Relative motion is negligible — fall back to static distance.
        // Both objects are moving together or are stationary relative to
        // each other, so the distance is essentially constant.
        min_distance = rel_start.length();
    } else {
        // Find t* = -b/a, the unconstrained minimum of the quadratic.
        let t_star = -b / a;

        let (dist_at_0, dist_at_1) = {
            let at_0 = rel_start.length();
            let at_1 = (rel_start + rel_vel).length();
            (at_0, at_1)
        };

        // If t* is outside [0, 1], the minimum is at an endpoint.
        // Otherwise, the minimum is at t* (interior).
        if (0.0..=1.0).contains(&t_star) {
            // Interior minimum: formula is min_dist² = c - b²/a.
            // This is the minimum squared distance at t = t*.
            let min_dist_sq = c - (b * b) / a;
            min_distance = if min_dist_sq > 0.0 {
                min_dist_sq.sqrt()
            } else {
                // Numerical edge case: paths intersect, distance is 0.
                0.0
            };
        } else {
            // Minimum is at whichever endpoint is closer.
            min_distance = dist_at_0.min(dist_at_1);
        }
    }

    SweptProximity {
        min_distance,
        acquired: min_distance <= radius,
    }
}

impl Add for Vec2 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub for Vec2 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl MulAssign<f32> for Vec2 {
    fn mul_assign(&mut self, rhs: f32) {
        self.x *= rhs;
        self.y *= rhs;
    }
}

impl Mul<Vec2> for f32 {
    type Output = Vec2;

    fn mul(self, rhs: Vec2) -> Self::Output {
        Vec2 {
            x: self * rhs.x,
            y: self * rhs.y,
        }
    }
}

impl Div<f32> for Vec2 {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x / rhs,
            y: self.y / rhs,
        }
    }
}

impl DivAssign<f32> for Vec2 {
    fn div_assign(&mut self, rhs: f32) {
        self.x /= rhs;
        self.y /= rhs;
    }
}

impl Eq for Vec2 {}

impl Hash for Vec2 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.x.to_bits().hash(state);
        self.y.to_bits().hash(state);
    }
}

#[derive(Debug, Clone, Copy, Resource)]
pub struct PitchDimensions {
    pub width: f32,
    pub length: f32,
    pub penalty_area: Rect,
    pub goal_area: Rect,
    pub center_circle: Circle,
}

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub min: Vec2,
    pub max: Vec2,
}

#[derive(Debug, Clone, Copy)]
pub struct Circle {
    pub center: Vec2,
    pub radius: f32,
}

impl PitchDimensions {
    #[must_use]
    pub const fn standard() -> Self {
        Self {
            width: 105.0,
            length: 68.0,
            // Standard pitch: penalty area is 16.5m from goal line, 40.32m wide.
            // The pitch is 105m long and 68m wide; goal line at x=0.
            penalty_area: Rect {
                min: Vec2::new(0.0, 13.84),
                max: Vec2::new(16.5, 54.16),
            },
            // Goal area is 5.5m from goal line, 18.32m wide.
            goal_area: Rect {
                min: Vec2::new(0.0, 24.84),
                max: Vec2::new(5.5, 43.16),
            },
            center_circle: Circle {
                center: Vec2::new(52.5, 34.0),
                radius: 9.15,
            },
        }
    }

    #[must_use]
    pub const fn width(&self) -> f32 {
        self.width
    }

    #[must_use]
    pub const fn length(&self) -> f32 {
        self.length
    }

    #[must_use]
    pub fn contains(&self, point: Vec2) -> bool {
        point.x >= 0.0 && point.x <= self.width && point.y >= 0.0 && point.y <= self.length
    }
}

#[allow(
    clippy::too_long_first_doc_paragraph,
    reason = "the doc paragraph explains the cross-crate mirroring contract; splitting it would weaken the rationale"
)]
/// Number of `CurveParams` records in [`Tuning::curve_params`]: one per
/// consumed attribute. Mirrors `sim_components::profile::ATTRIBUTE_COUNT`
/// (30); it is defined here because `sim-math` cannot depend on
/// `sim-components`. The cross-crate consistency is asserted by a test in
/// `sim-components`.
pub const CURVE_PARAM_COUNT: usize = 30;

/// Logistic response-curve parameters for a single attribute consumer.
///
/// One `CurveParams` record per consumed attribute lives in [`Tuning`];
/// this is the single edit surface for §13.3 calibration. `curve` is the
/// only place that converts a raw attribute integer into a numeric
/// mechanic parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveParams {
    /// Attribute value (`0..=200`) at which the curve reaches its
    /// midpoint, i.e. the output is halfway between `min` and `max`.
    pub midpoint: f32,
    /// Steepness of the logistic transition. Must be positive for a
    /// strict monotonic response; the magnitude controls how quickly the
    /// output moves from `min` toward `max` around the midpoint.
    pub steepness: f32,
    /// Mechanic parameter value at the low end of the attribute range.
    pub min: f32,
    /// Mechanic parameter value at the high end of the attribute range
    /// (when `rising` is `true`).
    pub max: f32,
    /// `true` when the mechanic parameter rises with the attribute
    /// (e.g. save probability vs `Reflexes`); `false` when it falls
    /// (e.g. shot error vs `Shooting`).
    pub rising: bool,
}

/// Evaluate the logistic response curve for a raw attribute value.
///
/// `attribute` is clamped to `0..=200` and normalised to `0..=1`; the
/// logistic maps that to `0..=1`, which is then scaled to `[min, max]`
/// (reversed when `rising` is `false`). With `steepness > 0` the output
/// is strictly monotonic in `attribute`, rising when `rising` is true
/// and falling otherwise.
///
/// This is the **only** place that converts a raw attribute integer into
/// a numeric mechanic parameter; every mechanic consumer reads through
/// it (PRD §6.3).
#[allow(
    clippy::cast_precision_loss,
    reason = "attribute is clamped to 0..=200 before the cast, which fits exactly in f32's 23-bit mantissa"
)]
#[must_use]
pub fn curve(attribute: i32, params: &CurveParams) -> f32 {
    // Normalise the raw attribute (0..=200) to the unit interval.
    let x = attribute.clamp(0, 200) as f32 / 200.0;
    let mid = params.midpoint.clamp(0.0, 200.0) / 200.0;
    let logistic = 1.0 / (1.0 + f32::exp(-params.steepness * (x - mid)));
    let span = params.max - params.min;
    let output = if params.rising {
        params.min + logistic * span
    } else {
        params.max - logistic * span
    };
    output.clamp(params.min.min(params.max), params.max.max(params.min))
}

/// Which position group a slot belongs to for quality weighting
/// (PRD §6.4). The 14 slots collapse to six groups, each with its own
/// attribute weight table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QualityGroup {
    /// Defensive slots: `DL`, `DC`, `DR`.
    Defence,
    /// Wide slots: `ML`, `MR`.
    Wide,
    /// Midfield slots: `DML`, `DMC`, `DMR`, `MC`.
    Midfield,
    /// Attacking-midfield slots: `AML`, `AMC`, `AMR`.
    Attacking,
    /// Striker slot: `ST`.
    Striker,
    /// Goalkeeper slot: `GK`.
    Goalkeeper,
}

/// Number of position groups for quality weighting (PRD §6.4).
pub const QUALITY_GROUP_COUNT: usize = 6;

/// Per-position-group quality weights.
///
/// One row per [`QualityGroup`] (in declaration order); each row is
/// `[f32; CURVE_PARAM_COUNT]` of attribute weights in `sim_components`
/// `Attribute::ALL` order. Attributes irrelevant to a group carry `0.0`.
/// The weighted mean divides by the row's weight sum, so rows need not
/// sum to exactly one, though the defaults do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualityWeights {
    /// `table[group as usize]` is the weight row for that group.
    pub table: [[f32; CURVE_PARAM_COUNT]; QUALITY_GROUP_COUNT],
}

impl QualityWeights {
    /// Default quality-weight assumptions (pre-calibration).
    ///
    /// Each row weights the attributes most relevant to its group and
    /// sums to `1.0` so the weighted mean is directly interpretable on
    /// the `1..=200` attribute scale.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "default assumptions are f32 literals; a const fn is possible but the table is clearer as a runtime initializer"
    )]
    pub fn default_assumptions() -> Self {
        let w = |values: [f32; CURVE_PARAM_COUNT]| values;
        Self {
            table: [
                // Defence: DL, DC, DR — tackling, marking, positioning, heading.
                w([
                    0.18, 0.16, 0.14, 0.10, 0.06, // Tackling, Marking, Positioning, Heading, Bravery
                    0.04, 0.03, 0.02, 0.01, 0.01, // Passing, Dribbling, Crossing, Shooting, Finishing
                    0.08, 0.07, 0.05, 0.03, 0.02, // Outfield fitness/strength/aggression/speed/creativity
                    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, // Goalkeeping block
                    0.0, 0.0, 0.0, 0.0, 0.0, // Goalkeeper physical & mental
                ]),
                // Wide: ML, MR — crossing, dribbling, passing, speed.
                w([
                    0.06, 0.04, 0.04, 0.0, 0.0, // Tackling, Marking, Positioning, Heading, Bravery
                    0.12, 0.14, 0.14, 0.0, 0.0, // Passing, Dribbling, Crossing, Shooting, Finishing
                    0.10, 0.04, 0.08, 0.12, 0.12, // Outfield fitness/strength/aggression/speed/creativity
                    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, // Goalkeeping block
                    0.0, 0.0, 0.0, 0.0, 0.0, // Goalkeeper physical & mental
                ]),
                // Midfield: DML, DMC, DMR, MC — passing, tackling, positioning, creativity.
                w([
                    0.10, 0.06, 0.08, 0.0, 0.0, // Tackling, Marking, Positioning, Heading, Bravery
                    0.16, 0.10, 0.04, 0.04, 0.02, // Passing, Dribbling, Crossing, Shooting, Finishing
                    0.10, 0.04, 0.06, 0.08, 0.12, // Outfield fitness/strength/aggression/speed/creativity
                    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, // Goalkeeping block
                    0.0, 0.0, 0.0, 0.0, 0.0, // Goalkeeper physical & mental
                ]),
                // Attacking: AML, AMC, AMR — finishing, shooting, passing, creativity.
                w([
                    0.02, 0.02, 0.04, 0.0, 0.0, // Tackling, Marking, Positioning, Heading, Bravery
                    0.14, 0.12, 0.10, 0.12, 0.14, // Passing, Dribbling, Crossing, Shooting, Finishing
                    0.06, 0.0, 0.08, 0.06, 0.10, // Outfield fitness/strength/aggression/speed/creativity
                    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, // Goalkeeping block
                    0.0, 0.0, 0.0, 0.0, 0.0, // Goalkeeper physical & mental
                ]),
                // Striker: ST — finishing, shooting, heading, strength.
                w([
                    0.02, 0.02, 0.04, 0.10, 0.0, // Tackling, Marking, Positioning, Heading, Bravery
                    0.06, 0.08, 0.02, 0.18, 0.20, // Passing, Dribbling, Crossing, Shooting, Finishing
                    0.06, 0.08, 0.06, 0.08, 0.0, // Outfield fitness/strength/aggression/speed/creativity
                    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, // Goalkeeping block
                    0.0, 0.0, 0.0, 0.0, 0.0, // Goalkeeper physical & mental
                ]),
                // Goalkeeper: GK — reflexes, agility, anticipation, handling,
                // and the goalkeeper physical & mental block only. Outfield
                // attributes carry `0.0` and are never read for a GK profile.
                w([
                    0.0, 0.0, 0.0, 0.0, 0.0, // Outfield defence block
                    0.0, 0.0, 0.0, 0.0, 0.0, // Outfield attack block
                    0.0, 0.0, 0.0, 0.0, 0.0, // Outfield physical & mental block
                    0.15, 0.12, 0.05, 0.06, 0.06, // Reflexes, Anticipation, RushingOut, Throwing, Kicking
                    0.05, 0.08, 0.06, 0.14, 0.07, // Punching, AerialReach, Communication, Agility, Concentration
                    0.05, 0.04, 0.03, 0.03, 0.01, // Goalkeeper fitness/strength/aggression/speed/creativity
                ]),
            ],
        }
    }

    /// The weight row for `group`, indexed by `Attribute::ALL` order.
    #[must_use]
    pub const fn row(self, group: QualityGroup) -> [f32; CURVE_PARAM_COUNT] {
        self.table[group as usize]
    }
}

impl Default for QualityWeights {
    fn default() -> Self {
        Self::default_assumptions()
    }
}

/// Tuning configuration for match mechanics and response curves.
///
/// All tunable constants live in this struct (PRD §11). It is edited
/// only during calibration (one commit per batch of batch results).
///
/// The tuning struct is a singleton resource so every system reads
/// the same parameters. It is immutable during a match (no mid-match
/// tuning changes).
#[derive(Debug, Clone, Resource)]
pub struct Tuning {
    /// Position-fit multipliers (PRD §6.5).
    pub fit_multipliers: FitMultipliers,
    /// One `CurveParams` per consumed attribute, in `Attribute::ALL`
    /// order (sim-components). Index `i` corresponds to `Attribute::ALL[i]`.
    /// All fields live here so calibration has a single edit surface.
    pub curve_params: [CurveParams; CURVE_PARAM_COUNT],
    /// Per-position-group quality weights (PRD §6.4). Used only by
    /// reporting / display code; the match simulation never reads them.
    pub quality_weights: QualityWeights,
    /// Tackle success probability is read off the `Tackling` response
    /// curve (`curve_params[Attribute::Tackling as usize]`); its `min`
    /// is the base probability for an attribute of zero and its `max`
    /// the ceiling for an attribute of 200.
    ///
    /// Number of ticks a tackler is out of position after a *failed*
    /// tackle (PRD §7.3: about 1.5 s / 90 ticks). During this window
    /// the tackler moves slower and cannot re-engage; the carrier skips
    /// past.
    pub tackle_recovery_ticks: u64,
    /// Stamina (0..=1) drained per tick while the player is running or
    /// moving (a costful action under PRD §7.3). Fitness scales the
    /// effective drain downwards through its response curve.
    pub stamina_drain_per_action: f32,
    /// Stamina (0..=1) recovered per tick while idle (PRD §7.3: Fitness
    /// governs drain and recovery).
    pub stamina_recovery_rate: f32,
    /// Multiplier on `stamina_drain_per_action` for a press attempt,
    /// before the Aggression multiplier (ticket 25 wires this to
    /// `Aggression`; placeholder 1.0 here).
    pub press_drain_multiplier: f32,
    /// Multiplier on `stamina_drain_per_action` for a tackle attempt,
    /// before the Aggression multiplier (ticket 25 wires this to
    /// `Aggression`; placeholder 1.0 here).
    pub tackle_drain_multiplier: f32,
}

impl Tuning {
    /// Default tuning assumptions (pre-calibration).
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "constructor is not const due to Resource derive"
    )]
    pub fn default_assumptions() -> Self {
        Self {
            fit_multipliers: FitMultipliers::default_assumptions(),
            curve_params: [
                // Outfield defence.
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // Tackling
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.02, max: 0.70, rising: false },  // Marking
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.02, max: 0.70, rising: false },  // Positioning
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // Heading
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // Bravery
                // Outfield attack.
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.02, max: 0.70, rising: false },  // Passing
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // Dribbling
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // Crossing
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.02, max: 0.70, rising: false },  // Shooting
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // Finishing
                // Physical & mental (outfield).
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.02, max: 0.70, rising: false },  // OutfieldFitness
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // OutfieldStrength
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // OutfieldAggression
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // OutfieldSpeed
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // OutfieldCreativity
                // Goalkeeping.
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // Reflexes
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // Anticipation
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // RushingOut
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // Throwing
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // Kicking
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // Punching
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // AerialReach
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // Communication
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // Agility
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.02, max: 0.70, rising: false },  // Concentration
                // Goalkeeper physical & mental.
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.02, max: 0.70, rising: false },  // GoalkeeperFitness
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // GoalkeeperStrength
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // GoalkeeperAggression
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // GoalkeeperSpeed
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // GoalkeeperCreativity
            ],
            quality_weights: QualityWeights::default_assumptions(),
            tackle_recovery_ticks: 90,
            stamina_drain_per_action: 0.002,
            stamina_recovery_rate: 0.001,
            press_drain_multiplier: 25.0,
            tackle_drain_multiplier: 25.0,
        }
    }
}

impl Default for Tuning {
    fn default() -> Self {
        Self::default_assumptions()
    }
}

/// Position-fit multiplier table (PRD §6.5).
///
/// Multipliers are applied to every attribute when a player occupies
/// a slot that doesn't match any of their trained positions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitMultipliers {
    /// Distance 0 (trained position).
    pub at_distance_0: f32,
    /// Distance 1.
    pub at_distance_1: f32,
    /// Distance ≥ 2.
    pub at_distance_2_or_more: f32,
}

impl FitMultipliers {
    /// Default assumptions per PRD §6.5.
    #[must_use]
    #[allow(clippy::missing_const_for_fn, reason = "constructor is not const due to struct initialization")]
    pub const fn default_assumptions() -> Self {
        Self {
            at_distance_0: 1.00,
            at_distance_1: 0.92,
            at_distance_2_or_more: 0.75,
        }
    }
}

impl Default for FitMultipliers {
    fn default() -> Self {
        Self::default_assumptions()
    }
}

/// Chebyshev distance on the 5×3 position grid.
///
/// The grid (rows back to front, as in PRD §6.5):
/// ```text
/// row 5:            ST
/// row 4:  AML       AMC       AMR
/// row 3:  ML        MC        MR
/// row 2:  DML       DMC       DMR
/// row 1:  DL        DC        DR
/// ```
///
/// Distance 0 = same slot, Distance 1 = adjacent, Distance ≥ 2 = penalty.
#[must_use]
pub const fn chebyshev_distance(slot_1: PositionSlot, slot_2: PositionSlot) -> u8 {
    // Map each slot to (row, col) where rows go back-to-front (1=GK line, 5=front line)
    // and cols go left-to-right (1=left, 2=center, 3=right).
    // GK is handled separately (not on the 13-slot grid).
    let (row1, col1) = slot_to_row_col(slot_1);
    let (row2, col2) = slot_to_row_col(slot_2);

    // Cast to i32 first, compute signed diff, then take unsigned abs
    // Row and column values are 0-5 and 0-3 respectively, so the diff is always < 256
    // and fits in u8 without truncation.
    #[allow(clippy::cast_possible_truncation, reason = "grid coordinates 0-5 fit in u8")]
    let row_diff = (row1 as i32 - row2 as i32).unsigned_abs() as u8;
    #[allow(clippy::cast_possible_truncation, reason = "grid coordinates 0-5 fit in u8")]
    let col_diff = (col1 as i32 - col2 as i32).unsigned_abs() as u8;

    // Use manual max since row_diff.max(col_diff) isn't const-stable in Rust 2024
    if row_diff >= col_diff {
        row_diff
    } else {
        col_diff
    }
}

/// Map a `PositionSlot` to (row, col) for distance calculations.
///
/// Rows: 1=GK line (DL/DC/DR), 2=DML/DMC/DMR, 3=ML/MC/MR, 4=AML/AMC/AMR, 5=ST.
/// Cols: 1=left, 2=center, 3=right. GK returns (0, 0) as a sentinel.
const fn slot_to_row_col(slot: PositionSlot) -> (u8, u8) {
    match slot {
        PositionSlot::GK => (0, 0),
        PositionSlot::DL => (1, 1),
        PositionSlot::DC => (1, 2),
        PositionSlot::DR => (1, 3),
        PositionSlot::DML => (2, 1),
        PositionSlot::DMC => (2, 2),
        PositionSlot::DMR => (2, 3),
        PositionSlot::ML => (3, 1),
        PositionSlot::MC => (3, 2),
        PositionSlot::MR => (3, 3),
        PositionSlot::AML => (4, 1),
        PositionSlot::AMC => (4, 2),
        PositionSlot::AMR => (4, 3),
        PositionSlot::ST => (5, 2),
    }
}

/// Compute the minimum Chebyshev distance from `slot` to any of the
/// player's `trained_positions`.
///
/// Returns `None` if `slot` is `GK` (GK is never on the 13-slot grid),
/// or if `trained_positions` is empty (should be impossible in practice).
///
/// See PRD §6.5 for the grid layout and distance rules.
#[must_use]
pub fn min_fit_distance(slot: PositionSlot, trained_positions: &[PositionSlot]) -> Option<u8> {
    if trained_positions.is_empty() {
        return None;
    }
    if slot == PositionSlot::GK {
        return None;
    }

    // Filter to only outfield positions and compute min distance.
    let mut min_dist: Option<u8> = None;
    for &trained in trained_positions {
        if trained == PositionSlot::GK {
            continue;
        }
        let d = chebyshev_distance(slot, trained);
        min_dist = Some(min_dist.map_or(d, |current_min| current_min.min(d)));
    }
    min_dist
}

/// Compute the fit multiplier for a player in a given slot, based on
/// their trained positions.
///
/// Returns `1.00` for distance 0, `0.92` for distance 1, `0.75` for
/// distance ≥ 2 (PRD §6.5).
///
/// This is the function that feeds both Quality-in-slot and every
/// mechanic downstream (tackle success, pass accuracy, etc.).
#[must_use]
pub fn fit_multiplier(
    slot: PositionSlot,
    trained_positions: &[PositionSlot],
    tuning: &FitMultipliers,
) -> f32 {
    let dist = min_fit_distance(slot, trained_positions).map_or(0, i32::from);

    match dist {
        0 => tuning.at_distance_0,
        1 => tuning.at_distance_1,
        _ => tuning.at_distance_2_or_more,
    }
}

/// Compute the effective attribute value after applying the fit multiplier.
///
/// This is the single helper that every attribute consumer calls to get
/// the player's quality-adjusted attribute for a given slot.
#[must_use]
pub const fn effective_attribute(raw_attribute: u8, multiplier: f32) -> f32 {
    raw_attribute as f32 * multiplier
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests assert exact arithmetic results from integer-literal vector operands"
)]
mod tests {
    use super::*;

    #[test]
    fn test_vec2_operations() {
        let v1 = Vec2::new(1.0, 2.0);
        let v2 = Vec2::new(3.0, 4.0);

        assert_eq!(v1 + v2, Vec2::new(4.0, 6.0));
        assert_eq!(v2 - v1, Vec2::new(2.0, 2.0));
        assert_eq!(v1 * 2.0, Vec2::new(2.0, 4.0));
        assert_eq!(2.0 * v1, Vec2::new(2.0, 4.0));
        assert_eq!(v1.dot(v2), 11.0);
        assert_eq!(v1.length_squared(), 5.0);
    }

    #[test]
    fn test_swept_proximity_static_no_acquire() {
        // Player at (0,0), ball at (5,0), both stationary.
        // Distance is 5m > 1.5m acquisition radius.
        let result = swept_proximity(
            Vec2::new(0.0, 0.0),
            Vec2::zero(),
            Vec2::new(5.0, 0.0),
            Vec2::zero(),
            1.5,
        );
        assert_eq!(result.min_distance, 5.0);
        assert!(!result.acquired);
    }

    #[test]
    fn test_swept_proximity_static_acquire() {
        // Player at (0,0), ball at (1.0, 0), both stationary.
        // Distance is 1.0m < 1.5m acquisition radius.
        let result = swept_proximity(
            Vec2::new(0.0, 0.0),
            Vec2::zero(),
            Vec2::new(1.0, 0.0),
            Vec2::zero(),
            1.5,
        );
        assert_eq!(result.min_distance, 1.0);
        assert!(result.acquired);
    }

    #[test]
    fn test_swept_proximity_player_crosses_ball() {
        // Player moves from (0,0) to (5,0) — crosses the ball at (2.5, 0).
        // Ball stays at (2.5, 0). Minimum distance is 0 at t=0.5.
        let result = swept_proximity(
            Vec2::new(0.0, 0.0), // player start
            Vec2::new(5.0, 0.0), // player velocity: 5m/tick
            Vec2::new(2.5, 0.0), // ball start
            Vec2::zero(),        // ball stationary
            1.5,
        );
        assert!((result.min_distance - 0.0).abs() < 1e-6);
        assert!(result.acquired);
    }

    #[test]
    fn test_swept_proximity_player_misses_ball() {
        // Player moves from (0,0) to (5,0), ball stays at (0, 5).
        // Minimum distance is 5m (player passes 5m away).
        let result = swept_proximity(
            Vec2::new(0.0, 0.0),
            Vec2::new(5.0, 0.0),
            Vec2::new(0.0, 5.0),
            Vec2::zero(),
            1.5,
        );
        assert_eq!(result.min_distance, 5.0);
        assert!(!result.acquired);
    }

    #[test]
    fn test_swept_proximity_both_moving_towards() {
        // Player at (0,0) moving right at 3m/tick, ball at (5,0) moving left at 2m/tick.
        // They approach each other. Meeting point: t=1, position (1,0).
        // Minimum distance is 0 (they meet).
        let result = swept_proximity(
            Vec2::new(0.0, 0.0),
            Vec2::new(3.0, 0.0),
            Vec2::new(5.0, 0.0),
            Vec2::new(-2.0, 0.0),
            1.5,
        );
        assert!((result.min_distance - 0.0).abs() < 1e-6);
        assert!(result.acquired);
    }

    #[test]
    fn test_swept_proximity_ball_moving_past_stationary_player() {
        // Ball moving from (0,0) to (5,0), player stationary at (2.5, 0).
        // Ball passes directly through player at t=0.5.
        let result = swept_proximity(
            Vec2::new(2.5, 0.0),
            Vec2::zero(),
            Vec2::new(0.0, 0.0),
            Vec2::new(5.0, 0.0),
            1.5,
        );
        assert!((result.min_distance - 0.0).abs() < 1e-6);
        assert!(result.acquired);
    }

    #[test]
    fn test_swept_proximity_endpoint_acquire() {
        // Player starts at (1.4, 0) moving to (1.6, 0), ball at (0, 0) stationary.
        // Player's path comes within 1.4m at t=0, which is < 1.5m.
        let result = swept_proximity(
            Vec2::new(1.4, 0.0),
            Vec2::new(0.2, 0.0),
            Vec2::new(0.0, 0.0),
            Vec2::zero(),
            1.5,
        );
        assert!((result.min_distance - 1.4).abs() < 1e-5);
        assert!(result.acquired);
    }

    #[test]
    fn test_swept_proximity_past_endpoint_acquire() {
        // Player starts at (0, 0) moving to (3.0, 0), ball at (0, 1.4) stationary.
        // Minimum distance is 1.4 at t=0 (player passes 1.4m from ball).
        let result = swept_proximity(
            Vec2::new(0.0, 0.0),
            Vec2::new(3.0, 0.0),
            Vec2::new(0.0, 1.4),
            Vec2::zero(),
            1.5,
        );
        assert!((result.min_distance - 1.4).abs() < 1e-6);
        assert!(result.acquired);
    }

    #[test]
    fn test_swept_proximity_past_endpoint_no_acquire() {
        // Player starts at (0, 0) moving to (3.0, 0), ball at (0, 2.0) stationary.
        // Minimum distance is 2.0 at t=0, which is > 1.5m.
        let result = swept_proximity(
            Vec2::new(0.0, 0.0),
            Vec2::new(3.0, 0.0),
            Vec2::new(0.0, 2.0),
            Vec2::zero(),
            1.5,
        );
        assert!((result.min_distance - 2.0).abs() < 1e-6);
        assert!(!result.acquired);
    }

    #[test]
    fn test_swept_proximity_diagonal_motion() {
        // Player moves diagonally from (0,0) to (3,4) (length 5m at angle).
        // Ball at (2.5, 2.0). Closest approach at t=0.62:
        // player at (1.86, 2.48), distance = sqrt(0.64 + 0.2304) = sqrt(0.8704) ≈ 0.933m
        let result = swept_proximity(
            Vec2::new(0.0, 0.0),
            Vec2::new(3.0, 4.0),
            Vec2::new(2.5, 2.0),
            Vec2::zero(),
            1.5,
        );
        assert!((result.min_distance - 0.8).abs() < 1e-4);
        assert!(result.acquired);
    }

    #[test]
    fn test_swept_proximity_inner_endpoint() {
        // Player moves from (0,0) to (5,0), ball moves from (3,0) to (4,0).
        // Relative motion: player starts 3m behind and closes 1m gap.
        // They overlap at t=0.6: player at 3.0, ball at 3.6.
        // Min distance should be 0 (they cross paths).
        let result = swept_proximity(
            Vec2::new(0.0, 0.0),
            Vec2::new(5.0, 0.0),
            Vec2::new(3.0, 0.0),
            Vec2::new(1.0, 0.0),
            1.5,
        );
        assert!(result.min_distance < 1e-5);
        assert!(result.acquired);
    }

    // === PositionSlot and fit distance tests ===

    #[test]
    fn test_position_slot_grid_coordinates() {
        // Verify the (row, col) mapping for the 5×3 grid.
        assert_eq!(slot_to_row_col(PositionSlot::GK), (0, 0)); // Sentinel

        // Row 1 (defensive line)
        assert_eq!(slot_to_row_col(PositionSlot::DL), (1, 1));
        assert_eq!(slot_to_row_col(PositionSlot::DC), (1, 2));
        assert_eq!(slot_to_row_col(PositionSlot::DR), (1, 3));

        // Row 2 (defensive midfield)
        assert_eq!(slot_to_row_col(PositionSlot::DML), (2, 1));
        assert_eq!(slot_to_row_col(PositionSlot::DMC), (2, 2));
        assert_eq!(slot_to_row_col(PositionSlot::DMR), (2, 3));

        // Row 3 (midfield)
        assert_eq!(slot_to_row_col(PositionSlot::ML), (3, 1));
        assert_eq!(slot_to_row_col(PositionSlot::MC), (3, 2));
        assert_eq!(slot_to_row_col(PositionSlot::MR), (3, 3));

        // Row 4 (attacking midfield)
        assert_eq!(slot_to_row_col(PositionSlot::AML), (4, 1));
        assert_eq!(slot_to_row_col(PositionSlot::AMC), (4, 2));
        assert_eq!(slot_to_row_col(PositionSlot::AMR), (4, 3));

        // Row 5 (striker)
        assert_eq!(slot_to_row_col(PositionSlot::ST), (5, 2));
    }

    #[test]
    fn test_chebyshev_distance_same_slot() {
        assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::MC), 0);
        assert_eq!(chebyshev_distance(PositionSlot::ST, PositionSlot::ST), 0);
    }

    #[test]
    fn test_chebyshev_distance_distance_1() {
        // Adjacent cells horizontally
        assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::ML), 1);
        assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::MR), 1);

        // Adjacent cells vertically
        assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::DMC), 1);
        assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::AMC), 1);

        // Diagonal adjacency (MC at row 3 col 2)
        assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::ML), 1); // same row, col diff 1
        assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::MR), 1); // same row, col diff 1
        assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::AMC), 1); // row diff 1, same col
        assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::AMR), 1); // diagonally adjacent: MC=(3,2), AMR=(4,3), diff=(1,1), max=1
    }

    #[test]
    fn test_chebyshev_distance_distance_2() {
        // Two steps horizontally
        assert_eq!(chebyshev_distance(PositionSlot::ML, PositionSlot::MR), 2);

        // Two steps vertically
        assert_eq!(chebyshev_distance(PositionSlot::DMC, PositionSlot::AMC), 2);

        // Diagonal (2,2) -> (3,3) is distance 1, but (1,1) -> (3,3) is distance 2
        assert_eq!(chebyshev_distance(PositionSlot::DL, PositionSlot::MC), 2);
    }

    #[test]
    fn test_chebyshev_distance_gk_sentinel() {
        // GK maps to the off-grid sentinel (0,0), so it must be at non-zero
        // distance from every other slot (in both directions) and at distance
        // 0 from itself.
        assert_eq!(chebyshev_distance(PositionSlot::GK, PositionSlot::GK), 0);
        for slot in [
            PositionSlot::DL, PositionSlot::DC, PositionSlot::DR,
            PositionSlot::DML, PositionSlot::DMC, PositionSlot::DMR,
            PositionSlot::ML, PositionSlot::MC, PositionSlot::MR,
            PositionSlot::AML, PositionSlot::AMC, PositionSlot::AMR,
            PositionSlot::ST,
        ] {
            assert!(chebyshev_distance(PositionSlot::GK, slot) > 0, "GK -> {slot:?}");
            assert_eq!(
                chebyshev_distance(PositionSlot::GK, slot),
                chebyshev_distance(slot, PositionSlot::GK),
                "distance must be symmetric for {slot:?}"
            );
        }
    }

    #[test]
    fn test_min_fit_distance_basic() {
        let tuning = FitMultipliers::default_assumptions();

        // Player trained at MC, playing at MC -> distance 0 -> multiplier 1.00
        let trained = vec![PositionSlot::MC];
        let dist = min_fit_distance(PositionSlot::MC, &trained);
        assert_eq!(dist, Some(0));
        assert_eq!(fit_multiplier(PositionSlot::MC, &trained, &tuning), 1.00);

        // Player trained at MC, playing at ML -> distance 1 -> multiplier 0.92
        let trained = vec![PositionSlot::MC];
        let dist = min_fit_distance(PositionSlot::ML, &trained);
        assert_eq!(dist, Some(1));
        assert_eq!(fit_multiplier(PositionSlot::ML, &trained, &tuning), 0.92);

        // Player trained at MC, playing at DL -> distance 2 -> multiplier 0.75
        let trained = vec![PositionSlot::MC];
        let dist = min_fit_distance(PositionSlot::DL, &trained);
        assert_eq!(dist, Some(2));
        assert_eq!(fit_multiplier(PositionSlot::DL, &trained, &tuning), 0.75);
    }

    #[test]
    fn test_min_fit_distance_multiple_trained_positions() {
        let tuning = FitMultipliers::default_assumptions();

        // Player trained at both MC and MR, playing at ML (closest is MC, distance 1)
        let trained = vec![PositionSlot::MC, PositionSlot::MR];
        let dist = min_fit_distance(PositionSlot::ML, &trained);
        assert_eq!(dist, Some(1));
        assert_eq!(fit_multiplier(PositionSlot::ML, &trained, &tuning), 0.92);

        // Player trained at DC and ST, playing at MC (DC is distance 2, ST is distance 3, so min is 2)
        let trained = vec![PositionSlot::DC, PositionSlot::ST];
        let dist = min_fit_distance(PositionSlot::MC, &trained);
        assert_eq!(dist, Some(2));
        assert_eq!(fit_multiplier(PositionSlot::MC, &trained, &tuning), 0.75);
    }

    #[test]
    fn test_min_fit_distance_gk() {
        // GK slot should return None (not on the 13-slot grid)
        let trained = vec![PositionSlot::GK];
        let dist = min_fit_distance(PositionSlot::GK, &trained);
        assert_eq!(dist, None);

        let trained = vec![PositionSlot::DL, PositionSlot::DC, PositionSlot::DR];
        let dist = min_fit_distance(PositionSlot::GK, &trained);
        assert_eq!(dist, None);
    }

    #[test]
    fn test_min_fit_distance_empty() {
        // Empty trained positions returns None
        let trained: Vec<PositionSlot> = vec![];
        let dist = min_fit_distance(PositionSlot::MC, &trained);
        assert_eq!(dist, None);
    }

    #[test]
    fn test_fit_multiplier_with_custom_tuning() {
        let custom = FitMultipliers {
            at_distance_0: 1.00,
            at_distance_1: 0.85,  // More lenient
            at_distance_2_or_more: 0.60, // More severe
        };

        let trained = vec![PositionSlot::MC];

        assert_eq!(fit_multiplier(PositionSlot::MC, &trained, &custom), 1.00);
        assert_eq!(fit_multiplier(PositionSlot::ML, &trained, &custom), 0.85);
        assert_eq!(fit_multiplier(PositionSlot::DL, &trained, &custom), 0.60);
    }

    #[test]
    fn test_effective_attribute() {
        // Raw attribute 100 with multiplier 1.00 -> 100.0
        assert!((effective_attribute(100, 1.00) - 100.0).abs() < f32::EPSILON);
        // Raw attribute 100 with multiplier 0.92 -> 92.0
        assert!((effective_attribute(100, 0.92) - 92.0).abs() < f32::EPSILON);
        // Raw attribute 100 with multiplier 0.75 -> 75.0
        assert!((effective_attribute(100, 0.75) - 75.0).abs() < f32::EPSILON);
        // Raw attribute 50 with multiplier 0.75 -> 37.5
        assert!((effective_attribute(50, 0.75) - 37.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_tuning_default() {
        let tuning = Tuning::default();
        assert_eq!(tuning.fit_multipliers.at_distance_0, 1.00);
        assert_eq!(tuning.fit_multipliers.at_distance_1, 0.92);
        assert_eq!(tuning.fit_multipliers.at_distance_2_or_more, 0.75);
        assert_eq!(tuning.curve_params.len(), CURVE_PARAM_COUNT);
        assert_eq!(tuning.tackle_recovery_ticks, 90);
        assert!((0.0..=1.0).contains(&tuning.stamina_drain_per_action));
        assert!((0.0..=1.0).contains(&tuning.stamina_recovery_rate));
        assert!(tuning.press_drain_multiplier > 0.0);
        assert!(tuning.tackle_drain_multiplier > 0.0);
    }

    #[test]
    fn test_curve_midpoint_returns_mid_range() {
        let params = CurveParams {
            midpoint: 100.0,
            steepness: 6.0,
            min: 0.0,
            max: 1.0,
            rising: true,
        };
        assert!((curve(100, &params) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_curve_clamps_attribute_to_unit_interval() {
        let params = CurveParams {
            midpoint: 100.0,
            steepness: 6.0,
            min: 0.0,
            max: 1.0,
            rising: true,
        };
        assert!((curve(-50, &params) - curve(0, &params)).abs() < 1e-6);
        assert!((curve(250, &params) - curve(200, &params)).abs() < 1e-6);
    }

    #[test]
    fn test_curve_is_strictly_monotonic_rising() {
        let params = CurveParams {
            midpoint: 100.0,
            steepness: 6.0,
            min: 0.05,
            max: 0.95,
            rising: true,
        };
        let mut prev = curve(0, &params);
        for attr in 1..=200 {
            let value = curve(attr, &params);
            assert!(value > prev, "curve not strictly rising at attribute {attr}");
            prev = value;
        }
    }

    #[test]
    fn test_curve_is_strictly_monotonic_falling() {
        let params = CurveParams {
            midpoint: 100.0,
            steepness: 6.0,
            min: 0.02,
            max: 0.70,
            rising: false,
        };
        let mut prev = curve(0, &params);
        for attr in 1..=200 {
            let value = curve(attr, &params);
            assert!(value < prev, "curve not strictly falling at attribute {attr}");
            prev = value;
        }
    }

    #[test]
    fn test_curve_output_stays_within_min_max() {
        let params = CurveParams {
            midpoint: 100.0,
            steepness: 6.0,
            min: 0.02,
            max: 0.70,
            rising: false,
        };
        for attr in 0..=200 {
            let value = curve(attr, &params);
            assert!((0.02..=0.70).contains(&value), "attribute {attr}: {value}");
        }
    }

    #[test]
    fn test_curve_falling_is_reverse_of_rising() {
        // A falling curve with the same span is the mirror of the rising one.
        let rising = CurveParams {
            midpoint: 100.0,
            steepness: 6.0,
            min: 0.05,
            max: 0.95,
            rising: true,
        };
        let falling = CurveParams {
            midpoint: 100.0,
            steepness: 6.0,
            min: 0.05,
            max: 0.95,
            rising: false,
        };
        for attr in 0..=200 {
            assert!(
                (curve(attr, &rising) + curve(attr, &falling) - 1.0).abs() < 1e-6,
                "attribute {attr}"
            );
        }
    }
}
