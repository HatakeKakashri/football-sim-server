//! Test doubles for the store's injection seams.
//!
//! Shared by the store-level failure tests and the HTTP-level failure
//! test, so both drive the same scripted behaviour.

use super::runner::{MatchOutcome, MatchRunner, manifest_teams};
use super::spawn::{OsThreadSpawner, SpawnError, ThreadSpawner};
use super::stream::{CompletedMatch, HalfBoundaries};
use super::MatchFixture;

/// Runner that panics for match ids starting with `panic-` and
/// succeeds (immediately) for every other id.
#[derive(Debug, Default)]
pub struct ScriptedRunner;

impl MatchRunner for ScriptedRunner {
    fn run(&self, fixture: &MatchFixture, _tick_cap: u64) -> MatchOutcome {
        assert!(
            !fixture.validated_match.id.starts_with("panic-"),
            "injected runner panic for {}",
            fixture.validated_match.id
        );
        MatchOutcome::Finished(completed_match(fixture))
    }
}

/// A finished match with no events and no frames: enough to build the
/// 12-message stream the API serves.
pub fn completed_match(fixture: &MatchFixture) -> CompletedMatch {
    CompletedMatch {
        match_id: fixture.match_id.clone(),
        seed: fixture.validated_match.seed,
        events: Vec::new(),
        frames: Vec::new(),
        roster: Vec::new(),
        teams: manifest_teams(fixture),
        score: (0, 0),
        state_hash: 1,
        added_time_ticks: (0, 0),
        boundaries: HalfBoundaries {
            first_half_end: 162_000,
            full_time: 324_000,
        },
        attack_home: (1, -1),
        attack_away: (-1, 1),
    }
}

/// Spawner that refuses to start threads for match ids starting with
/// `spawn-fail-` and starts every other thread.
#[derive(Debug, Default)]
pub struct ScriptedSpawner;

impl ThreadSpawner for ScriptedSpawner {
    fn spawn(&self, name: String, task: Box<dyn FnOnce() + Send>) -> Result<(), SpawnError> {
        if name.contains("spawn-fail-") {
            return Err(SpawnError {
                message: "injected spawn failure".to_string(),
            });
        }
        OsThreadSpawner.spawn(name, task)
    }
}
