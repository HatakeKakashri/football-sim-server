//! The manifest's two representations and the answers they get
//! (server-API ticket 10).
//!
//! The manifest is one resource with two representations: the
//! uncompressed body and its gzip coding. A client that accepts gzip
//! is served the coding, every other client the body. Both decode to
//! the same document, so the manifest means the same thing either way.
//!
//! # Two validators
//!
//! Two representations behind one URL cannot share one validator: a
//! `304` is a claim about the bytes the client would receive, so a
//! cache holding the gzip representation must not be told it is fresh
//! against the identity one. Each representation therefore carries its
//! own strong validator, derived from the build id:
//!
//! | Representation | `ETag` |
//! |---|---|
//! | identity (the client did not accept gzip) | `"<build_id>"` |
//! | gzip (the client accepted gzip) | `"<build_id>-gzip"` |
//!
//! The suffix is derived, not stored: both validators are built from
//! the same build id, so they can never disagree about which build
//! they describe. A `304` is decided against the representation the
//! request would receive, so a gzip client that revalidates with the
//! identity validator gets the whole gzipped body instead of a `304`.
//!
//! # The bytes
//!
//! Both representations are published: the render step compresses the
//! manifest body next to the identity bytes (see
//! [`PublishedMatch`]), so a request picks between two sets of bytes
//! that already exist and never compresses or decompresses a manifest.
//!
//! The identity validator is byte-for-byte the one the server sent
//! before this ticket, and the build id is unchanged: the coding never
//! changes a build id or a published URL.

use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::api::render::{ENCODING_GZIP, PublishedMatch};
use crate::http::conditional::if_none_match_matches;
use crate::http::content_negotiation::accepts_gzip;

use super::{CONTENT_TYPE_JSON, Revalidation, VARY_ACCEPT_ENCODING, append_vary, set_header};

/// The `Cache-Control` of the manifest's answers.
///
/// The manifest may be cached, but it must be revalidated before it is
/// reused, so the answer is `no-cache` and not `no-store` (PRD §10.5).
const MANIFEST_CACHE_CONTROL: &str = "no-cache";

/// The suffix of the gzip representation's validator.
///
/// The suffix is derived from the build id rather than stored beside
/// it, so the two validators can never disagree about which build they
/// describe.
const GZIP_VALIDATOR_SUFFIX: &str = "-gzip";

/// The manifest's two representations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Representation {
    /// The uncompressed body.
    Identity,
    /// The gzip coding of the body.
    Gzip,
}

impl Representation {
    /// The representation a request asks for.
    ///
    /// The coding is negotiated by the shared rule, so the manifest and
    /// the segments are served the same way.
    fn for_request(headers: &HeaderMap) -> Self {
        if accepts_gzip(headers) {
            Self::Gzip
        } else {
            Self::Identity
        }
    }

    /// The validator of the representation: a strong entity tag.
    ///
    /// The build id is a digest of the rendered content, so it is a
    /// strong validator for the manifest. The gzip representation adds
    /// a suffix, so a cache never confuses the two.
    fn validator(self, published: &PublishedMatch) -> String {
        match self {
            Self::Identity => format!("\"{}\"", published.build_id()),
            Self::Gzip => format!("\"{}{}\"", published.build_id(), GZIP_VALIDATOR_SUFFIX),
        }
    }

    /// The published bytes of the representation.
    fn body(self, published: &PublishedMatch) -> Arc<[u8]> {
        match self {
            Self::Identity => published.manifest_body(),
            Self::Gzip => published.gzipped_manifest_body(),
        }
    }
}

/// The manifest's answer: the 200 or the 304.
///
/// The body is the bytes the render step published, copied into the
/// response: the manifest is never cloned and never re-serialised, so
/// a request costs no string per manifest row.
///
/// A client that revalidates the manifest — one that carries the
/// validator of the representation it would receive in `If-None-Match`
/// — is answered with a 304 and no body: it already holds these bytes
/// (RFC 9110 §13.1.2).
pub(super) fn response(
    published: &PublishedMatch,
    headers: &HeaderMap,
    revalidation: Revalidation,
) -> Response {
    let representation = Representation::for_request(headers);
    let validator = representation.validator(published);
    if revalidation == Revalidation::Allowed && if_none_match_matches(headers, &validator) {
        return not_modified_response(&validator);
    }
    let body = Bytes::from_owner(representation.body(published));
    let mut response = (StatusCode::OK, Body::from(body)).into_response();
    set_header(&mut response, header::CONTENT_TYPE, CONTENT_TYPE_JSON);
    set_header(&mut response, header::ETAG, &validator);
    set_header(&mut response, header::CACHE_CONTROL, MANIFEST_CACHE_CONTROL);
    // The answer depends on the coding the client accepts, so a cache
    // must keep the two representations apart.
    append_vary(&mut response, VARY_ACCEPT_ENCODING);
    if representation == Representation::Gzip {
        set_header(&mut response, header::CONTENT_ENCODING, ENCODING_GZIP);
    }
    response
}

/// The 304: the client already holds the manifest.
///
/// The answer carries the validator and the cache header of the 200 and
/// no body: the bytes are the ones the client already has. The cache
/// header is the manifest's `no-cache`, not `no-store`: the answer is
/// cacheable by design (PRD §10.5).
///
/// The `Vary` of the 200 is carried too: the `304` is about one
/// representation, and a cache has to know which (RFC 9110 §15.4.5).
fn not_modified_response(validator: &str) -> Response {
    let mut response = StatusCode::NOT_MODIFIED.into_response();
    set_header(&mut response, header::ETAG, validator);
    set_header(&mut response, header::CACHE_CONTROL, MANIFEST_CACHE_CONTROL);
    append_vary(&mut response, VARY_ACCEPT_ENCODING);
    response
}
