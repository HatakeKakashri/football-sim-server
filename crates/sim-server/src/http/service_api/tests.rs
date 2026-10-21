//! The status matrix of the start and manifest routes (server-API
//! ticket 08).
//!
//! Every row of the matrix is a case in one table. A request goes
//! through the router with `tower::ServiceExt::oneshot`, so no socket
//! is opened and no handler is called directly. The service under test
//! is driven by the fake runners, never by the real simulation.
//!
//! This module holds the shared fixtures; the tests live in the
//! submodules next to it.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use tower::ServiceExt;

use crate::api::runner::MatchRunner;
use crate::api::{MatchRegistry, MatchStore, ServerConfig};
use crate::http::router_with_service;
use crate::service::{MatchService, MatchState, StartOutcome};
use crate::test_support::{
    AlwaysTransientRunner, GateGuard, GatedRunner, InstantRunner, TickCapFailureRunner, as_dyn,
    fixture_set, wait_until,
};

mod matrix;
mod routes;
mod segments;

/// A counter of the runs of the runner under a row.
type RunCounter = Arc<dyn Fn() -> u32 + Send + Sync>;

/// One row of the status matrix: a service, a request, and what the
/// response must say.
struct Row {
    /// What the row covers.
    name: &'static str,
    /// The service under test.
    service: Arc<MatchService>,
    /// The method the row sends.
    method: Method,
    /// The path the row sends.
    path: &'static str,
    /// The status the response must carry.
    status: StatusCode,
    /// The `state` the body must carry.
    state: &'static str,
    /// The `Cache-Control` the response must carry.
    cache: &'static str,
    /// Whether the response must carry a `Retry-After` header.
    retry_after: bool,
    /// Whether the body must carry `retry_after_ms`.
    retry_after_ms: bool,
    /// The failure's code, when the row is a failure.
    code: Option<&'static str>,
    /// Whether the body must be the manifest.
    manifest: bool,
    /// How many times the runner must have run, when the row proves
    /// that a start is idempotent.
    expected_runs: Option<u32>,
    /// The runner's run counter, when the row counts runs.
    runs: Option<RunCounter>,
    /// Opens the gate of a blocked run when the row is done, so a
    /// blocked run cannot outlive the test.
    gate: Option<GateGuard>,
}

impl Row {
    /// A row that expects the given status, state and cache header.
    fn new(
        name: &'static str,
        service: Arc<MatchService>,
        method: Method,
        path: &'static str,
        status: StatusCode,
        state: &'static str,
    ) -> Self {
        Self {
            name,
            service,
            method,
            path,
            status,
            state,
            cache: "no-store",
            retry_after: false,
            retry_after_ms: false,
            code: None,
            manifest: false,
            expected_runs: None,
            runs: None,
            gate: None,
        }
    }

    /// Open the gate of a blocked run, so it cannot outlive the row.
    ///
    /// The guard opens the gate when it is dropped as well, so a row
    /// that fails an assertion still releases its run.
    fn release(&self) {
        if let Some(gate) = &self.gate {
            gate.open();
        }
    }
}

/// A service with the given matches and runner.
fn service_with(
    runner: Arc<dyn MatchRunner>,
    match_ids: &[&str],
    cap: usize,
) -> Arc<MatchService> {
    Arc::new(MatchService::new(fixture_set(match_ids), runner).with_cap(cap))
}

/// A request for one of the routes.
fn request(method: Method, path: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .expect("invariant: a test request is a valid HTTP request")
}

/// The legacy store the router needs; the new routes do not use it.
fn test_store() -> Arc<MatchStore> {
    let mut registry = MatchRegistry::default();
    let single = MatchRegistry::single_test("m-0000", 7);
    registry.fixtures.extend(single.fixtures);
    Arc::new(MatchStore::new(ServerConfig::default(), registry))
}

/// The status, the headers and the body of a response.
async fn response_parts(
    response: axum::response::Response,
) -> (StatusCode, axum::http::HeaderMap, String) {
    let (mut parts, body) = response.into_parts();
    let headers = std::mem::take(&mut parts.headers);
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .expect("invariant: a test body fits in memory");
    (
        parts.status,
        headers,
        String::from_utf8(bytes.to_vec()).expect("invariant: a response body is UTF-8"),
    )
}
