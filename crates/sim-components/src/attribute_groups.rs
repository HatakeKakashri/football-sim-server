//! Attribute group structs: the validated storage blocks behind
//! `PlayerProfile` (PRD §6.1).
//!
//! Fields are private; construction goes through `new` (range-checked
//! `1..=200`) and reads go through `PlayerProfile::get`, so no match
//! code can name a field directly.

use serde::{Deserialize, Serialize};

use super::attribute::Attribute;
use super::profile::{AttributeError, check_range};
/// Outfield defence attributes (5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OutfieldDefence {
    tackling: u8,
    marking: u8,
    positioning: u8,
    heading: u8,
    bravery: u8,
}

impl OutfieldDefence {
    /// Build with validation (`1..=200` per field).
    ///
    /// # Errors
    ///
    /// Returns [`AttributeError::OutOfRange`] when any field is outside
    /// `1..=200`.
    pub const fn new(
        tackling: u8,
        marking: u8,
        positioning: u8,
        heading: u8,
        bravery: u8,
    ) -> Result<Self, AttributeError> {
        match (
            check_range(tackling),
            check_range(marking),
            check_range(positioning),
            check_range(heading),
            check_range(bravery),
        ) {
            (Ok(tackling), Ok(marking), Ok(positioning), Ok(heading), Ok(bravery)) => Ok(Self {
                tackling,
                marking,
                positioning,
                heading,
                bravery,
            }),
            (Err(e), _, _, _, _)
            | (_, Err(e), _, _, _)
            | (_, _, Err(e), _, _)
            | (_, _, _, Err(e), _)
            | (_, _, _, _, Err(e)) => Err(e),
        }
    }

    /// Mid-range placeholder block for [`PlayerProfile::default_outfield`].
    /// `pub(crate)` so defaults stay beside the validated constructor.
    pub(crate) const fn placeholder() -> Self {
        Self {
            tackling: 70,
            marking: 70,
            positioning: 70,
            heading: 70,
            bravery: 70,
        }
    }

    /// Read one defence slot through the accessor. Returns `None` for
    /// variants outside this block. `pub(crate)` so only
    /// `PlayerProfile::get` routes here.
    pub(crate) const fn read(self, attribute: Attribute) -> Option<u8> {
        match attribute {
            Attribute::Tackling => Some(self.tackling),
            Attribute::Marking => Some(self.marking),
            Attribute::Positioning => Some(self.positioning),
            Attribute::Heading => Some(self.heading),
            Attribute::Bravery => Some(self.bravery),
            _ => None,
        }
    }

    /// Write one defence slot through the accessor. Returns whether the
    /// variant belonged to this block. `pub(crate)` so only
    /// `PlayerProfile::set` routes here.
    pub(crate) const fn write(&mut self, attribute: Attribute, value: u8) -> bool {
        match attribute {
            Attribute::Tackling => self.tackling = value,
            Attribute::Marking => self.marking = value,
            Attribute::Positioning => self.positioning = value,
            Attribute::Heading => self.heading = value,
            Attribute::Bravery => self.bravery = value,
            _ => return false,
        }
        true
    }
}

/// Outfield attack attributes (5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OutfieldAttack {
    passing: u8,
    dribbling: u8,
    crossing: u8,
    shooting: u8,
    finishing: u8,
}

impl OutfieldAttack {
    /// Build with validation (`1..=200` per field).
    ///
    /// # Errors
    ///
    /// Returns [`AttributeError::OutOfRange`] when any field is outside
    /// `1..=200`.
    pub const fn new(
        passing: u8,
        dribbling: u8,
        crossing: u8,
        shooting: u8,
        finishing: u8,
    ) -> Result<Self, AttributeError> {
        match (
            check_range(passing),
            check_range(dribbling),
            check_range(crossing),
            check_range(shooting),
            check_range(finishing),
        ) {
            (Ok(passing), Ok(dribbling), Ok(crossing), Ok(shooting), Ok(finishing)) => Ok(Self {
                passing,
                dribbling,
                crossing,
                shooting,
                finishing,
            }),
            (Err(e), _, _, _, _)
            | (_, Err(e), _, _, _)
            | (_, _, Err(e), _, _)
            | (_, _, _, Err(e), _)
            | (_, _, _, _, Err(e)) => Err(e),
        }
    }

    /// Mid-range placeholder block for [`PlayerProfile::default_outfield`].
    /// `pub(crate)` so defaults stay beside the validated constructor.
    pub(crate) const fn placeholder() -> Self {
        Self {
            passing: 70,
            dribbling: 70,
            crossing: 70,
            shooting: 70,
            finishing: 70,
        }
    }

    /// Read one attack slot through the accessor. Returns `None` for
    /// variants outside this block. `pub(crate)` so only
    /// `PlayerProfile::get` routes here.
    pub(crate) const fn read(self, attribute: Attribute) -> Option<u8> {
        match attribute {
            Attribute::Passing => Some(self.passing),
            Attribute::Dribbling => Some(self.dribbling),
            Attribute::Crossing => Some(self.crossing),
            Attribute::Shooting => Some(self.shooting),
            Attribute::Finishing => Some(self.finishing),
            _ => None,
        }
    }

    /// Write one attack slot through the accessor. Returns whether the
    /// variant belonged to this block. `pub(crate)` so only
    /// `PlayerProfile::set` routes here.
    pub(crate) const fn write(&mut self, attribute: Attribute, value: u8) -> bool {
        match attribute {
            Attribute::Passing => self.passing = value,
            Attribute::Dribbling => self.dribbling = value,
            Attribute::Crossing => self.crossing = value,
            Attribute::Shooting => self.shooting = value,
            Attribute::Finishing => self.finishing = value,
            _ => return false,
        }
        true
    }
}

/// Physical & mental attributes (5), shared by both profiles (PRD §6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhysicalMental {
    fitness: u8,
    strength: u8,
    aggression: u8,
    speed: u8,
    creativity: u8,
}

impl PhysicalMental {
    /// Build with validation (`1..=200` per field).
    ///
    /// # Errors
    ///
    /// Returns [`AttributeError::OutOfRange`] when any field is outside
    /// `1..=200`.
    pub const fn new(
        fitness: u8,
        strength: u8,
        aggression: u8,
        speed: u8,
        creativity: u8,
    ) -> Result<Self, AttributeError> {
        match (
            check_range(fitness),
            check_range(strength),
            check_range(aggression),
            check_range(speed),
            check_range(creativity),
        ) {
            (Ok(fitness), Ok(strength), Ok(aggression), Ok(speed), Ok(creativity)) => Ok(Self {
                fitness,
                strength,
                aggression,
                speed,
                creativity,
            }),
            (Err(e), _, _, _, _)
            | (_, Err(e), _, _, _)
            | (_, _, Err(e), _, _)
            | (_, _, _, Err(e), _)
            | (_, _, _, _, Err(e)) => Err(e),
        }
    }

    /// Mid-range placeholder block shared by both profile defaults.
    /// `pub(crate)` so defaults stay beside the validated constructor.
    pub(crate) const fn placeholder() -> Self {
        Self {
            fitness: 75,
            strength: 75,
            aggression: 75,
            speed: 75,
            creativity: 75,
        }
    }

    /// Read one outfield physical & mental slot. Returns `None` for any
    /// other variant, including the goalkeeper physical variants.
    /// `pub(crate)` so only `PlayerProfile::get` routes here.
    pub(crate) const fn read_outfield(self, attribute: Attribute) -> Option<u8> {
        match attribute {
            Attribute::OutfieldFitness => Some(self.fitness),
            Attribute::OutfieldStrength => Some(self.strength),
            Attribute::OutfieldAggression => Some(self.aggression),
            Attribute::OutfieldSpeed => Some(self.speed),
            Attribute::OutfieldCreativity => Some(self.creativity),
            _ => None,
        }
    }

    /// Write one outfield physical & mental slot. Returns whether the
    /// variant belonged to the outfield block. `pub(crate)` so only
    /// `PlayerProfile::set` routes here.
    pub(crate) const fn write_outfield(&mut self, attribute: Attribute, value: u8) -> bool {
        match attribute {
            Attribute::OutfieldFitness => self.fitness = value,
            Attribute::OutfieldStrength => self.strength = value,
            Attribute::OutfieldAggression => self.aggression = value,
            Attribute::OutfieldSpeed => self.speed = value,
            Attribute::OutfieldCreativity => self.creativity = value,
            _ => return false,
        }
        true
    }

    /// Read one goalkeeper physical & mental slot. Returns `None` for any
    /// other variant, including the outfield physical variants.
    /// `pub(crate)` so only `PlayerProfile::get` routes here.
    pub(crate) const fn read_goalkeeper(self, attribute: Attribute) -> Option<u8> {
        match attribute {
            Attribute::GoalkeeperFitness => Some(self.fitness),
            Attribute::GoalkeeperStrength => Some(self.strength),
            Attribute::GoalkeeperAggression => Some(self.aggression),
            Attribute::GoalkeeperSpeed => Some(self.speed),
            Attribute::GoalkeeperCreativity => Some(self.creativity),
            _ => None,
        }
    }

    /// Write one goalkeeper physical & mental slot. Returns whether the
    /// variant belonged to the goalkeeper block. `pub(crate)` so only
    /// `PlayerProfile::set` routes here.
    pub(crate) const fn write_goalkeeper(&mut self, attribute: Attribute, value: u8) -> bool {
        match attribute {
            Attribute::GoalkeeperFitness => self.fitness = value,
            Attribute::GoalkeeperStrength => self.strength = value,
            Attribute::GoalkeeperAggression => self.aggression = value,
            Attribute::GoalkeeperSpeed => self.speed = value,
            Attribute::GoalkeeperCreativity => self.creativity = value,
            _ => return false,
        }
        true
    }
}

/// Goalkeeping attributes (10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Goalkeeping {
    reflexes: u8,
    anticipation: u8,
    rushing_out: u8,
    throwing: u8,
    kicking: u8,
    punching: u8,
    aerial_reach: u8,
    communication: u8,
    agility: u8,
    concentration: u8,
}

impl Goalkeeping {
    /// Build with validation (`1..=200` per field).
    ///
    /// # Errors
    ///
    /// Returns [`AttributeError::OutOfRange`] when any field is outside
    /// `1..=200`.
    #[allow(
        clippy::too_many_arguments,
        reason = "constructor mirrors the 10-field PRD goalkeeping block field-for-field; a builder would scatter the field set and obscure the validation"
    )]
    pub const fn new(
        reflexes: u8,
        anticipation: u8,
        rushing_out: u8,
        throwing: u8,
        kicking: u8,
        punching: u8,
        aerial_reach: u8,
        communication: u8,
        agility: u8,
        concentration: u8,
    ) -> Result<Self, AttributeError> {
        match (
            check_range(reflexes),
            check_range(anticipation),
            check_range(rushing_out),
            check_range(throwing),
            check_range(kicking),
            check_range(punching),
            check_range(aerial_reach),
            check_range(communication),
            check_range(agility),
            check_range(concentration),
        ) {
            (
                Ok(reflexes),
                Ok(anticipation),
                Ok(rushing_out),
                Ok(throwing),
                Ok(kicking),
                Ok(punching),
                Ok(aerial_reach),
                Ok(communication),
                Ok(agility),
                Ok(concentration),
            ) => Ok(Self {
                reflexes,
                anticipation,
                rushing_out,
                throwing,
                kicking,
                punching,
                aerial_reach,
                communication,
                agility,
                concentration,
            }),
            (Err(e), _, _, _, _, _, _, _, _, _)
            | (_, Err(e), _, _, _, _, _, _, _, _)
            | (_, _, Err(e), _, _, _, _, _, _, _)
            | (_, _, _, Err(e), _, _, _, _, _, _)
            | (_, _, _, _, Err(e), _, _, _, _, _)
            | (_, _, _, _, _, Err(e), _, _, _, _)
            | (_, _, _, _, _, _, Err(e), _, _, _)
            | (_, _, _, _, _, _, _, Err(e), _, _)
            | (_, _, _, _, _, _, _, _, Err(e), _)
            | (_, _, _, _, _, _, _, _, _, Err(e)) => Err(e),
        }
    }

    /// Mid-range placeholder block for [`PlayerProfile::default_goalkeeper`].
    /// `pub(crate)` so defaults stay beside the validated constructor.
    pub(crate) const fn placeholder() -> Self {
        Self {
            reflexes: 70,
            anticipation: 70,
            rushing_out: 70,
            throwing: 70,
            kicking: 70,
            punching: 70,
            aerial_reach: 70,
            communication: 70,
            agility: 70,
            concentration: 70,
        }
    }

    /// Read one goalkeeping slot through the accessor. Returns `None`
    /// for variants outside this block. `pub(crate)` so only
    /// `PlayerProfile::get` routes here.
    pub(crate) const fn read(self, attribute: Attribute) -> Option<u8> {
        match attribute {
            Attribute::Reflexes => Some(self.reflexes),
            Attribute::Anticipation => Some(self.anticipation),
            Attribute::RushingOut => Some(self.rushing_out),
            Attribute::Throwing => Some(self.throwing),
            Attribute::Kicking => Some(self.kicking),
            Attribute::Punching => Some(self.punching),
            Attribute::AerialReach => Some(self.aerial_reach),
            Attribute::Communication => Some(self.communication),
            Attribute::Agility => Some(self.agility),
            Attribute::Concentration => Some(self.concentration),
            _ => None,
        }
    }

    /// Write one goalkeeping slot through the accessor. Returns whether
    /// the variant belonged to this block. `pub(crate)` so only
    /// `PlayerProfile::set` routes here.
    pub(crate) const fn write(&mut self, attribute: Attribute, value: u8) -> bool {
        match attribute {
            Attribute::Reflexes => self.reflexes = value,
            Attribute::Anticipation => self.anticipation = value,
            Attribute::RushingOut => self.rushing_out = value,
            Attribute::Throwing => self.throwing = value,
            Attribute::Kicking => self.kicking = value,
            Attribute::Punching => self.punching = value,
            Attribute::AerialReach => self.aerial_reach = value,
            Attribute::Communication => self.communication = value,
            Attribute::Agility => self.agility = value,
            Attribute::Concentration => self.concentration = value,
            _ => return false,
        }
        true
    }
}
