//! `sim-math`: mathematical primitives for the football-sim-server
//! workspace.
//!
//! Public surface: `Vec2` and arithmetic operators, pitch geometry
//! constants and shapes, position slots and fit-distance calculations,
//! response curves, and the `Tuning` configuration.

pub mod curve;
pub mod pitch;
pub mod slots;
pub mod swept;
pub mod tuning;
pub mod vec2;

pub use curve::{
    CurveParams, QualityGroup, QUALITY_GROUP_COUNT, QualityWeights, curve, CURVE_PARAM_COUNT,
};
pub use pitch::{Circle, PitchDimensions, Rect, GOAL_Y_CENTER, PITCH_LENGTH, PITCH_WIDTH};
pub use pitch::goal_position_for;
pub use slots::{
    PositionSlot, chebyshev_distance, effective_attribute, fit_multiplier, min_fit_distance,
};
pub use swept::{SweptProximity, swept_proximity};
pub use tuning::{FitMultipliers, Tuning};
pub use vec2::Vec2;

#[cfg(test)]
mod tests;
