//! Position slots and fit distance calculations (PRD §6.5).

use serde::{Deserialize, Serialize};
use crate::tuning::FitMultipliers;

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
pub(crate) const fn slot_to_row_col(slot: PositionSlot) -> (u8, u8) {
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
