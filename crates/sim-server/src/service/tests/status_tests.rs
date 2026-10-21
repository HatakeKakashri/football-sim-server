//! The status answer (server-API ticket 02).
//!
//! The service answers a match's status in one call, and the answer
//! covers every case of a match's lifecycle: an id that is not a
//! fixture, a match that was never started, a queued match, a running
//! match, a ready match and a failed match.
//!
//! The service is driven entirely through the fake runners: no HTTP,
//! no sockets and no direct state poking.

use std::time::Duration;

use super::*;

use crate::test_support::GateGuard;

/// An id that is not a fixture is not a match that was never started.
#[tokio::test]
async fn an_id_that_is_not_a_fixture_is_unknown() {
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::new(InstantRunner))).with_cap(1),
    );

    assert_eq!(service.status("nope"), MatchState::UnknownFixture);
}

/// A fixture that was never started says so.
#[tokio::test]
async fn a_fixture_that_was_never_started_is_not_started() {
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::new(InstantRunner))).with_cap(1),
    );

    assert_eq!(service.status("m-0001"), MatchState::NotStarted);
}

/// The match that took the permit is running; the one that found no
/// permit is queued.
#[tokio::test]
async fn a_running_match_and_a_queued_match_are_told_apart() {
    let runner = Arc::new(GatedRunner::default());
    let gate = GateGuard::new(Arc::clone(&runner));
    let service = Arc::new(
        MatchService::new(
            fixture_set(&["m-0", "m-1"]),
            as_dyn(Arc::clone(&runner)),
        )
        .with_cap(1),
    );

    // The first match takes the only permit and stays in flight.
    let _ = service.start("m-0").await;
    wait_until(&service, "m-0", |state| {
        matches!(state, MatchState::Running)
    })
    .await;
    // The second match finds no permit: it waits in the queue.
    let _ = service.start("m-1").await;

    assert_eq!(service.status("m-0"), MatchState::Running);
    assert_eq!(service.status("m-1"), MatchState::Queued);

    drop(gate);
}

/// A ready match carries the published bytes of the match, so a caller
/// that is told "ready" can read the manifest's bytes and a segment's
/// bytes without asking a second question.
#[tokio::test]
async fn a_ready_match_carries_the_published_match() {
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::new(InstantRunner))).with_cap(1),
    );

    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Ready { .. })
    })
    .await;

    let status = service.status("m-0001");
    let MatchState::Ready { published } = status else {
        panic!("expected Ready");
    };
    assert!(!published.build_id().is_empty(), "the match has a build id");
    assert!(
        !published.manifest_body().is_empty(),
        "the published match carries the manifest's body"
    );
    assert!(!published.is_empty(), "a rendered match has segments");
    let first = published
        .segment(0)
        .expect("invariant: the manifest lists the first segment");
    assert!(
        !first.body.is_empty(),
        "the published segment carries its body"
    );
    assert!(
        !first.gzipped_body.is_empty(),
        "the published segment carries its gzip coding"
    );
}

/// A failed match carries the failure and the wait before a retry: a
/// deterministic failure is never retried, so it carries no wait.
#[tokio::test]
async fn a_failed_match_carries_the_failure() {
    let runner = Arc::new(TickCapFailureRunner::default());
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::clone(&runner))).with_cap(1),
    );

    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Failed { .. })
    })
    .await;

    let status = service.status("m-0001");
    let MatchState::Failed {
        code,
        message,
        last_good_tick,
        retry_at,
        retry_in,
    } = status
    else {
        panic!("expected Failed");
    };
    assert_eq!(code, "tick_cap_exceeded");
    assert!(!message.is_empty());
    assert_eq!(last_good_tick, TICK_CAP);
    assert!(
        retry_at.is_none(),
        "a deterministic failure is never retried"
    );
    assert!(
        retry_in.is_none(),
        "a deterministic failure carries no wait"
    );
}

/// A transient failure carries the time left before the match may be
/// retried, so a caller never has to compute a wait from a deadline.
#[tokio::test]
async fn a_transient_failure_carries_the_time_left_before_a_retry() {
    let window = Duration::from_mins(1);
    let runner = Arc::new(AlwaysTransientRunner::default());
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::clone(&runner)))
            .with_cap(1)
            .with_retry_window(window),
    );

    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Failed { .. })
    })
    .await;

    let status = service.status("m-0001");
    let MatchState::Failed {
        code,
        last_good_tick,
        retry_at,
        retry_in,
        ..
    } = status
    else {
        panic!("expected Failed");
    };
    assert_eq!(code, "resource_exhausted");
    assert_eq!(last_good_tick, 120);
    assert!(
        retry_at.is_some(),
        "a transient failure opens a window before a retry"
    );
    let retry_in = retry_in.expect("a transient failure carries the wait");
    assert!(
        retry_in > Duration::ZERO,
        "the wait is positive inside the window: {retry_in:?}"
    );
    assert!(
        retry_in <= window,
        "the wait is not longer than the window: {retry_in:?}"
    );
}
