//! The CORS edge behaviour of the shared middleware (server-API ticket
//! 10).
//!
//! Every case goes through `router`, so the assertions are about the
//! middleware's effect on the real router, not about a helper called
//! directly.

use super::*;

/// The allowed origin the tests reflect.
const ORIGIN: &str = "http://example.com";

/// Add an `Origin` header to a request.
fn with_origin(mut request: Request<Body>, origin: &str) -> Request<Body> {
    request.headers_mut().insert(
        header::ORIGIN,
        origin
            .parse()
            .expect("invariant: a test origin is a valid header value"),
    );
    request
}

/// An allowed origin's response varies on `Origin`, so a shared cache
/// cannot replay it to another origin.
#[tokio::test]
async fn an_allowed_origin_response_varies_on_origin() {
    let service = service_with(as_dyn(Arc::new(InstantRunner)), &["m-0001"], 1);
    let router = router(service, http_config(None, &[ORIGIN]));

    let response = router
        .oneshot(with_origin(
            request(Method::GET, "/match/m-0001/manifest"),
            ORIGIN,
        ))
        .await
        .expect("invariant: the router answers every request");
    let (_status, headers, _body) = response_parts(response).await;

    let vary: Vec<String> = headers
        .get_all(header::VARY)
        .iter()
        .filter_map(|value| value.to_str().ok().map(ToString::to_string))
        .collect();
    assert!(
        vary.iter().any(|value| value
            .split(',')
            .any(|part| part.trim().eq_ignore_ascii_case("Origin"))),
        "the response must vary on Origin; Vary was: {vary:?}"
    );
}

/// A cross-origin script can read `Retry-After` and `ETag`: both are
/// named in `Access-Control-Expose-Headers`.
#[tokio::test]
async fn an_allowed_origin_can_read_retry_after_and_etag() {
    let service = service_with(as_dyn(Arc::new(InstantRunner)), &["m-0001"], 1);
    let _ = service.start("m-0001").await;
    wait_until(&service, "m-0001", |state| {
        matches!(state, MatchState::Ready { .. })
    })
    .await;
    let router = router(service, http_config(None, &[ORIGIN]));

    let response = router
        .oneshot(with_origin(
            request(Method::GET, "/match/m-0001/manifest"),
            ORIGIN,
        ))
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, _body) = response_parts(response).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        headers.contains_key(header::ETAG),
        "the manifest under test must carry its ETag"
    );
    let exposed = headers
        .get(header::ACCESS_CONTROL_EXPOSE_HEADERS)
        .expect("invariant: an allowed origin is told which headers it can read")
        .to_str()
        .expect("invariant: the exposed headers are ASCII");
    let exposed = exposed.to_ascii_lowercase();
    assert!(
        exposed.contains("retry-after"),
        "Retry-After must be exposed; header was: {exposed}"
    );
    assert!(
        exposed.contains("etag"),
        "ETag must be exposed; header was: {exposed}"
    );
}

/// A preflight for `POST` with an `Authorization` header from an
/// allowed origin succeeds and names the header the client sends.
#[tokio::test]
async fn preflight_for_post_with_authorization_from_an_allowed_origin_succeeds() {
    let service = service_with(as_dyn(Arc::new(InstantRunner)), &["m-0001"], 1);
    let router = router(service, http_config(None, &[ORIGIN]));

    let mut request = with_origin(request(Method::OPTIONS, "/match/m-0001/start"), ORIGIN);
    request.headers_mut().insert(
        header::ACCESS_CONTROL_REQUEST_METHOD,
        "POST".parse().expect("invariant: a valid header value"),
    );
    request.headers_mut().insert(
        header::ACCESS_CONTROL_REQUEST_HEADERS,
        "authorization"
            .parse()
            .expect("invariant: a valid header value"),
    );

    let response = router
        .oneshot(request)
        .await
        .expect("invariant: the router answers every request");
    let (status, headers, _body) = response_parts(response).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        headers
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|value| value.to_str().ok()),
        Some(ORIGIN)
    );
    let methods = headers
        .get(header::ACCESS_CONTROL_ALLOW_METHODS)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_uppercase();
    assert!(methods.contains("POST"), "allow-methods was: {methods}");
    let allowed_headers = headers
        .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(
        allowed_headers.contains("authorization"),
        "allow-headers was: {allowed_headers}"
    );
}
