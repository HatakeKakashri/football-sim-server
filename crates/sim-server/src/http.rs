//! Axum HTTP front end for the match service.
//!
//! Routes:
//! - `POST /match/{match_id}/start` starts a match and reports its state.
//! - `GET /match/{match_id}/manifest` reads a started match's manifest.
//! - `GET /match/{match_id}/segments/{build_id}/{segment}` serves one
//!   immutable segment of a ready match.
//!
//! The status matrix these routes implement lives in [`service_api`].
//!
//! Which representation a client gets — the compressed body or the
//! plain one — is decided by one shared rule, not per route; it lives
//! in the `content_negotiation` module.
//!
//! Whether a client revalidates what it already holds is decided by one
//! shared rule as well; it lives in the `conditional` module. Only the
//! manifest route revalidates: a segment's bytes never change behind
//! their URL, so there is nothing to revalidate there.
//!
//! Security (PRD §4):
//! - `POST` requires `Authorization: Bearer <token>` when
//!   [`HttpConfig::api_token`] is set; missing/invalid tokens get 401.
//! - Responses carry `Access-Control-Allow-Origin` only for origins in
//!   [`HttpConfig::cors_origins`]; an empty list disables CORS
//!   (local-only deployment). `OPTIONS` preflight is answered with the
//!   matching CORS headers.
//! - Every response varies on `Origin`, so a shared cache cannot serve
//!   one origin's answer to another.
//! - An allowed origin can read `Retry-After` and `ETag` from a
//!   cross-origin `fetch`.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;

mod auth;
pub(crate) mod conditional;
pub(crate) mod content_negotiation;
pub mod service_api;

use crate::service::MatchService;
pub use service_api::MatchApiState;

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
/// These are deliberately separate from the service's policy settings:
/// the bind address, the token and the CORS allowlist belong to the
/// transport, while the concurrency cap and the tick cap belong to the
/// simulation service.
#[derive(Debug, Clone)]
pub struct HttpConfig {
    /// Listen address.
    pub bind: String,
    /// Bearer token required for `POST` when set (`Some`). Unset
    /// (`None`) disables auth for local-only deployments (PRD §4
    /// Security).
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
    State(state): State<MatchApiState>,
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
                        (
                            header::ACCESS_CONTROL_ALLOW_METHODS,
                            HeaderValue::from_static("GET, POST, OPTIONS"),
                        ),
                        (
                            header::ACCESS_CONTROL_ALLOW_HEADERS,
                            HeaderValue::from_static("authorization, content-type"),
                        ),
                        (
                            header::ACCESS_CONTROL_EXPOSE_HEADERS,
                            HeaderValue::from_static(EXPOSED_HEADERS),
                        ),
                        (
                            header::ACCESS_CONTROL_MAX_AGE,
                            HeaderValue::from_static("86400"),
                        ),
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
            HeaderValue::from_str(origin)
                .expect("invariant: allowed CORS origin is a valid header value"),
        );
        let _ = headers.insert(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            HeaderValue::from_static(EXPOSED_HEADERS),
        );
    }
    add_vary_origin(&mut response);
    response
}

/// Build the axum router for the match service.
pub fn router(service: Arc<MatchService>, http: HttpConfig) -> Router {
    let state = MatchApiState { service, http };
    service_api::routes(state.clone())
        .layer(middleware::from_fn_with_state(state, cors_middleware))
}

/// Serve forever on `bind` (e.g. `127.0.0.1:8080`).
///
/// # Errors
///
/// Returns the bind or accept-loop I/O error.
pub async fn serve(service: Arc<MatchService>, http: HttpConfig) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(&http.bind).await?;
    tracing::info!("football-sim HTTP listening on {}", http.bind);
    axum::serve(listener, router(service, http)).await
}
