//! The behaviour of the start and manifest routes that the matrix
//! does not cover (server-API ticket 08).

use super::*;

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
        service.state("m-0001"),
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
        service.state("m-0001"),
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
