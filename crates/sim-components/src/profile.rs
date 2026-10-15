//! `PlayerProfile`: the 30 attribute slots across the two profiles
//! (PRD §6.1) plus the single read/write accessor.
//!
//! `Outfield` holds defence (5) + attack (5) + physical & mental (5);
//! `Goalkeeper` holds goalkeeping (10) + physical & mental (5). Every
//! read goes through [`PlayerProfile::get`]; the group-struct fields
//! stay private to this module tree so match code cannot bypass it.

use bevy_ecs::prelude::Component;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter, Result as FmtResult};

use super::attribute_groups::{Goalkeeping, OutfieldAttack, OutfieldDefence, PhysicalMental};
use super::attribute::Attribute;
/// Minimum legal attribute value (PRD §5.3).
pub const ATTRIBUTE_MIN: u8 = 1;
/// Maximum legal attribute value (PRD §5.3).
pub const ATTRIBUTE_MAX: u8 = 200;
/// Total attribute slots across both profiles (15 + 15).
pub const ATTRIBUTE_COUNT: usize = 30;

/// Which profile an [`Attribute`] variant belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProfileKind {
    /// Outfield profile attribute.
    Outfield,
    /// Goalkeeper profile attribute.
    Goalkeeper,
}

/// Error from attribute construction or [`PlayerProfile::set`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeError {
    /// Value outside `1..=200`.
    OutOfRange {
        /// The rejected value.
        value: u8,
    },
    /// Attribute does not belong to the profile it was read from or
    /// written to.
    ProfileMismatch,
}

impl Display for AttributeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match *self {
            Self::OutOfRange { value } => {
                write!(f, "attribute value {value} outside 1..=200")
            }
            Self::ProfileMismatch => {
                write!(f, "attribute does not belong to this profile")
            }
        }
    }
}

impl std::error::Error for AttributeError {}

/// Validate a raw attribute value against `1..=200` (PRD §5.3).
pub(crate) const fn check_range(value: u8) -> Result<u8, AttributeError> {
    if value < ATTRIBUTE_MIN || value > ATTRIBUTE_MAX {
        Err(AttributeError::OutOfRange { value })
    } else {
        Ok(value)
    }
}
/// Player profile carrying the 30 attribute slots (PRD §6.1).
///
/// `Outfield` holds defence (5) + attack (5) + physical & mental (5);
/// `Goalkeeper` holds goalkeeping (10) + physical & mental (5).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayerProfile {
    /// Outfield player: 15 attribute slots.
    Outfield {
        /// Defence block (5).
        defence: OutfieldDefence,
        /// Attack block (5).
        attack: OutfieldAttack,
        /// Physical & mental block (5).
        physical_mental: PhysicalMental,
    },
    /// Goalkeeper: 15 attribute slots.
    Goalkeeper {
        /// Goalkeeping block (10).
        goalkeeping: Goalkeeping,
        /// Physical & mental block (5).
        physical_mental: PhysicalMental,
    },
}

impl PlayerProfile {
    /// Default outfield profile (mid-range placeholder until the fixture
    /// ticket supplies real per-player values).
    #[must_use]
    pub const fn default_outfield() -> Self {
        Self::Outfield {
            defence: OutfieldDefence::placeholder(),
            attack: OutfieldAttack::placeholder(),
            physical_mental: PhysicalMental::placeholder(),
        }
    }

    /// Default goalkeeper profile (mid-range placeholder until the
    /// fixture ticket supplies real per-player values).
    #[must_use]
    pub const fn default_goalkeeper() -> Self {
        Self::Goalkeeper {
            goalkeeping: Goalkeeping::placeholder(),
            physical_mental: PhysicalMental::placeholder(),
        }
    }

    /// Which profile kind this value carries.
    #[must_use]
    pub const fn kind(&self) -> ProfileKind {
        match *self {
            Self::Outfield { .. } => ProfileKind::Outfield,
            Self::Goalkeeper { .. } => ProfileKind::Goalkeeper,
        }
    }

    /// The single attribute read path: return the stored value for
    /// `attribute`, or `None` when the variant belongs to the other
    /// profile. All match-code reads go through here; the group-level
    /// `read` helpers are `pub(crate)` routing details.
    #[must_use]
    pub const fn get(&self, attribute: Attribute) -> Option<u8> {
        match self {
            Self::Outfield {
                defence,
                attack,
                physical_mental,
            } => match defence.read(attribute) {
                Some(value) => Some(value),
                None => match attack.read(attribute) {
                    Some(value) => Some(value),
                    None => physical_mental.read_outfield(attribute),
                },
            },
            Self::Goalkeeper {
                goalkeeping,
                physical_mental,
            } => match goalkeeping.read(attribute) {
                Some(value) => Some(value),
                None => physical_mental.read_goalkeeper(attribute),
            },
        }
    }

    /// Write `value` to `attribute` (construction, tests, and
    /// single-attribute stat sweeps). Rejects out-of-range values and
    /// cross-profile writes.
    ///
    /// # Errors
    ///
    /// Returns [`AttributeError::OutOfRange`] when `value` is outside
    /// `1..=200`, or [`AttributeError::ProfileMismatch`] when
    /// `attribute` belongs to the other profile.
    pub const fn set(&mut self, attribute: Attribute, value: u8) -> Result<(), AttributeError> {
        if value < ATTRIBUTE_MIN || value > ATTRIBUTE_MAX {
            return Err(AttributeError::OutOfRange { value });
        }
        let written = match self {
            Self::Outfield {
                defence,
                attack,
                physical_mental,
            } => {
                defence.write(attribute, value)
                    || attack.write(attribute, value)
                    || physical_mental.write_outfield(attribute, value)
            }
            Self::Goalkeeper {
                goalkeeping,
                physical_mental,
            } => {
                goalkeeping.write(attribute, value)
                    || physical_mental.write_goalkeeper(attribute, value)
            }
        };
        if written {
            Ok(())
        } else {
            Err(AttributeError::ProfileMismatch)
        }
    }

    /// Build the default profile of `attribute`'s kind with that single
    /// attribute set to `value`. The seam for stat-strength sweeps
    /// (`A10`): one attribute varies in isolation.
    ///
    /// # Panics
    ///
    /// Panics when `value` is outside `1..=200`, which would be a test or
    /// sweep programming error (the value is a literal at every call site).
    #[must_use]
    pub fn with_attribute(attribute: Attribute, value: u8) -> Self {
        let mut profile = match attribute.profile_kind() {
            ProfileKind::Outfield => Self::default_outfield(),
            ProfileKind::Goalkeeper => Self::default_goalkeeper(),
        };
        profile
            .set(attribute, value)
            .expect("invariant: sweep values are in range and match the profile");
        profile
    }
}
