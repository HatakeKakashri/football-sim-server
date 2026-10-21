//! Tests for the match service (server-API ticket 05).
//!
//! The service is driven entirely through the fake runners: no HTTP, no
//! sockets and no direct state poking.

mod queue_tests;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use sim_components::{Formation, Mentality, PosSlot, ProfileKind, Tactic};
use sim_components::fixture::{ValidatedMatch, ValidatedPlayer, ValidatedSide, ValidatedTeam};
use sim_components::PlayerProfile;

use super::*;

use crate::api::registry::MatchFixture;
use crate::api::runner::{manifest_teams, MatchOutcome, MatchRunner};
use crate::api::stream::{CompletedMatch, HalfBoundaries};
use crate::fixtures::FixtureSet;

/// A gate a blocking runner can wait on, so the `Running` state is
/// observable while a match is in flight.
#[derive(Debug)]
struct Gate {
    open: Mutex<bool>,
    cvar: Condvar,
}

impl Gate {
    fn new() -> Self {
        Self {
            open: Mutex::new(false),
            cvar: Condvar::new(),
        }
    }

    #[allow(
        clippy::significant_drop_tightening,
        reason = "the MutexGuard must be held across `cvar.wait`, so it intentionally lives for the whole wait loop"
    )]
    fn wait(&self) {
        let mut guard = self.open.lock().expect("invariant: gate lock is not poisoned");
        while !*guard {
            guard = self.cvar.wait(guard).expect("invariant: gate wait");
        }
    }

    fn open(&self) {
        *self.open.lock().expect("invariant: gate lock is not poisoned") = true;
        self.cvar.notify_all();
    }
}

/// A runner that completes immediately with a fabricated match.
#[derive(Debug, Default)]
struct InstantRunner;

impl MatchRunner for InstantRunner {
    fn run(&self, fixture: &MatchFixture, _tick_cap: u64) -> MatchOutcome {
        MatchOutcome::Finished(completed_for(fixture))
    }
}

/// A runner that panics for match ids starting with `panic-` and
/// succeeds for every other id.
#[derive(Debug, Default)]
struct PanickingRunner;

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
struct TickCapFailureRunner {
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
    fn runs(&self) -> u32 {
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
        }
    }
}

/// A runner that blocks on a gate before completing, counting runs.
struct GatedRunner {
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
    fn gate(&self) -> Arc<Gate> {
        Arc::clone(&self.gate)
    }

    fn runs(&self) -> u32 {
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

/// A runner that blocks on a gate only for matches whose id is
/// `blocked-0` and completes instantly for every other id.
///
/// Used to prove that starting one blocked match does not prevent
/// another match from running to completion on the executor.
struct BlockOnIdRunner {
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
    fn gate(&self) -> Arc<Gate> {
        Arc::clone(&self.gate)
    }

    fn runs(&self) -> u32 {
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
fn completed_for(fixture: &MatchFixture) -> CompletedMatch {
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
fn as_dyn(runner: Arc<impl MatchRunner + 'static>) -> Arc<dyn MatchRunner> {
    runner
}

/// Build a fixture set with two shared teams and one match per id.
fn fixture_set(match_ids: &[&str]) -> FixtureSet {
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
async fn wait_until(
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

#[tokio::test]
async fn unknown_fixture_is_rejected() {
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::new(InstantRunner))).with_cap(1),
    );

    assert_eq!(service.start("nope").await, Err(StartError::UnknownFixture));
    assert_eq!(service.state("nope"), MatchState::NotStarted);
    assert_eq!(service.state("m-0001"), MatchState::NotStarted);
}

#[tokio::test]
async fn known_match_moves_out_of_not_started_and_becomes_ready() {
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::new(InstantRunner))).with_cap(1),
    );

    assert_eq!(service.state("m-0001"), MatchState::NotStarted);

    assert_eq!(service.start("m-0001").await, Ok(StartOutcome::Started));

    wait_until(&service, "m-0001", |state| matches!(state, MatchState::Ready { .. }))
        .await;
    assert!(matches!(service.state("m-0001"), MatchState::Ready { .. }));
}

#[tokio::test]
async fn concurrent_starts_of_the_same_match_run_once() {
    let runner = Arc::new(GatedRunner::default());
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::clone(&runner))).with_cap(1),
    );
    let match_id = "m-0001".to_string();
    let service_a = Arc::clone(&service);
    let service_b = Arc::clone(&service);

    let (res_a, res_b) = tokio::join!(
        service_a.start(&match_id),
        service_b.start(&match_id)
    );
    let outcomes = [
        res_a.expect("start must not error"),
        res_b.expect("start must not error"),
    ];
    assert!(outcomes.contains(&StartOutcome::Started));
    assert!(outcomes.contains(&StartOutcome::AlreadyRunning));

    // The single in-flight run is blocked on the gate: still running.
    wait_until(&service, &match_id, |_state| runner.runs() >= 1)
        .await;
    assert_eq!(runner.runs(), 1);
    assert_eq!(service.state(&match_id), MatchState::Running);

    runner.gate().open();
    wait_until(&service, &match_id, |state| matches!(state, MatchState::Ready { .. }))
        .await;
    assert_eq!(runner.runs(), 1, "the match must not be re-run");
}

#[tokio::test]
async fn ready_only_after_rendering_exposes_manifest_and_gzipped_segments() {
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::new(InstantRunner))).with_cap(1),
    );

    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| matches!(state, MatchState::Ready { .. }))
        .await;

    let state = service.state("m-0001");
    let MatchState::Ready { manifest, segments } = state else {
        panic!("expected Ready, got {state:?}");
    };
    assert_eq!(manifest.match_id, "m-0001");
    assert!(!manifest.build_id.is_empty());
    assert!(!segments.is_empty(), "at least one segment must be rendered");
    for segment in segments.iter() {
        assert!(
            segment.metadata.gzipped_bytes.is_some(),
            "gzipped byte length must be populated"
        );
        assert_eq!(
            segment.metadata.gzipped_bytes,
            Some(segment.gzipped_bytes().len() as u64)
        );
        assert!(!segment.gzipped_bytes().is_empty());
        segment
            .decompress()
            .expect("invariant: stored segment is valid gzip");
    }
}

#[tokio::test]
async fn panicking_runner_fails_and_service_keeps_working() {
    let service = Arc::new(
        MatchService::new(
            fixture_set(&["panic-0", "ok-0"]),
            as_dyn(Arc::new(PanickingRunner)),
        )
        .with_cap(1),
    );

    let _ = service.start("panic-0").await;
    wait_until(&service, "panic-0", |state| matches!(state, MatchState::Failed { .. }))
        .await;

    match service.state("panic-0") {
        MatchState::Failed {
            code,
            message,
            last_good_tick,
        } => {
            assert_eq!(code, "runner_panic", "a panic reports a distinct code");
            assert!(!message.is_empty());
            assert_eq!(last_good_tick, 0);
        }
        other => panic!("expected Failed, got {other:?}"),
    }

    // The service keeps working for other matches.
    let _ = service.start("ok-0").await;
    wait_until(&service, "ok-0", |state| matches!(state, MatchState::Ready { .. }))
        .await;
    assert!(matches!(service.state("ok-0"), MatchState::Ready { .. }));
}

#[tokio::test]
async fn deterministic_failure_is_cached_and_not_rerun() {
    let runner = Arc::new(TickCapFailureRunner::default());
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::clone(&runner))).with_cap(1),
    );

    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| matches!(state, MatchState::Failed { .. }))
        .await;

    let MatchState::Failed {
        code,
        message,
        last_good_tick,
    } = service.state("m-0001")
    else {
        panic!("expected Failed");
    };
    assert_eq!(code, "tick_cap_exceeded");
    assert!(!message.is_empty());
    assert_eq!(runner.runs(), 1);

    // A later start must not re-run the deterministic failure.
    assert_eq!(service.start("m-0001").await, Ok(StartOutcome::AlreadyComplete));
    assert_eq!(runner.runs(), 1, "the failure is cached, not re-run");

    match service.state("m-0001") {
        MatchState::Failed {
            code: cached_code,
            message: cached_message,
            last_good_tick: cached_tick,
        } => {
            assert_eq!(cached_code, code);
            assert_eq!(cached_message, message);
            assert_eq!(cached_tick, last_good_tick);
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn nothing_blocks_on_a_running_match() {
    let runner = Arc::new(BlockOnIdRunner::default());
    let service = Arc::new(
        MatchService::new(
            fixture_set(&["blocked-0", "ok-0"]),
            as_dyn(Arc::clone(&runner)),
        )
        .with_cap(2),
    );

    // The gated match starts and runs (blocked on the gate) without
    // blocking the executor: another match completes in the meantime.
    let _ = service.start("blocked-0").await;
    assert_eq!(service.state("blocked-0"), MatchState::Running);

    let _ = service.start("ok-0").await;
    wait_until(&service, "ok-0", |state| matches!(state, MatchState::Ready { .. }))
        .await;
    assert!(matches!(service.state("ok-0"), MatchState::Ready { .. }));

    runner.gate().open();
    wait_until(&service, "blocked-0", |state| matches!(state, MatchState::Ready { .. }))
        .await;
    assert_eq!(runner.runs(), 2, "both matches ran exactly once");
}
