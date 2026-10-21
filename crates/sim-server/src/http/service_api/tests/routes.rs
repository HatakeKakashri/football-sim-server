//! The behaviour of the start and manifest routes that the matrix
//! does not cover (server-API ticket 08).

use super::*;

/// The manifest route serves the published bytes, byte for byte.
///
/// The bytes are produced once, at render time, so a request copies
/// them instead of re-serialising the manifest. The assertion pins
/// the wire: the body the client receives is the published body, with
/// the manifest's own `state` field last.
#[tokio::test]
async fn the_manifest_route_serves_the_published_bytes() {
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
    let router = router(Arc::clone(&service), http_config(None, &[]));

    let response = router
        .oneshot(request(Method::GET, "/match/m-0001/manifest"))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body.as_bytes(),
        &published.manifest_body()[..],
        "the route serves the published bytes"
    );
    assert_eq!(
        header_value(&headers, header::CONTENT_TYPE),
        "application/json",
        "the manifest body is JSON"
    );
}

/// A `GET` on a match that was never started starts no work.
#[tokio::test]
async fn get_on_a_never_started_match_starts_no_work() {
    let runner = Arc::new(InstantRunner);
    let service = service_with(as_dyn(Arc::clone(&runner)), &["m-0001"], 1);
    let router = router(Arc::clone(&service), http_config(None, &[]));

    let response = router
        .oneshot(request(Method::GET, "/match/m-0001/manifest"))
        .await
        .expect("invariant: the router answers every request");
    let (status, _headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("not_started"), "body was: {body}");
    assert_eq!(
        service.status("m-0001"),
        MatchState::NotStarted,
        "a read must not start the match"
    );
}

/// A `POST` on a match that was never started does start the work.
#[tokio::test]
async fn post_on_a_never_started_match_starts_the_work() {
    let runner = Arc::new(InstantRunner);
    let service = service_with(as_dyn(Arc::clone(&runner)), &["m-0001"], 1);
    let router = router(Arc::clone(&service), http_config(None, &[]));

    let response = router
        .oneshot(request(Method::POST, "/match/m-0001/start"))
        .await
        .expect("invariant: the router answers every request");
    let (status, _headers, _body) = response_parts(response).await;

    assert_eq!(status, StatusCode::ACCEPTED);
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Ready { .. })
    })
    .await;
}

/// A `POST` without the bearer token is 401, and starts no work.
#[tokio::test]
async fn post_without_the_bearer_token_is_401() {
    let runner = Arc::new(InstantRunner);
    let service = service_with(as_dyn(Arc::clone(&runner)), &["m-0001"], 1);
    let router = router(Arc::clone(&service), http_config(Some("secret"), &[]));

    let response = router
        .oneshot(request(Method::POST, "/match/m-0001/start"))
        .await
        .expect("invariant: the router answers every request");
    let (status, _headers, body) = response_parts(response).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("unauthorized"), "body was: {body}");
    assert_eq!(
        service.status("m-0001"),
        MatchState::NotStarted,
        "a rejected start must not start the match"
    );
}

/// A `POST` with the bearer token is served.
#[tokio::test]
async fn post_with_the_bearer_token_is_served() {
    let runner = Arc::new(InstantRunner);
    let service = service_with(as_dyn(Arc::clone(&runner)), &["m-0001"], 1);
    let router = router(Arc::clone(&service), http_config(Some("secret"), &[]));

    let mut authorized = request(Method::POST, "/match/m-0001/start");
    authorized
        .headers_mut()
        .insert(header::AUTHORIZATION, "Bearer secret".parse().expect("invariant: a valid header value"));
    let response = router
        .oneshot(authorized)
        .await
        .expect("invariant: the router answers every request");
    let (status, _headers, _body) = response_parts(response).await;

    assert_eq!(status, StatusCode::ACCEPTED);
}
