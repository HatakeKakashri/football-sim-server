//! The rows of the status matrix (server-API ticket 08).
//!
//! Every row of the matrix is a case in the table: a request, and what
//! the response must say. The rows live in their own module so the
//! matrix's assertions stay readable.

use super::*;
/// Every row of the matrix, in the order the matrix reads.
pub(super) async fn matrix_rows() -> Vec<Row> {
    vec![
        post_unknown_fixture(),
        post_not_started(),
        post_already_running().await,
        post_queued().await,
        post_ready().await,
        post_deterministic_failure().await,
        post_transient_failure().await,
        get_unknown_fixture(),
        get_not_started(),
        get_queued().await,
        get_running().await,
        get_ready().await,
        get_ready_gzip().await,
        get_deterministic_failure().await,
        get_transient_failure().await,
    ]
}

/// `POST` on an id that is not a fixture: 404.
fn post_unknown_fixture() -> Row {
    let service = service_with(as_dyn(Arc::new(InstantRunner)), &["m-0001"], 1);
    let mut row = Row::new(
        "post: an unknown fixture is 404",
        service,
        Method::POST, "/match/nope/start",
        StatusCode::NOT_FOUND,
        "unknown",
    );
    row.retry_after_ms = false;
    row
}

/// `POST` on a match that was never started: 202, and the match runs.
fn post_not_started() -> Row {
    let service = service_with(as_dyn(Arc::new(InstantRunner)), &["m-0001"], 1);
    let mut row = Row::new(
        "post: a match that was not started is 202",
        service,
        Method::POST, "/match/m-0001/start",
        StatusCode::ACCEPTED,
        "running",
    );
    row.retry_after = true;
    row.retry_after_ms = true;
    row
}

/// `POST` on a match that is already running: 202, and the match is
/// not run a second time.
async fn post_already_running() -> Row {
    let runner = Arc::new(GatedRunner::default());
    let gate = GateGuard::new(Arc::clone(&runner));
    let service = service_with(as_dyn(Arc::clone(&runner)), &["m-0001"], 1);

    let first = service.start("m-0001").await;
    assert_eq!(first, Ok(StartOutcome::Started));
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Running)
    })
    .await;

    let mut row = Row::new(
        "post: a running match is 202 and is not run again",
        service,
        Method::POST, "/match/m-0001/start",
        StatusCode::ACCEPTED,
        "running",
    );
    row.retry_after = true;
    row.retry_after_ms = true;
    row.runs = Some(Arc::new({
        let runner = Arc::clone(&runner);
        move || runner.runs()
    }));
    row.expected_runs = Some(1);
    row.gate = Some(gate);
    row
}

/// `POST` on a match that waits for a permit: 202, queued.
async fn post_queued() -> Row {
    let runner = Arc::new(GatedRunner::default());
    let gate = GateGuard::new(Arc::clone(&runner));
    let service = service_with(as_dyn(Arc::clone(&runner)), &["m-0", "m-1"], 1);

    // The first match takes the only permit and stays in flight.
    let _ = service.start("m-0").await;
    wait_until(&service, "m-0", |state| {
        matches!(state, MatchState::Running)
    })
    .await;
    // The second match finds no permit: it waits in the queue.
    let _ = service.start("m-1").await;
    assert_eq!(service.status("m-1"), MatchState::Queued);

    let mut row = Row::new(
        "post: a queued match is 202",
        service,
        Method::POST,
        "/match/m-1/start",
        StatusCode::ACCEPTED,
        "queued",
    );
    row.retry_after = true;
    row.retry_after_ms = true;
    row.gate = Some(gate);
    row
}

/// `POST` on a match that is ready: 200 with the manifest.
async fn post_ready() -> Row {
    let service = service_with(as_dyn(Arc::new(InstantRunner)), &["m-0001"], 1);
    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Ready { .. })
    })
    .await;

    let mut row = Row::new(
        "post: a ready match is 200 with the manifest",
        service,
        Method::POST, "/match/m-0001/start",
        StatusCode::OK,
        "ready",
    );
    row.cache = "no-cache";
    row.representation = Representation::Identity;
    row
}

/// `POST` on a match that failed deterministically: 500.
async fn post_deterministic_failure() -> Row {
    let runner = Arc::new(TickCapFailureRunner::default());
    let service = service_with(as_dyn(Arc::clone(&runner)), &["m-0001"], 1);
    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Failed { .. })
    })
    .await;

    let mut row = Row::new(
        "post: a deterministic failure is 500",
        service,
        Method::POST, "/match/m-0001/start",
        StatusCode::INTERNAL_SERVER_ERROR,
        "failed",
    );
    row.code = Some("tick_cap_exceeded");
    row
}

/// `POST` on a match that failed transiently inside its retry window:
/// 503 with a `Retry-After`.
async fn post_transient_failure() -> Row {
    let runner = Arc::new(AlwaysTransientRunner::default());
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::clone(&runner)))
            .with_cap(1)
            // A window long enough that the start of the row is still
            // inside it.
            .with_retry_window(Duration::from_mins(1)),
    );
    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Failed { .. })
    })
    .await;

    let mut row = Row::new(
        "post: a transient failure is 503 with a retry",
        service,
        Method::POST, "/match/m-0001/start",
        StatusCode::SERVICE_UNAVAILABLE,
        "failed",
    );
    row.retry_after = true;
    row.retry_after_ms = true;
    row.code = Some("resource_exhausted");
    row
}

/// `GET` on an id that is not a fixture: 404.
fn get_unknown_fixture() -> Row {
    let service = service_with(as_dyn(Arc::new(InstantRunner)), &["m-0001"], 1);
    Row::new(
        "get: an unknown fixture is 404",
        service,
        Method::GET, "/match/nope/manifest",
        StatusCode::NOT_FOUND,
        "unknown",
    )
}

/// `GET` on a match that was never started: 409.
fn get_not_started() -> Row {
    let service = service_with(as_dyn(Arc::new(InstantRunner)), &["m-0001"], 1);
    Row::new(
        "get: a match that was not started is 409",
        service,
        Method::GET, "/match/m-0001/manifest",
        StatusCode::CONFLICT,
        "not_started",
    )
}

/// `GET` on a match that waits for a permit: 202.
async fn get_queued() -> Row {
    let runner = Arc::new(GatedRunner::default());
    let gate = GateGuard::new(Arc::clone(&runner));
    let service = service_with(as_dyn(Arc::clone(&runner)), &["m-0", "m-1"], 1);

    let _ = service.start("m-0").await;
    wait_until(&service, "m-0", |state| {
        matches!(state, MatchState::Running)
    })
    .await;
    let _ = service.start("m-1").await;
    assert_eq!(service.status("m-1"), MatchState::Queued);

    let mut row = Row::new(
        "get: a queued match is 202",
        service,
        Method::GET, "/match/m-1/manifest",
        StatusCode::ACCEPTED,
        "queued",
    );
    row.retry_after = true;
    row.retry_after_ms = true;
    row.gate = Some(gate);
    row
}

/// `GET` on a match that is running: 202.
async fn get_running() -> Row {
    let runner = Arc::new(GatedRunner::default());
    let gate = GateGuard::new(Arc::clone(&runner));
    let service = service_with(as_dyn(Arc::clone(&runner)), &["m-0001"], 1);

    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Running)
    })
    .await;

    let mut row = Row::new(
        "get: a running match is 202",
        service,
        Method::GET, "/match/m-0001/manifest",
        StatusCode::ACCEPTED,
        "running",
    );
    row.retry_after = true;
    row.retry_after_ms = true;
    row.gate = Some(gate);
    row
}

/// `GET` on a match that is ready: 200 with the manifest.
async fn get_ready() -> Row {
    let service = service_with(as_dyn(Arc::new(InstantRunner)), &["m-0001"], 1);
    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Ready { .. })
    })
    .await;

    let mut row = Row::new(
        "get: a ready match is 200 with the manifest",
        service,
        Method::GET, "/match/m-0001/manifest",
        StatusCode::OK,
        "ready",
    );
    row.cache = "no-cache";
    row.representation = Representation::Identity;
    row
}

/// `GET` on a match that is ready, by a client that accepts gzip: 200
/// with the gzipped manifest and the gzip validator.
async fn get_ready_gzip() -> Row {
    let service = service_with(as_dyn(Arc::new(InstantRunner)), &["m-0001"], 1);
    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Ready { .. })
    })
    .await;

    let mut row = Row::new(
        "get: a ready match is 200 with the gzipped manifest",
        service,
        Method::GET, "/match/m-0001/manifest",
        StatusCode::OK,
        "ready",
    );
    row.cache = "no-cache";
    row.representation = Representation::Gzip;
    row
}

/// `GET` on a match that failed deterministically: 500.
async fn get_deterministic_failure() -> Row {
    let runner = Arc::new(TickCapFailureRunner::default());
    let service = service_with(as_dyn(Arc::clone(&runner)), &["m-0001"], 1);
    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Failed { .. })
    })
    .await;

    let mut row = Row::new(
        "get: a deterministic failure is 500",
        service,
        Method::GET, "/match/m-0001/manifest",
        StatusCode::INTERNAL_SERVER_ERROR,
        "failed",
    );
    row.code = Some("tick_cap_exceeded");
    row
}

/// `GET` on a match that failed transiently: 503 with a `Retry-After`.
async fn get_transient_failure() -> Row {
    let runner = Arc::new(AlwaysTransientRunner::default());
    let service = Arc::new(
        MatchService::new(fixture_set(&["m-0001"]), as_dyn(Arc::clone(&runner)))
            .with_cap(1)
            .with_retry_window(Duration::from_mins(1)),
    );
    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Failed { .. })
    })
    .await;

    let mut row = Row::new(
        "get: a transient failure is 503 with a retry",
        service,
        Method::GET, "/match/m-0001/manifest",
        StatusCode::SERVICE_UNAVAILABLE,
        "failed",
    );
    row.retry_after = true;
    row.retry_after_ms = true;
    row.code = Some("resource_exhausted");
    row
}
