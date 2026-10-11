//! Formation 4-4-2 slots and directional home slot calculations.

use bevy_ecs::prelude::Component;
use serde::{Deserialize, Serialize};
use sim_math::{GOAL_Y_CENTER, PITCH_LENGTH, PITCH_WIDTH, Vec2};

use crate::Role;

/// Canonical slots in a 4-4-2 formation.
///
/// Order corresponds to standard lineup order:
/// GK, DL, DC (left), DC (right), DR, ML, MC (left), MC (right), MR, ST (left), ST (right).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FormationSlot {
    GK,
    DL,
    DC1,
    DC2,
    DR,
    ML,
    MC1,
    MC2,
    MR,
    ST1,
    ST2,
}

/// Component storing a player's assigned home slot coordinates.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HomeSlot(pub Vec2);

impl FormationSlot {
    /// The 11 slots of a 4-4-2 formation in canonical order.
    pub const ALL_442: [Self; 11] = [
        Self::GK,
        Self::DL,
        Self::DC1,
        Self::DC2,
        Self::DR,
        Self::ML,
        Self::MC1,
        Self::MC2,
        Self::MR,
        Self::ST1,
        Self::ST2,
    ];

    /// Resolve a 4-4-2 slot by team index (0..11).
    #[must_use]
    pub const fn from_index(index: usize) -> Option<Self> {
        if index < Self::ALL_442.len() {
            Some(Self::ALL_442[index])
        } else {
            None
        }
    }

    /// Return the index (0..11) of this slot in the canonical 4-4-2 order.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::GK => 0,
            Self::DL => 1,
            Self::DC1 => 2,
            Self::DC2 => 3,
            Self::DR => 4,
            Self::ML => 5,
            Self::MC1 => 6,
            Self::MC2 => 7,
            Self::MR => 8,
            Self::ST1 => 9,
            Self::ST2 => 10,
        }
    }

    /// Primary role associated with this slot.
    #[must_use]
    pub const fn role(self) -> Role {
        match self {
            Self::GK => Role::Goalkeeper,
            Self::DL | Self::DR => Role::FullBack,
            Self::DC1 | Self::DC2 => Role::CenterBack,
            Self::ML | Self::MR => Role::Winger,
            Self::MC1 | Self::MC2 => Role::CentralMidfielder,
            Self::ST1 | Self::ST2 => Role::Striker,
        }
    }

    /// Pitch position for this slot given the team's attack direction.
    ///
    /// For a team attacking right (`attack_direction > 0`), the slot is positioned
    /// in the defending half (deepest x for GK near x=5, forwards near x=60).
    /// For a team attacking left (`attack_direction <= 0`), the slot is mirrored
    /// across the pitch center `(PITCH_LENGTH - x, PITCH_WIDTH - y)`.
    #[must_use]
    pub fn position_for(self, attack_direction: i8) -> Vec2 {
        let base = match self {
            Self::GK => Vec2::new(5.0, GOAL_Y_CENTER),
            Self::DL => Vec2::new(20.0, 12.0),
            Self::DC1 => Vec2::new(18.0, 26.0),
            Self::DC2 => Vec2::new(18.0, 42.0),
            Self::DR => Vec2::new(20.0, 56.0),
            Self::ML => Vec2::new(40.0, 12.0),
            Self::MC1 => Vec2::new(38.0, 26.0),
            Self::MC2 => Vec2::new(38.0, 42.0),
            Self::MR => Vec2::new(40.0, 56.0),
            Self::ST1 => Vec2::new(60.0, 27.0),
            Self::ST2 => Vec2::new(60.0, 41.0),
        };
        if attack_direction > 0 {
            base
        } else {
            Vec2::new(PITCH_LENGTH - base.x, PITCH_WIDTH - base.y)
        }
    }
}

/// Lookup slot position for 4-4-2 by slot index and team attack direction.
#[must_use]
pub fn formation_442_slot_position(slot_index: usize, attack_direction: i8) -> Vec2 {
    FormationSlot::from_index(slot_index).map_or_else(
        || Vec2::new(PITCH_LENGTH * 0.5, GOAL_Y_CENTER),
        |s| s.position_for(attack_direction),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_formation_slot_round_trip() {
        for (i, &slot) in FormationSlot::ALL_442.iter().enumerate() {
            assert_eq!(slot.index(), i);
            assert_eq!(FormationSlot::from_index(i), Some(slot));
        }
        assert_eq!(FormationSlot::from_index(11), None);
    }

    #[test]
    fn test_formation_slot_roles() {
        assert_eq!(FormationSlot::GK.role(), Role::Goalkeeper);
        assert_eq!(FormationSlot::DL.role(), Role::FullBack);
        assert_eq!(FormationSlot::DC1.role(), Role::CenterBack);
        assert_eq!(FormationSlot::DC2.role(), Role::CenterBack);
        assert_eq!(FormationSlot::DR.role(), Role::FullBack);
        assert_eq!(FormationSlot::ML.role(), Role::Winger);
        assert_eq!(FormationSlot::MC1.role(), Role::CentralMidfielder);
        assert_eq!(FormationSlot::MC2.role(), Role::CentralMidfielder);
        assert_eq!(FormationSlot::MR.role(), Role::Winger);
        assert_eq!(FormationSlot::ST1.role(), Role::Striker);
        assert_eq!(FormationSlot::ST2.role(), Role::Striker);
    }

    #[test]
    fn test_formation_slot_positions_attack_right() {
        let gk = FormationSlot::GK.position_for(1);
        assert_eq!(gk, Vec2::new(5.0, 34.0));

        let dl = FormationSlot::DL.position_for(1);
        let dr = FormationSlot::DR.position_for(1);
        assert_eq!(dl, Vec2::new(20.0, 12.0));
        assert_eq!(dr, Vec2::new(20.0, 56.0));
        // DL and DR symmetric about center line y=34
        assert!(((dl.y + dr.y).mul_add(0.5, -34.0)).abs() < f32::EPSILON);

        let st1 = FormationSlot::ST1.position_for(1);
        let st2 = FormationSlot::ST2.position_for(1);
        assert_eq!(st1, Vec2::new(60.0, 27.0));
        assert_eq!(st2, Vec2::new(60.0, 41.0));
        assert!(((st1.y + st2.y).mul_add(0.5, -34.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn test_formation_slot_positions_attack_left_is_exact_mirror() {
        for &slot in &FormationSlot::ALL_442 {
            let right = slot.position_for(1);
            let left = slot.position_for(-1);

            // Left pos must be exactly (105 - right.x, 68 - right.y)
            assert!((left.x - (PITCH_LENGTH - right.x)).abs() < f32::EPSILON);
            assert!((left.y - (PITCH_WIDTH - right.y)).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn test_slot_positions_show_spread() {
        // Assert meaningful spread across 4-4-2 slots
        let min_x = FormationSlot::ALL_442
            .iter()
            .map(|s| s.position_for(1).x)
            .fold(f32::INFINITY, f32::min);
        let max_x = FormationSlot::ALL_442
            .iter()
            .map(|s| s.position_for(1).x)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = FormationSlot::ALL_442
            .iter()
            .map(|s| s.position_for(1).y)
            .fold(f32::INFINITY, f32::min);
        let max_y = FormationSlot::ALL_442
            .iter()
            .map(|s| s.position_for(1).y)
            .fold(f32::NEG_INFINITY, f32::max);

        assert!(
            max_x - min_x >= 50.0,
            "x-spread must be at least 50m (GK to ST)"
        );
        assert!(
            max_y - min_y >= 40.0,
            "y-spread must be at least 40m (flank to flank)"
        );
    }
}
