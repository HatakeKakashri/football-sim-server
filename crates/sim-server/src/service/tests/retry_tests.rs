//! Tests for the retry window of a transient failure (server-API
//! ticket 07).
//!
//! A transient failure is a resource failure: the run could not get a
//! resource it needed, and the pressure belongs to the host, not to
//! the match. The failure is therefore not cached forever: it records a
//! retry time, and a start inside the window reports the failure with
//! the remaining wait, while a start after the window schedules the
//! match again.
//!
//! The service is driven entirely through the fake runner: no HTTP, no
//! sockets and no direct state poking.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;

use crate::api::registry::MatchFixture;
use crate::api::runner::{FailureKind, MatchOutcome};

/// The retry window the tests that wait for it use: short enough to
/// wait for, long enough to observe a start inside it.
const WINDOW: Duration = Duration::from_millis(200);

/// The retry window the tests that only look inside it use: long
/// enough that a slow machine cannot step over it.
const PATIENCE: Duration = Duration::from_secs(5);

/// A runner that reports a transient resource failure on its first
/// run and completes on every later run, counting the runs.
#[derive(Debug)]
struct TransientThenOkRunner {
    runs: AtomicU32,
}

impl Default for TransientThenOkRunner {
    fn default() -> Self {
        Self {
            runs: AtomicU32::new(0),
        }
    }
}

impl TransientThenOkRunner {
    fn runs(&self) -> u32 {
        self.runs.load(Ordering::SeqCst)
    }
}

impl MatchRunner for TransientThenOkRunner {
    fn run(&self, fixture: &MatchFixture, _tick_cap: u64) -> MatchOutcome {
        if self.runs.fetch_add(1, Ordering::SeqCst) == 0 {
            return MatchOutcome::Failed {
                code: "resource_exhausted".to_string(),
                message: "could not allocate the frame buffer".to_string(),
                last_good_tick: 120,
                kind: FailureKind::Transient,
            };
        }
        MatchOutcome::Finished(completed_for(fixture))
    }
}

/// A runner that fails transiently for the id `fail-` and blocks on a
/// gate for every other id, so a test can hold the only permit.
#[derive(Debug)]
struct GatedTransientRunner {
    gate: Arc<Gate>,
    runs: AtomicU32,
}

impl Default for GatedTransientRunner {
    fn default() -> Self {
        Self {
            gate: Arc::new(Gate::new()),
            runs: AtomicU32::new(0),
        }
    }
}

impl GatedTransientRunner {
    fn gate(&self) -> Arc<Gate> {
        Arc::clone(&self.gate)
    }
}

impl MatchRunner for GatedTransientRunner {
    fn run(&self, fixture: &MatchFixture, _tick_cap: u64) -> MatchOutcome {
        self.runs.fetch_add(1, Ordering::SeqCst);
        if fixture.validated_match.id == "fail-0" {
            return MatchOutcome::Failed {
                code: "resource_exhausted".to_string(),
                message: "could not allocate the frame buffer".to_string(),
                last_good_tick: 120,
                kind: FailureKind::Transient,
            };
        }
        self.gate.wait();
        MatchOutcome::Finished(completed_for(fixture))
    }
}

/// Opens the gate when the test ends, so a failed assertion cannot
/// leave a blocked run behind.
struct GateGuard(Arc<GatedTransientRunner>);

impl Drop for GateGuard {
    fn drop(&mut self) {
        self.0.gate().open();
    }
}

/// A transient failure records the time a later start may try the
/// match again.
#[tokio::test]
async fn a_transient_failure_records_a_retry_time() {
    let runner = Arc::new(TransientThenOkRunner::default());
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::clone(&runner)))
            .with_cap(1)
            .with_retry_window(PATIENCE),
    );

    let before = Instant::now();
    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Failed { .. })
    })
    .await;

    let MatchState::Failed {
        code,
        message,
        last_good_tick,
        retry_at,
        ..
    } = service.status("m-0001")
    else {
        panic!("expected Failed");
    };
    assert_eq!(code, "resource_exhausted");
    assert!(!message.is_empty());
    assert_eq!(last_good_tick, 120);
    let retry_at = retry_at.expect("a transient failure records a retry time");
    assert!(
        retry_at >= before + PATIENCE,
        "the retry time must open a full window after the failure"
    );
    assert!(
        retry_at <= Instant::now() + PATIENCE,
        "the retry time must not be further away than the window"
    );
}

/// A start inside the retry window reports the failure with the time
/// left to wait, and does not run the match again.
#[tokio::test]
async fn a_start_inside_the_window_reports_the_failure_with_the_remaining_wait() {
    let runner = Arc::new(TransientThenOkRunner::default());
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::clone(&runner)))
            .with_cap(1)
            .with_retry_window(PATIENCE),
    );

    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Failed { .. })
    })
    .await;

    let outcome = service.start("m-0001").await;
    let Ok(StartOutcome::RetryAfter {
        code,
        message,
        last_good_tick,
        retry_in,
    }) = outcome
    else {
        panic!("expected RetryAfter, got {outcome:?}");
    };
    assert_eq!(code, "resource_exhausted");
    assert!(!message.is_empty());
    assert_eq!(last_good_tick, 120);
    assert!(
        retry_in > Duration::ZERO,
        "the remaining wait must be positive inside the window"
    );
    assert!(
        retry_in <= PATIENCE,
        "the remaining wait must not exceed the window: {retry_in:?}"
    );
    assert_eq!(
        runner.runs(),
        1,
        "a start inside the window must not run the match again"
    );
}

/// A start after the retry window schedules the match again, and the
/// match runs to completion.
#[tokio::test]
async fn a_start_after_the_window_runs_the_match_again() {
    let runner = Arc::new(TransientThenOkRunner::default());
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::clone(&runner)))
            .with_cap(1)
            .with_retry_window(WINDOW),
    );

    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Failed { .. })
    })
    .await;

    tokio::time::sleep(WINDOW + Duration::from_millis(20)).await;

    let outcome = service.start("m-0001").await;
    assert_eq!(outcome, Ok(StartOutcome::Started));

    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Ready { .. })
    })
    .await;
    assert_eq!(runner.runs(), 2, "the match ran again after the window");
}

/// A start after the retry window re-queues the match when the
/// concurrency cap is full: the match waits for a permit like any
/// other queued match.
#[tokio::test]
async fn a_start_after_the_window_re_queues_the_match_when_the_cap_is_full() {
    let runner = Arc::new(GatedTransientRunner::default());
    let _guard = GateGuard(Arc::clone(&runner));
    let service = Arc::new(
        MatchService::new(fixture_set(&["fail-0", "m-0002"]), as_dyn(Arc::clone(&runner)))
            .with_cap(1)
            .with_retry_window(WINDOW),
    );

    // The first match takes the only permit and fails transiently.
    let _ = service.start("fail-0").await;
    wait_until(&service, "fail-0", |state| {
        matches!(state, MatchState::Failed { .. })
    })
    .await;

    // The second match takes the permit and stays in flight.
    let _ = service.start("m-0002").await;
    wait_until(&service, "m-0002", |state| {
        matches!(state, MatchState::Running)
    })
    .await;

    tokio::time::sleep(WINDOW + Duration::from_millis(20)).await;

    // The retried match finds no permit: it waits in the queue.
    let outcome = service.start("fail-0").await;
    assert_eq!(outcome, Ok(StartOutcome::Queued));
    assert_eq!(service.status("fail-0"), MatchState::Queued);
}

/// A transient failure that fails again records a fresh retry window.
#[tokio::test]
async fn a_transient_failure_that_fails_again_records_a_new_window() {
    let runner = Arc::new(AlwaysTransientRunner::default());
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::clone(&runner)))
            .with_cap(1)
            .with_retry_window(WINDOW),
    );

    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Failed { .. })
    })
    .await;
    let first = service.status("m-0001");
    let MatchState::Failed {
        retry_at: Some(first_retry_at),
        ..
    } = first
    else {
        panic!("expected a transient failure");
    };

    tokio::time::sleep(WINDOW + Duration::from_millis(20)).await;

    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Failed { .. })
    })
    .await;
    let second = service.status("m-0001");
    let MatchState::Failed {
        retry_at: Some(second_retry_at),
        ..
    } = second
    else {
        panic!("expected a transient failure");
    };

    assert!(
        second_retry_at > first_retry_at,
        "the second failure opens a fresh window: {second_retry_at:?} vs {first_retry_at:?}"
    );
    assert_eq!(runner.runs(), 2);
}

/// A deterministic failure keeps its cached result and is never
/// retried.
#[tokio::test]
async fn a_deterministic_failure_is_never_retried() {
    let runner = Arc::new(TickCapFailureRunner::default());
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::clone(&runner)))
            .with_cap(1)
            .with_retry_window(PATIENCE),
    );

    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Failed { .. })
    })
    .await;

    // The window is long, so this start is still inside it: the
    // deterministic failure is not a transient one and is never
    // retried, whatever the window says.
    let outcome = service.start("m-0001").await;
    assert_eq!(outcome, Ok(StartOutcome::AlreadyComplete));
    assert_eq!(
        runner.runs(),
        1,
        "a deterministic failure is cached, not re-run"
    );
}
