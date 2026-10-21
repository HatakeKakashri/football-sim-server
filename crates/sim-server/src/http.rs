//! Axum HTTP front end for the one-fixture match stream (PRD §10.1).
//!
//! Routes:
//! - `POST /match/{match_id}` → 202 started, 200 already known,
//!   401 missing/invalid bearer token (when `api_token` is set),
//!   404 unknown id, 503 over the concurrency cap.
//! - `GET /match/{match_id}/stream` (`text/event-stream`) → 200
//!   stream (resumable via `Last-Event-ID`), 204 when the resume
//!   index is at or beyond the final message, 404 unknown id, 409
//!   never `POST`ed, 410 result expired.
//!
//! The match service's own routes live beside them (server-API tickets
//! 08 and 09): `POST /match/{match_id}/start`,
//! `GET /match/{match_id}/manifest` and
//! `GET /match/{match_id}/segments/{build_id}/{segment}`. See
//! [`service_api`] for the status matrix they implement.
//!
//! Security (PRD §4):
//! - `POST` requires `Authorization: Bearer <token>` when
//!   [`HttpConfig::api_token`] is set; missing/invalid tokens get
//!   401. `GET` is never authenticated (browser `EventSource` cannot
//!   set headers).
//! - `GET` responses carry `Access-Control-Allow-Origin` only for
//!   origins in [`HttpConfig::cors_origins`]; an empty list
//!   disables CORS (local-only deployment). `OPTIONS` preflight is
//!   answered with the matching CORS headers.
//! - Every response varies on `Origin`, so a shared cache cannot serve
//!   one origin's answer to another.
//! - An allowed origin can read `Retry-After` and `ETag` from a
//!   cross-origin `fetch`.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing};

use crate::api::{MatchStore, StreamResponse};
use crate::service::MatchService;

mod auth;
pub mod service_api;

pub use service_api::MatchApiState;

/// Keep-alive comment interval while a simulation is still running
/// (PRD §10.1: the connection stays open until the first message is
/// ready).
const KEEP_ALIVE_SECS: u64 = 15;

/// Default listen address (PRD §4: bind `127.0.0.1`).
pub const DEFAULT_BIND: &str = "127.0.0.1:8080";

/// The response headers a cross-origin script may read.
///
/// A browser only exposes the CORS-safelisted response headers to
/// `fetch`; `Retry-After` and `ETag` are not safelisted, so they must
/// be named here or the client cannot pace its polling or revalidate
/// its manifest.
const EXPOSED_HEADERS: &str = "Retry-After, ETag";

/// The transport settings of the HTTP server (PRD §4 configuration
/// table).
///
/// These are deliberately separate from the service's policy settings
/// ([`crate::api::ServerConfig`]): the bind address, the token and the
/// CORS allowlist belong to the transport, while the cap, the result
/// TTL and the tick cap belong to the simulation service.
#[derive(Debug, Clone)]
pub struct HttpConfig {
    /// Listen address.
    pub bind: String,
    /// Bearer token required for `POST` when set (`Some`). Unset
    /// (`None`) disables auth for local-only deployments (PRD §4
    /// Security). `GET` is never authenticated.
    pub api_token: Option<String>,
    /// Allowed browser origins for CORS. Empty disables CORS for
    /// local-only deployments (PRD §4).
    pub cors_origins: Vec<String>,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND.to_string(),
            api_token: None,
            cors_origins: Vec::new(),
        }
    }
}

/// Shared handler state: the match store plus the transport config.
#[derive(Debug, Clone)]
pub struct AppState {
    /// Match store behind the routes.
    pub store: Arc<MatchStore>,
    /// Transport settings: bind, bearer token and CORS origins.
    pub http: HttpConfig,
}

/// `Access-Control-Allow-Origin` for `origin` when it is in the
/// allowlist; `None` when CORS is disabled or the origin is not
/// allowed (PRD §4: empty list disables CORS).
fn allowed_origin<'a>(origin: &'a str, cors_origins: &'a [String]) -> Option<&'a str> {
    if cors_origins.is_empty() {
        return None;
    }
    cors_origins
        .iter()
        .find(|o| o.as_str() == origin)
        .map(String::as_str)
}

/// Add `Origin` to a response's `Vary` header.
///
/// A response that varies by origin must say so, or a shared cache can
/// replay one origin's answer to another. Existing `Vary` values (for
/// example a segment's `Accept-Encoding`) are preserved as extra header
/// values.
fn add_vary_origin(response: &mut Response) {
    response
        .headers_mut()
        .append(header::VARY, HeaderValue::from_static("Origin"));
}

/// CORS middleware applied to every request (PRD §4).
///
/// `OPTIONS` preflight requests match no `GET`/`POST` route, so they
/// are short-circuited here with the matching CORS headers. All other
/// responses get `Access-Control-Allow-Origin` reflected when the
/// request `Origin` is in the allowlist, plus the exposed response
/// headers a cross-origin script may read.
async fn cors_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let origin: Option<String> = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);

    let allowed = origin
        .as_deref()
        .and_then(|o| allowed_origin(o, &state.http.cors_origins));

    // Preflight: answer before routing; an unlisted origin gets no
    // `Access-Control-Allow-Origin` so the browser blocks it.
    if request.method() == Method::OPTIONS {
        let mut response = allowed.map_or_else(
            || StatusCode::NO_CONTENT.into_response(),
            |origin| {
                (
                    StatusCode::NO_CONTENT,
                    [
                        (
                            header::ACCESS_CONTROL_ALLOW_ORIGIN,
                            HeaderValue::from_str(origin)
                                .expect("invariant: allowed CORS origin is a valid header value"),
                        ),
                        (header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, OPTIONS")),
                        (
                            header::ACCESS_CONTROL_ALLOW_HEADERS,
                            HeaderValue::from_static("authorization, last-event-id, content-type"),
                        ),
                        (
                            header::ACCESS_CONTROL_EXPOSE_HEADERS,
                            HeaderValue::from_static(EXPOSED_HEADERS),
                        ),
                        (header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("86400")),
                    ],
                )
                    .into_response()
            },
        );
        add_vary_origin(&mut response);
        return response;
    }

    let mut response = next.run(request).await;
    if let Some(origin) = allowed {
        let headers = response.headers_mut();
        let _ = headers.insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_str(origin).expect("invariant: allowed CORS origin is a valid header value"),
        );
        let _ = headers.insert(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            HeaderValue::from_static(EXPOSED_HEADERS),
        );
    }
    add_vary_origin(&mut response);
    response
}

/// Build the axum router for a store.
pub fn router(store: Arc<MatchStore>, http: HttpConfig) -> Router {
    let state = AppState { store, http };
    let legacy: Router = legacy_routes().with_state(state.clone());
    legacy.layer(middleware::from_fn_with_state(state, cors_middleware))
}

/// Build the axum router with the match service's routes beside the
/// legacy SSE routes.
///
/// The two route sets live side by side: the legacy stream routes keep
/// their behaviour, and the start and manifest routes of
/// [`service_api`] answer the status matrix of server-API ticket 08.
pub fn router_with_service(
    store: Arc<MatchStore>,
    service: Arc<MatchService>,
    http: HttpConfig,
) -> Router {
    let state = AppState {
        store,
        http: http.clone(),
    };
    let legacy: Router = legacy_routes().with_state(state.clone());
    let next = service_api::routes(MatchApiState { service, http });
    legacy
        .merge(next)
        .layer(middleware::from_fn_with_state(state, cors_middleware))
}

/// The legacy SSE routes, without the CORS middleware.
fn legacy_routes() -> Router<AppState> {
    Router::new()
        .route("/match/{match_id}", routing::post(post_match_handler))
        .route(
            "/match/{match_id}/stream",
            routing::get(get_stream_handler),
        )
}

/// Serve forever on `bind` (e.g. `127.0.0.1:8080`).
///
/// # Errors
///
/// Returns the bind or accept-loop I/O error.
pub async fn serve(
    store: Arc<MatchStore>,
    service: Arc<MatchService>,
    http: HttpConfig,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(&http.bind).await?;
    tracing::info!("football-sim HTTP listening on {}", http.bind);
    axum::serve(listener, router_with_service(store, service, http)).await
}

/// `POST /match/{match_id}`: start the match on its own OS thread
/// (inside [`MatchStore::post_match`]) and report 202 / 200 / 404 /
/// 503 per PRD §10.1. Requires `Authorization: Bearer <token>` when
/// [`HttpConfig::api_token`] is set; missing/invalid tokens get 401.
async fn post_match_handler(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    // Auth is checked before the registry so a missing token is 401
    // even for an unknown id (PRD §10.1 lists 401 before 404).
    if let Some(expected_token) = &state.http.api_token
        && !auth::bearer_token_matches(expected_token, &headers)
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "missing or invalid bearer token"})),
        )
            .into_response();
    }

    let outcome = state.store.post_match(&match_id);
    let status = StatusCode::from_u16(outcome.status)
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = (status, Json(outcome.body)).into_response();
    if let Some(retry_after) = outcome.retry_after_secs {
        response.headers_mut().insert(
            header::RETRY_AFTER,
            HeaderValue::from_str(&retry_after.to_string())
                .unwrap_or(HeaderValue::from_static("5")),
        );
    }
    response
}

/// `GET /match/{match_id}/stream`: serve the cached SSE stream,
/// resuming after `Last-Event-ID`. While the simulation runs, the
/// connection stays open with a keep-alive comment every
/// [`KEEP_ALIVE_SECS`] seconds.
async fn get_stream_handler(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|text| text.trim().parse::<u64>().ok());
    let mut prefix = String::new();
    loop {
        match state.store.stream_slice(&match_id, last_event_id) {
            StreamResponse::Ready(body) => {
                return sse_response(StatusCode::OK, format!("{prefix}{body}"));
            }
            StreamResponse::Running => {
                tokio::select! {
                    () = state.store.wait_until_done(&match_id) => {}
                    () = tokio::time::sleep(Duration::from_secs(KEEP_ALIVE_SECS)) => {
                        prefix.push_str(": keep-alive\n\n");
                    }
                }
            }
            StreamResponse::NoContent => {
                return (
                    StatusCode::NO_CONTENT,
                    [(header::CACHE_CONTROL, "no-cache")],
                )
                    .into_response();
            }
            StreamResponse::Unknown => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "unknown match_id"})),
                )
                    .into_response();
            }
            StreamResponse::NotStarted => {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({"error": "match not started; POST first"})),
                )
                    .into_response();
            }
            StreamResponse::Expired => {
                return (
                    StatusCode::GONE,
                    Json(serde_json::json!({"error": "result expired; POST again"})),
                )
                    .into_response();
            }
        }
    }
}

/// Wrap an SSE body with the streaming content headers.
fn sse_response(status: StatusCode, body: String) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE, "text/event-stream"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::test_doubles::ScriptedRunner;
    use crate::api::{MatchRegistry, ServerConfig};

    fn test_state(ttl_secs: u64, tick_cap: u64) -> AppState {
        test_state_with(ttl_secs, tick_cap, None, Vec::new())
    }

    fn test_state_with(
        ttl_secs: u64,
        tick_cap: u64,
        api_token: Option<String>,
        cors_origins: Vec<String>,
    ) -> AppState {
        let mut registry = MatchRegistry::default();
        let single = MatchRegistry::single_test("m-0000", 7);
        registry.fixtures.extend(single.fixtures);
        let store = Arc::new(MatchStore::new(
            ServerConfig {
                result_ttl_secs: ttl_secs,
                tick_cap,
                ..ServerConfig::default()
            },
            registry,
        ));
        AppState {
            store,
            http: HttpConfig {
                api_token,
                cors_origins,
                ..HttpConfig::default()
            },
        }
    }

    async fn response_body(response: Response) -> (StatusCode, HeaderMap, String) {
        let (mut parts, body) = response.into_parts();
        let headers = std::mem::take(&mut parts.headers);
        let bytes = axum::body::to_bytes(body, usize::MAX)
            .await
            .expect("invariant: test bodies fit in memory");
        (
            parts.status,
            headers,
            String::from_utf8(bytes.to_vec()).expect("invariant: SSE bodies are UTF-8"),
        )
    }

    #[tokio::test]
    async fn post_unknown_match_id_is_404() {
        let state = test_state(3600, 120);
        let response =
            post_match_handler(State(state), Path("nope".to_string()), HeaderMap::new()).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn post_starts_thread_and_stream_serves_error_over_real_flow() {
        // `tick_cap = 120` fails the match fast, exercising the real
        // POST → OS-thread → failure-stream path without a full match.
        let state = test_state(3600, 120);
        let response = post_match_handler(
            State(state.clone()),
            Path("m-0000".to_string()),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);

        let response = get_stream_handler(
            State(state.clone()),
            Path("m-0000".to_string()),
            HeaderMap::new(),
        )
        .await;
        let (status, headers, body) = response_body(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers
                .get(header::CONTENT_TYPE)
                .expect("invariant: SSE content type is set"),
            "text/event-stream"
        );
        assert!(body.contains("event: error"), "body was: {body}");
        assert!(body.contains("tick_cap_exceeded"), "body was: {body}");
    }

    #[tokio::test]
    async fn expired_result_is_410() {
        let state = test_state(0, 120);
        let response = post_match_handler(
            State(state.clone()),
            Path("m-0000".to_string()),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let response = get_stream_handler(
            State(state),
            Path("m-0000".to_string()),
            HeaderMap::new(),
        )
        .await;
        // TTL 0: by the time the thread finishes, the entry expired.
        assert_eq!(response.status(), StatusCode::GONE);
    }

    #[tokio::test]
    async fn expired_then_regenerate_is_byte_identical_end_to_end() {
        // TTL = 1 second: fresh immediately after completion, expired
        // shortly after. The 120-tick run fails fast on the cap, so the
        // stream is deterministic and the test finishes in ms.
        let state = test_state(1, 120);

        let response = post_match_handler(
            State(state.clone()),
            Path("m-0000".to_string()),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        state.store.wait_until_done("m-0000").await;

        let (status, _, first_body) = response_body(
            get_stream_handler(
                State(state.clone()),
                Path("m-0000".to_string()),
                HeaderMap::new(),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(first_body.contains("tick_cap_exceeded"));

        // Let the TTL elapse, then GET must return 410.
        tokio::time::sleep(Duration::from_secs_f32(1.5)).await;
        let response = get_stream_handler(
            State(state.clone()),
            Path("m-0000".to_string()),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::GONE);

        // Re-POST re-simulates the same seed (PRD §10.4).
        let response = post_match_handler(
            State(state.clone()),
            Path("m-0000".to_string()),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        state.store.wait_until_done("m-0000").await;

        let (status, _, second_body) = response_body(
            get_stream_handler(
                State(state.clone()),
                Path("m-0000".to_string()),
                HeaderMap::new(),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            first_body, second_body,
            "evict-and-regenerate must be byte-identical (uncompressed, PRD §10.4)"
        );
    }

    #[tokio::test]
    async fn real_socket_serves_sse_with_resume_and_204() {
        // Bind an ephemeral port and serve over a real socket; speak
        // raw HTTP/1.1 to prove the contract bytes.
        let mut registry = MatchRegistry::default();
        let single = MatchRegistry::single_test("m-0000", 7);
        registry.fixtures.extend(single.fixtures);
        let store = Arc::new(MatchStore::new(ServerConfig::default(), registry));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("invariant: loopback bind works in tests");
        let addr = listener
            .local_addr()
            .expect("invariant: bound socket has an address");
        tokio::spawn(async move {
            axum::serve(listener, router(store, HttpConfig::default()))
                .await
                .expect("invariant: test server serves");
        });

        // Unknown id → 404; known-but-never-started → 409.
        assert!(
            raw_get(addr, "/match/nope/stream", "")
                .await
                .starts_with("HTTP/1.1 404")
        );
        assert!(
            raw_get(addr, "/match/m-0000/stream", "")
                .await
                .starts_with("HTTP/1.1 409")
        );
    }

    #[tokio::test]
    async fn post_401_when_api_token_missing() {
        let state = test_state_with(3600, 120, Some("secret".to_string()), Vec::new());
        let response =
            post_match_handler(State(state), Path("m-0000".to_string()), HeaderMap::new()).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn post_401_when_api_token_invalid() {
        let state = test_state_with(3600, 120, Some("secret".to_string()), Vec::new());
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer wrong"));
        let response = post_match_handler(
            State(state),
            Path("m-0000".to_string()),
            headers,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn post_202_when_api_token_valid() {
        let state = test_state_with(3600, 120, Some("secret".to_string()), Vec::new());
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer secret"));
        let response = post_match_handler(
            State(state),
            Path("m-0000".to_string()),
            headers,
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn post_open_when_no_api_token_configured() {
        // Auth disabled for local-only deployment: no bearer required.
        let state = test_state(3600, 120);
        let response =
            post_match_handler(State(state), Path("m-0000".to_string()), HeaderMap::new()).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn get_status_matrix_over_real_socket() {
        // Default TTL: the finished entry is served (200) until it
        // expires; 404/409/204 are exercised over a real socket.
        let mut registry = MatchRegistry::default();
        let single1 = MatchRegistry::single_test("m-0000", 7);
        let single2 = MatchRegistry::single_test("m-0001", 7);
        registry.fixtures.extend(single1.fixtures);
        registry.fixtures.extend(single2.fixtures);
        let store = Arc::new(MatchStore::new(
            ServerConfig {
                tick_cap: 120,
                ..ServerConfig::default()
            },
            registry,
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("invariant: loopback bind works in tests");
        let addr = listener
            .local_addr()
            .expect("invariant: bound socket has an address");
        let server_store = store.clone();
        tokio::spawn(async move {
            axum::serve(listener, router(server_store, HttpConfig::default()))
                .await
                .expect("invariant: test server serves");
        });

        // 404: unknown id.
        assert!(
            raw_get(addr, "/match/nope/stream", "")
                .await
                .starts_with("HTTP/1.1 404")
        );
        // 409: known but never POSTed.
        assert!(
            raw_get(addr, "/match/m-0001/stream", "")
                .await
                .starts_with("HTTP/1.1 409")
        );

        // POST starts the fast cap-failing simulation → 202.
        assert!(
            raw_post(addr, "/match/m-0000").await.starts_with("HTTP/1.1 202")
        );
        store.wait_until_done("m-0000").await;

        // 200: stream ready (the single cached error message).
        let body = raw_get(addr, "/match/m-0000/stream", "").await;
        assert!(body.starts_with("HTTP/1.1 200"), "body was: {body}");
        assert!(body.contains("event: error"), "body was: {body}");

        // 204: Last-Event-ID at or beyond the final message (id 0).
        assert!(
            raw_get(addr, "/match/m-0000/stream", "last-event-id: 0\r\n")
                .await
                .starts_with("HTTP/1.1 204")
        );
    }

    #[tokio::test]
    async fn get_expired_is_410_over_real_socket() {
        // TTL 0: the entry expires as soon as the thread finishes.
        let mut registry = MatchRegistry::default();
        let single = MatchRegistry::single_test("m-0000", 7);
        registry.fixtures.extend(single.fixtures);
        let store = Arc::new(MatchStore::new(
            ServerConfig {
                result_ttl_secs: 0,
                tick_cap: 120,
                ..ServerConfig::default()
            },
            registry,
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("invariant: loopback bind works in tests");
        let addr = listener
            .local_addr()
            .expect("invariant: bound socket has an address");
        let server_store = store.clone();
        tokio::spawn(async move {
            axum::serve(listener, router(server_store, HttpConfig::default()))
                .await
                .expect("invariant: test server serves");
        });

        assert!(
            raw_post(addr, "/match/m-0000").await.starts_with("HTTP/1.1 202")
        );
        store.wait_until_done("m-0000").await;
        assert!(
            raw_get(addr, "/match/m-0000/stream", "")
                .await
                .starts_with("HTTP/1.1 410")
        );
    }

    #[tokio::test]
    async fn cors_over_real_socket() {
        let mut registry = MatchRegistry::default();
        let single = MatchRegistry::single_test("m-0000", 7);
        registry.fixtures.extend(single.fixtures);
        let store = Arc::new(MatchStore::new(
            ServerConfig {
                tick_cap: 120,
                ..ServerConfig::default()
            },
            registry,
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("invariant: loopback bind works in tests");
        let addr = listener
            .local_addr()
            .expect("invariant: bound socket has an address");
        let server_store = store.clone();
        tokio::spawn(async move {
            axum::serve(
                listener,
                router(
                    server_store,
                    HttpConfig {
                        cors_origins: vec!["http://example.com".to_string()],
                        ..HttpConfig::default()
                    },
                ),
            )
            .await
            .expect("invariant: test server serves");
        });

        assert!(
            raw_post(addr, "/match/m-0000").await.starts_with("HTTP/1.1 202")
        );
        store.wait_until_done("m-0000").await;

        // Allowed origin → reflected on the GET response.
        let allowed = raw_get(
            addr,
            "/match/m-0000/stream",
            "origin: http://example.com\r\n",
        )
        .await;
        assert!(allowed.starts_with("HTTP/1.1 200"), "body was: {allowed}");
        assert!(
            allowed.to_ascii_lowercase().contains("access-control-allow-origin: http://example.com"),
            "body was: {allowed}"
        );

        // Disallowed origin → no Access-Control-Allow-Origin header.
        let disallowed = raw_get(
            addr,
            "/match/m-0000/stream",
            "origin: http://evil.com\r\n",
        )
        .await;
        assert!(disallowed.starts_with("HTTP/1.1 200"), "body was: {disallowed}");
        assert!(
            !disallowed
                .to_ascii_lowercase()
                .contains("access-control-allow-origin"),
            "body was: {disallowed}"
        );

        // OPTIONS preflight for an allowed origin → 204 + CORS headers.
        let preflight = raw_options(
            addr,
            "/match/m-0000/stream",
            "origin: http://example.com\r\n",
        )
        .await;
        assert!(preflight.starts_with("HTTP/1.1 204"), "body was: {preflight}");
        assert!(
            preflight.to_ascii_lowercase().contains("access-control-allow-origin: http://example.com"),
            "body was: {preflight}"
        );
        assert!(
            preflight.to_ascii_lowercase().contains("access-control-allow-methods"),
            "body was: {preflight}"
        );
    }

    /// A panicked simulation must not wedge the stream: `GET` returns
    /// the terminal error message instead of waiting for a match that
    /// never finishes.
    #[tokio::test]
    async fn get_after_a_runner_panic_returns_the_terminal_error() {
        let mut registry = MatchRegistry::default();
        let single = MatchRegistry::single_test("panic-0", 7);
        registry.fixtures.extend(single.fixtures);
        let store = Arc::new(
            MatchStore::new(ServerConfig::default(), registry)
                .with_runner(Arc::new(ScriptedRunner)),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("invariant: loopback bind works in tests");
        let addr = listener
            .local_addr()
            .expect("invariant: bound socket has an address");
        let server_store = store.clone();
        tokio::spawn(async move {
            axum::serve(listener, router(server_store, HttpConfig::default()))
                .await
                .expect("invariant: test server serves");
        });

        assert!(
            raw_post(addr, "/match/panic-0")
                .await
                .starts_with("HTTP/1.1 202")
        );

        // The stream must not wait forever: the panicked match is
        // served as a terminal error.
        let body = tokio::time::timeout(
            Duration::from_secs(10),
            raw_get(addr, "/match/panic-0/stream", ""),
        )
        .await
        .expect("the stream must not wait forever after a runner panic");
        assert!(body.starts_with("HTTP/1.1 200"), "body was: {body}");
        assert!(body.contains("event: error"), "body was: {body}");
        assert!(body.contains("runner_panic"), "body was: {body}");
    }

    /// One raw HTTP/1.1 GET over a fresh connection; returns the raw
    /// response (status line + headers + body).
    async fn raw_get(addr: std::net::SocketAddr, path: &str, extra: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut stream = tokio::net::TcpStream::connect(addr)
            .await
            .expect("invariant: test server accepts");
        let request =
            format!("GET {path} HTTP/1.1\r\nhost: {addr}\r\nconnection: close\r\n{extra}\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .expect("invariant: test request writes");
        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .await
            .expect("invariant: test response reads");
        String::from_utf8(raw).expect("invariant: test response is UTF-8")
    }

    /// One raw HTTP/1.1 POST with an empty body over a fresh connection.
    async fn raw_post(addr: std::net::SocketAddr, path: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut stream = tokio::net::TcpStream::connect(addr)
            .await
            .expect("invariant: test server accepts");
        let request = format!(
            "POST {path} HTTP/1.1\r\nhost: {addr}\r\nconnection: close\r\ncontent-length: 0\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .await
            .expect("invariant: test request writes");
        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .await
            .expect("invariant: test response reads");
        String::from_utf8(raw).expect("invariant: test response is UTF-8")
    }

    /// One raw HTTP/1.1 OPTIONS preflight over a fresh connection;
    /// returns the raw response (status line + headers).
    async fn raw_options(addr: std::net::SocketAddr, path: &str, extra: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut stream = tokio::net::TcpStream::connect(addr)
            .await
            .expect("invariant: test server accepts");
        let request = format!(
            "OPTIONS {path} HTTP/1.1\r\nhost: {addr}\r\nconnection: close\r\n{extra}\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .await
            .expect("invariant: test request writes");
        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .await
            .expect("invariant: test response reads");
        String::from_utf8(raw).expect("invariant: test response is UTF-8")
    }

    /// End-to-end concurrency cap over a real socket: one simulation
    /// slot, a second `POST` while a full match runs gets `503` with
    /// `Retry-After: 5`, and the slot releases when the match finishes.
    /// Runs full matches, so it is minutes-long in debug — `#[ignore]`d;
    /// verify with `cargo test --release -p sim-server --lib --ignored`.
    #[tokio::test]
    #[ignore = "long-running end-to-end concurrency-cap test over a real socket"]
    async fn concurrency_cap_end_to_end_503_over_cap() {
        let mut registry = MatchRegistry::default();
        let single1 = MatchRegistry::single_test("m-0000", 7);
        let single2 = MatchRegistry::single_test("m-0001", 42);
        registry.fixtures.extend(single1.fixtures);
        registry.fixtures.extend(single2.fixtures);
        let store = Arc::new(MatchStore::new(
            ServerConfig {
                max_concurrent_sims: 1,
                ..ServerConfig::default()
            },
            registry,
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("invariant: loopback bind works in tests");
        let addr = listener
            .local_addr()
            .expect("invariant: bound socket has an address");
        let server_store = store.clone();
        tokio::spawn(async move {
            axum::serve(listener, router(server_store, HttpConfig::default()))
                .await
                .expect("invariant: test server serves");
        });

        // POST #1 starts a full match on its own OS thread → 202.
        let first = raw_post(addr, "/match/m-0000").await;
        assert!(first.starts_with("HTTP/1.1 202"), "first POST: {first}");

        // POST #2 while #1 is still running → 503 + Retry-After.
        let second = raw_post(addr, "/match/m-0001").await;
        assert!(second.starts_with("HTTP/1.1 503"), "second POST: {second}");
        assert!(
            second.to_ascii_lowercase().contains("retry-after: 5"),
            "missing Retry-After header: {second}"
        );

        // The slot releases when match #1 reaches FullTime.
        store.wait_until_done("m-0000").await;

        // POST #2 again → 202 once the cap has released.
        let third = raw_post(addr, "/match/m-0001").await;
        assert!(third.starts_with("HTTP/1.1 202"), "third POST: {third}");

        // Finished results are still served: GET is never held up by the cap.
        let stream = raw_get(addr, "/match/m-0000/stream", "").await;
        assert!(stream.starts_with("HTTP/1.1 200"), "stream GET: {stream}");
        assert!(stream.contains("event: full_time"));
    }
}
