//! Formation tables: the five PRD §7.1 formations as 11-slot sets.
//!
//! Each formation is an ordered list of 11 slots drawn from the 14
//! positions (PRD §6.5). A slot pairs a position with its home
//! coordinate for a team attacking right (`attack_direction = +1`);
//! a team attacking left (`-1`) plays the exact 180° mirror across the
//! pitch centre `(PITCH_LENGTH - x, PITCH_WIDTH - y)`, so one table
//! serves both halves and both teams.
//!
//! This replaces the Phase 0 hardcoded 4-4-2 (ticket 03) with a
//! registry that looks up a formation's table by name.

use bevy_ecs::prelude::Component;
use sim_math::{GOAL_Y_CENTER, PITCH_LENGTH, PITCH_WIDTH, Vec2};

use crate::{Formation, PosSlot, Role};

/// One slot in a formation table: the position it plays plus its home
/// coordinate for a team attacking right (`+1`, toward `x = 105`).
///
/// A team attacking left (`-1`) resolves the mirror through
/// [`Self::position_for`]; the `home` field itself always stores the
/// attack-right reference, so half-time end swaps need only flip the
/// direction, never rewrite the slot table.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FormationSlot {
    /// The position this slot plays (one of the 14 positions).
    pub position: PosSlot,
    /// Home coordinate when the team attacks right (`+1`).
    pub home: Vec2,
}

impl FormationSlot {
    /// Resolve this slot's pitch coordinate for the team's attack
    /// direction: `home` when attacking right (`attack_direction > 0`),
    /// otherwise the 180° mirror across the pitch centre.
    #[must_use]
    pub const fn position_for(self, attack_direction: i8) -> Vec2 {
        if attack_direction > 0 {
            self.home
        } else {
            Vec2::new(PITCH_LENGTH - self.home.x, PITCH_WIDTH - self.home.y)
        }
    }

    /// Primary role associated with this slot's position.
    #[must_use]
    pub const fn role(self) -> Role {
        match self.position {
            PosSlot::GK => Role::Goalkeeper,
            PosSlot::DL | PosSlot::DR | PosSlot::DML | PosSlot::DMR => Role::FullBack,
            PosSlot::DC => Role::CenterBack,
            PosSlot::DMC => Role::DefensiveMidfielder,
            PosSlot::ML | PosSlot::MR => Role::Winger,
            PosSlot::MC => Role::CentralMidfielder,
            PosSlot::AML | PosSlot::AMC | PosSlot::AMR => Role::AttackingMidfielder,
            PosSlot::ST => Role::Striker,
        }
    }
}

/// An 11-slot formation table (PRD §7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FormationTable {
    /// The 11 slots in canonical order (slot index 0..11).
    pub slots: [FormationSlot; 11],
}

const fn slot(position: PosSlot, x: f32, y: f32) -> FormationSlot {
    FormationSlot {
        position,
        home: Vec2::new(x, y),
    }
}

// ---------------------------------------------------------------------------
// Formation tables (PRD §7.1): each an 11-slot set from the 14 positions.
//
// Coordinates are the home positions for a team attacking right (+1);
// the mirror (attacking left, -1) is resolved by `position_for`.
//
// 4-4-2 keeps the Phase 0 (ticket 03) coordinates exactly, so existing
// behaviour and state hashes for the canonical formation are unchanged.
// ---------------------------------------------------------------------------

const TABLE_442: FormationTable = FormationTable {
    slots: [
        // GK, DL, DC, DC, DR, ML, MC, MC, MR, ST, ST
        slot(PosSlot::GK, 5.0, GOAL_Y_CENTER),
        slot(PosSlot::DL, 20.0, 12.0),
        slot(PosSlot::DC, 18.0, 26.0),
        slot(PosSlot::DC, 18.0, 42.0),
        slot(PosSlot::DR, 20.0, 56.0),
        slot(PosSlot::ML, 40.0, 12.0),
        slot(PosSlot::MC, 38.0, 26.0),
        slot(PosSlot::MC, 38.0, 42.0),
        slot(PosSlot::MR, 40.0, 56.0),
        slot(PosSlot::ST, 60.0, 27.0),
        slot(PosSlot::ST, 60.0, 41.0),
    ],
};

const TABLE_433: FormationTable = FormationTable {
    slots: [
        // GK, DL, DC, DC, DR, MC, MC, MC, AML, ST, AMR
        slot(PosSlot::GK, 5.0, GOAL_Y_CENTER),
        slot(PosSlot::DL, 20.0, 12.0),
        slot(PosSlot::DC, 18.0, 26.0),
        slot(PosSlot::DC, 18.0, 42.0),
        slot(PosSlot::DR, 20.0, 56.0),
        slot(PosSlot::MC, 35.0, 20.0),
        slot(PosSlot::MC, 35.0, 34.0),
        slot(PosSlot::MC, 35.0, 48.0),
        slot(PosSlot::AML, 52.0, 20.0),
        slot(PosSlot::ST, 65.0, 34.0),
        slot(PosSlot::AMR, 52.0, 48.0),
    ],
};

const TABLE_352: FormationTable = FormationTable {
    slots: [
        // GK, DC, DC, DC, ML, MC, MC, MC, MR, ST, ST
        slot(PosSlot::GK, 5.0, GOAL_Y_CENTER),
        slot(PosSlot::DC, 18.0, 20.0),
        slot(PosSlot::DC, 18.0, 34.0),
        slot(PosSlot::DC, 18.0, 48.0),
        slot(PosSlot::ML, 38.0, 12.0),
        slot(PosSlot::MC, 35.0, 22.0),
        slot(PosSlot::MC, 35.0, 34.0),
        slot(PosSlot::MC, 35.0, 46.0),
        slot(PosSlot::MR, 38.0, 56.0),
        slot(PosSlot::ST, 60.0, 27.0),
        slot(PosSlot::ST, 60.0, 41.0),
    ],
};

const TABLE_4231: FormationTable = FormationTable {
    slots: [
        // GK, DL, DC, DC, DR, DMC, DMC, AML, AMC, AMR, ST
        slot(PosSlot::GK, 5.0, GOAL_Y_CENTER),
        slot(PosSlot::DL, 20.0, 12.0),
        slot(PosSlot::DC, 18.0, 26.0),
        slot(PosSlot::DC, 18.0, 42.0),
        slot(PosSlot::DR, 20.0, 56.0),
        slot(PosSlot::DMC, 30.0, 28.0),
        slot(PosSlot::DMC, 30.0, 40.0),
        slot(PosSlot::AML, 52.0, 20.0),
        slot(PosSlot::AMC, 52.0, 34.0),
        slot(PosSlot::AMR, 52.0, 48.0),
        slot(PosSlot::ST, 70.0, 34.0),
    ],
};

const TABLE_532: FormationTable = FormationTable {
    slots: [
        // GK, DL, DC, DC, DC, DR, MC, MC, MC, ST, ST
        slot(PosSlot::GK, 5.0, GOAL_Y_CENTER),
        slot(PosSlot::DL, 20.0, 10.0),
        slot(PosSlot::DC, 18.0, 22.0),
        slot(PosSlot::DC, 18.0, 34.0),
        slot(PosSlot::DC, 18.0, 46.0),
        slot(PosSlot::DR, 20.0, 58.0),
        slot(PosSlot::MC, 35.0, 24.0),
        slot(PosSlot::MC, 35.0, 34.0),
        slot(PosSlot::MC, 35.0, 44.0),
        slot(PosSlot::ST, 60.0, 27.0),
        slot(PosSlot::ST, 60.0, 41.0),
    ],
};

// ---------------------------------------------------------------------------
// Registry: formation name -> 11-slot table
// ---------------------------------------------------------------------------

impl Formation {
    /// The formation's serialised name, e.g. `"4-4-2"`. This is the
    /// spelling used in match fixtures (PRD §5.4 example) and is the
    /// key of the formation registry.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::FourFourTwo => "4-4-2",
            Self::FourThreeThree => "4-3-3",
            Self::ThreeFiveTwo => "3-5-2",
            Self::FourTwoThreeOne => "4-2-3-1",
            Self::FiveThreeTwo => "5-3-2",
        }
    }

    /// Look up a formation by its serialised name (the registry lookup).
    /// Returns `None` for unknown names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "4-4-2" => Some(Self::FourFourTwo),
            "4-3-3" => Some(Self::FourThreeThree),
            "3-5-2" => Some(Self::ThreeFiveTwo),
            "4-2-3-1" => Some(Self::FourTwoThreeOne),
            "5-3-2" => Some(Self::FiveThreeTwo),
            _ => None,
        }
    }

    /// The 11-slot table for this formation (PRD §7.1).
    #[must_use]
    pub const fn table(self) -> &'static FormationTable {
        match self {
            Self::FourFourTwo => &TABLE_442,
            Self::FourThreeThree => &TABLE_433,
            Self::ThreeFiveTwo => &TABLE_352,
            Self::FourTwoThreeOne => &TABLE_4231,
            Self::FiveThreeTwo => &TABLE_532,
        }
    }
}

/// Component storing a player's assigned home slot coordinates.
///
/// Inner `Vec2` is private (§5): access goes through `position()` /
/// `set_position()` so the invariant "`HomeSlot.position()` agrees with
/// the team's `FormationSlot.position_for(attack_direction)`" stays
/// enforced via `apply_player_slot`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HomeSlot(Vec2);

impl HomeSlot {
    /// Position of this home slot in pitch coordinates.
    #[must_use]
    pub const fn position(self) -> Vec2 {
        self.0
    }

    /// Set the slot position. Only `apply_player_slot` calls this;
    /// production callers outside that function should be a smell.
    /// `pub` because the workspace spans multiple crates; the doc
    /// comment is the load-bearing guard, not the visibility.
    pub const fn set_position(&mut self, pos: Vec2) {
        self.0 = pos;
    }

    /// Construct a new `HomeSlot` at `pos`. Field-private so external
    /// callers cannot bypass `apply_player_slot` to plant an arbitrary
    /// coordinate.
    #[must_use]
    pub const fn new(pos: Vec2) -> Self {
        Self(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_FORMATIONS: [Formation; 5] = [
        Formation::FourFourTwo,
        Formation::FourThreeThree,
        Formation::ThreeFiveTwo,
        Formation::FourTwoThreeOne,
        Formation::FiveThreeTwo,
    ];

    /// PRD §7.1 slot sets, in canonical (unsorted) order.
    const PRD_SLOT_SETS: [&[PosSlot; 11]; 5] = [
        &[
            PosSlot::GK,
            PosSlot::DL,
            PosSlot::DC,
            PosSlot::DC,
            PosSlot::DR,
            PosSlot::ML,
            PosSlot::MC,
            PosSlot::MC,
            PosSlot::MR,
            PosSlot::ST,
            PosSlot::ST,
        ],
        &[
            PosSlot::GK,
            PosSlot::DL,
            PosSlot::DC,
            PosSlot::DC,
            PosSlot::DR,
            PosSlot::MC,
            PosSlot::MC,
            PosSlot::MC,
            PosSlot::AML,
            PosSlot::ST,
            PosSlot::AMR,
        ],
        &[
            PosSlot::GK,
            PosSlot::DC,
            PosSlot::DC,
            PosSlot::DC,
            PosSlot::ML,
            PosSlot::MC,
            PosSlot::MC,
            PosSlot::MC,
            PosSlot::MR,
            PosSlot::ST,
            PosSlot::ST,
        ],
        &[
            PosSlot::GK,
            PosSlot::DL,
            PosSlot::DC,
            PosSlot::DC,
            PosSlot::DR,
            PosSlot::DMC,
            PosSlot::DMC,
            PosSlot::AML,
            PosSlot::AMC,
            PosSlot::AMR,
            PosSlot::ST,
        ],
        &[
            PosSlot::GK,
            PosSlot::DL,
            PosSlot::DC,
            PosSlot::DC,
            PosSlot::DC,
            PosSlot::DR,
            PosSlot::MC,
            PosSlot::MC,
            PosSlot::MC,
            PosSlot::ST,
            PosSlot::ST,
        ],
    ];

    #[test]
    fn test_all_five_formations_have_eleven_slots() {
        for (formation, expected) in ALL_FORMATIONS.iter().zip(PRD_SLOT_SETS.iter()) {
            let table = formation.table();
            assert_eq!(
                table.slots.len(),
                11,
                "{}: a formation must have exactly 11 slots",
                formation.name()
            );

            let positions: Vec<PosSlot> = table
                .slots
                .iter()
                .map(|slot| slot.position)
                .collect();

            // Every slot is one of the 14 positions (PRD §6.5 / §7.1).
            for &pos in &positions {
                assert!(
                    PosSlot::ALL.contains(&pos),
                    "{}: {:?} is not one of the 14 positions",
                    formation.name(),
                    pos
                );
            }

            // GK-is-goalkeeper-only: exactly one GK slot, and it is the
            // only slot holding a goalkeeper profile; every other slot
            // is outfield.
            let gk_count = positions.iter().filter(|&&p| p == PosSlot::GK).count();
            assert_eq!(
                gk_count,
                1,
                "{}: exactly one GK slot (GK-is-goalkeeper-only)",
                formation.name()
            );
            for &pos in &positions {
                if pos != PosSlot::GK {
                    assert!(
                        pos.is_outfield(),
                        "{}: {:?} must be an outfield position",
                        formation.name(),
                        pos
                    );
                }
            }

            // Slot set matches PRD §7.1 exactly (order-independent).
            let mut actual = positions.clone();
            let mut expected_sorted = expected.to_vec();
            actual.sort();
            expected_sorted.sort();
            assert_eq!(
                actual, expected_sorted,
                "{}: slot set must match PRD §7.1",
                formation.name()
            );
        }
    }

    #[test]
    fn test_formation_name_and_lookup_round_trip() {
        for &formation in &ALL_FORMATIONS {
            let name = formation.name();
            assert_eq!(
                Formation::from_name(name),
                Some(formation),
                "from_name(name()) must round-trip for {formation:?}",
            );
        }
        assert_eq!(Formation::from_name("4-4-2"), Some(Formation::FourFourTwo));
        assert_eq!(Formation::from_name("bogus"), None);
        assert_eq!(Formation::from_name(""), None);
    }

    #[test]
    fn test_slot_positions_mirror_across_attack_direction() {
        // A single table serves both halves and both teams: attacking
        // left (-1) is the exact 180° mirror of attacking right (+1).
        for &formation in &ALL_FORMATIONS {
            let table = formation.table();
            for slot in table.slots {
                let right = slot.position_for(1);
                let left = slot.position_for(-1);
                assert!(
                    (left.x - (PITCH_LENGTH - right.x)).abs() < f32::EPSILON,
                    "{}: {:?} left x must mirror right x",
                    formation.name(),
                    slot.position
                );
                assert!(
                    (left.y - (PITCH_WIDTH - right.y)).abs() < f32::EPSILON,
                    "{}: {:?} left y must mirror right y",
                    formation.name(),
                    slot.position
                );
            }
        }
    }

    #[test]
    fn test_442_home_coords_match_ticket_03() {
        // Phase 0 regression: the 4-4-2 table keeps the ticket-03
        // coordinates exactly.
        let table = Formation::FourFourTwo.table();
        let slots = &table.slots;

        assert_eq!(slots[0], FormationSlot { position: PosSlot::GK, home: Vec2::new(5.0, 34.0) });
        assert_eq!(slots[1].position, PosSlot::DL);
        assert_eq!(slots[1].home, Vec2::new(20.0, 12.0));
        assert_eq!(slots[2].position, PosSlot::DC);
        assert_eq!(slots[2].home, Vec2::new(18.0, 26.0));
        assert_eq!(slots[3].position, PosSlot::DC);
        assert_eq!(slots[3].home, Vec2::new(18.0, 42.0));
        assert_eq!(slots[4].position, PosSlot::DR);
        assert_eq!(slots[9].position, PosSlot::ST);
        assert_eq!(slots[9].home, Vec2::new(60.0, 27.0));
        assert_eq!(slots[10].position, PosSlot::ST);
        assert_eq!(slots[10].home, Vec2::new(60.0, 41.0));
    }

    #[test]
    fn test_slot_positions_show_spread() {
        // Assert meaningful spread across 4-4-2 slots (ticket 03).
        let table = Formation::FourFourTwo.table();
        let coords: Vec<Vec2> = table.slots.iter().map(|s| s.position_for(1)).collect();

        let min_x = coords.iter().map(|c| c.x).fold(f32::INFINITY, f32::min);
        let max_x = coords.iter().map(|c| c.x).fold(f32::NEG_INFINITY, f32::max);
        let min_y = coords.iter().map(|c| c.y).fold(f32::INFINITY, f32::min);
        let max_y = coords.iter().map(|c| c.y).fold(f32::NEG_INFINITY, f32::max);

        assert!(
            max_x - min_x >= 50.0,
            "x-spread must be at least 50m (GK to ST)"
        );
        assert!(
            max_y - min_y >= 40.0,
            "y-spread must be at least 40m (flank to flank)"
        );
    }

    #[test]
    fn test_formation_slot_role() {
        assert_eq!(
            FormationSlot {
                position: PosSlot::GK,
                home: Vec2::zero()
            }
            .role(),
            Role::Goalkeeper
        );
        assert_eq!(
            FormationSlot {
                position: PosSlot::DC,
                home: Vec2::zero()
            }
            .role(),
            Role::CenterBack
        );
        assert_eq!(
            FormationSlot {
                position: PosSlot::DMC,
                home: Vec2::zero()
            }
            .role(),
            Role::DefensiveMidfielder
        );
        assert_eq!(
            FormationSlot {
                position: PosSlot::AML,
                home: Vec2::zero()
            }
            .role(),
            Role::AttackingMidfielder
        );
        assert_eq!(
            FormationSlot {
                position: PosSlot::ST,
                home: Vec2::zero()
            }
            .role(),
            Role::Striker
        );
    }
}
