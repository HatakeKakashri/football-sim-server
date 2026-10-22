//! The manifest route's conditional answers (server-API ticket 04).
//!
//! A client that revalidates the manifest sends the validator it
//! already holds in `If-None-Match`. The route answers the request
//! with a 304 and no body when that validator names the manifest's
//! current one, and with the whole manifest when it does not.
//!
//! Every request goes through the router with
//! `tower::ServiceExt::oneshot`, so no socket is opened and no handler
//! is called directly.

use super::*;

/// A manifest request that carries this `If-None-Match`.
fn manifest_request(if_none_match: &str) -> Request<Body> {
    let mut request = request(Method::GET, "/match/m-0001/manifest");
    request.headers_mut().insert(
        header::IF_NONE_MATCH,
        if_none_match
            .parse()
            .expect("invariant: a valid header value"),
    );
    request
}

/// A client that names the manifest's validator is answered with a 304.
#[tokio::test]
async fn a_client_that_names_the_validator_is_answered_with_304() {
    let (_service, router, _build_id) = ready_match().await;

    // The 200 the client holds: its validator is what it revalidates
    // with.
    let full = router
        .clone()
        .oneshot(request(Method::GET, "/match/m-0001/manifest"))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, _body) = response_parts(full).await;
    assert_eq!(status, StatusCode::OK);
    let validator = header_value(&headers, header::ETAG);
    assert!(!validator.is_empty(), "the 200 carries a validator");

    let response = router
        .oneshot(manifest_request(&validator))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert_eq!(
        header_value(&headers, header::ETAG),
        validator,
        "the 304 carries the manifest's validator"
    );
    assert_eq!(
        header_value(&headers, header::CACHE_CONTROL),
        "no-cache",
        "the 304 is cacheable by design"
    );
    assert!(body.is_empty(), "a 304 carries no body; body was: {body}");
}

/// A client that sends no validator gets the whole manifest.
#[tokio::test]
async fn a_client_that_sends_no_validator_gets_the_whole_manifest() {
    let (_service, router, build_id) = ready_match().await;
    let validator = identity_validator(&build_id);

    let response = router
        .oneshot(request(Method::GET, "/match/m-0001/manifest"))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_value(&headers, header::ETAG),
        validator,
        "the 200 carries the manifest's validator"
    );
    assert_eq!(header_value(&headers, header::CACHE_CONTROL), "no-cache");
    let json: serde_json::Value =
        serde_json::from_str(&body).expect("invariant: a manifest body is JSON");
    assert_eq!(json["state"], "ready", "the 200 is the manifest");
    assert!(
        !json["build_id"].as_str().unwrap_or_default().is_empty(),
        "the 200 is the manifest; body was: {body}"
    );
}

/// A client that names another validator gets the whole manifest.
#[tokio::test]
async fn a_client_that_names_another_validator_gets_the_whole_manifest() {
    let (_service, router, _build_id) = ready_match().await;

    let response = router
        .oneshot(manifest_request("\"0123456789abcdef\""))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(header_value(&headers, header::CACHE_CONTROL), "no-cache");
    let json: serde_json::Value =
        serde_json::from_str(&body).expect("invariant: a manifest body is JSON");
    assert_eq!(json["state"], "ready", "the 200 is the manifest");
}

/// A list of validators matches when one of them names the manifest.
#[tokio::test]
async fn a_list_of_validators_matches_when_one_of_them_names_the_manifest() {
    let (_service, router, build_id) = ready_match().await;
    let validator = identity_validator(&build_id);

    let response = router
        .oneshot(manifest_request(&format!("\"one\", {validator}, \"two\"")))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert_eq!(header_value(&headers, header::ETAG), validator);
    assert!(body.is_empty(), "a 304 carries no body; body was: {body}");
}

/// A list of validators that names none of them gets the whole manifest.
#[tokio::test]
async fn a_list_of_validators_that_names_none_of_them_gets_the_whole_manifest() {
    let (_service, router, _validator) = ready_match().await;

    let response = router
        .oneshot(manifest_request("\"one\", \"two\""))
        .await
        .expect("invariant: the router answers every request");
    let (status, _headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value =
        serde_json::from_str(&body).expect("invariant: a manifest body is JSON");
    assert_eq!(json["state"], "ready", "the 200 is the manifest");
}

/// A weak validator matches the manifest's strong one.
#[tokio::test]
async fn a_weak_validator_matches_the_manifest() {
    let (_service, router, build_id) = ready_match().await;
    let validator = identity_validator(&build_id);

    let response = router
        .oneshot(manifest_request(&format!("W/{validator}")))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert_eq!(header_value(&headers, header::ETAG), validator);
    assert!(body.is_empty(), "a 304 carries no body; body was: {body}");
}

/// A match that was never started keeps its conflict.
#[tokio::test]
async fn a_match_that_was_never_started_keeps_its_conflict() {
    let runner = Arc::new(InstantRunner);
    let service = service_with(as_dyn(Arc::clone(&runner)), &["m-0001"], 1);
    let router = router(Arc::clone(&service), http_config(None, &[]));

    let response = router
        .oneshot(manifest_request("\"0123456789abcdef\""))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(header_value(&headers, header::CACHE_CONTROL), "no-store");
    assert!(body.contains("not_started"), "body was: {body}");
}

/// An unknown match keeps its 404.
#[tokio::test]
async fn an_unknown_match_keeps_its_not_found() {
    let runner = Arc::new(InstantRunner);
    let service = service_with(as_dyn(Arc::clone(&runner)), &["m-0001"], 1);
    let router = router(Arc::clone(&service), http_config(None, &[]));

    let mut request = request(Method::GET, "/match/nope/manifest");
    request.headers_mut().insert(
        header::IF_NONE_MATCH,
        "\"0123456789abcdef\""
            .parse()
            .expect("invariant: a valid header value"),
    );
    let response = router
        .oneshot(request)
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(header_value(&headers, header::CACHE_CONTROL), "no-store");
    assert!(body.contains("unknown"), "body was: {body}");
}

/// A running match keeps its pending answer.
#[tokio::test]
async fn a_running_match_keeps_its_pending_answer() {
    let runner = Arc::new(GatedRunner::default());
    let _gate = GateGuard::new(Arc::clone(&runner));
    let service = service_with(as_dyn(Arc::clone(&runner)), &["m-0001"], 1);
    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Running)
    })
    .await;
    let router = router(Arc::clone(&service), http_config(None, &[]));

    let response = router
        .oneshot(manifest_request("\"0123456789abcdef\""))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(header_value(&headers, header::CACHE_CONTROL), "no-store");
    assert!(
        !headers.contains_key(header::ETAG),
        "a pending answer carries no validator"
    );
    assert!(body.contains("running"), "body was: {body}");
}

/// A failed match keeps its failure.
#[tokio::test]
async fn a_failed_match_keeps_its_failure() {
    let runner = Arc::new(TickCapFailureRunner::default());
    let service = service_with(as_dyn(Arc::clone(&runner)), &["m-0001"], 1);
    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Failed { .. })
    })
    .await;
    let router = router(Arc::clone(&service), http_config(None, &[]));

    let response = router
        .oneshot(manifest_request("\"0123456789abcdef\""))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(header_value(&headers, header::CACHE_CONTROL), "no-store");
    assert!(body.contains("failed"), "body was: {body}");
}

/// The start route does not revalidate: it answers with the manifest.
#[tokio::test]
async fn the_start_route_does_not_revalidate() {
    let (service, router, build_id) = ready_match().await;
    let validator = identity_validator(&build_id);

    let mut request = request(Method::POST, "/match/m-0001/start");
    request.headers_mut().insert(
        header::IF_NONE_MATCH,
        validator
            .parse()
            .expect("invariant: a valid header value"),
    );
    let response = router
        .oneshot(request)
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(header_value(&headers, header::ETAG), validator);
    assert_eq!(header_value(&headers, header::CACHE_CONTROL), "no-cache");
    let json: serde_json::Value =
        serde_json::from_str(&body).expect("invariant: a manifest body is JSON");
    assert_eq!(json["state"], "ready", "the start route serves the manifest");
    assert!(
        matches!(service.status("m-0001"), MatchState::Ready { .. }),
        "a revalidation runs no work"
    );
}
