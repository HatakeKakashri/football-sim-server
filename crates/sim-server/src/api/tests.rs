//! Unit tests for the runner and the shared concurrency default.
//!
//! The full-match end-to-end tests are `#[ignore]`d (minutes-long in
//! debug) and live with the layers they exercise.

use sim_core::Simulation;

use super::default_concurrency_cap;
use super::registry::MatchRegistry;
use super::runner::{MatchOutcome, build_roster, capture_frame, run_match_blocking};

/// A fixture from the single-test registry.
fn test_fixture() -> super::MatchFixture {
    MatchRegistry::single_test("m-0000", 42)
        .fixtures
        .into_values()
        .next()
        .expect("the test registry holds exactly one fixture")
}

#[test]
fn default_concurrency_cap_is_cores_minus_one() {
    let expected = std::thread::available_parallelism()
        .map_or(1, |cores| cores.get().max(2) - 1);
    assert_eq!(default_concurrency_cap(), expected);
    assert!(
        default_concurrency_cap() >= 1,
        "concurrency cap must allow at least one simulation"
    );
}

#[test]
fn short_run_fails_on_tick_cap_with_last_good_tick() {
    let fixture = test_fixture();
    let outcome = run_match_blocking(&fixture, 120);
    let MatchOutcome::Failed {
        code,
        last_good_tick,
        ..
    } = outcome
    else {
        panic!("a 120-tick run cannot reach FullTime");
    };
    assert_eq!(code, "tick_cap_exceeded");
    assert_eq!(last_good_tick, 120);
}

#[test]
fn frame_capture_runs_every_sixth_tick_with_22_players() {
    let fixture = test_fixture();
    let sim = Simulation::new_from_fixture(
        &fixture.validated_match,
        &fixture.home_team,
        &fixture.away_team,
    );
    let roster = build_roster(&sim, &fixture);
    assert_eq!(roster.entries.len(), 22);
    let frame = capture_frame(&sim, &roster.ids).expect("invariant: the frame fits in memory");
    assert_eq!(frame.players.len(), 22);
    assert_eq!(frame.ball.state, "free");
}
