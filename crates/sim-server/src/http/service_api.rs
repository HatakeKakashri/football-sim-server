//! The match service's HTTP surface: start a match, read its manifest.
//!
//! Two routes, both driven by the [`MatchService`] (server-API ticket
//! 08):
//!
//! - `POST /match/{match_id}/start` starts the match and reports what
//!   the start did.
//! - `GET /match/{match_id}/manifest` reads the manifest of a match
//!   that has already started. It never starts work.
//!
//! # The status matrix
//!
//! | Match state | `POST /start` | `GET /manifest` |
//! |---|---|---|
//! | unknown fixture | 404 | 404 |
//! | not started | 202 (queued or running) | 409 `{state: not_started}` |
//! | queued | 202 | 202 + `Retry-After` |
//! | running | 202 | 202 + `Retry-After` |
//! | ready | 200 + manifest | 200 + manifest |
//! | failed, deterministic | 500 | 500 |
//! | failed, transient | 503 + `Retry-After` | 503 + `Retry-After` |
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
//! The 200 manifest is the only answer that may be cached: it carries
//! an `ETag` (the content-derived build id) and `Cache-Control:
//! no-cache`. Every other answer is `no-store`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing};
use serde::Serialize;

use crate::api::render::Manifest;
use crate::service::{MatchService, MatchState, StartError, StartOutcome};

#[cfg(test)]
mod tests;

/// How long a client should wait before it polls the manifest again,
/// in seconds.
pub const POLL_RETRY_AFTER_SECS: u64 = 1;

/// The same wait in milliseconds, carried in the body of a 202 so a
/// browser that cannot read the `Retry-After` header can pace itself.
pub const POLL_RETRY_AFTER_MS: u64 = POLL_RETRY_AFTER_SECS * 1_000;

/// The `state` a response reports for an id that is not a fixture.
const STATE_UNKNOWN: &str = "unknown";

/// The `state` a response reports for a match that was never started.
const STATE_NOT_STARTED: &str = "not_started";

/// The `state` a response reports for a queued match.
const STATE_QUEUED: &str = "queued";

/// The `state` a response reports for a running match.
const STATE_RUNNING: &str = "running";

/// The `state` a response reports for a ready match.
const STATE_READY: &str = "ready";

/// The `state` a response reports for a failed match.
const STATE_FAILED: &str = "failed";

/// The `state` a response reports for a rejected request.
const STATE_UNAUTHORIZED: &str = "unauthorized";

/// The state of the start and manifest routes.
#[derive(Debug, Clone)]
pub struct MatchApiState {
    /// The match service behind the routes.
    pub service: Arc<MatchService>,
    /// Bearer token required by `POST`; `None` disables auth
    /// (PRD §4).
    pub api_token: Option<String>,
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

/// The body of a 200: the manifest, plus the state that produced it.
#[derive(Debug, Serialize)]
struct ManifestBody {
    /// The manifest, flattened into the body.
    #[serde(flatten)]
    manifest: Manifest,
    /// Always `ready`.
    state: &'static str,
}

/// The routes of the match service: start a match, read its manifest.
///
/// The routes live beside the legacy SSE routes and do not disturb
/// them; see [`crate::http::router_with_service`].
pub fn routes(state: MatchApiState) -> Router {
    Router::new()
        .route("/match/{match_id}/start", routing::post(post_start_handler))
        .route(
            "/match/{match_id}/manifest",
            routing::get(get_manifest_handler),
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
        Ok(StartOutcome::Started) => pending_response(STATE_RUNNING, &match_id),
        Ok(StartOutcome::Queued) => pending_response(STATE_QUEUED, &match_id),
        // A run is already in flight, or the match is already terminal:
        // the state it is in now decides the answer.
        Ok(StartOutcome::AlreadyRunning | StartOutcome::AlreadyComplete) => {
            let current = state.service.state(&match_id);
            state_response(&current, &match_id)
        }
        Ok(StartOutcome::RetryAfter {
            code,
            message,
            last_good_tick,
            retry_in,
        }) => failure_response(&match_id, &code, &message, last_good_tick, Some(retry_in)),
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
    if !state.service.is_known(&match_id) {
        return unknown_response(&match_id);
    }
    let current = state.service.state(&match_id);
    if matches!(current, MatchState::NotStarted) {
        return not_started_response(&match_id);
    }
    state_response(&current, &match_id)
}

/// The response for a match's current state.
///
/// `not_started` never reaches this function: the `GET` route answers
/// it with a conflict, and a `POST` has already started the match by
/// the time it asks.
fn state_response(current: &MatchState, match_id: &str) -> Response {
    match current {
        MatchState::NotStarted | MatchState::Running => pending_response(STATE_RUNNING, match_id),
        MatchState::Queued => pending_response(STATE_QUEUED, match_id),
        MatchState::Ready { manifest, .. } => manifest_response(manifest),
        MatchState::Failed {
            code,
            message,
            last_good_tick,
            retry_at,
        } => failure_response(
            match_id,
            code,
            message,
            *last_good_tick,
            retry_at.map(|at| at.saturating_duration_since(Instant::now())),
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

/// The 200: the manifest, with the state that produced it.
fn manifest_response(manifest: &Manifest) -> Response {
    let body = ManifestBody {
        manifest: manifest.clone(),
        state: STATE_READY,
    };
    let mut response = (StatusCode::OK, Json(body)).into_response();
    // The build id is a digest of the rendered content, so it is a
    // strong validator for the manifest.
    set_header(
        &mut response,
        header::ETAG,
        &format!("\"{}\"", manifest.build_id),
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
    let expected_token = state.api_token.as_ref()?;
    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let expected = format!("Bearer {expected_token}");
    if provided == Some(expected.as_str()) {
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
