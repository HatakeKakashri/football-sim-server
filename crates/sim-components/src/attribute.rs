//! The attribute accessor enum: one variant per slot (PRD §6.2).
//!
//! Each variant declares its consumer system via [`Attribute::consumer`]
//! (or `Reserved`); the `A11` test fails on any undeclared variant. The
//! reserved list (PRD §6.6) is currently empty: mechanic tickets move a
//! variant to `Reserved` only when no honest consumer exists.

use serde::{Deserialize, Serialize};

use super::profile::ProfileKind;
use super::profile::ATTRIBUTE_COUNT;
/// Attribute accessor enum: one variant per slot (PRD §6.2).
///
/// Physical & mental attributes appear once per profile
/// (`OutfieldFitness` / `GoalkeeperFitness`, etc.) because their
/// consumers differ by profile; the fixture key ([`Attribute::name`])
/// is shared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Attribute {
    /// Outfield defence: tackle success probability.
    Tackling,
    /// Outfield defence: mark distance-to-mark error.
    Marking,
    /// Outfield defence: defensive target-position error.
    Positioning,
    /// Outfield defence: aerial duel outcome; header accuracy and power.
    Heading,
    /// Outfield defence: duel-contest decision weight.
    Bravery,
    /// Outfield attack: pass accuracy error spread.
    Passing,
    /// Outfield attack: ball retention against tackles.
    Dribbling,
    /// Outfield attack: cross accuracy.
    Crossing,
    /// Outfield attack: shot power and accuracy.
    Shooting,
    /// Outfield attack: conversion quality close to goal and under pressure.
    Finishing,
    /// Outfield physical & mental: stamina drain and recovery.
    OutfieldFitness,
    /// Outfield physical & mental: hold-off, tackle and aerial duel outcomes.
    OutfieldStrength,
    /// Outfield physical & mental: press frequency and trigger distance; tackle propensity.
    OutfieldAggression,
    /// Outfield physical & mental: sprint speed (lower with the ball than without).
    OutfieldSpeed,
    /// Outfield physical & mental: key-pass / through-ball option weight.
    OutfieldCreativity,
    /// Goalkeeping: save probability on close and fast shots.
    Reflexes,
    /// Goalkeeping: shot reading; positioning relative to the shooter.
    Anticipation,
    /// Goalkeeping: rush-out decision and angle-closing quality.
    RushingOut,
    /// Goalkeeping: hand-distribution accuracy and range.
    Throwing,
    /// Goalkeeping: kick-distribution accuracy and range.
    Kicking,
    /// Goalkeeping: cross clearance by punching.
    Punching,
    /// Goalkeeping: reach for shots and crosses.
    AerialReach,
    /// Goalkeeping: reduces defender positioning and marking error in the keeper's own half.
    Communication,
    /// Goalkeeping: lateral movement in goal and dive reach on saves.
    Agility,
    /// Goalkeeping: save-lapse probability, rising with time since the keeper's last touch.
    Concentration,
    /// Goalkeeper physical & mental: stamina drain and recovery.
    GoalkeeperFitness,
    /// Goalkeeper physical & mental: contested claims.
    GoalkeeperStrength,
    /// Goalkeeper physical & mental: rush-out and claim propensity.
    GoalkeeperAggression,
    /// Goalkeeper physical & mental: movement and rush-out speed.
    GoalkeeperSpeed,
    /// Goalkeeper physical & mental: long / through-ball distribution option weight.
    GoalkeeperCreativity,
}

impl Attribute {
    /// Every variant, in declaration order. The `A11` test iterates this
    /// and fails on any variant without a declared consumer; adding a
    /// variant here without extending [`Attribute::consumer`] fails to
    /// compile (non-exhaustive match).
    pub const ALL: [Self; ATTRIBUTE_COUNT] = [
        Self::Tackling,
        Self::Marking,
        Self::Positioning,
        Self::Heading,
        Self::Bravery,
        Self::Passing,
        Self::Dribbling,
        Self::Crossing,
        Self::Shooting,
        Self::Finishing,
        Self::OutfieldFitness,
        Self::OutfieldStrength,
        Self::OutfieldAggression,
        Self::OutfieldSpeed,
        Self::OutfieldCreativity,
        Self::Reflexes,
        Self::Anticipation,
        Self::RushingOut,
        Self::Throwing,
        Self::Kicking,
        Self::Punching,
        Self::AerialReach,
        Self::Communication,
        Self::Agility,
        Self::Concentration,
        Self::GoalkeeperFitness,
        Self::GoalkeeperStrength,
        Self::GoalkeeperAggression,
        Self::GoalkeeperSpeed,
        Self::GoalkeeperCreativity,
    ];

    /// Attributes proven by full-match sweeps (PRD §13.1 `A10`); the
    /// remaining 25 are proven at unit level only.
    pub const FULL_MATCH_SWEEP: [Self; 5] = [
        Self::Shooting,
        Self::Finishing,
        Self::Passing,
        Self::Tackling,
        Self::Reflexes,
    ];

    /// The 25 attributes proven only at unit level (PRD §13.1 `A10`).
    pub const UNIT_ONLY: [Self; 25] = [
        Self::Marking,
        Self::Positioning,
        Self::Heading,
        Self::Bravery,
        Self::Dribbling,
        Self::Crossing,
        Self::OutfieldFitness,
        Self::OutfieldStrength,
        Self::OutfieldAggression,
        Self::OutfieldSpeed,
        Self::OutfieldCreativity,
        Self::Anticipation,
        Self::RushingOut,
        Self::Throwing,
        Self::Kicking,
        Self::Punching,
        Self::AerialReach,
        Self::Communication,
        Self::Agility,
        Self::Concentration,
        Self::GoalkeeperFitness,
        Self::GoalkeeperStrength,
        Self::GoalkeeperAggression,
        Self::GoalkeeperSpeed,
        Self::GoalkeeperCreativity,
    ];

    /// Every variant, in declaration order (alias for [`Attribute::ALL`]).
    #[must_use]
    pub const fn all() -> [Self; ATTRIBUTE_COUNT] {
        Self::ALL
    }

    /// Which profile this variant belongs to.
    #[must_use]
    pub const fn profile_kind(self) -> ProfileKind {
        match self {
            Self::Tackling
            | Self::Marking
            | Self::Positioning
            | Self::Heading
            | Self::Bravery
            | Self::Passing
            | Self::Dribbling
            | Self::Crossing
            | Self::Shooting
            | Self::Finishing
            | Self::OutfieldFitness
            | Self::OutfieldStrength
            | Self::OutfieldAggression
            | Self::OutfieldSpeed
            | Self::OutfieldCreativity => ProfileKind::Outfield,
            Self::Reflexes
            | Self::Anticipation
            | Self::RushingOut
            | Self::Throwing
            | Self::Kicking
            | Self::Punching
            | Self::AerialReach
            | Self::Communication
            | Self::Agility
            | Self::Concentration
            | Self::GoalkeeperFitness
            | Self::GoalkeeperStrength
            | Self::GoalkeeperAggression
            | Self::GoalkeeperSpeed
            | Self::GoalkeeperCreativity => ProfileKind::Goalkeeper,
        }
    }

    /// Stable fixture key for this attribute (PRD §5.4). Physical &
    /// mental pairs share a key; the profile disambiguates.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Tackling => "tackling",
            Self::Marking => "marking",
            Self::Positioning => "positioning",
            Self::Heading => "heading",
            Self::Bravery => "bravery",
            Self::Passing => "passing",
            Self::Dribbling => "dribbling",
            Self::Crossing => "crossing",
            Self::Shooting => "shooting",
            Self::Finishing => "finishing",
            Self::OutfieldFitness | Self::GoalkeeperFitness => "fitness",
            Self::OutfieldStrength | Self::GoalkeeperStrength => "strength",
            Self::OutfieldAggression | Self::GoalkeeperAggression => "aggression",
            Self::OutfieldSpeed | Self::GoalkeeperSpeed => "speed",
            Self::OutfieldCreativity | Self::GoalkeeperCreativity => "creativity",
            Self::Reflexes => "reflexes",
            Self::Anticipation => "anticipation",
            Self::RushingOut => "rushing_out",
            Self::Throwing => "throwing",
            Self::Kicking => "kicking",
            Self::Punching => "punching",
            Self::AerialReach => "aerial_reach",
            Self::Communication => "communication",
            Self::Agility => "agility",
            Self::Concentration => "concentration",
        }
    }

    /// Declared consumer system for this attribute (PRD §6.2), or
    /// `"Reserved: <reason>"` for entries on the reserved list (PRD
    /// §6.6, currently empty). Never empty: the `A11` test fails on an
    /// undeclared variant.
    #[must_use]
    pub const fn consumer(self) -> &'static str {
        match self {
            Self::Tackling => "tackle success probability",
            Self::Marking => "mark distance-to-mark error",
            Self::Positioning => "defensive target-position error",
            Self::Heading => "aerial duel outcome; header accuracy and power",
            Self::Bravery => "duel-contest decision weight",
            Self::Passing => "pass accuracy error spread",
            Self::Dribbling => "ball retention against tackles",
            Self::Crossing => "cross accuracy",
            Self::Shooting => "shot power and accuracy",
            Self::Finishing => "conversion quality close to goal and under pressure",
            Self::OutfieldFitness | Self::GoalkeeperFitness => "stamina drain and recovery",
            Self::OutfieldStrength => "hold-off, tackle and aerial duel outcomes",
            Self::GoalkeeperStrength => "contested claims",
            Self::OutfieldAggression => "press frequency and trigger distance; tackle propensity",
            Self::GoalkeeperAggression => "rush-out and claim propensity",
            Self::OutfieldSpeed => "sprint speed (lower with the ball than without)",
            Self::GoalkeeperSpeed => "movement and rush-out speed",
            Self::OutfieldCreativity => "key-pass / through-ball option weight",
            Self::GoalkeeperCreativity => "long / through-ball distribution option weight",
            Self::Reflexes => "save probability on close and fast shots",
            Self::Anticipation => "shot reading; positioning relative to the shooter",
            Self::RushingOut => "rush-out decision and angle-closing quality",
            Self::Throwing => "hand-distribution accuracy and range",
            Self::Kicking => "kick-distribution accuracy and range",
            Self::Punching => "cross clearance by punching",
            Self::AerialReach => "reach for shots and crosses",
            Self::Communication => "defender positioning and marking error in own half",
            Self::Agility => "lateral movement in goal and dive reach on saves",
            Self::Concentration => "save-lapse probability rising with time since last touch",
        }
    }

    /// Whether this variant sits on the reserved list (PRD §6.6).
    /// Currently `false` for every variant; mechanic tickets flip a
    /// variant to reserved (with a reason surfaced through
    /// [`Attribute::consumer`]) when no honest consumer exists.
    #[must_use]
    pub const fn is_reserved(self) -> bool {
        let _ = self;
        false
    }
}
