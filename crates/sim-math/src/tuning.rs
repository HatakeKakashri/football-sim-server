//! Tuning configuration for match mechanics and response curves.

use bevy_ecs::prelude::Resource;
use crate::curve::{CurveParams, QualityWeights};

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
    pub curve_params: [CurveParams; crate::curve::CURVE_PARAM_COUNT],
    /// Per-position-group quality weights (PRD §6.4). Used only by
    /// reporting / display code; the match simulation never reads them.
    pub quality_weights: QualityWeights,
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
                // Outfield attack - CALIBRATED: encourage forward play.
                CurveParams { midpoint: 90.0, steepness: 8.0, min: 0.05, max: 0.90, rising: false },   // Passing (easier, steeper)
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // Dribbling
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // Crossing
                CurveParams { midpoint: 95.0, steepness: 8.0, min: 0.05, max: 0.90, rising: false },   // Shooting (easier, steeper)
                CurveParams { midpoint: 95.0, steepness: 8.0, min: 0.05, max: 0.90, rising: true },    // Finishing (easier, steeper)
                // Physical & mental (outfield) - CALIBRATED: encourage forward play.
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.02, max: 0.70, rising: false },  // OutfieldFitness
                CurveParams { midpoint: 100.0, steepness: 6.0, min: 0.05, max: 0.95, rising: true },   // OutfieldStrength
                CurveParams { midpoint: 95.0, steepness: 8.0, min: 0.05, max: 0.95, rising: true },    // OutfieldAggression (more aggressive)
                CurveParams { midpoint: 95.0, steepness: 8.0, min: 0.05, max: 0.95, rising: true },    // OutfieldSpeed (faster)
                CurveParams { midpoint: 90.0, steepness: 8.0, min: 0.05, max: 0.95, rising: true },    // OutfieldCreativity (more creative)
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
