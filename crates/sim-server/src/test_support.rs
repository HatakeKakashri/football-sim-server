//! Shared test doubles: fixture builders and fake match runners.
//!
//! The match service's own tests (server-API ticket 05) and the HTTP
//! layer's status-matrix tests (server-API ticket 08) drive the same
//! fakes, so they live here instead of being written twice.
//!
//! Nothing in this module is compiled into the binary: it is a
//! `#[cfg(test)]` module.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use sim_components::fixture::{ValidatedMatch, ValidatedPlayer, ValidatedSide, ValidatedTeam};
use sim_components::{Formation, Mentality, PlayerProfile, PosSlot, ProfileKind, Tactic};

use crate::api::registry::MatchFixture;
use crate::api::runner::{FailureKind, manifest_teams, MatchOutcome, MatchRunner};
use crate::api::model::{CompletedMatch, HalfBoundaries};
use crate::fixtures::FixtureSet;
use crate::service::{MatchService, MatchState};

/// A gate a blocking runner can wait on, so the `Running` state is
/// observable while a match is in flight.
#[derive(Debug)]
pub struct Gate {
    open: Mutex<bool>,
    cvar: Condvar,
}

impl Gate {
    /// A closed gate.
    pub fn new() -> Self {
        Self {
            open: Mutex::new(false),
            cvar: Condvar::new(),
        }
    }

    /// Wait until the gate is open.
    #[allow(
        clippy::significant_drop_tightening,
        reason = "the MutexGuard must be held across `cvar.wait`, so it intentionally lives for the whole wait loop"
    )]
    pub fn wait(&self) {
        let mut guard = self.open.lock().expect("invariant: gate lock is not poisoned");
        while !*guard {
            guard = self.cvar.wait(guard).expect("invariant: gate wait");
        }
    }

    /// Open the gate: the waiting run continues.
    pub fn open(&self) {
        *self.open.lock().expect("invariant: gate lock is not poisoned") = true;
        self.cvar.notify_all();
    }
}

/// A runner that completes immediately with a fabricated match.
#[derive(Debug, Default)]
pub struct InstantRunner;

impl MatchRunner for InstantRunner {
    fn run(&self, fixture: &MatchFixture, _tick_cap: u64) -> MatchOutcome {
        MatchOutcome::Finished(completed_for(fixture))
    }
}

/// A runner that panics for match ids starting with `panic-` and
/// succeeds for every other id.
#[derive(Debug, Default)]
pub struct PanickingRunner;

impl MatchRunner for PanickingRunner {
    fn run(&self, fixture: &MatchFixture, _tick_cap: u64) -> MatchOutcome {
        assert!(
            !fixture.validated_match.id.starts_with("panic-"),
            "injected panic for {}",
            fixture.validated_match.id
        );            MatchOutcome::Finished(completed_for(fixture))
    }
}

/// A runner that always reports a deterministic tick-cap failure and
/// counts how many times it was run.
#[derive(Debug)]
pub struct TickCapFailureRunner {
    runs: AtomicU32,
}

impl Default for TickCapFailureRunner {
    fn default() -> Self {
        Self {
            runs: AtomicU32::new(0),
        }
    }
}

impl TickCapFailureRunner {
    /// How many times the runner was asked to run a match.
    pub fn runs(&self) -> u32 {
        self.runs.load(Ordering::SeqCst)
    }
}

impl MatchRunner for TickCapFailureRunner {
    fn run(&self, _fixture: &MatchFixture, tick_cap: u64) -> MatchOutcome {
        self.runs.fetch_add(1, Ordering::SeqCst);
        MatchOutcome::Failed {
            code: "tick_cap_exceeded".to_string(),
            message: format!("match did not reach FullTime within {tick_cap} ticks"),
            last_good_tick: tick_cap,
            kind: FailureKind::Deterministic,
        }
    }
}

/// A runner that always reports a transient resource failure, counting
/// the runs.
#[derive(Debug, Default)]
pub struct AlwaysTransientRunner {
    runs: AtomicU32,
}

impl AlwaysTransientRunner {
    /// How many times the runner was asked to run a match.
    pub fn runs(&self) -> u32 {
        self.runs.load(Ordering::SeqCst)
    }
}

impl MatchRunner for AlwaysTransientRunner {
    fn run(&self, _fixture: &MatchFixture, _tick_cap: u64) -> MatchOutcome {
        self.runs.fetch_add(1, Ordering::SeqCst);
        MatchOutcome::Failed {
            code: "resource_exhausted".to_string(),
            message: "could not allocate the frame buffer".to_string(),
            last_good_tick: 120,
            kind: FailureKind::Transient,
        }
    }
}

/// A runner that blocks on a gate before completing, counting runs.
pub struct GatedRunner {
    gate: Arc<Gate>,
    runs: AtomicU32,
}

impl Default for GatedRunner {
    fn default() -> Self {
        Self {
            gate: Arc::new(Gate::new()),
            runs: AtomicU32::new(0),
        }
    }
}

impl std::fmt::Debug for GatedRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatedRunner")
            .field("runs", &self.runs)
            .finish_non_exhaustive()
    }
}

impl GatedRunner {
    /// The gate the runner waits on.
    pub fn gate(&self) -> Arc<Gate> {
        Arc::clone(&self.gate)
    }

    /// How many times the runner was asked to run a match.
    pub fn runs(&self) -> u32 {
        self.runs.load(Ordering::SeqCst)
    }
}

impl MatchRunner for GatedRunner {
    fn run(&self, fixture: &MatchFixture, _tick_cap: u64) -> MatchOutcome {
        self.runs.fetch_add(1, Ordering::SeqCst);
        self.gate.wait();
        MatchOutcome::Finished(completed_for(fixture))
    }
}

/// Opens a gated runner's gate when the guard is dropped, so a blocked
/// run cannot outlive the test that started it.
pub struct GateGuard(Arc<GatedRunner>);

impl GateGuard {
    /// A guard that opens the runner's gate when it is dropped.
    pub fn new(runner: Arc<GatedRunner>) -> Self {
        Self(runner)
    }

    /// Open the gate now.
    pub fn open(&self) {
        self.0.gate().open();
    }
}

impl Drop for GateGuard {
    fn drop(&mut self) {
        self.0.gate().open();
    }
}

/// A runner that blocks on a gate only for matches whose id is
/// `blocked-0` and completes instantly for every other id.
///
/// Used to prove that starting one blocked match does not prevent
/// another match from running to completion on the executor.
pub struct BlockOnIdRunner {
    gate: Arc<Gate>,
    runs: AtomicU32,
}

impl Default for BlockOnIdRunner {
    fn default() -> Self {
        Self {
            gate: Arc::new(Gate::new()),
            runs: AtomicU32::new(0),
        }
    }
}

impl std::fmt::Debug for BlockOnIdRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockOnIdRunner")
            .field("runs", &self.runs)
            .finish_non_exhaustive()
    }
}

impl BlockOnIdRunner {
    /// The gate the blocked match waits on.
    pub fn gate(&self) -> Arc<Gate> {
        Arc::clone(&self.gate)
    }

    /// How many times the runner was asked to run a match.
    pub fn runs(&self) -> u32 {
        self.runs.load(Ordering::SeqCst)
    }
}

impl MatchRunner for BlockOnIdRunner {
    fn run(&self, fixture: &MatchFixture, _tick_cap: u64) -> MatchOutcome {
        self.runs.fetch_add(1, Ordering::SeqCst);
        if fixture.validated_match.id == "blocked-0" {
            self.gate.wait();
        }
        MatchOutcome::Finished(completed_for(fixture))
    }
}

/// A fabricated completed match sufficient for rendering.
pub fn completed_for(fixture: &MatchFixture) -> CompletedMatch {
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

/// Coerce a concrete runner into the trait object used by the service.
pub fn as_dyn(runner: Arc<impl MatchRunner + 'static>) -> Arc<dyn MatchRunner> {
    runner
}

/// Build a fixture set with two shared teams and one match per id.
pub fn fixture_set(match_ids: &[&str]) -> FixtureSet {
    let home = test_team("t-home", "Home FC", "home");
    let away = test_team("t-away", "Away FC", "away");
    let setup = ValidatedSide {
        formation: Formation::FourFourTwo,
        mentality: Mentality::Balance,
        tactic: Tactic::Possession,
    };
    let mut teams = HashMap::new();
    teams.insert(home.id.clone(), home);
    teams.insert(away.id.clone(), away);
    let mut matches = HashMap::new();
    for (index, id) in match_ids.iter().enumerate() {
        matches.insert(
            id.to_string(),
            ValidatedMatch {
                id: id.to_string(),
                seed: 42 + u64::try_from(index).expect("invariant: fixture index fits in u64"),
                home: "t-home".to_string(),
                away: "t-away".to_string(),
                home_setup: setup.clone(),
                away_setup: setup.clone(),
            },
        );
    }
    FixtureSet { teams, matches }
}

/// One validated team with 11 players using default profiles.
fn test_team(id: &str, name: &str, prefix: &str) -> ValidatedTeam {
    let players: Vec<ValidatedPlayer> = (0..11)
        .map(|i| ValidatedPlayer {
            id: format!("{prefix}-{i:02}"),
            name: format!("Player {i}"),
            number: u8::try_from(i + 1).expect("invariant: player number fits in u8"),
            kind: if i == 0 {
                ProfileKind::Goalkeeper
            } else {
                ProfileKind::Outfield
            },
            trained_positions: if i == 0 {
                vec![PosSlot::GK]
            } else {
                vec![PosSlot::MC]
            },
            profile: if i == 0 {
                PlayerProfile::default_goalkeeper()
            } else {
                PlayerProfile::default_outfield()
            },
        })
        .collect();
    ValidatedTeam {
        id: id.to_string(),
        name: name.to_string(),
        players,
    }
}

/// Poll until `pred` holds for `match_id`'s state, then return.
pub async fn wait_until(
    service: &MatchService,
    match_id: &str,
    mut pred: impl FnMut(&MatchState) -> bool,
) {
    for _ in 0..500 {
        if pred(&service.state(match_id)) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "timed out waiting for match {match_id} state; last state: {:?}",
        service.state(match_id)
    );
}
