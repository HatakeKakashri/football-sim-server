//! The immutable segment route (server-API ticket 09).
//!
//! Every case goes through the router with `tower::ServiceExt::oneshot`
//! and the fake runners, so no socket is opened and the real
//! simulation never runs.
//!
//! The cases cover both encodings of a ready segment and every row of
//! the route's status matrix: an unknown match, a match that is not
//! ready, a wrong build id, a path that is not a segment URL and an
//! unknown segment number. The round trip of the URL scheme itself —
//! every URL the manifest publishes, served by the router — lives in
//! `segment_urls`.

use std::io::Read;

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

use super::*;

/// The `Cache-Control` a served segment carries.
const IMMUTABLE_CACHE: &str = "private, max-age=31536000, immutable";

/// One segment of a ready match: what the manifest publishes about it
/// and what the service stores for it.
struct Segment {
    /// The immutable URL the manifest publishes.
    url: String,
    /// The build id the URL embeds.
    build_id: String,
    /// The segment number the URL ends in.
    number: u32,
    /// The SHA-256 of the uncompressed body, as the manifest publishes it.
    sha256: String,
    /// The stored gzip bytes.
    gzipped: Vec<u8>,
}

/// The first segment of a ready match, as the manifest publishes it and
/// as the service stores it.
///
/// The manifest is read from the published bytes the client receives,
/// so the test pins the URLs and the hashes the client actually sees.
fn first_segment(service: &Arc<MatchService>, match_id: &str) -> Segment {
    let MatchState::Ready { published } = service.status(match_id) else {
        panic!("invariant: the match under test is ready");
    };
    let manifest: serde_json::Value =
        serde_json::from_slice(&published.manifest_body()[..])
            .expect("invariant: the published manifest body is JSON");
    let row = &manifest["segments"][0];
    let number = u32::try_from(row["segment"].as_u64().expect("invariant: a segment number"))
        .expect("invariant: a segment number fits in u32");
    let published_segment = published
        .segment(number)
        .expect("invariant: the manifest lists the segment");
    Segment {
        url: row["url"]
            .as_str()
            .expect("invariant: the manifest publishes a segment URL")
            .to_string(),
        build_id: published.build_id().to_string(),
        number,
        sha256: row["sha256"]
            .as_str()
            .expect("invariant: the manifest publishes a segment hash")
            .to_string(),
        gzipped: published_segment.gzipped_body.to_vec(),
    }
}

/// A request for a segment, with the coding the client accepts.
fn segment_request(url: &str, accept_encoding: Option<&str>) -> Request<Body> {
    let mut request = request(Method::GET, url);
    if let Some(coding) = accept_encoding {
        request.headers_mut().insert(
            header::ACCEPT_ENCODING,
            coding.parse().expect("invariant: a valid header value"),
        );
    }
    request
}

/// The gzip bytes decoded back into the segment's body.
fn gunzip(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .expect("invariant: a served gzip body decodes");
    out
}

/// A ready segment is 200 and carries the immutable cache headers.
#[tokio::test]
async fn a_ready_segment_is_served_with_the_immutable_cache_headers() {
    let service = ready_service("m-0001").await;
    let segment = first_segment(&service, "m-0001");
    let router = router(service, http_config(None, &[]));

    let response = router
        .oneshot(segment_request(&segment.url, None))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_bytes(response).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_value(&headers, header::CACHE_CONTROL),
        IMMUTABLE_CACHE,
        "the segment must be cacheable for a year"
    );
    assert_eq!(
        header_value(&headers, header::VARY),
        "Accept-Encoding",
        "the answer varies on the accepted coding"
    );
    assert_eq!(
        header_value(&headers, header::CONTENT_TYPE),
        "application/json",
        "a segment body is JSON"
    );
    assert!(!body.is_empty(), "the segment body must not be empty");
}

/// A client that accepts gzip gets the stored gzip bytes.
#[tokio::test]
async fn a_gzip_client_gets_the_stored_gzipped_bytes() {
    let service = ready_service("m-0001").await;
    let segment = first_segment(&service, "m-0001");
    let router = router(service, http_config(None, &[]));

    let response = router
        .oneshot(segment_request(&segment.url, Some("gzip, deflate")))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_bytes(response).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_value(&headers, header::CONTENT_ENCODING),
        "gzip",
        "a client that accepts gzip is told so"
    );
    assert_eq!(
        body, segment.gzipped,
        "the stored gzip bytes are served as they are"
    );
    let decoded = gunzip(&body);
    assert_eq!(
        hex(&Sha256::digest(&decoded)),
        segment.sha256,
        "the decoded body hashes to the manifest's hash"
    );
}

/// A client that does not accept gzip gets the decompressed bytes.
#[tokio::test]
async fn a_client_without_gzip_gets_the_decompressed_bytes() {
    let service = ready_service("m-0001").await;
    let segment = first_segment(&service, "m-0001");
    let router = router(service, http_config(None, &[]));

    let response = router
        .oneshot(segment_request(&segment.url, None))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_bytes(response).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        !headers.contains_key(header::CONTENT_ENCODING),
        "a body that is not coded says so by not saying anything"
    );
    assert_eq!(body, gunzip(&segment.gzipped), "the decompressed bytes are served");
    assert_eq!(
        hex(&Sha256::digest(&body)),
        segment.sha256,
        "the body hashes to the manifest's hash"
    );
}

/// A client that refuses gzip (`q=0`) gets the decompressed bytes.
#[tokio::test]
async fn a_client_that_refuses_gzip_gets_the_decompressed_bytes() {
    let service = ready_service("m-0001").await;
    let segment = first_segment(&service, "m-0001");
    let router = router(service, http_config(None, &[]));

    let response = router
        .oneshot(segment_request(&segment.url, Some("gzip;q=0, deflate")))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_bytes(response).await;

    assert_eq!(status, StatusCode::OK);
    assert!(!headers.contains_key(header::CONTENT_ENCODING));
    assert_eq!(body, gunzip(&segment.gzipped));
}

/// A match that was never started has no segment: 409.
///
/// A server that restarted forgot the match the same way, so the
/// answer is the same.
#[tokio::test]
async fn a_match_that_was_never_started_is_409() {
    let service = service_with(as_dyn(Arc::new(InstantRunner)), &["m-0001"], 1);
    let router = router(service, http_config(None, &[]));

    let response = router
        .oneshot(request(
            Method::GET,
            "/match/m-0001/segments/deadbeef/0.json",
        ))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("not_ready"), "body was: {body}");
    assert_eq!(
        header_value(&headers, header::CACHE_CONTROL),
        "no-store",
        "an error is never cached"
    );
}

/// A running match has no segment yet: 409.
#[tokio::test]
async fn a_running_match_is_409() {
    let runner = Arc::new(GatedRunner::default());
    let gate = GateGuard::new(Arc::clone(&runner));
    let service = service_with(as_dyn(Arc::clone(&runner)), &["m-0001"], 1);
    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Running)
    })
    .await;
    let router = router(Arc::clone(&service), http_config(None, &[]));

    let response = router
        .oneshot(request(
            Method::GET,
            "/match/m-0001/segments/deadbeef/0.json",
        ))
        .await
        .expect("invariant: the router answers every request");
    let (status, _headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("not_ready"), "body was: {body}");
    drop(gate);
}

/// A failed match has no segment: 409.
#[tokio::test]
async fn a_failed_match_is_409() {
    let runner = Arc::new(TickCapFailureRunner::default());
    let service = service_with(as_dyn(Arc::clone(&runner)), &["m-0001"], 1);
    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Failed { .. })
    })
    .await;
    let router = router(service, http_config(None, &[]));

    let response = router
        .oneshot(request(
            Method::GET,
            "/match/m-0001/segments/deadbeef/0.json",
        ))
        .await
        .expect("invariant: the router answers every request");
    let (status, _headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("not_ready"), "body was: {body}");
}

/// An unknown match id is 404.
#[tokio::test]
async fn an_unknown_match_is_404() {
    let service = ready_service("m-0001").await;
    let router = router(service, http_config(None, &[]));

    let response = router
        .oneshot(request(
            Method::GET,
            "/match/nope/segments/deadbeef/0.json",
        ))
        .await
        .expect("invariant: the router answers every request");
    let (status, _headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("unknown"), "body was: {body}");
}

/// A wrong build id is 404: the bytes behind that build id are gone.
#[tokio::test]
async fn a_wrong_build_id_is_404() {
    let service = ready_service("m-0001").await;
    let segment = first_segment(&service, "m-0001");
    let router = router(service, http_config(None, &[]));

    let wrong = format!(
        "/match/m-0001/segments/{}/0.json",
        "0".repeat(segment.build_id.len())
    );
    let response = router
        .oneshot(request(Method::GET, &wrong))
        .await
        .expect("invariant: the router answers every request");
    let (status, _headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("no_segment"), "body was: {body}");
}

/// A segment number beyond the last one is 404.
#[tokio::test]
async fn an_out_of_range_segment_is_404() {
    let service = ready_service("m-0001").await;
    let segment = first_segment(&service, "m-0001");
    let router = router(service, http_config(None, &[]));

    let out_of_range = format!(
        "/match/m-0001/segments/{}/{}.json",
        segment.build_id,
        segment.number + 1_000
    );
    let response = router
        .oneshot(request(Method::GET, &out_of_range))
        .await
        .expect("invariant: the router answers every request");
    let (status, _headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("no_segment"), "body was: {body}");
}

/// A path that names no segment is 404.
#[tokio::test]
async fn a_bad_segment_number_is_404() {
    let service = ready_service("m-0001").await;
    let segment = first_segment(&service, "m-0001");
    let router = router(service, http_config(None, &[]));

    let bad = format!(
        "/match/m-0001/segments/{}/first.json",
        segment.build_id
    );
    let response = router
        .oneshot(request(Method::GET, &bad))
        .await
        .expect("invariant: the router answers every request");
    let (status, _headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("no_segment"), "body was: {body}");
}

/// A path without the `.json` suffix names no segment: 404.
///
/// The suffix is part of the segment's value, not of the route, so
/// the route matches the path and the scheme's parser rejects it.
#[tokio::test]
async fn a_path_without_the_suffix_is_404() {
    let service = ready_service("m-0001").await;
    let segment = first_segment(&service, "m-0001");
    let router = router(service, http_config(None, &[]));

    let without_suffix = format!(
        "/match/m-0001/segments/{}/{}",
        segment.build_id, segment.number
    );
    let response = router
        .oneshot(request(Method::GET, &without_suffix))
        .await
        .expect("invariant: the router answers every request");
    let (status, _headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("no_segment"), "body was: {body}");
}

/// A path with an extra part names no segment: 404.
///
/// The route matches whole path segments, so the router does not
/// route this path at all and answers with its own not-found.
#[tokio::test]
async fn a_path_with_an_extra_part_is_404() {
    let service = ready_service("m-0001").await;
    let segment = first_segment(&service, "m-0001");
    let router = router(service, http_config(None, &[]));

    let extra = format!(
        "/match/m-0001/segments/{}/{}.json/extra",
        segment.build_id, segment.number
    );
    let response = router
        .oneshot(request(Method::GET, &extra))
        .await
        .expect("invariant: the router answers every request");
    let (status, _headers, _body) = response_parts(response).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}
