//! The states a match goes through and the outcomes a start reports.
//!
//! These are the types the HTTP layer reads (ticket 08): the lifecycle
//! of one match, and what a start did with it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::api::render::{Manifest, RenderedSegment};

/// Terminal outcome of [`crate::service::MatchService::start`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartOutcome {
    /// This caller initiated the run; the match is running now.
    Started,
    /// The concurrency cap is full: the match waits in `Queued` and
    /// runs, in arrival order, when a permit frees.
    Queued,
    /// A run is already in flight for this match (single-flight).
    AlreadyRunning,
    /// The match is already terminal (`Ready` or a deterministic
    /// `Failed`); the cached result is kept and not re-run.
    AlreadyComplete,
    /// A transient failure is still inside its retry window: the
    /// failure is reported again, with the time left before a start
    /// may try the match again.
    RetryAfter {
        /// The failure's code, as the runner reported it.
        code: String,
        /// The failure's message.
        message: String,
        /// The tick the failed run reached.
        last_good_tick: u64,
        /// The time left before the match may be tried again.
        retry_in: Duration,
    },
}

/// Why [`crate::service::MatchService::start`] could not begin a run.
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
    /// A failure. A deterministic failure is cached for the life of
    /// the process; a transient failure carries the time a later start
    /// may try the match again.
    Failed {
        code: String,
        message: String,
        last_good_tick: u64,
        /// When a later start may try the match again. `None` for a
        /// deterministic failure, which is never retried.
        retry_at: Option<Instant>,
    },
}
