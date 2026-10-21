//! The states a match goes through and the outcomes a start reports.
//!
//! These are the types the HTTP layer reads (ticket 08): the lifecycle
//! of one match, and what a start did with it.
//!
//! The lifecycle is one type, [`MatchState`], and it covers every case
//! a match can be in: an id that is not a fixture at all, a match that
//! was never started, a queued match, a running match, a ready match
//! and a failed match. The service answers a match's status in one
//! call (see [`crate::service::MatchService::status`]), so a caller
//! never has to ask two questions to tell the cases apart.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::api::render::PublishedMatch;

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
///
/// This is the service's one lifecycle type: every case a match can be
/// in has a case here, including the case of an id that is not a
/// fixture at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchState {
    /// The `match_id` is not a fixture this service knows.
    ///
    /// It is not a match that was never started: there is no match.
    UnknownFixture,
    /// Never started.
    NotStarted,
    /// Waiting for a concurrency permit. The match runs when a permit
    /// frees, in the order the starts arrived.
    Queued,
    /// The simulation and rendering are in flight on a blocking thread.
    Running,
    /// Rendering finished: the published bytes of the match.
    Ready {
        /// The published bytes of the match: the manifest's answer body
        /// and every segment's body, indexed by segment number, next
        /// to the build id the published URLs carry.
        ///
        /// A caller that is told the match is ready reads the
        /// manifest's bytes and a segment's bytes from here, without
        /// asking a second question: the match is published once, and
        /// the ready case holds nothing but the published bytes.
        published: Arc<PublishedMatch>,
    },
    /// A failure. A deterministic failure is cached for the life of
    /// the process; a transient failure carries the time left before a
    /// later start may try the match again.
    Failed {
        /// The failure's code, as the runner reported it.
        code: String,
        /// The failure's message.
        message: String,
        /// The tick the failed run reached.
        last_good_tick: u64,
        /// When the match may be tried again. `None` for a
        /// deterministic failure, which is never retried.
        ///
        /// The service keeps this deadline to decide whether a later
        /// start may run the match again.
        retry_at: Option<Instant>,
        /// The time left before the match may be tried again, as of the
        /// moment the state was answered. `None` for a deterministic
        /// failure, which is never retried.
        ///
        /// This is the wait a caller honours: it never has to compute
        /// one from the deadline. The service refreshes it on every
        /// answer, so the answer a caller reads is never stale.
        retry_in: Option<Duration>,
    },
}
