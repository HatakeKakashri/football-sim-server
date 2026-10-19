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
//! Stream layout itself lives in [`stream`](crate::api::stream);
//! the simulation runner in [`runner`](crate::api::runner).

pub mod runner;
pub mod stream;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use sim_core::TICK_CAP;
use tokio::sync::Notify;

use runner::{MatchOutcome, run_match_blocking};
use stream::{SseMessage, build_error_stream, build_stream, render_body};

/// `Retry-After` seconds sent with `503` over the concurrency cap.
pub const RETRY_AFTER_SECS: u64 = 5;

/// Default cache lifetime for finished/failed results (PRD §4).
pub const DEFAULT_RESULT_TTL_SECS: u64 = 3600;

/// Default listen address (PRD §4: bind `127.0.0.1`).
pub const DEFAULT_BIND: &str = "127.0.0.1:8080";

/// Default concurrency cap: one simulation per core minus one for
/// the executor (PRD §4), at least one.
#[must_use]
pub fn default_concurrency_cap() -> usize {
    std::thread::available_parallelism().map_or(1, |cores| cores.get().max(2) - 1)
}

/// Server knobs (PRD §4 configuration table).
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Listen address.
    pub bind: String,
    /// Simultaneous simulation thread cap.
    pub max_concurrent_sims: usize,
    /// Cache lifetime for finished/failed results.
    pub result_ttl_secs: u64,
    /// Hard stop per match; exceeding it fails the match (PRD §12).
    pub tick_cap: u64,
    /// Bearer token required for `POST /match/{id}` when set (`Some`).
    /// Unset (`None`) disables auth for local-only deployments (PRD §4
    /// Security). `GET` is never authenticated.
    pub api_token: Option<String>,
    /// Allowed browser origins for CORS on `GET /match/{id}/stream`.
    /// Empty disables CORS for local-only deployments (PRD §4).
    pub cors_origins: Vec<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND.to_string(),
            max_concurrent_sims: default_concurrency_cap(),
            result_ttl_secs: DEFAULT_RESULT_TTL_SECS,
            tick_cap: TICK_CAP,
            api_token: None,
            cors_origins: Vec::new(),
        }
    }
}

/// One registered fixture: a `match_id` with its validated match data and
/// the two teams' player rosters.
#[derive(Debug, Clone)]
pub struct MatchFixture {
    /// Fixture match id from the URL path.
    pub match_id: String,
    /// The validated match (seed, home/away team IDs, setups).
    pub validated_match: sim_components::fixture::ValidatedMatch,
    /// The home team's validated players.
    pub home_team: sim_components::fixture::ValidatedTeam,
    /// The away team's validated players.
    pub away_team: sim_components::fixture::ValidatedTeam,
}

/// Fixture registry. Unknown `match_id`s get `404` (PRD §10.1).
#[derive(Debug, Clone, Default)]
pub struct MatchRegistry {
    /// Fixtures by `match_id`.
    pub fixtures: HashMap<String, MatchFixture>,
}

impl MatchRegistry {
    /// Registry with a single fixture (this slice serves one fixture).
    #[must_use]
    pub fn single(
        match_id: &str,
        validated_match: sim_components::fixture::ValidatedMatch,
        home_team: sim_components::fixture::ValidatedTeam,
        away_team: sim_components::fixture::ValidatedTeam,
    ) -> Self {
        let mut registry = Self::default();
        registry.insert(match_id, validated_match, home_team, away_team);
        registry
    }

    /// Test-only constructor: creates a registry with a single fixture using
    /// placeholder profiles (for tests that don't need real fixture data).
    #[cfg(test)]
    #[must_use]
    pub fn single_test(match_id: &str, seed: u64) -> Self {
        use sim_components::{
            fixture::{ValidatedMatch, ValidatedSide, ValidatedTeam, ValidatedPlayer},
            profile::ProfileKind,
            Formation, Mentality, Tactic, PosSlot, PlayerProfile,
        };
        let mut registry = Self::default();

        // Create minimal validated match with placeholder data
        let validated_match = ValidatedMatch {
            id: match_id.to_string(),
            seed,
            home: "test-home".to_string(),
            away: "test-away".to_string(),
            home_setup: ValidatedSide {
                formation: Formation::FourFourTwo,
                mentality: Mentality::Balance,
                tactic: Tactic::Possession,
            },
            away_setup: ValidatedSide {
                formation: Formation::FourFourTwo,
                mentality: Mentality::Balance,
                tactic: Tactic::Possession,
            },
        };

        // Create minimal validated teams with 11 placeholder players each
        let make_team = |id: &str, name: &str| ValidatedTeam {
            id: id.to_string(),
            name: name.to_string(),
            players: (0..11).map(|i| ValidatedPlayer {
                id: format!("{id}-p{i:02}"),
                name: format!("Player {i}"),
                number: (i + 1) as u8,
                kind: if i == 0 { ProfileKind::Goalkeeper } else { ProfileKind::Outfield },
                trained_positions: if i == 0 { vec![PosSlot::GK] } else { vec![PosSlot::MC] },
                profile: if i == 0 { PlayerProfile::default_goalkeeper() } else { PlayerProfile::default_outfield() },
            }).collect(),
        };

        let home_team = make_team("test-home", "Home");
        let away_team = make_team("test-away", "Away");

        registry.insert(match_id, validated_match, home_team, away_team);
        registry
    }

    /// Add (or replace) a fixture.
    pub fn insert(
        &mut self,
        match_id: &str,
        validated_match: sim_components::fixture::ValidatedMatch,
        home_team: sim_components::fixture::ValidatedTeam,
        away_team: sim_components::fixture::ValidatedTeam,
    ) {
        self.fixtures.insert(
            match_id.to_string(),
            MatchFixture {
                match_id: match_id.to_string(),
                validated_match,
                home_team,
                away_team,
            },
        );
    }

    /// Look up a fixture by `match_id`.
    #[must_use]
    pub fn get(&self, match_id: &str) -> Option<&MatchFixture> {
        self.fixtures.get(match_id)
    }
}

/// Lifecycle of one match entry in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    /// Simulation thread still running.
    Running,
    /// Reached `FullTime`; the 12-message stream is cached.
    Finished,
    /// Invariant violation or tick-cap breach; the 1-message error
    /// stream is cached (PRD §12: failures cache like successes).
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
    /// id, 503 over the concurrency cap.
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
            state: Mutex::new(StoreState::default()),
            finished: Notify::new(),
        }
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
    /// Panics if the store lock is poisoned or the OS thread spawn
    /// fails; both are programmer errors, never client input.
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
                let fixture = fixture.clone();
                std::thread::Builder::new()
                    .name(format!("sim-{match_id}"))
                    .spawn(move || {
                        let outcome = run_match_blocking(&fixture, tick_cap);
                        store.finish_match(&fixture.validated_match.id, outcome);
                    })
                    .expect("invariant: OS thread spawn for a match simulation");
                PostResponse {
                    status: 202,
                    body: serde_json::json!({"match_id": match_id, "status": "running"}),
                    retry_after_secs: None,
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
            self.finished.notified().await;
        }
    }
}
