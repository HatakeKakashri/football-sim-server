//! Tests for the wire shape of an animation frame (server-API ticket
//! 05).
//!
//! A captured frame names its players by their roster row, so the body
//! writer resolves the row back to the fixture id. The bytes on the
//! wire are the bytes the earlier format produced, and that is what
//! these tests pin.

use sim_core::Simulation;

use crate::api::model::{CompletedMatch, HalfBoundaries};
use crate::api::render::render_match;
use crate::api::render_doubles::{committed_fixture, committed_fixtures};
use crate::api::runner::{build_roster, capture_frame, manifest_teams};

/// The body spells out the player's fixture id: the frame names the
/// player by its roster row, and the body writer resolves the row back
/// to the id the manifest publishes.
#[test]
fn the_rendered_body_spells_out_the_players_fixture_id() {
    let set = committed_fixtures();
    let fixture = committed_fixture(&set, "m-0001");
    let sim = Simulation::new_from_fixture(
        &fixture.validated_match,
        &fixture.home_team,
        &fixture.away_team,
    );
    let roster = build_roster(&sim, &fixture);
    let frame = capture_frame(&sim, &roster).expect("invariant: the frame fits in memory");

    let completed = CompletedMatch {
        match_id: fixture.match_id.clone(),
        seed: fixture.validated_match.seed,
        events: Vec::new(),
        frames: vec![frame],
        roster: roster.entries.clone(),
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
    let rendered = render_match(&fixture, &completed);

    let body = rendered.segments[0]
        .decompress()
        .expect("a stored segment decompresses");
    let body: serde_json::Value =
        serde_json::from_slice(&body).expect("a segment body is JSON");
    let players = body["frames"][0]["players"]
        .as_array()
        .expect("a frame lists its players");
    let ids: Vec<&str> = players
        .iter()
        .map(|player| player["id"].as_str().expect("a player carries its id"))
        .collect();

    // The frame lists its players by id, so the body spells out the
    // fixture's ids in that order.
    let mut expected: Vec<&str> = roster
        .entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    expected.sort_unstable();
    assert_eq!(ids, expected, "the body spells out the fixture's player ids");
    assert_eq!(
        body["frames"][0]["ball"]["state"], "free",
        "the ball's state is spelled out as before"
    );
    assert!(
        body["frames"][0]["ball"]["possessor"].is_null(),
        "a ball nobody possesses carries no possessor"
    );
}
