//! Tests for the match lifecycle (server-API ticket 05).
//!
//! The service is driven entirely through the fake runners: no HTTP,
//! no sockets and no direct state poking.

use super::*;


#[tokio::test]
async fn unknown_fixture_is_rejected() {
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::new(InstantRunner))).with_cap(1),
    );

    assert_eq!(service.start("nope").await, Err(StartError::UnknownFixture));
    assert_eq!(service.status("nope"), MatchState::UnknownFixture);
    assert_eq!(service.status("m-0001"), MatchState::NotStarted);
}

#[tokio::test]
async fn known_match_moves_out_of_not_started_and_becomes_ready() {
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::new(InstantRunner))).with_cap(1),
    );

    assert_eq!(service.status("m-0001"), MatchState::NotStarted);

    assert_eq!(service.start("m-0001").await, Ok(StartOutcome::Started));

    wait_until(&service, "m-0001", |state| matches!(state, MatchState::Ready { .. }))
        .await;
    assert!(matches!(service.status("m-0001"), MatchState::Ready { .. }));
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
    assert_eq!(service.status(&match_id), MatchState::Running);

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

    let state = service.status("m-0001");
    let MatchState::Ready { published } = state else {
        panic!("expected Ready, got {state:?}");
    };
    assert!(!published.build_id().is_empty());
    assert!(
        !published.manifest_body().is_empty(),
        "the manifest's body is published"
    );
    assert!(!published.is_empty(), "at least one segment must be rendered");
    for number in 0..u32::try_from(published.len()).expect("invariant: a segment count fits in u32") {
        let segment = published
            .segment(number)
            .expect("invariant: every published segment is in the index");
        assert!(
            !segment.body.is_empty(),
            "segment {number} carries its uncompressed body"
        );
        assert!(
            !segment.gzipped_body.is_empty(),
            "segment {number} carries its gzip coding"
        );
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

    match service.status("panic-0") {
        MatchState::Failed {
            code,
            message,
            last_good_tick,
            retry_at,
            ..
        } => {
            assert_eq!(code, "runner_panic", "a panic reports a distinct code");
            assert!(!message.is_empty());
            assert_eq!(last_good_tick, 0);
            assert!(
                retry_at.is_none(),
                "a panic repeats: it is never retried"
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }

    // The service keeps working for other matches.
    let _ = service.start("ok-0").await;
    wait_until(&service, "ok-0", |state| matches!(state, MatchState::Ready { .. }))
        .await;
    assert!(matches!(service.status("ok-0"), MatchState::Ready { .. }));
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
        retry_at,
        ..
    } = service.status("m-0001")
    else {
        panic!("expected Failed");
    };
    assert_eq!(code, "tick_cap_exceeded");
    assert!(!message.is_empty());
    assert_eq!(runner.runs(), 1);
    assert!(
        retry_at.is_none(),
        "a deterministic failure is cached, not retried"
    );

    // A later start must not re-run the deterministic failure.
    assert_eq!(service.start("m-0001").await, Ok(StartOutcome::AlreadyComplete));
    assert_eq!(runner.runs(), 1, "the failure is cached, not re-run");

    match service.status("m-0001") {
        MatchState::Failed {
            code: cached_code,
            message: cached_message,
            last_good_tick: cached_tick,
            retry_at: cached_retry_at,
            ..
        } => {
            assert_eq!(cached_code, code);
            assert_eq!(cached_message, message);
            assert_eq!(cached_tick, last_good_tick);
            assert_eq!(cached_retry_at, retry_at);
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
    assert_eq!(service.status("blocked-0"), MatchState::Running);

    let _ = service.start("ok-0").await;
    wait_until(&service, "ok-0", |state| matches!(state, MatchState::Ready { .. }))
        .await;
    assert!(matches!(service.status("ok-0"), MatchState::Ready { .. }));

    runner.gate().open();
    wait_until(&service, "blocked-0", |state| matches!(state, MatchState::Ready { .. }))
        .await;
    assert_eq!(runner.runs(), 2, "both matches ran exactly once");
}
