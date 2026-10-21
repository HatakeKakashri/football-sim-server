//! Match service: one match lifecycle behind a single interface.
//!
//! The simulation is hidden behind a [`MatchRunner`] trait. The real
//! adapter ([`crate::api::runner::SimulationRunner`]) wraps the existing
//! simulation; tests inject fakes. The service tracks each match through
//! `NotStarted`, `Queued`, `Running`, `Ready` and `Failed`.
//!
//! Starting is single-flight and runs immediately (queueing under a cap
//! is ticket 06). The simulation and rendering run together on a blocking
//! thread ([`tokio::task::spawn_blocking`]) so the async executor never
//! does CPU work. A panic on that thread is caught and becomes `Failed`
//! with the distinct `runner_panic` code; deterministic failures (tick
//! cap, invariant violation, panic) are cached with their `code`,
//! `message` and `last_good_tick` for the life of the process. `Ready`
//! is set only after rendering is complete, and the raw completed match
//! is dropped afterwards.
//!
//! The service does not use any wait or notify primitive: callers read
//! state, and the state map's lock is never held across an await point.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use sim_core::TICK_CAP;

use crate::api::registry::{MatchFixture, MatchRegistry};
use crate::api::render::{Manifest, RenderedSegment, render_match};
use crate::api::runner::{MatchOutcome, MatchRunner, run_catching_panic};
use crate::fixtures::FixtureSet;

/// Terminal outcome of [`MatchService::start`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOutcome {
    /// This caller initiated the run.
    Started,
    /// A run is already in flight for this match (single-flight).
    AlreadyRunning,
    /// The match is already terminal (`Ready` or `Failed`); the cached
    /// result is kept and not re-run.
    AlreadyComplete,
}

/// Why [`MatchService::start`] could not begin a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// The `match_id` is not in the fixture set the service was built
    /// from.
    UnknownFixture,
}

/// Lifecycle of one match managed by the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchState {
    /// Never started.
    NotStarted,
    /// Queued to run (reserved for ticket 06; ticket 05 starts
    /// immediately, so this state is not observed yet).
    Queued,
    /// The simulation and rendering are in flight on a blocking thread.
    Running,
    /// Rendering finished: the manifest and the gzipped segments.
    Ready {
        manifest: Arc<Manifest>,
        segments: Arc<[RenderedSegment]>,
    },
    /// A deterministic failure, cached for the life of the process.
    Failed {
        code: String,
        message: String,
        last_good_tick: u64,
    },
}

/// `match_id` → lifecycle state.
#[derive(Debug)]
struct ServiceState {
    entries: HashMap<String, MatchState>,
}

/// A match lifecycle driven by a [`MatchRunner`].
///
/// Constructed from a loaded [`FixtureSet`] and a runner. Any
/// `match_id` not in the fixture set is rejected by [`start`].
#[derive(Debug)]
pub struct MatchService {
    fixtures: MatchRegistry,
    runner: Arc<dyn MatchRunner>,
    tick_cap: u64,
    state: Mutex<ServiceState>,
}

impl MatchService {
    /// Build a service from the loaded fixture set and a runner.
    ///
    /// The real adapter for production is
    /// [`crate::api::runner::SimulationRunner`], which wraps the existing
    /// simulation. Every fixture in `fixture_set` is known and starts in
    /// `NotStarted`; any other `match_id` is rejected.
    ///
    /// # Panics
    ///
    /// Panics if the fixture set is not validated (a referenced team is
    /// missing), which is a programmer error: [`FixtureSet::load`] only
    /// produces validated fixture sets.
    #[must_use]
    pub fn new(fixture_set: FixtureSet, runner: Arc<dyn MatchRunner>) -> Self {
        let mut fixtures = MatchRegistry::default();
        for (id, mat) in &fixture_set.matches {
            let home = fixture_set
                .teams
                .get(&mat.home)
                .expect("invariant: fixture set is validated, home team exists");
            let away = fixture_set
                .teams
                .get(&mat.away)
                .expect("invariant: fixture set is validated, away team exists");
            fixtures.insert(id, mat.clone(), home.clone(), away.clone());
        }
        let entries: HashMap<String, MatchState> = fixtures
            .fixtures
            .keys()
            .map(|id| (id.clone(), MatchState::NotStarted))
            .collect();
        Self {
            fixtures,
            runner,
            tick_cap: TICK_CAP,
            state: Mutex::new(ServiceState { entries }),
        }
    }

    /// Start a match. The run begins immediately and `start` returns
    /// without waiting for completion; the simulation and rendering run
    /// together on a blocking thread.
    ///
    /// Single-flight: concurrent starts of the same match run once.
    ///
    /// # Errors
    ///
    /// Returns [`StartError::UnknownFixture`] when `match_id` is not in
    /// the fixture set.
    ///
    /// # Panics
    ///
    /// Panics if the state lock is poisoned, a programmer error, never
    /// client input.
    #[allow(
        clippy::unused_async,
        reason = "the async signature is part of the service's async lifecycle API; the blocking work is fire-and-forget via `spawn_blocking`, so `start` itself has no await point"
    )]
    pub async fn start(self: &Arc<Self>, match_id: &str) -> Result<StartOutcome, StartError> {
        // Decide under the lock, then drop the guard before any blocking
        // work so the lock is never held across an await point.
        let decision = {
            let mut state = self
                .state
                .lock()
                .expect("invariant: service state lock is never poisoned");
            state.entries.get_mut(match_id).map_or(
                Decision::RejectUnknown,
                |entry| match entry {
                    MatchState::NotStarted => {
                        *entry = MatchState::Running;
                        Decision::Spawn {
                            match_id: match_id.to_string(),
                        }
                    }
                    MatchState::Queued | MatchState::Running => Decision::AlreadyRunning,
                    MatchState::Ready { .. } | MatchState::Failed { .. } => {
                        Decision::AlreadyComplete
                    }
                },
            )
        }; // lock dropped here

        match decision {
            Decision::RejectUnknown => Err(StartError::UnknownFixture),
            Decision::AlreadyRunning => Ok(StartOutcome::AlreadyRunning),
            Decision::AlreadyComplete => Ok(StartOutcome::AlreadyComplete),
            Decision::Spawn { match_id } => {
                let service = Arc::clone(self);
                let runner = Arc::clone(&self.runner);
                let fixture = self
                    .fixtures
                    .get(&match_id)
                    .expect("invariant: fixture was validated above")
                    .clone();
                let tick_cap = self.tick_cap;
                // Fire-and-forget on the blocking pool: the closure writes
                // its terminal result back into the state map, so callers
                // never block and no join handle is awaited.
                tokio::task::spawn_blocking(move || {
                    let entry = run_and_render(runner.as_ref(), &fixture, tick_cap);
                    service.finish(&match_id, entry);
                });
                Ok(StartOutcome::Started)
            }
        }
    }

    /// Read the current lifecycle state of `match_id`.
    ///
    /// Unknown ids report `NotStarted` (they were never started).
    ///
    /// # Panics
    ///
    /// Panics if the state lock is poisoned, a programmer error, never
    /// client input.
    #[must_use]
    pub fn state(&self, match_id: &str) -> MatchState {
        self.state
            .lock()
            .expect("invariant: service state lock is never poisoned")
            .entries
            .get(match_id)
            .cloned()
            .unwrap_or(MatchState::NotStarted)
    }
}

/// Decision computed under the lock in [`MatchService::start`]; the guard
/// is dropped before any blocking work.
#[derive(Debug)]
enum Decision {
    RejectUnknown,
    AlreadyRunning,
    AlreadyComplete,
    Spawn { match_id: String },
}

/// Run the runner and render on the blocking thread.
///
/// A panic in the runner is caught (it would otherwise surface as a join
/// error on the blocking thread) and reported as a `runner_panic`
/// failure, so the match always leaves `Running`. The raw completed match
/// is dropped as soon as it is rendered.
#[must_use]
fn run_and_render(runner: &dyn MatchRunner, fixture: &MatchFixture, tick_cap: u64) -> MatchState {
    let outcome = run_catching_panic(runner, fixture, tick_cap);
    match outcome {
        MatchOutcome::Finished(completed) => {
            let rendered = render_match(fixture, &completed);
            // The raw completed match is dropped now that it is rendered.
            drop(completed);
            MatchState::Ready {
                manifest: Arc::new(rendered.manifest),
                segments: Arc::from(rendered.segments),
            }
        }
        MatchOutcome::Failed {
            code,
            message,
            last_good_tick,
        } => MatchState::Failed {
            code,
            message,
            last_good_tick,
        },
    }
}

impl MatchService {
    /// Write the terminal result of one in-flight run back into the map.
    ///
    /// Only the blocking thread that started the match writes this, and
    /// only while the entry is `Running` (single-flight), so the replace
    /// is race-free.
    fn finish(&self, match_id: &str, entry: MatchState) {
        let mut state = self
            .state
            .lock()
            .expect("invariant: service state lock is never poisoned");
        if let Some(current) = state.entries.get_mut(match_id)
            && matches!(current, MatchState::Running)
        {
            *current = entry;
        }
    }
}

#[cfg(test)]
mod tests {
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
        let service = Arc::new(MatchService::new(
            fixture_set(&["m-0001"]),
            as_dyn(Arc::new(InstantRunner)),
        ));

        assert_eq!(service.start("nope").await, Err(StartError::UnknownFixture));
        assert_eq!(service.state("nope"), MatchState::NotStarted);
        assert_eq!(service.state("m-0001"), MatchState::NotStarted);
    }

    #[tokio::test]
    async fn known_match_moves_out_of_not_started_and_becomes_ready() {
        let service = Arc::new(MatchService::new(
            fixture_set(&["m-0001"]),
            as_dyn(Arc::new(InstantRunner)),
        ));

        assert_eq!(service.state("m-0001"), MatchState::NotStarted);

        assert_eq!(service.start("m-0001").await, Ok(StartOutcome::Started));

        wait_until(&service, "m-0001", |state| matches!(state, MatchState::Ready { .. }))
            .await;
        assert!(matches!(service.state("m-0001"), MatchState::Ready { .. }));
    }

    #[tokio::test]
    async fn concurrent_starts_of_the_same_match_run_once() {
        let runner = Arc::new(GatedRunner::default());
        let service = Arc::new(MatchService::new(
            fixture_set(&["m-0001"]),
            as_dyn(Arc::clone(&runner)),
        ));
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
        let service = Arc::new(MatchService::new(
            fixture_set(&["m-0001"]),
            as_dyn(Arc::new(InstantRunner)),
        ));

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
        let service = Arc::new(MatchService::new(
            fixture_set(&["panic-0", "ok-0"]),
            as_dyn(Arc::new(PanickingRunner)),
        ));

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
        let service = Arc::new(MatchService::new(
            fixture_set(&["m-0001"]),
            as_dyn(Arc::clone(&runner)),
        ));

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
        let service = Arc::new(MatchService::new(
            fixture_set(&["blocked-0", "ok-0"]),
            as_dyn(Arc::clone(&runner)),
        ));

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
}

