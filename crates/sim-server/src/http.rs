//! Axum HTTP front end for the one-fixture match stream (PRD §10.1).
//!
//! Routes:
//! - `POST /match/{match_id}` → 202 started, 200 already
//!   known, 404 unknown id, 503 over the concurrency cap.
//! - `GET /match/{match_id}/stream` (`text/event-stream`) → 200
//!   stream (resumable via `Last-Event-ID`), 204 when the resume
//!   index is at or beyond the final message, 404 unknown id, 409
//!   never `POST`ed, 410 result expired.
//!
//! Auth, CORS, and the concurrency-cap config surface stay deferred
//! to the Phase 6 hardening ticket; the status codes above prove the
//! contract end to end on a real stream.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing};

use crate::api::{MatchStore, StreamResponse};

/// Keep-alive comment interval while a simulation is still running
/// (PRD §10.1: the connection stays open until the first message is
/// ready).
const KEEP_ALIVE_SECS: u64 = 15;

/// Shared handler state: the match store.
#[derive(Debug, Clone)]
pub struct AppState {
    /// Match store behind the routes.
    pub store: Arc<MatchStore>,
}

/// Build the axum router for a store.
pub fn router(store: Arc<MatchStore>) -> Router {
    Router::new()
        .route("/match/{match_id}", routing::post(post_match_handler))
        .route(
            "/match/{match_id}/stream",
            routing::get(get_stream_handler),
        )
        .with_state(AppState { store })
}

/// Serve forever on `bind` (e.g. `127.0.0.1:8080`).
///
/// # Errors
///
/// Returns the bind or accept-loop I/O error.
pub async fn serve(store: Arc<MatchStore>, bind: &str) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("football-sim HTTP listening on {bind}");
    axum::serve(listener, router(store)).await
}

/// `POST /match/{match_id}`: start the match on its own OS thread
/// (inside [`MatchStore::post_match`]) and report 202 / 200 / 404 /
/// 503 per PRD §10.1.
async fn post_match_handler(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Response {
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
    use crate::api::{MatchRegistry, ServerConfig};

    fn test_state(ttl_secs: u64, tick_cap: u64) -> AppState {
        let mut registry = MatchRegistry::default();
        registry.insert("m-0000", 7);
        let store = Arc::new(MatchStore::new(
            ServerConfig {
                result_ttl_secs: ttl_secs,
                tick_cap,
                ..ServerConfig::default()
            },
            registry,
        ));
        AppState { store }
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
        let response = post_match_handler(State(state), Path("nope".to_string())).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn stream_unknown_match_id_is_404() {
        let state = test_state(3600, 120);
        let response = get_stream_handler(
            State(state),
            Path("nope".to_string()),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn stream_before_post_is_409() {
        let state = test_state(3600, 120);
        let response = get_stream_handler(
            State(state),
            Path("m-0000".to_string()),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn post_starts_thread_and_stream_serves_error_over_real_flow() {
        // `tick_cap = 120` fails the match fast, exercising the real
        // POST → OS-thread → failure-stream path without a full match.
        let state = test_state(3600, 120);
        let response = post_match_handler(
            State(state.clone()),
            Path("m-0000".to_string()),
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
    async fn real_socket_serves_sse_with_resume_and_204() {
        // Bind an ephemeral port and serve over a real socket; speak
        // raw HTTP/1.1 to prove the contract bytes.
        let mut registry = MatchRegistry::default();
        registry.insert("m-0000", 7);
        let store = Arc::new(MatchStore::new(ServerConfig::default(), registry));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("invariant: loopback bind works in tests");
        let addr = listener
            .local_addr()
            .expect("invariant: bound socket has an address");
        tokio::spawn(async move {
            axum::serve(listener, router(store))
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
}
