//! Position-group quality derivation (PRD §6.4).
//!
//! `quality()` returns a position-weighted mean of the attributes
//! relevant to a slot, range `0..=200`. It is **display / reporting
//! only**: the match simulation never reads it.

use sim_math::{QualityGroup, Tuning, effective_attribute};

use crate::attribute::Attribute;
use crate::position::PosSlot;
use crate::profile::PlayerProfile;

/// Map each `PosSlot` to its position group (PRD §6.4).
impl PosSlot {
    /// The position group of this slot.
    ///
    /// `GK` is the goalkeeper group; `DL`/`DC`/`DR` are defence;
    /// `ML`/`MR` are wide; `DML`/`DMC`/`DMR`/`MC` are midfield;
    /// `AML`/`AMC`/`AMR` are attacking; `ST` is striker.
    #[must_use]
    pub const fn group(self) -> QualityGroup {
        match self {
            Self::GK => QualityGroup::Goalkeeper,
            Self::DL | Self::DC | Self::DR => QualityGroup::Defence,
            Self::ML | Self::MR => QualityGroup::Wide,
            Self::DML | Self::DMC | Self::DMR | Self::MC => QualityGroup::Midfield,
            Self::AML | Self::AMC | Self::AMR => QualityGroup::Attacking,
            Self::ST => QualityGroup::Striker,
        }
    }
}

/// Compute the position-weighted Quality of a player in `slot`.
///
/// The result is the weighted mean of the attributes relevant to the
/// slot's position group, each scaled by the slot's fit multiplier, in
/// the range `0..=200`. Attributes that do not belong to the player's
/// profile (e.g. outfield attributes on a goalkeeper profile) contribute
/// nothing, so a goalkeeper's Quality is drawn exclusively from the 15
/// goalkeeper-profile attributes.
///
/// # Parameters
///
/// - `profile` — the player's attribute profile.
/// - `slot` — the formation slot the player occupies.
/// - `trained_positions` — the player's trained positions; the fit
///   multiplier for `slot` is applied to every attribute. In the MVP
///   players occupy their trained slots, so callers typically pass
///   `&[slot]` (multiplier `1.00`).
/// - `tuning` — tunable quality weights (and fit multipliers).
#[must_use]
pub fn quality(
    profile: &PlayerProfile,
    slot: PosSlot,
    trained_positions: &[PosSlot],
    tuning: &Tuning,
) -> f32 {
    let group = slot.group();
    let weights = tuning.quality_weights.row(group);
    let multiplier = crate::position::fit_multiplier(slot, trained_positions, tuning);

    let mut weighted_sum = 0.0f32;
    let mut weight_sum = 0.0f32;
    for (index, attribute) in Attribute::ALL.iter().enumerate() {
        let weight = weights[index];
        if weight == 0.0 {
            continue;
        }
        let Some(raw) = profile.get(*attribute) else {
            // Attribute not present on this profile (e.g. an outfield
            // attribute on a goalkeeper profile): not part of this
            // profile's Quality.
            continue;
        };
        let effective = effective_attribute(raw, multiplier);
        weighted_sum = effective.mul_add(weight, weighted_sum);
        weight_sum += weight;
    }

    if weight_sum == 0.0 {
        return 0.0;
    }
    (weighted_sum / weight_sum).clamp(0.0, 200.0)
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "quality values are computed from integer-literal weights and attributes; equality is exact"
)]
mod tests {
    use super::*;
    use crate::profile::{ATTRIBUTE_MAX, ATTRIBUTE_MIN};
    use sim_math::QualityGroup;

    /// Tolerance for accumulated f32 weighted-mean results.
    const QUALITY_TOL: f32 = 1e-4;

    fn default_tuning() -> Tuning {
        Tuning::default()
    }

    fn all_outfield(profile: &mut PlayerProfile) {
        // Every outfield attribute at 100, goalkeeper block unset on an
        // outfield profile (would be a ProfileMismatch).
        for attribute in [
            Attribute::Tackling,
            Attribute::Marking,
            Attribute::Positioning,
            Attribute::Heading,
            Attribute::Bravery,
            Attribute::Passing,
            Attribute::Dribbling,
            Attribute::Crossing,
            Attribute::Shooting,
            Attribute::Finishing,
            Attribute::OutfieldFitness,
            Attribute::OutfieldStrength,
            Attribute::OutfieldAggression,
            Attribute::OutfieldSpeed,
            Attribute::OutfieldCreativity,
        ] {
            profile.set(attribute, 100).expect("invariant: test values in range");
        }
    }

    fn all_goalkeeper(profile: &mut PlayerProfile) {
        for attribute in [
            Attribute::Reflexes,
            Attribute::Anticipation,
            Attribute::RushingOut,
            Attribute::Throwing,
            Attribute::Kicking,
            Attribute::Punching,
            Attribute::AerialReach,
            Attribute::Communication,
            Attribute::Agility,
            Attribute::Concentration,
            Attribute::GoalkeeperFitness,
            Attribute::GoalkeeperStrength,
            Attribute::GoalkeeperAggression,
            Attribute::GoalkeeperSpeed,
            Attribute::GoalkeeperCreativity,
        ] {
            profile.set(attribute, 100).expect("invariant: test values in range");
        }
    }

    #[test]
    fn test_quality_in_range() {
        let tuning = default_tuning();
        for &slot in &PosSlot::ALL {
            let profile = if slot == PosSlot::GK {
                PlayerProfile::default_goalkeeper()
            } else {
                PlayerProfile::default_outfield()
            };
            let q = quality(&profile, slot, &[slot], &tuning);
            assert!(
                (f32::from(ATTRIBUTE_MIN)..=f32::from(ATTRIBUTE_MAX)).contains(&q),
                "quality for {slot} out of range: {q}"
            );
        }
    }

    #[test]
    fn test_quality_uniform_attributes_is_uniform_quality() {
        let tuning = default_tuning();
        // Every attribute at 100 ⇒ every group's weighted mean is 100.
        for &slot in &PosSlot::ALL {
            let mut profile = if slot == PosSlot::GK {
                PlayerProfile::default_goalkeeper()
            } else {
                PlayerProfile::default_outfield()
            };
            if slot == PosSlot::GK {
                all_goalkeeper(&mut profile);
            } else {
                all_outfield(&mut profile);
            }
            let q = quality(&profile, slot, &[slot], &tuning);
            assert!((q - 100.0).abs() < QUALITY_TOL, "quality for {slot} should be 100, got {q}");
        }
    }

    #[test]
    fn test_quality_gk_uses_only_goalkeeper_attributes() {
        let tuning = default_tuning();
        let mut gk = PlayerProfile::default_goalkeeper();
        all_goalkeeper(&mut gk);
        let gk_q = quality(&gk, PosSlot::GK, &[PosSlot::GK], &tuning);
        assert!((gk_q - 100.0).abs() < QUALITY_TOL);

        // The same 100-valued goalkeeper attributes must not leak into an
        // outfield slot's Quality: outfield groups weight only outfield
        // attributes, which are unset (None) on a goalkeeper profile.
        let mut outfield = PlayerProfile::default_outfield();
        all_outfield(&mut outfield);
        let of_q = quality(&outfield, PosSlot::DC, &[PosSlot::DC], &tuning);
        assert!((of_q - 100.0).abs() < QUALITY_TOL);
    }

    #[test]
    fn test_quality_respects_position_group_weights() {
        let tuning = default_tuning();
        // Defence weights tackling heavily; set only tackling high.
        let mut profile = PlayerProfile::default_outfield();
        profile
            .set(Attribute::Tackling, ATTRIBUTE_MAX)
            .expect("invariant: 200 is in range");
        let tackling_q = quality(&profile, PosSlot::DC, &[PosSlot::DC], &tuning);

        // Midfield weights passing heavily; set only passing high.
        let mut profile = PlayerProfile::default_outfield();
        profile
            .set(Attribute::Passing, ATTRIBUTE_MAX)
            .expect("invariant: 200 is in range");
        let passing_q = quality(&profile, PosSlot::MC, &[PosSlot::MC], &tuning);

        // Both use weight 0.18/0.16 vs 0.16 on a single maxed attribute
        // with everything else at the default placeholder (70). They must
        // differ, proving the group weight tables are distinct.
        assert!((tackling_q - passing_q).abs() > QUALITY_TOL);
    }

    #[test]
    fn test_quality_fit_multiplier_applies() {
        let tuning = default_tuning();
        let mut profile = PlayerProfile::default_outfield();
        all_outfield(&mut profile);
        // Trained in MC, playing MC ⇒ distance 0 ⇒ multiplier 1.00.
        let trained = quality(&profile, PosSlot::MC, &[PosSlot::MC], &tuning);
        // Trained in MC, playing DL ⇒ distance 2 ⇒ multiplier 0.75.
        let out_of_position = quality(&profile, PosSlot::DL, &[PosSlot::MC], &tuning);
        assert!((trained - 100.0).abs() < QUALITY_TOL);
        assert!((out_of_position - 75.0).abs() < QUALITY_TOL);
    }

    #[test]
    fn test_quality_group_mapping_covers_all_slots() {
        let groups = [
            (PosSlot::GK, QualityGroup::Goalkeeper),
            (PosSlot::DL, QualityGroup::Defence),
            (PosSlot::DC, QualityGroup::Defence),
            (PosSlot::DR, QualityGroup::Defence),
            (PosSlot::ML, QualityGroup::Wide),
            (PosSlot::MR, QualityGroup::Wide),
            (PosSlot::DML, QualityGroup::Midfield),
            (PosSlot::DMC, QualityGroup::Midfield),
            (PosSlot::DMR, QualityGroup::Midfield),
            (PosSlot::MC, QualityGroup::Midfield),
            (PosSlot::AML, QualityGroup::Attacking),
            (PosSlot::AMC, QualityGroup::Attacking),
            (PosSlot::AMR, QualityGroup::Attacking),
            (PosSlot::ST, QualityGroup::Striker),
        ];
        for (slot, expected) in groups {
            assert_eq!(slot.group(), expected, "group mapping for {slot}");
        }
    }

    #[test]
    fn test_quality_default_weight_rows_sum_to_one() {
        let tuning = default_tuning();
        for group in [
            QualityGroup::Defence,
            QualityGroup::Wide,
            QualityGroup::Midfield,
            QualityGroup::Attacking,
            QualityGroup::Striker,
            QualityGroup::Goalkeeper,
        ] {
            let row = tuning.quality_weights.row(group);
            let sum: f32 = row.iter().sum();
            assert!((sum - 1.0).abs() < 1e-6, "weights for {group:?} sum to {sum}, not 1.0");
        }
    }
}
