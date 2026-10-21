//! Match service: one match lifecycle behind a single interface.
//!
//! The simulation is hidden behind a [`MatchRunner`] trait. The real
//! adapter ([`crate::api::runner::SimulationRunner`]) wraps the existing
//! simulation; tests inject fakes. The service tracks each match through
//! `NotStarted`, `Queued`, `Running`, `Ready` and `Failed`.
//!
//! Concurrency is capped by a fair semaphore sized to the cap
//! ([`MatchService::with_cap`]): a start that finds no free permit waits
//! in `Queued` and runs when a permit frees, in arrival order. There is
//! no over-cap rejection: a start beyond the cap is queued, never
//! refused. A permit travels with its run and is released on every path
//! out of it — success, failure and panic — so the cap never shrinks.
//!
//! Starting is single-flight. The simulation and rendering run together
//! on a blocking thread ([`tokio::task::spawn_blocking`]) so the async
//! executor never does CPU work. A panic on that thread is caught and
//! becomes `Failed` with the distinct `runner_panic` code; deterministic
//! failures (tick cap, invariant violation, panic) are cached with their
//! `code`, `message` and `last_good_tick` for the life of the process.
//! `Ready` is set only after rendering is complete, and the raw
//! completed match is dropped afterwards.
//!
//! The service does not use any wait or notify primitive: callers read
//! state, and the state map's lock is never held across an await point.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::Mutex;

use sim_core::TICK_CAP;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::api::default_concurrency_cap;
use crate::api::registry::{MatchFixture, MatchRegistry};
use crate::api::render::{Manifest, RenderedSegment, render_match};
use crate::api::runner::{MatchOutcome, MatchRunner, panic_message, run_catching_panic};
use crate::fixtures::FixtureSet;

/// Terminal outcome of [`MatchService::start`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOutcome {
    /// This caller initiated the run; the match is running now.
    Started,
    /// The concurrency cap is full: the match waits in `Queued` and
    /// runs, in arrival order, when a permit frees.
    Queued,
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
    /// Waiting for a concurrency permit. The match runs when a permit
    /// frees, in the order the starts arrived.
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

/// `match_id` → lifecycle state, plus the matches waiting for a permit.
#[derive(Debug)]
struct ServiceState {
    entries: HashMap<String, MatchState>,
    /// Matches that found no free permit, oldest first. A match leaves
    /// the queue when it is handed the permit of the run that finished
    /// before it, so queued matches start in arrival order.
    pending: VecDeque<String>,
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
    /// Fair semaphore sized to the concurrency cap: one permit per
    /// running match. A permit is held for the whole run and released
    /// on every path out of it.
    permits: Arc<Semaphore>,
    state: Mutex<ServiceState>,
}

impl MatchService {
    /// Build a service from the loaded fixture set and a runner.
    ///
    /// The real adapter for production is
    /// [`crate::api::runner::SimulationRunner`], which wraps the existing
    /// simulation. Every fixture in `fixture_set` is known and starts in
    /// `NotStarted`; any other `match_id` is rejected. The concurrency
    /// cap defaults to the server's default (`cores − 1`, the same
    /// value as [`crate::api::ServerConfig::max_concurrent_sims`]); use
    /// [`MatchService::with_cap`] to set it explicitly.
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
            permits: Arc::new(Semaphore::new(default_concurrency_cap())),
            state: Mutex::new(ServiceState {
                entries,
                pending: VecDeque::new(),
            }),
        }
    }

    /// Set the concurrency cap: how many matches may run at once.
    ///
    /// The cap sizes the fair semaphore that limits the running
    /// matches. A start beyond the cap is queued, never rejected. A cap
    /// below one is raised to one: a cap of zero would never release a
    /// permit, so the queue could never drain.
    ///
    /// The cap is a construction-time setting: set it before the first
    /// start.
    #[must_use]
    pub fn with_cap(mut self, cap: usize) -> Self {
        self.permits = Arc::new(Semaphore::new(cap.max(1)));
        self
    }

    /// Start a match. The run begins immediately when a permit is free;
    /// otherwise the match waits in `Queued` and runs when a permit
    /// frees. `start` returns without waiting for completion; the
    /// simulation and rendering run together on a blocking thread.
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
        // Decide under the lock; the guard is released inside `decide`,
        // before any blocking work, so the lock is never held across an
        // await point.
        match self.decide(match_id) {
            Decision::RejectUnknown => Err(StartError::UnknownFixture),
            Decision::AlreadyRunning => Ok(StartOutcome::AlreadyRunning),
            Decision::AlreadyComplete => Ok(StartOutcome::AlreadyComplete),
            Decision::Queued => Ok(StartOutcome::Queued),
            Decision::Spawn { match_id, permit } => {
                self.spawn_run(&match_id, permit);
                Ok(StartOutcome::Started)
            }
        }
    }

    /// Decide what a start does, under the state lock.
    ///
    /// A free permit starts the match now. A start that finds no free
    /// permit waits in the queue for the permit of the run that
    /// finishes first, so the cap is never exceeded and no start is
    /// refused.
    fn decide(&self, match_id: &str) -> Decision {
        let mut state = self
            .state
            .lock()
            .expect("invariant: service state lock is never poisoned");
        // The entry is copied out so the map can be changed below.
        let current = state.entries.get(match_id).cloned();
        match current {
            None => Decision::RejectUnknown,
            Some(MatchState::NotStarted) => {
                if let Ok(permit) = Arc::clone(&self.permits).try_acquire_owned() {
                    state.entries.insert(match_id.to_string(), MatchState::Running);
                    Decision::Spawn {
                        match_id: match_id.to_string(),
                        permit,
                    }
                } else {
                    state.entries.insert(match_id.to_string(), MatchState::Queued);
                    state.pending.push_back(match_id.to_string());
                    Decision::Queued
                }
            }
            Some(MatchState::Queued | MatchState::Running) => Decision::AlreadyRunning,
            Some(MatchState::Ready { .. } | MatchState::Failed { .. }) => Decision::AlreadyComplete,
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

    /// Run one match on the blocking pool, holding a permit for the
    /// whole run.
    ///
    /// The permit travels with the run: it is handed to the next queued
    /// match when the run ends, so the cap is released on every path —
    /// success, failure and panic.
    ///
    /// # Panics
    ///
    /// Panics if `match_id` is not a known fixture, which is a
    /// programmer error: only known fixtures are ever started.
    fn spawn_run(self: &Arc<Self>, match_id: &str, permit: OwnedSemaphorePermit) {
        let service = Arc::clone(self);
        let runner = Arc::clone(&self.runner);
        let fixture = self
            .fixtures
            .get(match_id)
            .expect("invariant: only known fixtures are started")
            .clone();
        let tick_cap = self.tick_cap;
        let thread_match_id = match_id.to_string();
        // Fire-and-forget on the blocking pool: the closure writes its
        // terminal result back into the state map and hands the permit
        // on, so callers never block and no join handle is awaited.
        tokio::task::spawn_blocking(move || {
            let entry = run_guarded(runner.as_ref(), &fixture, tick_cap);
            service.finish(&thread_match_id, entry, permit);
        });
    }

    /// Write the terminal result of one in-flight run back into the map
    /// and release its permit.
    ///
    /// The permit is handed to the match that has waited longest, so
    /// queued matches start in arrival order. With no match waiting,
    /// the permit returns to the pool and the cap shrinks back by one.
    ///
    /// Only the blocking thread that started the match writes this, and
    /// only while the entry is `Running` (single-flight), so the replace
    /// is race-free.
    fn finish(self: &Arc<Self>, match_id: &str, entry: MatchState, permit: OwnedSemaphorePermit) {
        let mut state = self
            .state
            .lock()
            .expect("invariant: service state lock is never poisoned");
        if let Some(current) = state.entries.get_mut(match_id)
            && matches!(current, MatchState::Running)
        {
            *current = entry;
        }
        // Hand the permit to the match that has waited longest: the
        // queue drains in arrival order. The permit never goes back to
        // the pool while a match is waiting for one, so a later start
        // can never jump the queue.
        while let Some(next_id) = state.pending.pop_front() {
            if !matches!(state.entries.get(&next_id), Some(MatchState::Queued)) {
                // A stale queue entry: the match is no longer waiting.
                continue;
            }
            if let Some(queued) = state.entries.get_mut(&next_id) {
                *queued = MatchState::Running;
            }
            // The next match is started under the lock, so two runs
            // finishing at once start the queued matches in arrival
            // order.
            self.spawn_run(&next_id, permit);
            return;
        }
        drop(state);
        // No match is waiting: the permit returns to the pool.
        drop(permit);
    }
}

/// Decision computed under the lock in [`MatchService::decide`]; the
/// guard is dropped before any blocking work.
#[derive(Debug)]
enum Decision {
    /// The `match_id` is not in the fixture set.
    RejectUnknown,
    /// A run is already in flight for this match.
    AlreadyRunning,
    /// The match already has a terminal result.
    AlreadyComplete,
    /// The match waits in the queue for a permit.
    Queued,
    /// The match took a permit and runs now.
    Spawn {
        match_id: String,
        permit: OwnedSemaphorePermit,
    },
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

/// Run the match, turning a panic that escapes the runner and the
/// render step into a failed match.
///
/// The runner's own panics are already caught by [`run_catching_panic`],
/// so a panic that escapes [`run_and_render`] comes from the render
/// step. It is caught here for the same reason the runner's panic is:
/// the permit must be released and handed to the next queued match, or
/// the matches behind this one would wait forever.
#[must_use]
fn run_guarded(runner: &dyn MatchRunner, fixture: &MatchFixture, tick_cap: u64) -> MatchState {
    let run = || run_and_render(runner, fixture, tick_cap);
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(entry) => entry,
        Err(payload) => MatchState::Failed {
            code: "render_panic".to_string(),
            message: panic_message(payload.as_ref()),
            last_good_tick: 0,
        },
    }
}

#[cfg(test)]
mod tests;
