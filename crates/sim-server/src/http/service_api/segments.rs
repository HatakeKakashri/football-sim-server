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
//! | ready, path is not a segment URL | 404 `no_segment` |
//! | ready, unknown build id | 404 `no_segment` |
//! | ready, unknown segment number | 404 `no_segment` |
//! | ready, known segment | 200 + the segment's bytes |
//!
//! # The URL scheme
//!
//! The route's pattern, the URLs the manifest publishes and the way
//! this route reads a path are one scheme, owned by
//! [`SegmentUrl`](crate::api::render::SegmentUrl). This route reads
//! the request path with that scheme's parser, so it never strips the
//! `.json` suffix or parses a segment number itself: a path that is
//! not a segment URL names no segment at all.
//!
//! # The two representations
//!
//! Both representations are published: the body — the uncompressed
//! JSON, whose SHA-256 is the hash the manifest publishes — and its
//! gzip coding. A client that accepts gzip is served the coding with
//! `Content-Encoding: gzip`; every other client is served the body.
//! Both decode to the same body, so the URL means the same thing
//! either way.
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
//!
//! # Resolution and negotiation
//!
//! The request is resolved first: the match's status says whether
//! there is a segment at all, the build id says whether the URL names
//! this match's bytes, and the published index says which segment the
//! URL names. The handler then does the rest: it negotiates the
//! coding and writes the headers.

use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::{OriginalUri, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::api::render::{ENCODING_GZIP, PublishedMatch, PublishedSegment, SegmentPath, SegmentUrl};
use crate::http::content_negotiation::accepts_gzip;
use crate::service::MatchState;

use super::{
    CONTENT_TYPE_JSON, MatchApiState, STATE_BROKEN, STATE_NO_SEGMENT, STATE_NOT_READY,
    set_header, state_response_with, unknown_response,
};

/// The `Cache-Control` of a served segment.
///
/// The body belongs to the client alone (`private`, not `public`,
/// until a CDN exists) and never changes behind its URL
/// (`immutable`).
const IMMUTABLE_CACHE_CONTROL: &str = "private, max-age=31536000, immutable";

/// The `Vary` header of every answer of this route: the answer depends
/// on the coding the client accepts.
const VARY_ACCEPT_ENCODING: &str = "Accept-Encoding";

/// The message of the 404 for a path that is not a segment URL.
const NOT_A_SEGMENT_URL: &str = "not a segment URL";

/// The message of the 404 for a URL that names another match's bytes.
const UNKNOWN_BUILD_ID: &str = "unknown build id";

/// The message of the 404 for a URL that names no segment.
const UNKNOWN_SEGMENT_NUMBER: &str = "unknown segment number";

/// What a segment request resolves to.
#[derive(Debug)]
enum Resolved {
    /// The build id or the segment number names nothing.
    NoSegment(&'static str),
    /// The segment's published bytes.
    Segment(PublishedSegment),
}

/// `GET /match/{match_id}/segments/{build_id}/{segment}`: one segment
/// of a ready match.
///
/// The route never starts work: a match that has not been rendered
/// answers with a conflict, and a match the server does not know
/// answers with a not-found.
pub(super) async fn get_segment_handler(
    State(state): State<MatchApiState>,
    OriginalUri(original): OriginalUri,
    headers: HeaderMap,
) -> Response {
    let path = original.path();
    // The match's status decides the answer before the path's shape
    // does: a match that is not ready has no segments at all, and a
    // match the server does not know is not a match at all. The
    // scheme's reader names the match even when the rest of the path
    // does not match the scheme.
    let match_id = SegmentUrl::match_id(path).unwrap_or_default();
    match state.service.status(match_id) {
        MatchState::UnknownFixture => unknown_response(match_id),
        // Not started, queued, running, failed, or forgotten by a
        // restart: there is no segment to serve yet.
        MatchState::NotStarted
        | MatchState::Queued
        | MatchState::Running
        | MatchState::Failed { .. } => not_ready_response(match_id),
        MatchState::Ready { published } => {
            // The scheme's parser reads the path: it owns the suffix
            // and the numbering, so this route does not.
            let Some(parts) = SegmentUrl::parse(path) else {
                return unknown_segment_response(match_id, NOT_A_SEGMENT_URL);
            };
            match resolve_in_match(&published, &parts) {
                Resolved::NoSegment(message) => unknown_segment_response(&parts.match_id, message),
                Resolved::Segment(found) => segment_response(&parts.match_id, &found, &headers),
            }
        }
    }
}

/// Resolve the request against a ready match's published bytes.
///
/// The resolution is this route's own rule: the build id says whether
/// the URL names this match's bytes, and the published index says
/// which segment the URL names.
fn resolve_in_match(published: &PublishedMatch, parts: &SegmentPath) -> Resolved {
    if published.build_id() != parts.build_id {
        return Resolved::NoSegment(UNKNOWN_BUILD_ID);
    }
    published.segment(parts.number).map_or_else(
        || Resolved::NoSegment(UNKNOWN_SEGMENT_NUMBER),
        |found| Resolved::Segment(found.clone()),
    )
}

/// The 200: the segment's published bytes, in the coding the client
/// accepts.
///
/// The bytes are the ones the render step published, so a client gets
/// a copy of bytes that already exist instead of a body derived again.
fn segment_response(
    match_id: &str,
    published: &PublishedSegment,
    headers: &HeaderMap,
) -> Response {
    let gzipped = accepts_gzip(headers);
    let bytes: &Arc<[u8]> = if gzipped {
        &published.gzipped_body
    } else {
        &published.body
    };
    if bytes.is_empty() {
        // The render step writes a body for every segment, so a
        // published body that is empty is a body that cannot be read:
        // the answer is a 500, not an empty 200.
        return broken_segment_response(match_id, "the published segment body is empty");
    }
    // The published bytes are shared and never mutated, so the body
    // points at them instead of copying them.
    let mut response = (
        StatusCode::OK,
        Body::from(Bytes::from_owner(Arc::clone(bytes))),
    )
        .into_response();
    set_header(&mut response, header::CONTENT_TYPE, CONTENT_TYPE_JSON);
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

/// The 500: the published segment cannot be read.
fn broken_segment_response(match_id: &str, message: &str) -> Response {
    state_response_with(
        StatusCode::INTERNAL_SERVER_ERROR,
        STATE_BROKEN,
        match_id,
        message,
    )
}
