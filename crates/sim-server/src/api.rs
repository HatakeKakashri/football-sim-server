//! One-fixture HTTP match streaming: registry, store, runner (issue 12).
//!
//! [`MatchRegistry`] maps fixture `match_id`s to seeds (unknown ids
//! get `404`). [`MatchStore`] owns the per-match lifecycle: `POST`
//! inserts a `Running` record and spawns the simulation on a
//! dedicated OS thread (never on the async executor); the thread
//! writes back a finished/failed 12-or-1-message stream that `GET`
//! serves with `Last-Event-ID` resume. Finished/failed results are
//! cached for [`ServerConfig::result_ttl_secs`]; after expiry `GET`
//! returns `410` until a re-`POST` re-simulates (byte-identical per
//! PRD §10.4). Beyond [`ServerConfig::max_concurrent_sims`],
//! `POST` returns `503` with `Retry-After`.
//!
//! A run that panics or whose thread cannot be spawned is a failed
//! match, not a wedged one: the concurrency slot is released, the
//! failure is cached, and `GET` returns the terminal error stream
//! instead of waiting for a match that never finishes.
//!
//! Stream layout itself lives in [`stream`](crate::api::stream);
//! the simulation runner in [`runner`](crate::api::runner); the
//! OS-thread spawn seam in [`spawn`](crate::api::spawn); the pure
//! render step that turns a completed match into a manifest and its
//! segments in [`render`](crate::api::render).

pub mod registry;
pub mod render;
pub mod runner;
pub mod spawn;
pub mod stream;
#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod manifest_tests;
#[cfg(test)]
pub(crate) mod render_doubles;
#[cfg(test)]
mod render_tests;
#[cfg(test)]
mod runner_tests;
#[cfg(test)]
pub(crate) mod test_doubles;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use sim_core::TICK_CAP;
use tokio::sync::Notify;

pub use registry::{MatchFixture, MatchRegistry};
use runner::{FailureKind, MatchOutcome, MatchRunner, SimulationRunner, run_catching_panic};
use spawn::{OsThreadSpawner, ThreadSpawner};
use stream::{SseMessage, build_error_stream, build_stream, render_body};

/// `Retry-After` seconds sent with `503` over the concurrency cap.
pub const RETRY_AFTER_SECS: u64 = 5;

/// Default cache lifetime for finished/failed results (PRD §4).
pub const DEFAULT_RESULT_TTL_SECS: u64 = 3600;

/// Default concurrency cap: one simulation per core minus one for
/// the executor (PRD §4), at least one.
#[must_use]
pub fn default_concurrency_cap() -> usize {
    std::thread::available_parallelism().map_or(1, |cores| cores.get().max(2) - 1)
}

/// The service's policy settings (PRD §4 configuration table).
///
/// Transport settings — bind address, API token and CORS origins — live
/// in [`crate::http::HttpConfig`]. Keeping the two apart means the
/// policy cannot diverge between the legacy store and the match service,
/// and neither struct carries a field the other layer owns.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Simultaneous simulation thread cap.
    pub max_concurrent_sims: usize,
    /// Cache lifetime for finished/failed results.
    pub result_ttl_secs: u64,
    /// Hard stop per match; exceeding it fails the match (PRD §12).
    pub tick_cap: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_sims: default_concurrency_cap(),
            result_ttl_secs: DEFAULT_RESULT_TTL_SECS,
            tick_cap: TICK_CAP,
        }
    }
}

/// Lifecycle of one match entry in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    /// Simulation thread still running.
    Running,
    /// Reached `FullTime`; the 12-message stream is cached.
    Finished,
    /// Invariant violation, tick-cap breach, runner panic or failed
    /// thread spawn; the 1-message error stream is cached (PRD §12:
    /// failures cache like successes).
    Failed,
}

impl JobStatus {
    /// Stable string for the `POST` response body.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Finished => "finished",
            Self::Failed => "failed",
        }
    }
}

/// One store entry: status, completion time, and the cached stream
/// (12 messages when finished, 1 `error` message when failed).
#[derive(Debug)]
struct MatchRecord {
    status: JobStatus,
    completed_at: Option<Instant>,
    stream: Vec<SseMessage>,
}

#[derive(Debug, Default)]
struct StoreState {
    records: HashMap<String, MatchRecord>,
    running: usize,
}

/// Lock-scoped snapshot for `stream_slice`: cloned out from under the
/// guard so rendering happens lock-free.
#[derive(Debug, Clone)]
enum StreamSnapshot {
    /// No record: `POST` was never called.
    Missing,
    /// Simulation thread still running.
    Running,
    /// Cached result expired.
    Expired,
    /// Cached stream ready to slice.
    Ready(Vec<SseMessage>),
}

/// Lock-scoped decision for `post_match`: what to do after the guard
/// drops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PostDecision {
    /// Serve the cached status without starting work.
    Cached(JobStatus),
    /// Concurrency cap reached.
    OverCap,
    /// Inserted a `Running` record; spawn the thread.
    Start,
}

/// Outcome of `POST /match/{id}` (PRD §10.1).
#[derive(Debug, Clone)]
pub struct PostResponse {
    /// HTTP status: 202 started now, 200 already known, 404 unknown
    /// id, 500 the simulation thread could not be started, 503 over
    /// the concurrency cap.
    pub status: u16,
    /// `{ match_id, status }` body.
    pub body: serde_json::Value,
    /// Set on 503 (`Retry-After`).
    pub retry_after_secs: Option<u64>,
}

/// Outcome of `GET /match/{id}/stream` (PRD §10.1).
#[derive(Debug, Clone)]
pub enum StreamResponse {
    /// Simulation still running; the handler awaits completion.
    Running,
    /// Stream slice from the resume index (SSE body).
    Ready(String),
    /// `Last-Event-ID` at or beyond the final message.
    NoContent,
    /// Unknown `match_id`.
    Unknown,
    /// Match known but never `POST`ed.
    NotStarted,
    /// Cached result expired; `POST` again to regenerate.
    Expired,
}

/// In-memory match store: registry + records + concurrency counting.
/// Locks are held only for map edits, never across simulation or I/O.
#[derive(Debug)]
pub struct MatchStore {
    config: ServerConfig,
    pub registry: MatchRegistry,
    /// Seam over the blocking match runner (real adapter in
    /// production, injected adapters in tests).
    runner: Arc<dyn MatchRunner>,
    /// Seam over OS-thread creation (real adapter in production,
    /// injected adapters in tests).
    spawner: Arc<dyn ThreadSpawner>,
    state: Mutex<StoreState>,
    finished: Notify,
}

impl MatchStore {
    /// Build a store. Holds no matches; `POST` starts them.
    #[must_use]
    pub fn new(config: ServerConfig, registry: MatchRegistry) -> Self {
        Self {
            config,
            registry,
            runner: Arc::new(SimulationRunner),
            spawner: Arc::new(OsThreadSpawner),
            state: Mutex::new(StoreState::default()),
            finished: Notify::new(),
        }
    }

    /// Replace the match runner (injection seam).
    ///
    /// Production uses [`MatchStore::new`]; tests inject a runner that
    /// panics or fails so the store's failure handling is exercised
    /// without relying on the tick cap.
    #[must_use]
    pub fn with_runner(mut self, runner: Arc<dyn MatchRunner>) -> Self {
        self.runner = runner;
        self
    }

    /// Replace the OS-thread spawner (injection seam).
    ///
    /// Production uses [`MatchStore::new`]; tests inject a spawner that
    /// refuses to start the thread, so the slot taken before the spawn
    /// is proven to be released.
    #[must_use]
    pub fn with_spawner(mut self, spawner: Arc<dyn ThreadSpawner>) -> Self {
        self.spawner = spawner;
        self
    }

    /// `true` when the record exists and its TTL has not elapsed.
    /// Running records never expire.
    fn is_fresh(record: &MatchRecord, ttl_secs: u64) -> bool {
        record
            .completed_at
            .is_none_or(|at| at.elapsed().as_secs() < ttl_secs)
    }

    /// Start a match, or report its current state (PRD §10.1 `POST`).
    /// Must be called on an `Arc` so the simulation thread can write
    /// back its result.
    ///
    /// # Panics
    ///
    /// Panics if the store lock is poisoned, which is a programmer
    /// error, never client input.
    pub fn post_match(self: &Arc<Self>, match_id: &str) -> PostResponse {
        let fixture = match self.registry.get(match_id) {
            Some(fixture) => fixture.clone(),
            None => {
                return PostResponse {
                    status: 404,
                    body: serde_json::json!({"error": "unknown match_id"}),
                    retry_after_secs: None,
                };
            }
        };
        // Decide under the lock; spawn outside it so the guard is
        // dropped before any blocking work.
        let decision = {
            let mut state = self
                .state
                .lock()
                .expect("invariant: match-store lock is never poisoned");
            if let Some(record) = state.records.get(match_id)
                && Self::is_fresh(record, self.config.result_ttl_secs)
            {
                PostDecision::Cached(record.status)
            } else if state.running >= self.config.max_concurrent_sims {
                PostDecision::OverCap
            } else {
                state.records.insert(
                    match_id.to_string(),
                    MatchRecord {
                        status: JobStatus::Running,
                        completed_at: None,
                        stream: Vec::new(),
                    },
                );
                state.running += 1;
                PostDecision::Start
            }
        };
        match decision {
            PostDecision::Cached(status) => PostResponse {
                status: 200,
                body: serde_json::json!({
                    "match_id": match_id,
                    "status": status.as_str(),
                }),
                retry_after_secs: None,
            },
            PostDecision::OverCap => PostResponse {
                status: 503,
                body: serde_json::json!({"error": "concurrency cap reached"}),
                retry_after_secs: Some(RETRY_AFTER_SECS),
            },
            PostDecision::Start => {
                let store = Arc::clone(self);
                let tick_cap = self.config.tick_cap;
                let runner = Arc::clone(&self.runner);
                let thread_match_id = fixture.validated_match.id.clone();
                let spawn = self.spawner.spawn(
                    format!("sim-{match_id}"),
                    Box::new(move || {
                        // A panic in the runner must not skip the
                        // completion bookkeeping below, so it is caught
                        // and turned into a cached failure.
                        let outcome = run_catching_panic(runner.as_ref(), &fixture, tick_cap);
                        store.finish_match(&thread_match_id, outcome);
                    }),
                );
                match spawn {
                    Ok(()) => PostResponse {
                        status: 202,
                        body: serde_json::json!({"match_id": match_id, "status": "running"}),
                        retry_after_secs: None,
                    },
                    Err(error) => {
                        // The concurrency slot was taken before the
                        // spawn: release it and cache the failure, so
                        // `GET` returns a terminal error instead of
                        // waiting for a match that never starts.
                        self.finish_match(
                            match_id,
                            MatchOutcome::Failed {
                                code: "thread_spawn_failed".to_string(),
                                message: format!(
                                    "could not start the simulation thread: {}",
                                    error.message
                                ),
                                last_good_tick: 0,
                                // The legacy store caches every
                                // failure for the life of the process;
                                // it has no retry window.
                                kind: FailureKind::Deterministic,
                            },
                        );
                        PostResponse {
                            status: 500,
                            body: serde_json::json!({
                                "error": "simulation thread could not be started",
                            }),
                            retry_after_secs: None,
                        }
                    }
                }
            }
        }
    }

    /// Store a finished/failed simulation result and wake waiters.
    fn finish_match(&self, match_id: &str, outcome: MatchOutcome) {
        let (status, stream) = match outcome {
            MatchOutcome::Finished(completed) => (JobStatus::Finished, build_stream(&completed)),
            MatchOutcome::Failed {
                code,
                message,
                last_good_tick,
                // The legacy store caches every failure, transient or
                // not: it has no retry window, and the new match
                // service (ticket 07) owns the retry semantics.
                ..
            } => (
                JobStatus::Failed,
                build_error_stream(&code, &message, last_good_tick),
            ),
        };
        {
            let mut state = self
                .state
                .lock()
                .expect("invariant: match-store lock is never poisoned");
            if let Some(record) = state.records.get_mut(match_id) {
                record.status = status;
                record.completed_at = Some(Instant::now());
                record.stream = stream;
            }
            state.running = state.running.saturating_sub(1);
        }
        self.finished.notify_waiters();
    }

    /// Read a stream slice resuming after `last_event_id` (PRD §10.1
    /// `GET`, §10.4 resume).
    ///
    /// # Panics
    ///
    /// Panics if the store lock is poisoned, which is a programmer
    /// error, never client input.
    #[must_use]
    pub fn stream_slice(&self, match_id: &str, last_event_id: Option<u64>) -> StreamResponse {
        if self.registry.get(match_id).is_none() {
            return StreamResponse::Unknown;
        }
        // Snapshot under the lock; render outside it.
        let snapshot = {
            let state = self
                .state
                .lock()
                .expect("invariant: match-store lock is never poisoned");
            match state.records.get(match_id) {
                None => StreamSnapshot::Missing,
                Some(record) if record.status == JobStatus::Running => StreamSnapshot::Running,
                Some(record)
                    if !Self::is_fresh(record, self.config.result_ttl_secs) =>
                {
                    StreamSnapshot::Expired
                }
                Some(record) => StreamSnapshot::Ready(record.stream.clone()),
            }
        };
        match snapshot {
            StreamSnapshot::Missing => StreamResponse::NotStarted,
            StreamSnapshot::Running => StreamResponse::Running,
            StreamSnapshot::Expired => StreamResponse::Expired,
            StreamSnapshot::Ready(stream) => {
                let from_index = last_event_id.map_or(0_usize, |id| {
                    usize::try_from(id.saturating_add(1)).unwrap_or(usize::MAX)
                });
                if from_index >= stream.len() {
                    StreamResponse::NoContent
                } else {
                    StreamResponse::Ready(render_body(&stream[from_index..]))
                }
            }
        }
    }

    /// Wait until `match_id` settles (finished or failed). Used by
    /// the stream handler's keep-alive loop and by tests.
    ///
    /// # Panics
    ///
    /// Panics if the store lock is poisoned, which is a programmer
    /// error, never client input.
    pub async fn wait_until_done(&self, match_id: &str) {
        loop {
            // Register for the wakeup *before* reading the state: a
            // completion that lands between the read and the await
            // would otherwise be missed, and the waiter would block
            // forever on a match that is already settled.
            let notified = self.finished.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let settled = {
                let state = self
                    .state
                    .lock()
                    .expect("invariant: match-store lock is never poisoned");
                state
                    .records
                    .get(match_id)
                    .is_none_or(|record| record.status != JobStatus::Running)
            };
            if settled {
                return;
            }
            notified.await;
        }
    }
}
