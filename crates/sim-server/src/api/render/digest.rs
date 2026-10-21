//! Content digests for the render step: the build id, the fixture
//! digest and the SHA-256 helper.
//!
//! The build id is derived from the rendered content alone: the
//! fixture's contents, the format version and every segment's hash.
//! It changes whenever the rendered bytes change and depends on
//! nothing else — no crate version, no git hash, neither of which
//! exists in this repository.

use serde::Serialize;
use sha2::{Digest, Sha256};
use sim_components::{PlayerProfile, PosSlot, ProfileKind};

use super::super::registry::MatchFixture;
use super::FORMAT_VERSION;

/// The content-derived build id of a rendered match.
///
/// SHA-256 over the fixture's contents, the format version and every
/// segment's hash, in segment order.
pub(super) fn build_id(fixture: &MatchFixture, segment_hashes: &[[u8; 32]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"football-sim/render");
    hasher.update(FORMAT_VERSION.to_le_bytes());
    hasher.update(fixture_digest(fixture));
    for hash in segment_hashes {
        hasher.update(hash);
    }
    hex(&hasher.finalize())
}

/// SHA-256 of the given bytes.
pub(super) fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0_u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Lowercase hex encoding of a digest.
pub(super) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The digest of the fixture's contents.
///
/// Everything that can change the rendered bytes is in here: the
/// match's id and seed, both sides' setups and both squads down to
/// the attribute values of every player.
fn fixture_digest(fixture: &MatchFixture) -> Vec<u8> {
    let digest = FixtureDigest {
        match_id: &fixture.match_id,
        seed: fixture.validated_match.seed,
        home: &fixture.validated_match.home,
        away: &fixture.validated_match.away,
        home_setup: SideDigest::from(&fixture.validated_match.home_setup),
        away_setup: SideDigest::from(&fixture.validated_match.away_setup),
        home_team: TeamDigest::from(&fixture.home_team),
        away_team: TeamDigest::from(&fixture.away_team),
    };
    serde_json::to_vec(&digest).expect("invariant: the fixture digest is JSON-serialisable")
}

/// The fixture digest's overall shape.
#[derive(Debug, Serialize)]
struct FixtureDigest<'a> {
    /// The fixture's match id.
    match_id: &'a str,
    /// The fixture's seed.
    seed: u64,
    /// The home team's id.
    home: &'a str,
    /// The away team's id.
    away: &'a str,
    /// The home side's setup.
    home_setup: SideDigest,
    /// The away side's setup.
    away_setup: SideDigest,
    /// The home squad.
    home_team: TeamDigest<'a>,
    /// The away squad.
    away_team: TeamDigest<'a>,
}

/// The fixture digest's per-side setup.
#[derive(Debug, Serialize)]
struct SideDigest {
    /// Formation name.
    formation: &'static str,
    /// Mentality name.
    mentality: &'static str,
    /// Tactic name.
    tactic: &'static str,
}

impl SideDigest {
    /// Digest one side's setup.
    const fn from(setup: &sim_components::fixture::ValidatedSide) -> Self {
        Self {
            formation: setup.formation.name(),
            mentality: setup.mentality.name(),
            tactic: setup.tactic.name(),
        }
    }
}

/// The fixture digest's per-team contents.
#[derive(Debug, Serialize)]
struct TeamDigest<'a> {
    /// The team's id.
    id: &'a str,
    /// The team's name.
    name: &'a str,
    /// The squad, in roster order.
    players: Vec<PlayerDigest<'a>>,
}

impl<'a> TeamDigest<'a> {
    /// Digest one team.
    fn from(team: &'a sim_components::fixture::ValidatedTeam) -> Self {
        Self {
            id: &team.id,
            name: &team.name,
            players: team.players.iter().map(PlayerDigest::from).collect(),
        }
    }
}

/// The fixture digest's per-player contents.
#[derive(Debug, Serialize)]
struct PlayerDigest<'a> {
    /// The fixture's player id.
    id: &'a str,
    /// The player's display name.
    name: &'a str,
    /// The squad number.
    number: u8,
    /// The profile kind (outfield or goalkeeper).
    kind: ProfileKind,
    /// The positions the player is trained for.
    trained_positions: Vec<PosSlot>,
    /// The player's attribute values.
    profile: &'a PlayerProfile,
}

impl<'a> PlayerDigest<'a> {
    /// Digest one player.
    fn from(player: &'a sim_components::fixture::ValidatedPlayer) -> Self {
        Self {
            id: &player.id,
            name: &player.name,
            number: player.number,
            kind: player.kind,
            trained_positions: player.trained_positions.clone(),
            profile: &player.profile,
        }
    }
}
