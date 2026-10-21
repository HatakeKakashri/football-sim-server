//! The match service's HTTP surface: start a match, read its manifest,
//! read its segments.
//!
//! Three routes, all driven by the [`MatchService`] (server-API tickets
//! 08 and 09):
//!
//! - `POST /match/{match_id}/start` starts the match and reports what
//!   the start did.
//! - `GET /match/{match_id}/manifest` reads the manifest of a match
//!   that has already started. It never starts work.
//! - `GET /match/{match_id}/segments/{build_id}/{segment}` reads one
//!   segment of a ready match from the immutable URL the manifest
//!   publishes. It never starts work either.
//!
//! # One rule, one answer
//!
//! The routes do not each decide what a match's state means. The
//! service answers a match's status in one call
//! ([`MatchService::status`]), and this module turns that answer into
//! a response in one place: [`status_answer`]. The start route and the
//! manifest route come through the same mapping, so "running" means
//! the same thing in both.
//!
//! The `state` values the wire uses are declared here, once, and both
//! route modules use them. The manifest's own `ready` state is the one
//! exception: it is declared with the body that carries it, in the
//! render module ([`crate::api::render::STATE_READY`]) — the wire
//! format of a manifest body lives in one place, and the render
//! layer must not depend on this one.
//!
//! The wait before a retry is the service's answer, so no route
//! computes one and no route reads the clock.
//!
//! # The status matrix
//!
//! | Match state | `POST /start` | `GET /manifest` | `GET /segment` |
//! |---|---|---|---|
//! | unknown fixture | 404 | 404 | 404 |
//! | not started | 202 (queued or running) | 409 `{state: not_started}` | 409 `{state: not_ready}` |
//! | queued | 202 | 202 + `Retry-After` | 409 |
//! | running | 202 | 202 + `Retry-After` | 409 |
//! | ready | 200 + manifest | 200 + manifest | 200 + the segment |
//! | failed, deterministic | 500 | 500 | 409 |
//! | failed, transient | 503 + `Retry-After` | 503 + `Retry-After` | 409 |
//!
//! A ready match answers a segment request for an unknown build id or
//! an unknown segment number with 404; see [`segments`] for the
//! segment route's own matrix.
//!
//! # Bodies and headers
//!
//! Every body is JSON and carries a `state` field. The 200 body is the
//! manifest itself plus `state: ready`, so it keeps the manifest's
//! `result` field (server-API ticket 03).
//!
//! Every 202 body carries `retry_after_ms` next to the `Retry-After`
//! header: a browser's cross-origin `fetch` cannot read the header, so
//! the body repeats the wait.
//!
//! A failed body keeps the wire shape the old error stream used
//! (`code`, `message`, `last_good_tick`) next to the `state` field.
//!
//! The 200 manifest is the only answer of the start and manifest
//! routes that may be cached: it carries an `ETag` (the content-derived
//! build id) and `Cache-Control: no-cache`. Every other answer of
//! those routes is `no-store`.
//!
//! The segment route is the exception: a segment's bytes never change
//! behind their URL, so its 200 carries `Cache-Control: private,
//! max-age=31536000, immutable` and `Vary: Accept-Encoding`. See
//! [`segments`].

use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing};
use serde::Serialize;

use crate::api::render::PublishedMatch;
use crate::http::HttpConfig;
use crate::service::{MatchService, MatchState, StartError, StartOutcome};

mod segments;

#[cfg(test)]
mod tests;

/// How long a client should wait before it polls the manifest again,
/// in seconds.
pub const POLL_RETRY_AFTER_SECS: u64 = 1;

/// The same wait in milliseconds, carried in the body of a 202 so a
/// browser that cannot read the `Retry-After` header can pace itself.
pub const POLL_RETRY_AFTER_MS: u64 = POLL_RETRY_AFTER_SECS * 1_000;

/// The content type of a JSON body.
pub(crate) const CONTENT_TYPE_JSON: &str = "application/json";

/// The `state` a response reports for an id that is not a fixture.
pub(crate) const STATE_UNKNOWN: &str = "unknown";

/// The `state` a response reports for a match that was never started.
pub(crate) const STATE_NOT_STARTED: &str = "not_started";

/// The `state` a response reports for a queued match.
pub(crate) const STATE_QUEUED: &str = "queued";

/// The `state` a response reports for a running match.
pub(crate) const STATE_RUNNING: &str = "running";

/// The `state` a response reports for a failed match.
pub(crate) const STATE_FAILED: &str = "failed";

/// The `state` a response reports for a rejected request.
pub(crate) const STATE_UNAUTHORIZED: &str = "unauthorized";

/// The `state` a response reports for a match that has no segments.
pub(crate) const STATE_NOT_READY: &str = "not_ready";

/// The `state` a response reports for a segment that does not exist.
pub(crate) const STATE_NO_SEGMENT: &str = "no_segment";

/// The `state` a response reports for a segment that cannot be read.
pub(crate) const STATE_BROKEN: &str = "broken";

/// The state of the start and manifest routes.
#[derive(Debug, Clone)]
pub struct MatchApiState {
    /// The match service behind the routes.
    pub service: Arc<MatchService>,
    /// Transport settings: the bearer token this route set checks.
    pub http: HttpConfig,
}

/// The body of a response that carries no result: the match's state.
#[derive(Debug, Serialize)]
struct StateBody {
    /// The state the response reports.
    state: &'static str,
    /// The match the response is about.
    match_id: String,
    /// Why the response is what it is.
    message: String,
}

/// The body of a 202: the match is queued or running.
#[derive(Debug, Serialize)]
struct PendingBody {
    /// The state the response reports (`queued` or `running`).
    state: &'static str,
    /// The match the response is about.
    match_id: String,
    /// How long to wait before polling again, in milliseconds.
    retry_after_ms: u64,
}

/// The body of a 500 or a 503: the match failed.
#[derive(Debug, Serialize)]
struct FailureBody {
    /// Always `failed`.
    state: &'static str,
    /// The match the response is about.
    match_id: String,
    /// The failure's code, as the runner reported it.
    code: String,
    /// The failure's message.
    message: String,
    /// The tick the failed run reached.
    last_good_tick: u64,
    /// How long to wait before retrying, in milliseconds. Absent when
    /// the failure is deterministic and is never retried.
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after_ms: Option<u64>,
}

/// The routes of the match service: start a match, read its manifest,
/// read its segments.
///
/// They are the server's only routes; see [`crate::http::router`].
pub fn routes(state: MatchApiState) -> Router {
    Router::new()
        .route("/match/{match_id}/start", routing::post(post_start_handler))
        .route(
            "/match/{match_id}/manifest",
            routing::get(get_manifest_handler),
        )
        .route(
            "/match/{match_id}/segments/{build_id}/{segment}",
            routing::get(segments::get_segment_handler),
        )
        .with_state(state)
}

/// `POST /match/{match_id}/start`: start the match, or report the state
/// it is already in.
///
/// A start is idempotent: a match that is already queued or running is
/// not run twice, and a match that is already ready answers with its
/// manifest.
async fn post_start_handler(
    State(state): State<MatchApiState>,
    Path(match_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Some(response) = unauthorized(&state, &match_id, &headers) {
        return response;
    }
    match state.service.start(&match_id).await {
        Err(StartError::UnknownFixture) => unknown_response(&match_id),
        // The start decided the match's state: it runs now, or it
        // waits for a permit. The state the service just set goes
        // through the same mapping the manifest route uses, so
        // "running" means the same thing in both. The service is not
        // asked for its status here: the run may already have
        // finished, and the answer would be a 200 instead of a 202.
        Ok(StartOutcome::Started) => status_answer(&MatchState::Running, &match_id),
        Ok(StartOutcome::Queued) => status_answer(&MatchState::Queued, &match_id),
        // A run is already in flight, the match is already terminal, or
        // it failed inside its retry window: the service's answer
        // decides the response, the wait before a retry included.
        Ok(
            StartOutcome::AlreadyRunning
            | StartOutcome::AlreadyComplete
            | StartOutcome::RetryAfter { .. },
        ) => {
            let current = state.service.status(&match_id);
            status_answer(&current, &match_id)
        }
    }
}

/// `GET /match/{match_id}/manifest`: the manifest of a match that has
/// already started.
///
/// The route never starts work: a match that was never started answers
/// with a conflict.
async fn get_manifest_handler(
    State(state): State<MatchApiState>,
    Path(match_id): Path<String>,
) -> Response {
    let current = state.service.status(&match_id);
    status_answer(&current, &match_id)
}

/// The answer for a match's status.
///
/// This is the one rule that turns a match's state into a response:
/// the start route and the manifest route both come through here, so
/// "running" means the same thing in both.
///
/// The wait before a retry is the service's answer, so the route never
/// computes one.
fn status_answer(status: &MatchState, match_id: &str) -> Response {
    match status {
        MatchState::UnknownFixture => unknown_response(match_id),
        MatchState::NotStarted => not_started_response(match_id),
        MatchState::Running => pending_response(STATE_RUNNING, match_id),
        MatchState::Queued => pending_response(STATE_QUEUED, match_id),
        MatchState::Ready { published } => manifest_response(published),
        MatchState::Failed {
            code,
            message,
            last_good_tick,
            retry_in,
            ..
        } => failure_response(
            match_id,
            code,
            message,
            *last_good_tick,
            *retry_in,
        ),
    }
}

/// The 404: the match id is not a known fixture.
fn unknown_response(match_id: &str) -> Response {
    state_response_with(
        StatusCode::NOT_FOUND,
        STATE_UNKNOWN,
        match_id,
        "unknown match_id",
    )
}

/// The 409: the match is known, but it was never started.
fn not_started_response(match_id: &str) -> Response {
    state_response_with(
        StatusCode::CONFLICT,
        STATE_NOT_STARTED,
        match_id,
        "match not started; POST first",
    )
}

/// The 202: the match is queued or running.
///
/// The body repeats the wait in milliseconds, because a browser's
/// cross-origin `fetch` cannot read the `Retry-After` header.
fn pending_response(state: &'static str, match_id: &str) -> Response {
    let body = PendingBody {
        state,
        match_id: match_id.to_string(),
        retry_after_ms: POLL_RETRY_AFTER_MS,
    };
    let mut response = (StatusCode::ACCEPTED, Json(body)).into_response();
    set_header(&mut response, header::CACHE_CONTROL, "no-store");
    set_header(
        &mut response,
        header::RETRY_AFTER,
        &POLL_RETRY_AFTER_SECS.to_string(),
    );
    response
}

/// The 200: the manifest's published bytes.
///
/// The body is the bytes the render step published, copied into the
/// response: the manifest is never cloned and never re-serialised, so
/// a request costs no string per manifest row.
fn manifest_response(published: &PublishedMatch) -> Response {
    let body = Bytes::from_owner(published.manifest_body());
    let mut response = (StatusCode::OK, Body::from(body)).into_response();
    set_header(&mut response, header::CONTENT_TYPE, CONTENT_TYPE_JSON);
    // The build id is a digest of the rendered content, so it is a
    // strong validator for the manifest.
    set_header(
        &mut response,
        header::ETAG,
        &format!("\"{}\"", published.build_id()),
    );
    set_header(&mut response, header::CACHE_CONTROL, "no-cache");
    response
}

/// The 500 or the 503: the match failed.
///
/// A transient failure is a 503 with a `Retry-After`: the pressure
/// belongs to the host, so the match may be tried again. A
/// deterministic failure is a 500: it repeats, so it is not retried.
fn failure_response(
    match_id: &str,
    code: &str,
    message: &str,
    last_good_tick: u64,
    retry_in: Option<Duration>,
) -> Response {
    let status = if retry_in.is_some() {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    let body = FailureBody {
        state: STATE_FAILED,
        match_id: match_id.to_string(),
        code: code.to_string(),
        message: message.to_string(),
        last_good_tick,
        retry_after_ms: retry_in.map(duration_ms),
    };
    let mut response = (status, Json(body)).into_response();
    set_header(&mut response, header::CACHE_CONTROL, "no-store");
    if let Some(retry_in) = retry_in {
        set_header(
            &mut response,
            header::RETRY_AFTER,
            &retry_after_secs(retry_in).to_string(),
        );
    }
    response
}

/// A response that carries the match's state and nothing else.
fn state_response_with(
    status: StatusCode,
    state: &'static str,
    match_id: &str,
    message: &str,
) -> Response {
    let body = StateBody {
        state,
        match_id: match_id.to_string(),
        message: message.to_string(),
    };
    let mut response = (status, Json(body)).into_response();
    set_header(&mut response, header::CACHE_CONTROL, "no-store");
    response
}

/// The 401: the request did not carry the configured bearer token.
fn unauthorized(state: &MatchApiState, match_id: &str, headers: &HeaderMap) -> Option<Response> {
    let expected_token = state.http.api_token.as_ref()?;
    if super::auth::bearer_token_matches(expected_token, headers) {
        return None;
    }
    Some(state_response_with(
        StatusCode::UNAUTHORIZED,
        STATE_UNAUTHORIZED,
        match_id,
        "missing or invalid bearer token",
    ))
}

/// Set a header on the response, replacing any value it carries.
fn set_header(response: &mut Response, name: header::HeaderName, value: &str) {
    let value = HeaderValue::from_str(value)
        .expect("invariant: the server's own header values are valid header values");
    response.headers_mut().insert(name, value);
}

/// A duration in whole milliseconds.
fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// A duration in whole seconds, rounded up and never below one: the
/// `Retry-After` header is a whole number of seconds.
fn retry_after_secs(duration: Duration) -> u64 {
    let rounded = duration.as_secs() + u64::from(duration.subsec_millis() > 0);
    rounded.max(1)
}
