//! The immutable segment route: `GET /match/{id}/segments/{build}/{n}`.
//!
//! One segment of a ready match is served from the versioned URL the
//! manifest publishes. The bytes behind such a URL never change: the
//! build id is a digest of the rendered content, so the URL names an
//! immutable body and the browser may keep it for a year.
//!
//! # The status matrix
//!
//! | Match state | Segment |
//! |---|---|
//! | unknown match id | 404 `unknown` |
//! | not ready: never started, queued, running, failed, or forgotten by a restart | 409 `not_ready` |
//! | ready, unknown build id | 404 `no_segment` |
//! | ready, unknown segment number | 404 `no_segment` |
//! | ready, known segment | 200 + the segment's bytes |
//!
//! # The two representations
//!
//! The bodies are stored gzipped. A client that accepts gzip is served
//! the stored bytes with `Content-Encoding: gzip`; every other client
//! is served the decompressed bytes. Both decode to the same body,
//! whose SHA-256 is the hash the manifest publishes, so the URL means
//! the same thing either way.
//!
//! Which representation a client gets is decided by the shared
//! negotiation rule in [`crate::http::content_negotiation`].
//!
//! The 200 carries `Vary: Accept-Encoding`, so a cache keeps the two
//! representations apart, and `Cache-Control: private,
//! max-age=31536000, immutable`: the body is private to the client
//! (`private`, not `public`, until a CDN exists) and never changes
//! behind its URL (`immutable`). The errors are `no-store`, so they
//! name no coding.

use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::api::render::{ENCODING_GZIP, RenderedSegment};
use crate::http::content_negotiation::accepts_gzip;
use crate::service::MatchState;

use super::{MatchApiState, STATE_UNKNOWN, set_header, state_response_with};

/// The `Cache-Control` of a served segment.
///
/// The body belongs to the client alone (`private`, not `public`,
/// until a CDN exists) and never changes behind its URL
/// (`immutable`).
const IMMUTABLE_CACHE_CONTROL: &str = "private, max-age=31536000, immutable";

/// The `Vary` header of every answer of this route: the answer depends
/// on the coding the client accepts.
const VARY_ACCEPT_ENCODING: &str = "Accept-Encoding";

/// The content type of a segment body.
const SEGMENT_CONTENT_TYPE: &str = "application/json";

/// The suffix every published segment URL ends in.
const SEGMENT_SUFFIX: &str = ".json";

/// The `state` a response reports for a match that has no segments.
const STATE_NOT_READY: &str = "not_ready";

/// The `state` a response reports for a segment that does not exist.
const STATE_NO_SEGMENT: &str = "no_segment";

/// The `state` a response reports for a segment that cannot be read.
const STATE_BROKEN: &str = "broken";

/// `GET /match/{match_id}/segments/{build_id}/{segment}`: one segment
/// of a ready match.
///
/// The route never starts work: a match that has not been rendered
/// answers with a conflict, and a match the server does not know
/// answers with a not-found.
pub(super) async fn get_segment_handler(
    State(state): State<MatchApiState>,
    Path((match_id, build_id, segment)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if !state.service.is_known(&match_id) {
        return unknown_response(&match_id);
    }
    let current = state.service.state(&match_id);
    let MatchState::Ready {
        manifest, segments, ..
    } = current
    else {
        // Not started, queued, running, failed, or forgotten by a
        // restart: there is no segment to serve yet.
        return not_ready_response(&match_id);
    };
    if manifest.build_id != build_id {
        return unknown_segment_response(&match_id, "unknown build id");
    }
    let Some(number) = segment_number(&segment) else {
        return unknown_segment_response(&match_id, "unknown segment number");
    };
    let Some(rendered) = segments.iter().find(|row| row.metadata.segment == number) else {
        return unknown_segment_response(&match_id, "unknown segment number");
    };
    segment_response(&match_id, rendered, &headers)
}

/// The segment number a URL names, or `None` when the path names no
/// segment.
///
/// The published URLs end in `.json`; a path that does not is not a
/// segment URL.
fn segment_number(segment: &str) -> Option<u32> {
    let number = segment.strip_suffix(SEGMENT_SUFFIX)?;
    number.parse::<u32>().ok()
}

/// The 200: the segment's bytes, in the coding the client accepts.
fn segment_response(
    match_id: &str,
    rendered: &RenderedSegment,
    headers: &HeaderMap,
) -> Response {
    let gzipped = accepts_gzip(headers);
    let body = if gzipped {
        // The stored bytes are shared and never mutated, so the body
        // points at them instead of copying them.
        Body::from(Bytes::from_owner(rendered.gzipped_body.clone()))
    } else {
        match rendered.decompress() {
            Ok(bytes) => Body::from(bytes),
            Err(error) => return broken_segment_response(match_id, &error.to_string()),
        }
    };
    let mut response = (StatusCode::OK, body).into_response();
    set_header(&mut response, header::CONTENT_TYPE, SEGMENT_CONTENT_TYPE);
    set_header(
        &mut response,
        header::CACHE_CONTROL,
        IMMUTABLE_CACHE_CONTROL,
    );
    // `Vary` is a list header: it is appended to, so another layer may
    // add its own condition (an origin, say) without losing this one.
    append_vary(&mut response, VARY_ACCEPT_ENCODING);
    if gzipped {
        set_header(&mut response, header::CONTENT_ENCODING, ENCODING_GZIP);
    }
    response
}

/// Add a condition to the response's `Vary` header, keeping the ones
/// already there.
fn append_vary(response: &mut Response, condition: &str) {
    let value = HeaderValue::from_str(condition)
        .expect("invariant: the server's own header values are valid header values");
    response.headers_mut().append(header::VARY, value);
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

/// The 409: the match has no rendered segments yet.
fn not_ready_response(match_id: &str) -> Response {
    state_response_with(
        StatusCode::CONFLICT,
        STATE_NOT_READY,
        match_id,
        "match not ready; read the manifest before its segments",
    )
}

/// The 404: the build id or the segment number names nothing.
fn unknown_segment_response(match_id: &str, message: &str) -> Response {
    state_response_with(StatusCode::NOT_FOUND, STATE_NO_SEGMENT, match_id, message)
}

/// The 500: the stored segment cannot be read.
fn broken_segment_response(match_id: &str, message: &str) -> Response {
    state_response_with(
        StatusCode::INTERNAL_SERVER_ERROR,
        STATE_BROKEN,
        match_id,
        message,
    )
}
