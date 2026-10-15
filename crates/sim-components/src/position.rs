//! The 14 football positions and fit distance computation (PRD §6.5).
//!
//! The 14 positions are: `GK, DL, DC, DR, DML, DMC, DMR, ML, MC, MR, AML, AMC, AMR, ST`.
//! The fit distance is the minimum Chebyshev distance on the 5×3 grid from the slot
//! to any of the player's trained positions.
//!
//! # Naming
//!
//! The 14-position enum is named `PosSlot` in this crate to avoid collision with the
//! `Position(pub Vec2)` component used for player locations. In the PRD and when
//! referring to the concept, use "position" or "slot" interchangeably.
//!
//! The underlying `sim_math::PositionSlot` is used internally.

#![allow(
    clippy::float_cmp,
    reason = "tests compare freshly-constructed float values; equality is exact"
)]
#![allow(
    clippy::match_same_arms,
    reason = "tests validate distinct outcomes for GK vs outfield profiles"
)]
#![allow(
    clippy::uninlined_format_args,
    reason = "tests use debug format for test error messages"
)]
#![allow(
    clippy::double_must_use,
    reason = "test functions may have #[must_use] return types for type safety"
)]
#![allow(
    clippy::missing_const_for_fn,
    reason = "function uses #[must_use] and #[allow(clippy::missing_const_for_fn)]"
)]

use serde::{Deserialize, Serialize};
use sim_math::{fit_multiplier as math_fit_multiplier, min_fit_distance as math_min_fit_distance, PositionSlot, Tuning};
use std::fmt::{Display, Formatter, Result as FmtResult};

use crate::profile::PlayerProfile;

/// The 14 football positions (PRD §6.5).
///
/// This wraps `sim_math::PositionSlot` for convenience in `sim-components`.
/// The `GK` slot is excluded from the fit grid.
///
/// Note: This is distinct from the `Position(pub Vec2)` component which
/// stores a player's current location on the pitch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PosSlot {
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

impl PosSlot {
    /// Convert to the underlying `sim_math::PositionSlot`.
    #[must_use]
    pub const fn to_sim_math(self) -> PositionSlot {
        match self {
            Self::GK => PositionSlot::GK,
            Self::DL => PositionSlot::DL,
            Self::DC => PositionSlot::DC,
            Self::DR => PositionSlot::DR,
            Self::DML => PositionSlot::DML,
            Self::DMC => PositionSlot::DMC,
            Self::DMR => PositionSlot::DMR,
            Self::ML => PositionSlot::ML,
            Self::MC => PositionSlot::MC,
            Self::MR => PositionSlot::MR,
            Self::AML => PositionSlot::AML,
            Self::AMC => PositionSlot::AMC,
            Self::AMR => PositionSlot::AMR,
            Self::ST => PositionSlot::ST,
        }
    }

    /// Convert from `sim_math::PositionSlot`.
    #[must_use]
    pub const fn from_sim_math(slot: PositionSlot) -> Option<Self> {
        match slot {
            PositionSlot::GK => Some(Self::GK),
            PositionSlot::DL => Some(Self::DL),
            PositionSlot::DC => Some(Self::DC),
            PositionSlot::DR => Some(Self::DR),
            PositionSlot::DML => Some(Self::DML),
            PositionSlot::DMC => Some(Self::DMC),
            PositionSlot::DMR => Some(Self::DMR),
            PositionSlot::ML => Some(Self::ML),
            PositionSlot::MC => Some(Self::MC),
            PositionSlot::MR => Some(Self::MR),
            PositionSlot::AML => Some(Self::AML),
            PositionSlot::AMC => Some(Self::AMC),
            PositionSlot::AMR => Some(Self::AMR),
            PositionSlot::ST => Some(Self::ST),
        }
    }

    /// All 14 positions in canonical order (GK first, then 13 outfield).
    pub const ALL: [Self; 14] = [
        Self::GK,
        Self::DL,
        Self::DC,
        Self::DR,
        Self::DML,
        Self::DMC,
        Self::DMR,
        Self::ML,
        Self::MC,
        Self::MR,
        Self::AML,
        Self::AMC,
        Self::AMR,
        Self::ST,
    ];

    /// All 13 outfield positions (excludes GK).
    pub const OUTFIELD: [Self; 13] = [
        Self::DL,
        Self::DC,
        Self::DR,
        Self::DML,
        Self::DMC,
        Self::DMR,
        Self::ML,
        Self::MC,
        Self::MR,
        Self::AML,
        Self::AMC,
        Self::AMR,
        Self::ST,
    ];

    /// The 5×3 grid representation of the 13 outfield positions.
    ///
    /// Grid layout (rows back to front, as in PRD §6.5):
    /// ```text
    /// row 5:            ST
    /// row 4:  AML       AMC       AMR
    /// row 3:  ML        MC        MR
    /// row 2:  DML       DMC       DMR
    /// row 1:  DL        DC        DR
    /// ```
    #[must_use]
    pub const fn grid_coords(pos: Self) -> (u8, u8) {
        // (row, col) where rows go back-to-front (1=GK line, 5=front line)
        // and cols go left-to-right (1=left, 2=center, 3=right).
        match pos {
            Self::GK => (0, 0), // Sentinel (not on grid)
            Self::DL => (1, 1),
            Self::DC => (1, 2),
            Self::DR => (1, 3),
            Self::DML => (2, 1),
            Self::DMC => (2, 2),
            Self::DMR => (2, 3),
            Self::ML => (3, 1),
            Self::MC => (3, 2),
            Self::MR => (3, 3),
            Self::AML => (4, 1),
            Self::AMC => (4, 2),
            Self::AMR => (4, 3),
            Self::ST => (5, 2),
        }
    }

    /// The row (1-5) of this position in the grid. GK returns 0.
    #[must_use]
    pub const fn row(self) -> u8 {
        Self::grid_coords(self).0
    }

    /// The column (1-3) of this position in the grid.
    #[must_use]
    pub const fn col(self) -> u8 {
        Self::grid_coords(self).1
    }

    /// Is this an outfield position (not GK)?
    #[must_use]
    pub const fn is_outfield(self) -> bool {
        !matches!(self, Self::GK)
    }
}

impl Display for PosSlot {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::GK => write!(f, "GK"),
            Self::DL => write!(f, "DL"),
            Self::DC => write!(f, "DC"),
            Self::DR => write!(f, "DR"),
            Self::DML => write!(f, "DML"),
            Self::DMC => write!(f, "DMC"),
            Self::DMR => write!(f, "DMR"),
            Self::ML => write!(f, "ML"),
            Self::MC => write!(f, "MC"),
            Self::MR => write!(f, "MR"),
            Self::AML => write!(f, "AML"),
            Self::AMC => write!(f, "AMC"),
            Self::AMR => write!(f, "AMR"),
            Self::ST => write!(f, "ST"),
        }
    }
}

/// Error from position-related operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionError {
    /// The `GK` slot must hold a goalkeeper profile.
    GKMustBeGoalkeeper,
    /// Outfield slots must hold outfield profiles.
    OutfieldSlotNeedsOutfieldProfile,
}

impl Display for PositionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::GKMustBeGoalkeeper => {
                write!(f, "GK slot must hold a goalkeeper profile")
            }
            Self::OutfieldSlotNeedsOutfieldProfile => {
                write!(f, "outfield slot must hold an outfield profile")
            }
        }
    }
}

impl std::error::Error for PositionError {}

/// Validate that the player's profile matches the slot type.
///
/// # Errors
///
/// Returns [`PositionError::GKMustBeGoalkeeper`] if the `GK` slot holds an
/// outfield profile, or [`PositionError::OutfieldSlotNeedsOutfieldProfile`]
/// if an outfield slot holds a goalkeeper profile.
#[must_use]
pub fn validate_profile_for_slot(slot: PosSlot, profile: &PlayerProfile) -> Result<(), PositionError> {
    match (slot, profile) {
        (PosSlot::GK, PlayerProfile::Goalkeeper { .. }) => Ok(()),
        (PosSlot::GK, PlayerProfile::Outfield { .. }) => Err(PositionError::GKMustBeGoalkeeper),
        (PosSlot::DL, PlayerProfile::Outfield { .. })
        | (PosSlot::DC, PlayerProfile::Outfield { .. })
        | (PosSlot::DR, PlayerProfile::Outfield { .. })
        | (PosSlot::DML, PlayerProfile::Outfield { .. })
        | (PosSlot::DMC, PlayerProfile::Outfield { .. })
        | (PosSlot::DMR, PlayerProfile::Outfield { .. })
        | (PosSlot::ML, PlayerProfile::Outfield { .. })
        | (PosSlot::MC, PlayerProfile::Outfield { .. })
        | (PosSlot::MR, PlayerProfile::Outfield { .. })
        | (PosSlot::AML, PlayerProfile::Outfield { .. })
        | (PosSlot::AMC, PlayerProfile::Outfield { .. })
        | (PosSlot::AMR, PlayerProfile::Outfield { .. })
        | (PosSlot::ST, PlayerProfile::Outfield { .. }) => Ok(()),
        (PosSlot::DL, PlayerProfile::Goalkeeper { .. })
        | (PosSlot::DC, PlayerProfile::Goalkeeper { .. })
        | (PosSlot::DR, PlayerProfile::Goalkeeper { .. })
        | (PosSlot::DML, PlayerProfile::Goalkeeper { .. })
        | (PosSlot::DMC, PlayerProfile::Goalkeeper { .. })
        | (PosSlot::DMR, PlayerProfile::Goalkeeper { .. })
        | (PosSlot::ML, PlayerProfile::Goalkeeper { .. })
        | (PosSlot::MC, PlayerProfile::Goalkeeper { .. })
        | (PosSlot::MR, PlayerProfile::Goalkeeper { .. })
        | (PosSlot::AML, PlayerProfile::Goalkeeper { .. })
        | (PosSlot::AMC, PlayerProfile::Goalkeeper { .. })
        | (PosSlot::AMR, PlayerProfile::Goalkeeper { .. })
        | (PosSlot::ST, PlayerProfile::Goalkeeper { .. }) => {
            Err(PositionError::OutfieldSlotNeedsOutfieldProfile)
        }
    }
}

/// Compute the minimum Chebyshev distance from `slot` to any of the
/// player's trained positions.
///
/// Returns `None` if `slot` is `GK` (GK is never on the 13-slot grid),
/// or if `trained_positions` is empty (should be impossible in practice).
#[must_use]
pub fn min_fit_distance(slot: PosSlot, trained_positions: &[PosSlot]) -> Option<u8> {
    if trained_positions.is_empty() {
        return None;
    }
    if slot == PosSlot::GK {
        return None;
    }

    // Convert to sim_math::PositionSlot and filter GK out
    let slot_slot = slot.to_sim_math();
    let trained_slots: Vec<PositionSlot> = trained_positions
        .iter()
        .filter(|p| *p != &PosSlot::GK)
        .map(|p| p.to_sim_math())
        .collect();

    math_min_fit_distance(slot_slot, &trained_slots)
}

/// Compute the fit multiplier for a player in a given slot, based on
/// their trained positions and the current tuning.
///
/// Returns `1.00` for distance 0, `0.92` for distance 1, `0.75` for
/// distance ≥ 2 (PRD §6.5).
#[must_use]
pub fn fit_multiplier(
    slot: PosSlot,
    trained_positions: &[PosSlot],
    tuning: &Tuning,
) -> f32 {
    if slot == PosSlot::GK {
        // GK has no fit penalty (only GK profiles go in the GK slot).
        return 1.00;
    }

    let slot_slot = slot.to_sim_math();
    let trained_slots: Vec<PositionSlot> = trained_positions
        .iter()
        .filter(|p| *p != &PosSlot::GK)
        .map(|p| p.to_sim_math())
        .collect();

    math_fit_multiplier(slot_slot, &trained_slots, &tuning.fit_multipliers)
}

/// Compute the effective attribute value after applying the fit multiplier.
///
/// This is the single helper that every attribute consumer calls to get
/// the player's quality-adjusted attribute for a given slot.
#[must_use]
pub const fn effective_attribute(raw_attribute: u8, multiplier: f32) -> f32 {
    raw_attribute as f32 * multiplier
}

/// Compute the effective attribute value by looking up the raw attribute
/// from the player's profile and applying the fit multiplier.
#[must_use]
pub fn effective_attribute_from_profile(
    profile: &PlayerProfile,
    attribute: crate::Attribute,
    slot: PosSlot,
    trained_positions: &[PosSlot],
    tuning: &Tuning,
) -> f32 {
    // Get the raw attribute value from the profile
    let raw_value = profile.get(attribute).unwrap_or(0);

    // Compute the fit multiplier
    let multiplier = fit_multiplier(slot, trained_positions, tuning);

    // Apply the multiplier
    effective_attribute(raw_value, multiplier)
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests compare freshly-constructed float values; equality is exact"
)]
mod tests {
    use super::*;

    #[test]
    fn test_pos_slot_to_sim_math_round_trip() {
        for &pos in &PosSlot::ALL {
            let slot = pos.to_sim_math();
            let recovered = PosSlot::from_sim_math(slot);
            assert_eq!(recovered, Some(pos), "round trip failed for {:?}", pos);
        }
    }

    #[test]
    fn test_pos_slot_grid_coords() {
        // Verify the grid coordinates match the specification
        assert_eq!(PosSlot::grid_coords(PosSlot::DL), (1, 1));
        assert_eq!(PosSlot::grid_coords(PosSlot::DC), (1, 2));
        assert_eq!(PosSlot::grid_coords(PosSlot::DR), (1, 3));
        assert_eq!(PosSlot::grid_coords(PosSlot::DML), (2, 1));
        assert_eq!(PosSlot::grid_coords(PosSlot::DMC), (2, 2));
        assert_eq!(PosSlot::grid_coords(PosSlot::DMR), (2, 3));
        assert_eq!(PosSlot::grid_coords(PosSlot::ML), (3, 1));
        assert_eq!(PosSlot::grid_coords(PosSlot::MC), (3, 2));
        assert_eq!(PosSlot::grid_coords(PosSlot::MR), (3, 3));
        assert_eq!(PosSlot::grid_coords(PosSlot::AML), (4, 1));
        assert_eq!(PosSlot::grid_coords(PosSlot::AMC), (4, 2));
        assert_eq!(PosSlot::grid_coords(PosSlot::AMR), (4, 3));
        assert_eq!(PosSlot::grid_coords(PosSlot::ST), (5, 2));
    }

    #[test]
    fn test_pos_slot_row_col() {
        assert_eq!(PosSlot::row(PosSlot::DL), 1);
        assert_eq!(PosSlot::col(PosSlot::DL), 1);
        assert_eq!(PosSlot::row(PosSlot::ST), 5);
        assert_eq!(PosSlot::col(PosSlot::ST), 2);
        assert_eq!(PosSlot::row(PosSlot::GK), 0);
        assert_eq!(PosSlot::col(PosSlot::GK), 0);
    }

    #[test]
    fn test_pos_slot_is_outfield() {
        assert!(PosSlot::DL.is_outfield());
        assert!(PosSlot::ST.is_outfield());
        assert!(PosSlot::MC.is_outfield());
        assert!(!PosSlot::GK.is_outfield());
    }

    #[test]
    fn test_min_fit_distance_same_slot() {
        let trained = vec![PosSlot::MC];
        let dist = min_fit_distance(PosSlot::MC, &trained);
        assert_eq!(dist, Some(0));
    }

    #[test]
    fn test_min_fit_distance_distance_1() {
        let trained = vec![PosSlot::MC];
        // ML is adjacent to MC
        let dist = min_fit_distance(PosSlot::ML, &trained);
        assert_eq!(dist, Some(1));
    }

    #[test]
    fn test_min_fit_distance_distance_2() {
        let trained = vec![PosSlot::MC];
        // DL is two steps from MC
        let dist = min_fit_distance(PosSlot::DL, &trained);
        assert_eq!(dist, Some(2));
    }

    #[test]
    fn test_min_fit_distance_gk() {
        let trained = vec![PosSlot::DL, PosSlot::DC, PosSlot::DR];
        // GK slot returns None
        let dist = min_fit_distance(PosSlot::GK, &trained);
        assert_eq!(dist, None);
    }

    #[test]
    fn test_min_fit_distance_multiple_trained() {
        let trained = vec![PosSlot::MC, PosSlot::MR];
        // ML is closer to MC (distance 1) than MR (distance 2)
        let dist = min_fit_distance(PosSlot::ML, &trained);
        assert_eq!(dist, Some(1));
    }

    #[test]
    fn test_fit_multiplier() {
        let tuning = Tuning::default();

        // Same position -> 1.00
        let trained = vec![PosSlot::MC];
        assert_eq!(fit_multiplier(PosSlot::MC, &trained, &tuning), 1.00);

        // Distance 1 -> 0.92
        let trained = vec![PosSlot::MC];
        assert_eq!(fit_multiplier(PosSlot::ML, &trained, &tuning), 0.92);

        // Distance 2 -> 0.75
        let trained = vec![PosSlot::MC];
        assert_eq!(fit_multiplier(PosSlot::DL, &trained, &tuning), 0.75);
    }

    #[test]
    fn test_fit_multiplier_gk() {
        let tuning = Tuning::default();
        // GK always gets 1.00 (only GK profiles in GK slot)
        let trained = vec![PosSlot::GK];
        assert_eq!(fit_multiplier(PosSlot::GK, &trained, &tuning), 1.00);
    }

    #[test]
    fn test_effective_attribute() {
        assert!((effective_attribute(100, 1.00) - 100.0).abs() < f32::EPSILON);
        assert!((effective_attribute(100, 0.92) - 92.0).abs() < f32::EPSILON);
        assert!((effective_attribute(100, 0.75) - 75.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_validate_profile_for_slot_gk() {
        let profile_gk = PlayerProfile::default_goalkeeper();
        let profile_of = PlayerProfile::default_outfield();

        assert!(validate_profile_for_slot(PosSlot::GK, &profile_gk).is_ok());
        assert_eq!(
            validate_profile_for_slot(PosSlot::GK, &profile_of),
            Err(PositionError::GKMustBeGoalkeeper)
        );
    }

    #[test]
    fn test_validate_profile_for_slot_outfield() {
        let profile_gk = PlayerProfile::default_goalkeeper();
        let profile_of = PlayerProfile::default_outfield();

        for &pos in &PosSlot::OUTFIELD {
            assert!(validate_profile_for_slot(pos, &profile_of).is_ok());
            assert_eq!(
                validate_profile_for_slot(pos, &profile_gk),
                Err(PositionError::OutfieldSlotNeedsOutfieldProfile)
            );
        }
    }
}
