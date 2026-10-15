//! Attribute accessor tests: A11 declaration coverage plus A10
//! accessor-level monotonicity and isolation (one test per variant).
//!
//! Follows the `*_tests.rs` sibling-file convention used across the
//! workspace (`sim-rules` clock/referee tests, etc.).

use crate::{Attribute, AttributeError, Goalkeeping, OutfieldAttack, OutfieldDefence, PhysicalMental, PlayerProfile, ProfileKind, ATTRIBUTE_COUNT};
use sim_math::{curve, Tuning};

#[test]
fn a11_attribute_count_is_thirty() {
    assert_eq!(Attribute::all().len(), ATTRIBUTE_COUNT);
    assert_eq!(ATTRIBUTE_COUNT, 30);
}

#[test]
fn a11_every_attribute_has_declared_consumer() {
    for attribute in Attribute::all() {
        assert!(
            !attribute.consumer().is_empty(),
            "attribute {attribute:?} has no declared consumer"
        );
    }
}

#[test]
fn a11_reserved_list_is_empty() {
    let reserved: Vec<Attribute> = Attribute::all()
        .into_iter()
        .filter(|a| a.is_reserved())
        .collect();
    assert!(reserved.is_empty(), "unexpected reserved attributes: {reserved:?}");
}

#[test]
fn a10_sweep_and_unit_partition_is_complete() {
    assert_eq!(
        Attribute::FULL_MATCH_SWEEP.len() + Attribute::UNIT_ONLY.len(),
        ATTRIBUTE_COUNT
    );
    for swept in Attribute::FULL_MATCH_SWEEP {
        assert!(
            !Attribute::UNIT_ONLY.contains(&swept),
            "attribute {swept:?} is both swept and unit-only"
        );
    }
    for attribute in Attribute::all() {
        assert!(
            Attribute::FULL_MATCH_SWEEP.contains(&attribute)
                || Attribute::UNIT_ONLY.contains(&attribute),
            "attribute {attribute:?} is in neither A10 tier"
        );
    }
}

#[test]
fn constructors_reject_out_of_range_values() {
    assert_eq!(
        OutfieldDefence::new(0, 70, 70, 70, 70),
        Err(AttributeError::OutOfRange { value: 0 })
    );
    assert_eq!(
        OutfieldAttack::new(70, 70, 70, 201, 70),
        Err(AttributeError::OutOfRange { value: 201 })
    );
    assert_eq!(
        PhysicalMental::new(70, 70, 70, 70, 0),
        Err(AttributeError::OutOfRange { value: 0 })
    );
    assert_eq!(
        Goalkeeping::new(70, 70, 70, 70, 70, 70, 70, 70, 70, 201),
        Err(AttributeError::OutOfRange { value: 201 })
    );
}

#[test]
fn cross_profile_access_returns_none() {
    let outfield = PlayerProfile::default_outfield();
    assert_eq!(outfield.get(Attribute::Reflexes), None);
    assert_eq!(outfield.get(Attribute::GoalkeeperFitness), None);
    let keeper = PlayerProfile::default_goalkeeper();
    assert_eq!(keeper.get(Attribute::Tackling), None);
    assert_eq!(keeper.get(Attribute::OutfieldSpeed), None);
}

#[test]
fn set_rejects_out_of_range_and_cross_profile_writes() {
    let mut outfield = PlayerProfile::default_outfield();
    assert_eq!(
        outfield.set(Attribute::Tackling, 0),
        Err(AttributeError::OutOfRange { value: 0 })
    );
    assert_eq!(
        outfield.set(Attribute::Reflexes, 100),
        Err(AttributeError::ProfileMismatch)
    );
    let mut keeper = PlayerProfile::default_goalkeeper();
    assert_eq!(
        keeper.set(Attribute::Shooting, 100),
        Err(AttributeError::ProfileMismatch)
    );
}

#[test]
fn defaults_round_trip_through_accessor() {
    let outfield = PlayerProfile::default_outfield();
    assert_eq!(outfield.get(Attribute::Tackling), Some(70));
    assert_eq!(outfield.get(Attribute::OutfieldFitness), Some(75));
    let keeper = PlayerProfile::default_goalkeeper();
    assert_eq!(keeper.get(Attribute::Reflexes), Some(70));
    assert_eq!(keeper.get(Attribute::GoalkeeperSpeed), Some(75));
}

/// A different variant of the same profile, used to prove read
/// isolation: varying one attribute never moves another read.
fn sibling_attribute(attribute: Attribute) -> Attribute {
    match attribute.profile_kind() {
        ProfileKind::Outfield => {
            if attribute == Attribute::Tackling {
                Attribute::Passing
            } else {
                Attribute::Tackling
            }
        }
        ProfileKind::Goalkeeper => {
            if attribute == Attribute::Reflexes {
                Attribute::Kicking
            } else {
                Attribute::Reflexes
            }
        }
    }
}

/// Accessor-level `A10`: raising the stored value strictly raises the
/// read, and no sibling read moves.
fn check_monotonic(attribute: Attribute) {
    let low = PlayerProfile::with_attribute(attribute, 50);
    let high = PlayerProfile::with_attribute(attribute, 150);
    let low_value = low
        .get(attribute)
        .expect("invariant: attribute matches its own profile");
    let high_value = high
        .get(attribute)
        .expect("invariant: attribute matches its own profile");
    assert!(
        low_value < high_value,
        "{attribute:?}: expected monotonic read, got {low_value} -> {high_value}"
    );
    let sibling = sibling_attribute(attribute);
    assert_eq!(
        low.get(sibling),
        high.get(sibling),
        "{attribute:?}: sibling read {sibling:?} moved"
    );
}

macro_rules! monotonic_tests {
    ($( $test_name:ident : $variant:ident ),* $(,)?) => {
        $(
            #[test]
            fn $test_name() {
                check_monotonic(Attribute::$variant);
            }
        )*
    };
}

monotonic_tests!(
    a10_monotonic_tackling: Tackling,
    a10_monotonic_marking: Marking,
    a10_monotonic_positioning: Positioning,
    a10_monotonic_heading: Heading,
    a10_monotonic_bravery: Bravery,
    a10_monotonic_passing: Passing,
    a10_monotonic_dribbling: Dribbling,
    a10_monotonic_crossing: Crossing,
    a10_monotonic_shooting: Shooting,
    a10_monotonic_finishing: Finishing,
    a10_monotonic_outfield_fitness: OutfieldFitness,
    a10_monotonic_outfield_strength: OutfieldStrength,
    a10_monotonic_outfield_aggression: OutfieldAggression,
    a10_monotonic_outfield_speed: OutfieldSpeed,
    a10_monotonic_outfield_creativity: OutfieldCreativity,
    a10_monotonic_reflexes: Reflexes,
    a10_monotonic_anticipation: Anticipation,
    a10_monotonic_rushing_out: RushingOut,
    a10_monotonic_throwing: Throwing,
    a10_monotonic_kicking: Kicking,
    a10_monotonic_punching: Punching,
    a10_monotonic_aerial_reach: AerialReach,
    a10_monotonic_communication: Communication,
    a10_monotonic_agility: Agility,
    a10_monotonic_concentration: Concentration,
    a10_monotonic_goalkeeper_fitness: GoalkeeperFitness,
    a10_monotonic_goalkeeper_strength: GoalkeeperStrength,
    a10_monotonic_goalkeeper_aggression: GoalkeeperAggression,
    a10_monotonic_goalkeeper_speed: GoalkeeperSpeed,
    a10_monotonic_goalkeeper_creativity: GoalkeeperCreativity,
);

/// `A10` attribute-sensitivity at the curve layer: for every consumed
/// attribute, raising the attribute strictly and monotonically moves the
/// mechanic parameter in the declared direction (`CurveParams.rising`).
///
/// This covers all 30 attributes: the 25 unit-only attributes required
/// by PRD §13.1 `A10`, plus the five (Shooting, Finishing, Passing,
/// Tackling, Reflexes) that additionally get full-match sweeps in ticket
/// 29. The direction table mirrors `Tuning::default_assumptions` and is
/// checked against it, so a reordering of `curve_params` fails this test.
#[test]
fn a10_curve_monotonicity_all_attributes() {
    let tuning = Tuning::default_assumptions();

    // One CurveParams record per consumed attribute, in Attribute::ALL order.
    assert_eq!(tuning.curve_params.len(), ATTRIBUTE_COUNT);

    for (index, &attribute) in Attribute::ALL.iter().enumerate() {
        let params = &tuning.curve_params[index];

        // The curve must be non-degenerate for monotonicity to be meaningful.
        assert!(
            params.steepness > 0.0,
            "{attribute:?}: curve steepness must be positive"
        );

        // The declared direction must match the documented consumer semantics.
        assert_eq!(
            params.rising,
            expected_curve_direction(attribute),
            "{attribute:?}: curve direction does not match consumer semantics"
        );

        // Strict monotonicity in the declared direction across the full range.
        let mut prev = curve(0, params);
        for attr in 1..=200i32 {
            let value = curve(attr, params);
            if params.rising {
                assert!(
                    value > prev,
                    "{attribute:?}: curve not strictly rising at attribute {attr}"
                );
            } else {
                assert!(
                    value < prev,
                    "{attribute:?}: curve not strictly falling at attribute {attr}"
                );
            }
            prev = value;
        }
    }
}

/// Declared direction of effect for each attribute's mechanic parameter
/// (PRD §6.2 consumers). `true` = higher attribute ⇒ higher parameter.
fn expected_curve_direction(attribute: Attribute) -> bool {
    match attribute {
        // Falling: higher attribute lowers the mechanic parameter
        // (error, drain, or lapse probability).
        Attribute::Marking
        | Attribute::Positioning
        | Attribute::Passing
        | Attribute::Shooting
        | Attribute::OutfieldFitness
        | Attribute::Concentration
        | Attribute::GoalkeeperFitness => false,
        // Rising: higher attribute raises the mechanic parameter
        // (probability, quality, speed, weight, reach, ...).
        Attribute::Tackling
        | Attribute::Heading
        | Attribute::Bravery
        | Attribute::Dribbling
        | Attribute::Crossing
        | Attribute::Finishing
        | Attribute::OutfieldStrength
        | Attribute::OutfieldAggression
        | Attribute::OutfieldSpeed
        | Attribute::OutfieldCreativity
        | Attribute::Reflexes
        | Attribute::Anticipation
        | Attribute::RushingOut
        | Attribute::Throwing
        | Attribute::Kicking
        | Attribute::Punching
        | Attribute::AerialReach
        | Attribute::Communication
        | Attribute::Agility
        | Attribute::GoalkeeperStrength
        | Attribute::GoalkeeperAggression
        | Attribute::GoalkeeperSpeed
        | Attribute::GoalkeeperCreativity => true,
    }
}
