//! Completed-match wire model shared by the runner and the render step.
//!
//! The runner builds a [`CompletedMatch`] from a simulation; the render
//! step turns it into a manifest and its immutable segments. Positions
//! are integer decimetres and the frames name their players by the
//! manifest's roster index, so the same match always renders the same
//! bytes.

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
///
/// The frame holds the state as a code, not as a string: a string per
/// frame is one allocation per frame the runner does not need, and the
/// wire spelling is derived from the code when the body is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameBallState {
    /// The ball is free to be played.
    Free,
    /// A player possesses the ball.
    Possessed,
    /// The ball is in flight.
    InFlight,
    /// The ball is dead: a restart is pending.
    Dead,
}

impl FrameBallState {
    /// The wire spelling of the state.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Possessed => "possessed",
            Self::InFlight => "in_flight",
            Self::Dead => "dead",
        }
    }
}

impl From<sim_components::BallState> for FrameBallState {
    fn from(state: sim_components::BallState) -> Self {
        match state {
            sim_components::BallState::Free => Self::Free,
            sim_components::BallState::Possessed => Self::Possessed,
            sim_components::BallState::InFlight => Self::InFlight,
            sim_components::BallState::Dead => Self::Dead,
        }
    }
}

/// A player's row in the manifest's roster.
///
/// The manifest publishes the roster as rows — the home side's players
/// first, then the away side's — and a frame names its players by the
/// row they occupy. The index is the mapping between a frame and the
/// manifest: the manifest's roster row `i` is the player a frame's
/// index `i` names.
///
/// The runner captures the index, never the player's id: an id string
/// per player per frame is 1,188,066 allocations and 69 MB more
/// resident memory for one match's frames. The ids live once, in the
/// roster, and the render step borrows them from there when it writes
/// the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RosterIndex(u8);

impl RosterIndex {
    /// The row of a player in the manifest's roster.
    #[must_use]
    pub const fn new(row: u8) -> Self {
        Self(row)
    }

    /// The row's position in the manifest's roster, counted from zero.
    #[must_use]
    pub const fn position(self) -> u8 {
        self.0
    }

    /// The roster row this index names.
    ///
    /// # Panics
    ///
    /// Panics if the roster has no row at this index, which cannot
    /// happen for a frame captured against the same roster.
    #[must_use]
    pub fn row(self, roster: &[RosterEntry]) -> &RosterEntry {
        roster
            .get(usize::from(self.0))
            .expect("invariant: the roster has a row for every captured index")
    }
}

/// One 10 Hz animation frame (PRD §9.4): pure positions, no extra
/// frames at event ticks. Coordinates are integer decimetres.
///
/// The frame names its players by their roster index — the row they
/// occupy in the manifest's roster — so a captured frame holds no
/// player id string. The render step resolves the index back to the
/// fixture id when it writes the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchFrame {
    /// Global tick of the frame.
    pub tick: u64,
    /// Half in progress when the frame was captured.
    pub half: u8,
    /// Ball position, state and possessor (if any).
    pub ball: FrameBall,
    /// All 22 players: roster index + position.
    pub players: Vec<FramePlayer>,
}

/// Ball projection inside a [`MatchFrame`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameBall {
    /// Ball x in decimetres.
    pub x: i32,
    /// Ball y in decimetres.
    pub y: i32,
    /// Ball state.
    pub state: FrameBallState,
    /// The possessor's roster index, if any.
    pub possessor: Option<RosterIndex>,
}

/// Player projection inside a [`MatchFrame`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FramePlayer {
    /// The player's row in the manifest's roster: the manifest's
    /// roster row `i` is the player this index names.
    pub roster_index: RosterIndex,
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
    /// 10 Hz frames in tick order. Every frame names its players by
    /// their roster index.
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
