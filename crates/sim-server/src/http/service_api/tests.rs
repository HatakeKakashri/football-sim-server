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

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::Router;
use flate2::read::GzDecoder;
use tower::ServiceExt;

use crate::api::runner::MatchRunner;
use crate::http::{HttpConfig, router};
use crate::service::{MatchService, MatchState, StartOutcome};
use crate::test_support::{
    AlwaysTransientRunner, GateGuard, GatedRunner, InstantRunner, TickCapFailureRunner, as_dyn,
    fixture_set, wait_until,
};

mod cors;
mod manifest_codings;
mod matrix;
mod matrix_table;
mod revalidation;
mod routes;
mod segment_urls;
mod segments;

/// A counter of the runs of the runner under a row.
type RunCounter = Arc<dyn Fn() -> u32 + Send + Sync>;

/// The representation a row expects the manifest in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Representation {
    /// The row serves no manifest at all.
    None,
    /// The identity body.
    Identity,
    /// The gzip coding of the body.
    Gzip,
}

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
    /// The representation of the manifest the row expects.
    representation: Representation,
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
            representation: Representation::None,
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

/// The transport settings of a router under test.
fn http_config(api_token: Option<&str>, cors_origins: &[&str]) -> HttpConfig {
    HttpConfig {
        api_token: api_token.map(ToString::to_string),
        cors_origins: cors_origins.iter().map(|origin| (*origin).to_string()).collect(),
        ..HttpConfig::default()
    }
}

/// A request for one of the routes.
fn request(method: Method, path: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .expect("invariant: a test request is a valid HTTP request")
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

/// The status, the headers and the raw bytes of a response.
///
/// The body is kept as bytes: a gzip body is not UTF-8.
async fn response_bytes(
    response: axum::response::Response,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let (mut parts, body) = response.into_parts();
    let headers = std::mem::take(&mut parts.headers);
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .expect("invariant: a test body fits in memory");
    (parts.status, headers, bytes.to_vec())
}

/// The status, the headers and the body of a response, decoded when
/// the response carries a gzip coding.
///
/// A coded body is not text, so a test that reads the document behind
/// it decodes it first.
async fn decoded_parts(
    response: axum::response::Response,
) -> (StatusCode, HeaderMap, String) {
    let (status, headers, bytes) = response_bytes(response).await;
    let body = if headers.contains_key(header::CONTENT_ENCODING) {
        String::from_utf8(gunzip(&bytes)).expect("invariant: a decoded body is UTF-8")
    } else {
        String::from_utf8(bytes).expect("invariant: a response body is UTF-8")
    };
    (status, headers, body)
}

/// The gzip bytes decoded back into the body they carry.
fn gunzip(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .expect("invariant: a served gzip body decodes");
    out
}

/// A service with one ready match, its router, and the build id of
/// the published match.
async fn ready_match() -> (Arc<MatchService>, Router, String) {
    let runner = Arc::new(InstantRunner);
    let service = service_with(as_dyn(Arc::clone(&runner)), &["m-0001"], 1);
    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Ready { .. })
    })
    .await;
    let MatchState::Ready { published } = service.status("m-0001") else {
        panic!("invariant: the match under test is ready");
    };
    let build_id = published.build_id().to_string();
    let router = router(Arc::clone(&service), http_config(None, &[]));
    (service, router, build_id)
}

/// The validator of the identity representation: the quoted build id.
fn identity_validator(build_id: &str) -> String {
    format!("\"{build_id}\"")
}

/// The validator of the gzip representation: the quoted build id and
/// the gzip suffix.
fn gzip_validator(build_id: &str) -> String {
    format!("\"{build_id}-gzip\"")
}

/// Whether the response varies on the coding the client accepts.
fn varies_on_accept_encoding(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::VARY)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|part| part.trim().eq_ignore_ascii_case("Accept-Encoding"))
}

/// A service with one ready match, driven by the instant runner.
async fn ready_service(match_id: &str) -> Arc<MatchService> {
    let service = service_with(as_dyn(Arc::new(InstantRunner)), &[match_id], 1);
    let _ = service.start(match_id).await;
    wait_until(&service, match_id, |state| {
        matches!(state, MatchState::Ready { .. })
    })
    .await;
    service
}

/// The lowercase hex of a digest.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The value of a header, or the empty string when it is absent.
fn header_value(headers: &HeaderMap, name: header::HeaderName) -> String {
    headers
        .get(name)
        .map(|value| value.to_str().unwrap_or_default().to_string())
        .unwrap_or_default()
}
