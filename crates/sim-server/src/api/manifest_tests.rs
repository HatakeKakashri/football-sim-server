//! Manifest content tests (ticket 02).
//!
//! The manifest must describe the fixture that actually ran: each side's
//! formation, mentality and tactic come from the fixture, the roster
//! rows are the fixture's own players, and the player ids in the frames
//! are the manifest's roster ids.

use std::path::{Path, PathBuf};

use sim_core::Simulation;

use super::registry::MatchFixture;
use super::runner::{build_roster, capture_frame, manifest_teams};
use super::stream::{CompletedMatch, HalfBoundaries, build_stream};
use crate::fixtures::{FixtureSet, load};

/// The committed fixtures (PRD §5), resolved from this crate's manifest
/// directory so the test does not depend on the working directory.
fn committed_fixtures() -> FixtureSet {
    let dir: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    load(&dir).expect("committed fixtures must load")
}

/// One committed match fixture with both validated teams.
fn fixture_for(set: &FixtureSet, match_id: &str) -> MatchFixture {
    let validated_match = set
        .matches
        .get(match_id)
        .unwrap_or_else(|| panic!("committed match fixture {match_id} must exist"));
    let home_team = set
        .teams
        .get(&validated_match.home)
        .unwrap_or_else(|| panic!("home team of {match_id} must exist"));
    let away_team = set
        .teams
        .get(&validated_match.away)
        .unwrap_or_else(|| panic!("away team of {match_id} must exist"));
    MatchFixture {
        match_id: match_id.to_string(),
        validated_match: validated_match.clone(),
        home_team: home_team.clone(),
        away_team: away_team.clone(),
    }
}

/// The manifest message of the stream for one fixture match.
///
/// The match is not simulated: the manifest only carries the fixture's
/// setups and the roster the world was built with, both of which exist
/// before the first tick.
fn manifest_for(set: &FixtureSet, match_id: &str) -> serde_json::Value {
    let fixture = fixture_for(set, match_id);
    let sim = Simulation::new_from_fixture(
        &fixture.validated_match,
        &fixture.home_team,
        &fixture.away_team,
    );
    let roster = build_roster(&sim, &fixture);
    let completed = CompletedMatch {
        match_id: fixture.match_id.clone(),
        seed: fixture.validated_match.seed,
        events: Vec::new(),
        frames: Vec::new(),
        roster: roster.entries,
        teams: manifest_teams(&fixture),
        score: (0, 0),
        state_hash: 0,
        added_time_ticks: (0, 0),
        boundaries: HalfBoundaries {
            first_half_end: 165_000,
            full_time: 330_100,
        },
        attack_home: (1, -1),
        attack_away: (-1, 1),
    };
    build_stream(&completed)
        .into_iter()
        .find(|message| message.event == "manifest")
        .expect("the stream always starts with the manifest")
        .data
}

/// The manifest's `teams` rows as `(id, formation, mentality, tactic)`.
fn manifest_teams_of(manifest: &serde_json::Value) -> Vec<(String, String, String, String)> {
    manifest["teams"]
        .as_array()
        .expect("the manifest lists both teams")
        .iter()
        .map(|team| {
            (
                team["id"].as_str().expect("team id").to_string(),
                team["formation"].as_str().expect("formation").to_string(),
                team["mentality"].as_str().expect("mentality").to_string(),
                team["tactic"].as_str().expect("tactic").to_string(),
            )
        })
        .collect()
}

/// The manifest's roster rows as `(id, team, name, number, position)`.
fn manifest_roster_of(manifest: &serde_json::Value) -> Vec<(String, String, String, u64, String)> {
    manifest["roster"]
        .as_array()
        .expect("the manifest lists the roster")
        .iter()
        .map(|row| {
            (
                row["id"].as_str().expect("roster id").to_string(),
                row["team"].as_str().expect("roster team").to_string(),
                row["name"].as_str().expect("roster name").to_string(),
                row["number"].as_u64().expect("roster number"),
                row["position"].as_str().expect("roster position").to_string(),
            )
        })
        .collect()
}

/// Owned copy of an expected-row table.
fn owned(rows: &[(&str, &str, &str, u64, &str)]) -> Vec<(String, String, String, u64, String)> {
    rows.iter()
        .map(|(id, team, name, number, position)| {
            (
                id.to_string(),
                team.to_string(),
                name.to_string(),
                *number,
                position.to_string(),
            )
        })
        .collect()
}

#[test]
fn manifest_publishes_both_sides_setups_from_the_fixture() {
    let set = committed_fixtures();

    // m-0001 (fixtures/matches/m-0001.toml): home 4-4-2 / balance /
    // possession, away 4-3-3 / attack / counter_attack.
    assert_eq!(
        manifest_teams_of(&manifest_for(&set, "m-0001")),
        vec![
            (
                "home".to_string(),
                "4-4-2".to_string(),
                "balance".to_string(),
                "possession".to_string(),
            ),
            (
                "away".to_string(),
                "4-3-3".to_string(),
                "attack".to_string(),
                "counter_attack".to_string(),
            ),
        ],
        "m-0001 must publish its own home and away setups"
    );

    // m-0002 (fixtures/matches/m-0002.toml): home 4-4-2 / defend /
    // park_the_bus, away 4-4-2 / attack / counter_attack.
    assert_eq!(
        manifest_teams_of(&manifest_for(&set, "m-0002")),
        vec![
            (
                "home".to_string(),
                "4-4-2".to_string(),
                "defend".to_string(),
                "park_the_bus".to_string(),
            ),
            (
                "away".to_string(),
                "4-4-2".to_string(),
                "attack".to_string(),
                "counter_attack".to_string(),
            ),
        ],
        "m-0002 must publish its own home and away setups"
    );

    // m-0003 (fixtures/matches/m-0003.toml): the mirror match, both
    // sides on the default 4-4-2 / balance / possession.
    assert_eq!(
        manifest_teams_of(&manifest_for(&set, "m-0003")),
        vec![
            (
                "home".to_string(),
                "4-4-2".to_string(),
                "balance".to_string(),
                "possession".to_string(),
            ),
            (
                "away".to_string(),
                "4-4-2".to_string(),
                "balance".to_string(),
                "possession".to_string(),
            ),
        ],
        "m-0003 must publish its own home and away setups"
    );
}

#[test]
fn manifest_roster_lists_the_fixture_players_by_roster_index() {
    let set = committed_fixtures();

    // m-0001: t-alpha (4-4-2) against t-bravo (4-3-3). The rows are the
    // fixture's own players, in the fixture's roster order; the position
    // is the formation slot the player occupies.
    assert_eq!(
        manifest_roster_of(&manifest_for(&set, "m-0001")),
        owned(&[
            ("p-alpha-01", "home", "R. Gloves", 1, "GK"),
            ("p-alpha-02", "home", "D. Left", 2, "DL"),
            ("p-alpha-03", "home", "D. Centre", 3, "DC"),
            ("p-alpha-06", "home", "M. Wing", 6, "DC"),
            ("p-alpha-05", "home", "D. Right", 5, "DR"),
            ("p-alpha-04", "home", "D. Cover", 4, "ML"),
            ("p-alpha-07", "home", "M. Pivot", 7, "MC"),
            ("p-alpha-08", "home", "M. Box", 8, "MC"),
            ("p-alpha-09", "home", "W. Right", 9, "MR"),
            ("p-alpha-10", "home", "S. Target", 10, "ST"),
            ("p-alpha-11", "home", "S. Poacher", 11, "ST"),
            ("p-bravo-01", "away", "B. Hands", 1, "GK"),
            ("p-bravo-02", "away", "B. Full", 2, "DL"),
            ("p-bravo-03", "away", "B. Stopper", 3, "DC"),
            ("p-bravo-06", "away", "B. Flank", 6, "DC"),
            ("p-bravo-05", "away", "B. Rightback", 5, "DR"),
            ("p-bravo-04", "away", "B. Sweeper", 4, "MC"),
            ("p-bravo-07", "away", "B. Pivot", 7, "MC"),
            ("p-bravo-08", "away", "B. Mid", 8, "MC"),
            ("p-bravo-09", "away", "B. Winger", 9, "AML"),
            ("p-bravo-10", "away", "B. Finisher", 10, "ST"),
            ("p-bravo-11", "away", "B. Fox", 11, "AMR"),
        ]),
        "m-0001 must publish the fixture's own players"
    );

    // m-0002: t-delta against t-echo — a different pair of squads, so
    // the roster ids follow the fixture too.
    let roster = manifest_roster_of(&manifest_for(&set, "m-0002"));
    assert_eq!(roster.len(), 22, "m-0002 must publish 22 roster rows");
    assert_eq!(roster[0].0, "p-delta-01");
    assert_eq!(roster[0].2, "D. Gloves");
    assert_eq!(roster[11].0, "p-echo-01");
    assert_eq!(roster[11].2, "E. Gloves");
    assert!(
        roster.iter().all(|(id, ..)| id.starts_with("p-delta-")
            || id.starts_with("p-echo-")),
        "m-0002 must not publish ids from another fixture"
    );

    // m-0003: the mirror match — the same squad on both sides, so the
    // two sides carry the same fixture player ids.
    let roster = manifest_roster_of(&manifest_for(&set, "m-0003"));
    assert_eq!(roster.len(), 22, "m-0003 must publish 22 roster rows");
    assert_eq!(roster[0].0, "p-alpha-01");
    assert_eq!(roster[11].0, "p-alpha-01");
    assert_eq!(roster[0].1, "home");
    assert_eq!(roster[11].1, "away");
}

#[test]
fn frame_player_ids_are_the_manifest_roster_ids() {
    let set = committed_fixtures();

    for match_id in ["m-0001", "m-0002", "m-0003"] {
        let fixture = fixture_for(&set, match_id);
        let sim = Simulation::new_from_fixture(
            &fixture.validated_match,
            &fixture.home_team,
            &fixture.away_team,
        );
        let roster = build_roster(&sim, &fixture);
        let frame = capture_frame(&sim, &roster.ids);

        let mut frame_ids: Vec<String> = frame.players.iter().map(|p| p.id.clone()).collect();
        let mut roster_ids: Vec<String> = roster.entries.iter().map(|e| e.id.clone()).collect();
        frame_ids.sort();
        roster_ids.sort();

        assert_eq!(frame_ids, roster_ids, "{match_id}: the frame names the manifest's players");
        assert!(
            frame_ids.iter().all(|id| !id.starts_with("e-")),
            "{match_id}: no frame may carry an invented id"
        );
    }
}

/// One side's players in world spawn order, as `(roster index, id)`.
fn spawned_players<'a>(sim: &'a Simulation, side: &str) -> Vec<(u8, &'a str)> {
    let team_id = if side == "home" {
        sim_components::TeamId(0)
    } else {
        sim_components::TeamId(1)
    };
    let mut players: Vec<(u64, &sim_components::PlayerIdentity)> = sim
        .world()
        .iter_entities()
        .filter_map(|entity| {
            let player = entity.get::<sim_components::Player>()?;
            let identity = entity.get::<sim_components::PlayerIdentity>()?;
            (player.team_id == team_id).then_some((entity.id().to_bits(), identity))
        })
        .collect();
    players.sort_by_key(|(bits, _)| *bits);
    players
        .into_iter()
        .map(|(_, identity)| (identity.roster_index, identity.id.as_str()))
        .collect()
}

/// Guard: the world must spawn each side's players in the fixture's
/// roster order. The manifest's roster row `i` and the world's `i`-th
/// player of that side must be the same person, so the two orders may
/// never diverge.
#[test]
fn world_spawn_order_matches_the_fixture_player_order() {
    let set = committed_fixtures();

    for match_id in ["m-0001", "m-0002", "m-0003"] {
        let fixture = fixture_for(&set, match_id);
        let sim = Simulation::new_from_fixture(
            &fixture.validated_match,
            &fixture.home_team,
            &fixture.away_team,
        );

        for (side, team) in [("home", &fixture.home_team), ("away", &fixture.away_team)] {
            let expected: Vec<(u8, &str)> = team
                .players
                .iter()
                .enumerate()
                .map(|(index, player)| {
                    (
                        u8::try_from(index).expect("invariant: a squad has 11 players"),
                        player.id.as_str(),
                    )
                })
                .collect();
            assert_eq!(
                spawned_players(&sim, side),
                expected,
                "{match_id} {side}: the world's spawn order must follow the fixture's player order"
            );
        }
    }
}
