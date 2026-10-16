//! Tactic parameters: `Mentality`, `Tactic` and formation as numeric
//! parameters (PRD §7.2).
//!
//! Every preset maps to a set of numeric parameters that feed the
//! existing utility-AI considerations. There are no special-case code
//! paths keyed on the enum variants outside this module: systems read
//! only the numeric fields below, and every observable tactic effect
//! flows through one of them.
//!
//! # Parameters
//!
//! * `pass_length_factor` — `Possession` shortens passes (factor < 1),
//!   `CounterAttack` lengthens them (factor > 1). Feeds pass kick speed
//!   and preferred pass distance in `sim-ai-player::execution`.
//! * `press_trigger_factor` — `HighPress` widens the press trigger
//!   distance (factor > 1). Feeds the `DistanceToPress` consideration
//!   curve midpoint in `sim-ai-player::brain_default`.
//! * `press_stamina_factor` — `HighPress` raises the press stamina cost
//!   (factor > 1). Feeds press execution in `sim-ai-player::execution`.
//! * `line_height_shift` — metres along the team's attacking axis
//!   (positive = toward the opponent goal). `Attack` shifts the block up,
//!   `Defend` down, `ParkTheBus` down (low block). Applied to formation
//!   slots' home coordinates in `sim-core::simulation::{construct,
//!   formation}`.
//! * `transition_speed_factor` — higher = quicker to attempt after
//!   winning a tackle. `CounterAttack` > 1, `Possession` < 1. Feeds the
//!   post-tackle settle window in `sim-ai-player::execution`.
//!
//! The values below are initial, pre-calibration assumptions (PRD §11)
//! and may change with calibration; the structure is stable.

use bevy_ecs::prelude::Component;
use serde::{Deserialize, Serialize};

use crate::{Formation, Mentality, Tactic};

/// Per-side match setup: formation, mentality, tactic (PRD §5.1: the
/// match registry carries per-side `formation`, `mentality`, `tactic`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TeamSetup {
    pub formation: Formation,
    pub mentality: Mentality,
    pub tactic: Tactic,
}

impl Default for TeamSetup {
    /// Canonical default: 4-4-2, Balance, Possession — the hardcoded
    /// setup `Simulation::new` used before per-side setups existed.
    fn default() -> Self {
        Self {
            formation: Formation::FourFourTwo,
            mentality: Mentality::Balance,
            tactic: Tactic::Possession,
        }
    }
}

/// Numeric parameters derived from a team's `Mentality` + `Tactic`.
///
/// Pure function of the two enums: every variant of both enums is
/// covered, and there are no side effects in any arm.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct TacticParameters {
    /// Multiplier on the default pass kick length (`1.0` = neutral).
    pub pass_length_factor: f32,
    /// Multiplier on the default press trigger distance (`1.0` =
    /// neutral; default distance is 8 m).
    pub press_trigger_factor: f32,
    /// Multiplier on the default press stamina cost (`1.0` = neutral).
    pub press_stamina_factor: f32,
    /// Line height shift in metres along the attacking axis (positive
    /// = toward the opponent goal; `0.0` = neutral).
    pub line_height_shift: f32,
    /// Multiplier on the post-tackle settle speed (`1.0` = neutral;
    /// higher = quicker to attempt after winning a tackle).
    pub transition_speed_factor: f32,
}

impl TacticParameters {
    /// Neutral reference: Balance mentality + Possession tactic — every
    /// factor is `1.0` and the line shift is `0.0`.
    #[must_use]
    pub const fn neutral() -> Self {
        Self {
            pass_length_factor: 1.0,
            press_trigger_factor: 1.0,
            press_stamina_factor: 1.0,
            line_height_shift: 0.0,
            transition_speed_factor: 1.0,
        }
    }

    /// Derive the numeric parameters for a `Mentality` + `Tactic`
    /// pairing.
    #[must_use]
    pub fn from_setup(mentality: Mentality, tactic: Tactic) -> Self {
        let base = Self::neutral();

        // --- Mentality: line height and risk appetite (PRD §7.2) ---
        let mentality_line_shift = match mentality {
            // Push the whole block up the pitch, closer to the opponent
            // goal: a higher defensive line and a higher pressing line.
            Mentality::Attack => 6.0,
            Mentality::Balance => 0.0,
            // Pull the block back toward the own goal: a deeper
            // defensive line, fewer through balls in behind.
            Mentality::Defend => -6.0,
        };

        // --- Tactic presets (PRD §7.2) ---
        let (pass_factor, press_trigger, press_stamina, tactic_line, transition) = match tactic {
            // Short passes, patient build-up: shorter passes and slower
            // transitions (longer post-tackle settle window).
            Tactic::Possession => (0.6, 1.0, 1.0, 0.0, 0.5),
            // Direct passing, fast transitions: longer passes and a
            // shorter post-tackle settle window.
            Tactic::CounterAttack => (1.5, 1.0, 1.0, 0.0, 3.0),
            // Larger press trigger distance and higher stamina cost.
            Tactic::HighPress => (1.0, 1.75, 2.0, 0.0, 1.0),
            // Compact low block: the deepest defensive line of any
            // preset.
            Tactic::ParkTheBus => (1.0, 1.0, 1.0, -10.0, 0.75),
        };

        Self {
            pass_length_factor: base.pass_length_factor * pass_factor,
            press_trigger_factor: base.press_trigger_factor * press_trigger,
            press_stamina_factor: base.press_stamina_factor * press_stamina,
            line_height_shift: base.line_height_shift + mentality_line_shift + tactic_line,
            transition_speed_factor: base.transition_speed_factor * transition,
        }
    }

    /// The `DistanceToPress` consideration threshold (default 8 m)
    /// scaled by this team's press trigger factor.
    #[must_use]
    pub const fn press_trigger_distance(self) -> f32 {
        8.0 * self.press_trigger_factor
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests compare freshly-constructed parameter values against the literals they were built from; equality is exact"
)]
mod tactic_params_tests {
    use super::*;

    /// `Balance`/`Possession` is the baseline preset; every other preset
    /// must differ from it on at least one signature parameter.
    #[test]
    fn baseline_is_balance_possession() {
        let baseline = TacticParameters::from_setup(Mentality::Balance, Tactic::Possession);
        // The baseline itself: short passes (0.6) and slow transitions (0.5).
        assert!((baseline.pass_length_factor - 0.6).abs() < f32::EPSILON);
        assert!((baseline.transition_speed_factor - 0.5).abs() < f32::EPSILON);
        assert!((baseline.line_height_shift - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn counterattack_lengthens_passes_possession_shortens_them() {
        let possession = TacticParameters::from_setup(Mentality::Balance, Tactic::Possession);
        let counter_attack =
            TacticParameters::from_setup(Mentality::Balance, Tactic::CounterAttack);
        assert!(
            counter_attack.pass_length_factor > 1.0,
            "CounterAttack lengthens pass length: got {}",
            counter_attack.pass_length_factor
        );
        assert!(
            possession.pass_length_factor < 1.0,
            "Possession shortens pass length: got {}",
            possession.pass_length_factor
        );
        assert!(
            counter_attack.pass_length_factor > possession.pass_length_factor,
            "CounterAttack passes longer than Possession passes: {} vs {}",
            counter_attack.pass_length_factor,
            possession.pass_length_factor
        );
    }

    #[test]
    fn high_press_widens_trigger_and_raises_cost() {
        let high_press = TacticParameters::from_setup(Mentality::Balance, Tactic::HighPress);
        let balance = TacticParameters::from_setup(Mentality::Balance, Tactic::Possession);
        assert!(
            high_press.press_trigger_factor > 1.0,
            "HighPress widens the press trigger: got {}",
            high_press.press_trigger_factor
        );
        assert!(
            high_press.press_stamina_factor > 1.0,
            "HighPress raises the press stamina cost: got {}",
            high_press.press_stamina_factor
        );
        assert!(
            high_press.press_trigger_factor > balance.press_trigger_factor
                && high_press.press_stamina_factor > balance.press_stamina_factor,
            "HighPress differs from Balance on both press parameters"
        );
        assert!((high_press.press_trigger_distance() - 14.0).abs() < f32::EPSILON);
    }

    #[test]
    fn park_the_bus_lowers_the_defensive_line() {
        let park_the_bus = TacticParameters::from_setup(Mentality::Balance, Tactic::ParkTheBus);
        let balance = TacticParameters::from_setup(Mentality::Balance, Tactic::Possession);
        assert!(
            park_the_bus.line_height_shift < 0.0,
            "ParkTheBus lowers the line: got {}",
            park_the_bus.line_height_shift
        );
        assert!(
            park_the_bus.line_height_shift < balance.line_height_shift,
            "ParkTheBus line lower than Balance: {} vs {}",
            park_the_bus.line_height_shift,
            balance.line_height_shift
        );
    }

    #[test]
    fn mentality_shifts_the_line_up_or_down() {
        let attack = TacticParameters::from_setup(Mentality::Attack, Tactic::Possession);
        let balance = TacticParameters::from_setup(Mentality::Balance, Tactic::Possession);
        let defend = TacticParameters::from_setup(Mentality::Defend, Tactic::Possession);
        assert!(
            attack.line_height_shift > 0.0,
            "Attack shifts the line up: got {}",
            attack.line_height_shift
        );
        assert!(
            defend.line_height_shift < 0.0,
            "Defend shifts the line down: got {}",
            defend.line_height_shift
        );
        assert!(
            attack.line_height_shift > balance.line_height_shift
                && balance.line_height_shift > defend.line_height_shift,
            "line height must order Attack > Balance > Defend: {} vs {} vs {}",
            attack.line_height_shift,
            balance.line_height_shift,
            defend.line_height_shift
        );
    }

    #[test]
    fn counter_attack_transitions_quickest_possession_slowest() {
        let counter_attack =
            TacticParameters::from_setup(Mentality::Balance, Tactic::CounterAttack);
        let possession = TacticParameters::from_setup(Mentality::Balance, Tactic::Possession);
        // Baseline (Balance/Possession) transitions slowest (0.5), CounterAttack quickest (3.0).
        assert!(
            counter_attack.transition_speed_factor > possession.transition_speed_factor,
            "CounterAttack transitions quicker than Possession: {} vs {}",
            counter_attack.transition_speed_factor,
            possession.transition_speed_factor
        );
        assert!(
            possession.transition_speed_factor < 1.0,
            "Possession transitions slower than neutral: {}",
            possession.transition_speed_factor
        );
    }

    #[test]
    fn mentality_and_tactic_line_shifts_add() {
        // Mentality and tactic line shifts add: Defend (-6) + ParkTheBus (-10) = -16.
        let defend_park = TacticParameters::from_setup(Mentality::Defend, Tactic::ParkTheBus);
        let park_only = TacticParameters::from_setup(Mentality::Balance, Tactic::ParkTheBus);
        assert!(
            defend_park.line_height_shift < park_only.line_height_shift,
            "Defend + ParkTheBus deeper than ParkTheBus alone: {} vs {}",
            defend_park.line_height_shift,
            park_only.line_height_shift
        );
        assert!(
            (defend_park.line_height_shift - -16.0).abs() < f32::EPSILON,
            "Defend + ParkTheBus = -16: got {}",
            defend_park.line_height_shift
        );
        // Attack (+6) + ParkTheBus (-10) = -4: Attack partially cancels ParkTheBus.
        let attack_park = TacticParameters::from_setup(Mentality::Attack, Tactic::ParkTheBus);
        assert!(
            (attack_park.line_height_shift - -4.0).abs() < f32::EPSILON,
            "Attack + ParkTheBus = -4: got {}",
            attack_park.line_height_shift
        );
    }
    #[test]
    fn every_enum_pair_is_covered() {
        for mentality in [
            Mentality::Attack,
            Mentality::Balance,
            Mentality::Defend,
        ] {
            for tactic in [
                Tactic::Possession,
                Tactic::CounterAttack,
                Tactic::HighPress,
                Tactic::ParkTheBus,
            ] {
                let params = TacticParameters::from_setup(mentality, tactic);
                assert!(
                    params.pass_length_factor > 0.0,
                    "pass length factor must stay positive for {mentality:?}/{tactic:?}"
                );
                assert!(
                    params.press_trigger_factor > 0.0,
                    "press trigger factor must stay positive for {mentality:?}/{tactic:?}"
                );
                assert!(
                    params.press_stamina_factor > 0.0,
                    "press stamina factor must stay positive for {mentality:?}/{tactic:?}"
                );
                assert!(
                    params.transition_speed_factor > 0.0,
                    "transition factor must stay positive for {mentality:?}/{tactic:?}"
                );
            }
        }
    }
}
