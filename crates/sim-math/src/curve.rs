//! Logistic response curves and quality weights.

use serde::{Deserialize, Serialize};

/// Number of `CurveParams` records in [`Tuning::curve_params`]: one per
/// consumed attribute. Mirrors `sim_components::profile::ATTRIBUTE_COUNT`
/// (30); it is defined here because `sim-math` cannot depend on
/// `sim-components`. The cross-crate consistency is asserted by a test in
/// `sim-components`.
#[allow(clippy::too_long_first_doc_paragraph, reason = "doc paragraph explains cross-crate contract")]
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
