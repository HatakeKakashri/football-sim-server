//! The manifest route's two representations (server-API ticket 10).
//!
//! The manifest is served gzipped to a client that accepts gzip and
//! uncompressed to every other client. The two representations are
//! two sets of published bytes, and each carries its own validator, so
//! a cache never holds one representation under the other's validator.
//!
//! Every case goes through the router with `tower::ServiceExt::oneshot`
//! and the fake runners, so no socket is opened and the real
//! simulation never runs.

use super::*;

/// A manifest request with the coding the client accepts and the
/// validator it holds.
fn manifest_request(accept_encoding: Option<&str>, if_none_match: Option<&str>) -> Request<Body> {
    let mut request = request(Method::GET, "/match/m-0001/manifest");
    if let Some(coding) = accept_encoding {
        request.headers_mut().insert(
            header::ACCEPT_ENCODING,
            coding.parse().expect("invariant: a valid header value"),
        );
    }
    if let Some(validator) = if_none_match {
        request.headers_mut().insert(
            header::IF_NONE_MATCH,
            validator
                .parse()
                .expect("invariant: a valid header value"),
        );
    }
    request
}

/// The identity body the server publishes, read from the match under
/// test.
fn identity_body(service: &Arc<MatchService>) -> Vec<u8> {
    let MatchState::Ready { published } = service.status("m-0001") else {
        panic!("invariant: the match under test is ready");
    };
    published.manifest_body().to_vec()
}

/// A client that accepts gzip is served the gzipped manifest.
#[tokio::test]
async fn a_gzip_client_gets_the_gzipped_manifest() {
    let (service, router, build_id) = ready_match().await;

    let response = router
        .oneshot(manifest_request(Some("gzip, deflate"), None))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_bytes(response).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_value(&headers, header::CONTENT_ENCODING),
        "gzip",
        "a client that accepts gzip is told so"
    );
    assert!(
        varies_on_accept_encoding(&headers),
        "the answer depends on the coding the client accepts"
    );
    assert_eq!(
        header_value(&headers, header::ETAG),
        gzip_validator(&build_id),
        "the gzip representation carries its own validator"
    );
    assert_eq!(header_value(&headers, header::CACHE_CONTROL), "no-cache");
    assert_eq!(
        gunzip(&body),
        identity_body(&service),
        "both representations decode to the same document"
    );
}

/// A client that does not accept gzip is served the identity manifest.
#[tokio::test]
async fn a_client_without_gzip_gets_the_identity_manifest() {
    let (service, router, build_id) = ready_match().await;

    let response = router
        .oneshot(manifest_request(None, None))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_bytes(response).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        !headers.contains_key(header::CONTENT_ENCODING),
        "a body that is not coded says so by not saying anything"
    );
    assert!(
        varies_on_accept_encoding(&headers),
        "the answer depends on the coding the client accepts"
    );
    assert_eq!(
        header_value(&headers, header::ETAG),
        identity_validator(&build_id),
        "the identity representation keeps the validator it always had"
    );
    assert_eq!(header_value(&headers, header::CACHE_CONTROL), "no-cache");
    assert_eq!(
        body,
        identity_body(&service),
        "the identity representation is the published body"
    );
}

/// A client that refuses gzip is served the identity manifest.
#[tokio::test]
async fn a_client_that_refuses_gzip_gets_the_identity_manifest() {
    let (service, router, build_id) = ready_match().await;

    let response = router
        .oneshot(manifest_request(Some("gzip;q=0, deflate"), None))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_bytes(response).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        !headers.contains_key(header::CONTENT_ENCODING),
        "a refused coding is not served"
    );
    assert_eq!(
        header_value(&headers, header::ETAG),
        identity_validator(&build_id),
        "the identity representation keeps the validator it always had"
    );
    assert_eq!(
        body,
        identity_body(&service),
        "the identity representation is the published body"
    );
}

/// A gzip client that names the gzip validator is answered with a 304.
#[tokio::test]
async fn a_gzip_client_that_names_the_gzip_validator_is_answered_with_304() {
    let (_service, router, build_id) = ready_match().await;
    let validator = gzip_validator(&build_id);

    let response = router
        .oneshot(manifest_request(Some("gzip"), Some(&validator)))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_bytes(response).await;

    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert_eq!(
        header_value(&headers, header::ETAG),
        validator,
        "the 304 carries the validator of the representation"
    );
    assert_eq!(header_value(&headers, header::CACHE_CONTROL), "no-cache");
    assert!(body.is_empty(), "a 304 carries no body; body had {} bytes", body.len());
}

/// A gzip client that names the identity validator gets the whole
/// manifest: the identity validator does not describe the gzip
/// representation.
#[tokio::test]
async fn a_gzip_client_that_names_the_identity_validator_gets_the_whole_manifest() {
    let (service, router, build_id) = ready_match().await;
    let validator = identity_validator(&build_id);

    let response = router
        .oneshot(manifest_request(Some("gzip"), Some(&validator)))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_bytes(response).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_value(&headers, header::CONTENT_ENCODING),
        "gzip",
        "the client is still served the representation it accepts"
    );
    assert_eq!(
        header_value(&headers, header::ETAG),
        gzip_validator(&build_id),
        "the 200 carries the validator of the representation it serves"
    );
    assert_eq!(
        gunzip(&body),
        identity_body(&service),
        "the whole manifest is served, gzipped"
    );
}

/// A client that names the identity validator is answered with a 304.
#[tokio::test]
async fn a_client_that_names_the_identity_validator_is_answered_with_304() {
    let (_service, router, build_id) = ready_match().await;
    let validator = identity_validator(&build_id);

    let response = router
        .oneshot(manifest_request(None, Some(&validator)))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_bytes(response).await;

    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert_eq!(
        header_value(&headers, header::ETAG),
        validator,
        "the 304 carries the validator of the representation"
    );
    assert_eq!(header_value(&headers, header::CACHE_CONTROL), "no-cache");
    assert!(body.is_empty(), "a 304 carries no body; body had {} bytes", body.len());
}

/// The build id is the same behind both representations: the coding
/// changes neither the digest nor the published URLs.
#[tokio::test]
async fn the_build_id_is_the_same_behind_both_representations() {
    let (_service, router, build_id) = ready_match().await;

    let gzipped = router
        .clone()
        .oneshot(manifest_request(Some("gzip"), None))
        .await
        .expect("invariant: the router answers every request");
    let (status, _headers, gzipped_body) = response_bytes(gzipped).await;
    assert_eq!(status, StatusCode::OK);

    let identity = router
        .oneshot(manifest_request(None, None))
        .await
        .expect("invariant: the router answers every request");
    let (status, _headers, identity_bytes) = response_bytes(identity).await;
    assert_eq!(status, StatusCode::OK);

    let gzipped_json: serde_json::Value =
        serde_json::from_slice(&gunzip(&gzipped_body)).expect("invariant: a manifest body is JSON");
    let identity_json: serde_json::Value =
        serde_json::from_slice(&identity_bytes).expect("invariant: a manifest body is JSON");
    assert_eq!(
        gzipped_json["build_id"], identity_json["build_id"],
        "both representations name the same build"
    );
    assert_eq!(
        gzipped_json["build_id"].as_str().unwrap_or_default(),
        build_id,
        "the build id is unchanged by the coding"
    );
    assert_eq!(
        gzipped_json["segments"], identity_json["segments"],
        "both representations publish the same URLs"
    );
}
