//! Completed-match wire model shared by the runner and the render step.
//!
//! The runner builds a [`CompletedMatch`] from a simulation; the render
//! step turns it into a manifest and its immutable segments. Positions
//! are integer decimetres and ids are the fixture's roster ids, so the
//! same match always renders the same bytes.

use serde::{Deserialize, Serialize};
use sim_core::MatchEvent;

/// The home side, as published on the wire.
pub const SIDE_HOME: &str = "home";

/// The away side, as published on the wire.
pub const SIDE_AWAY: &str = "away";

/// Half boundaries in global ticks, located from the event log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HalfBoundaries {
    /// Global tick of the `half_time` event (last tick of half one).
    pub first_half_end: u64,
    /// Global tick of the `full_time` event (last tick of the match).
    pub full_time: u64,
}

/// Metres → signed integer decimetres, rounded half away from zero
/// (PRD §9.4; maximum error 0.05 m).
#[must_use]
pub fn to_decimetres(metres: f32) -> i32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "pitch coordinates stay within ±100_000 dm; the clamp makes the `as` cast exact"
    )]
    let decimetres = (metres * 10.0).round().clamp(-100_000.0, 100_000.0) as i32;
    decimetres
}

/// Ball state discriminator on the wire (`free` | `possessed` |
/// `in_flight` | `dead`, PRD §9.4).
#[must_use]
pub const fn ball_state_str(state: sim_components::BallState) -> &'static str {
    match state {
        sim_components::BallState::Free => "free",
        sim_components::BallState::Possessed => "possessed",
        sim_components::BallState::InFlight => "in_flight",
        sim_components::BallState::Dead => "dead",
    }
}

/// One 10 Hz animation frame (PRD §9.4): pure positions, no extra
/// frames at event ticks. Coordinates are integer decimetres.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchFrame {
    /// Global tick of the frame.
    pub tick: u64,
    /// Half in progress when the frame was captured.
    pub half: u8,
    /// Ball position, state and possessor roster id (if any).
    pub ball: FrameBall,
    /// All 22 players: roster id + position.
    pub players: Vec<FramePlayer>,
}

/// Ball projection inside a [`MatchFrame`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameBall {
    /// Ball x in decimetres.
    pub x: i32,
    /// Ball y in decimetres.
    pub y: i32,
    /// Ball state discriminator.
    pub state: String,
    /// Roster id of the possessor, if any.
    pub possessor: Option<String>,
}

/// Player projection inside a [`MatchFrame`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FramePlayer {
    /// Stable roster id: the fixture's `player_id`, the same id the
    /// manifest's roster row carries.
    pub id: String,
    /// Player x in decimetres.
    pub x: i32,
    /// Player y in decimetres.
    pub y: i32,
}

/// One roster row published in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterEntry {
    /// Stable roster id: the fixture's `player_id`.
    pub id: String,
    /// Team side (`home` | `away`).
    pub team: String,
    /// Display name from the fixture.
    pub name: String,
    /// Squad number from the fixture.
    pub number: u8,
    /// Formation slot (`GK`, `DC`, …).
    pub position: String,
}

/// One side's lineup setup as published in the manifest (PRD §10.2).
///
/// The three values come from the fixture's per-side setup
/// (`[match.home_setup]` / `[match.away_setup]`, PRD §5.4), so the
/// manifest describes the match that actually ran instead of a
/// hardcoded default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestTeam {
    /// Side (`home` | `away`).
    pub id: String,
    /// Formation name, e.g. `4-3-3`.
    pub formation: String,
    /// Mentality name, e.g. `attack`.
    pub mentality: String,
    /// Tactic name, e.g. `counter_attack`.
    pub tactic: String,
}

impl ManifestTeam {
    /// Build the manifest row for one side from the fixture's setup.
    ///
    /// The setup's spellings are the fixture's own (PRD §5.4), so the
    /// published names round-trip through `Formation::from_name` and
    /// friends.
    #[must_use]
    pub fn from_setup(side: &str, setup: &sim_components::fixture::ValidatedSide) -> Self {
        Self {
            id: side.to_string(),
            formation: setup.formation.name().to_string(),
            mentality: setup.mentality.name().to_string(),
            tactic: setup.tactic.name().to_string(),
        }
    }
}

/// A completed match: everything the render step needs. Built by the
/// simulation runner on the match's blocking thread.
#[derive(Debug, Clone)]
pub struct CompletedMatch {
    /// Fixture match id (PRD §10.2 `match_id`).
    pub match_id: String,
    /// Fixture seed.
    pub seed: u64,
    /// Full event log in emission order.
    pub events: Vec<MatchEvent>,
    /// 10 Hz frames in tick order.
    pub frames: Vec<MatchFrame>,
    /// 22 roster rows: home rows first, then away rows, each in the
    /// fixture's roster order.
    pub roster: Vec<RosterEntry>,
    /// The two sides' lineup setups, home first.
    pub teams: Vec<ManifestTeam>,
    /// Final score `(home, away)`.
    pub score: (u8, u8),
    /// Deterministic state hash at full time.
    pub state_hash: u64,
    /// Added time per half in ticks `(first, second)`.
    pub added_time_ticks: (u64, u64),
    /// Half boundaries located from the event log.
    pub boundaries: HalfBoundaries,
    /// Attack direction per half: `(first_half, second_half)`.
    pub attack_home: (i8, i8),
    /// Attack direction per half: `(first_half, second_half)`.
    pub attack_away: (i8, i8),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimetres_round_half_away_from_zero() {
        assert_eq!(to_decimetres(1.25), 13);
        assert_eq!(to_decimetres(-1.25), -13);
        assert_eq!(to_decimetres(52.5), 525);
        assert_eq!(to_decimetres(0.0), 0);
    }
}
