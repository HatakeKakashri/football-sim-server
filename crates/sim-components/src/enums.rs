//! Domain enums: role, mentality, formation, tactic, match state, etc.

use serde::{Deserialize, Serialize};

/// Player role on the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    Goalkeeper,
    CenterBack,
    FullBack,
    DefensiveMidfielder,
    CentralMidfielder,
    AttackingMidfielder,
    Winger,
    Striker,
}

/// Team mentality setting (PRD §7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mentality {
    Attack,
    Balance,
    Defend,
}

impl Mentality {
    /// The serialised name used in match fixtures (PRD §5.4).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Attack => "attack",
            Self::Balance => "balance",
            Self::Defend => "defend",
        }
    }

    /// Look up a mentality by its serialised name. Returns `None` for
    /// unknown names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "attack" => Some(Self::Attack),
            "balance" => Some(Self::Balance),
            "defend" => Some(Self::Defend),
            _ => None,
        }
    }
}

/// Formation template (PRD §7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Formation {
    FourFourTwo,
    FourThreeThree,
    ThreeFiveTwo,
    FourTwoThreeOne,
    FiveThreeTwo,
}

/// Team tactic (PRD §7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tactic {
    Possession,
    CounterAttack,
    HighPress,
    ParkTheBus,
}

impl Tactic {
    /// The serialised name used in match fixtures (PRD §5.4).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Possession => "possession",
            Self::CounterAttack => "counter_attack",
            Self::HighPress => "high_press",
            Self::ParkTheBus => "park_the_bus",
        }
    }

    /// Look up a tactic by its serialised name. Returns `None` for
    /// unknown names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "possession" => Some(Self::Possession),
            "counter_attack" => Some(Self::CounterAttack),
            "high_press" => Some(Self::HighPress),
            "park_the_bus" => Some(Self::ParkTheBus),
            _ => None,
        }
    }
}

/// Match lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MatchState {
    PreMatch,
    Kickoff,
    InPlay,
    Stoppage,
    HalfTime,
    FullTime,
}

/// How the eleven slot Qualities aggregate into a team strength
/// (PRD §6.4). `Sum` returns the total; `Mean` returns the average.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrengthMode {
    #[default]
    Sum,
    Mean,
}

/// Card colour shown by the referee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CardColor {
    Yellow,
    Red,
}
