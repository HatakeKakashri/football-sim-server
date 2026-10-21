//! The status of a match, answered in one call (server-API ticket 02).
//!
//! The service owns the rule that separates a match that is not a
//! fixture from a match that was never started, so it answers that
//! question itself: [`MatchService::status`] reports every case of a
//! match's lifecycle — including an id the service does not know — in
//! one call that takes the state map's lock once.
//!
//! The answer is a [`MatchState`], the one lifecycle type:
//!
//! - A ready match carries the published bytes of the match, so a
//!   caller that is told "ready" can read the manifest's bytes and a
//!   segment's bytes without asking a second question.
//! - A failed match carries the failure's code, its message, the last
//!   good tick and the time left before the match may be retried, so a
//!   caller never has to compute a wait from a deadline.
//!
//! The lock is never held across an await point: the answer is read
//! under the lock and the guard is released before the answer is
//! built.

use std::time::Instant;

use super::{MatchService, MatchState};

impl MatchService {
    /// The status of `match_id`, in one call.
    ///
    /// The service's single status answer: it takes the state map's
    /// lock once and reports every case a match can be in.
    ///
    /// An id the service does not know answers
    /// [`MatchState::UnknownFixture`], which is not the same answer as
    /// a match that was never started
    /// ([`MatchState::NotStarted`]): the first is not a match at all,
    /// the second is a match that has not run yet.
    ///
    /// # Panics
    ///
    /// Panics if the state lock is poisoned, a programmer error, never
    /// client input.
    #[must_use]
    pub fn status(&self, match_id: &str) -> MatchState {
        let now = Instant::now();
        let state = self
            .state
            .lock()
            .expect("invariant: service state lock is never poisoned")
            .entries
            .get(match_id)
            .cloned();
        answered(state, now)
    }
}

/// The answer for the state the map holds, or for an id it does not
/// hold at all.
///
/// A transient failure answers the time left before the match may be
/// retried, brought up to date with the moment of the answer: the
/// deadline is the service's bookkeeping, the wait is the caller's.
fn answered(state: Option<MatchState>, now: Instant) -> MatchState {
    match state {
        // An id the service does not know is not a match that was
        // never started: it is not a match at all.
        None => MatchState::UnknownFixture,
        Some(MatchState::Failed {
            code,
            message,
            last_good_tick,
            retry_at,
            ..
        }) => MatchState::Failed {
            code,
            message,
            last_good_tick,
            retry_at,
            retry_in: retry_at.map(|at| at.saturating_duration_since(now)),
        },
        Some(other) => other,
    }
}
