//! Content-coding negotiation: what the client says it accepts, and
//! what the server answers with.
//!
//! One rule, shared by every route that may serve a compressed body:
//!
//! - gzip is served only when the client asks for it: an
//!   `Accept-Encoding` that names gzip with a quality above zero, or a
//!   wildcard that does while gzip itself is not named.
//! - An explicit `gzip` entry beats the wildcard (RFC 9110 §12.5.3):
//!   when gzip is listed, the wildcard does not apply to it, and a
//!   listed coding with `q=0` is refused.
//! - A `q` that does not parse is read as 0: the coding is then not
//!   acceptable, which is the safe reading of a malformed request.
//!
//! The rule lives here, once, so a route that serves a second
//! representation negotiates it with this module instead of copying
//! the parser. A route that serves a compressed body must also say so
//! with `Vary: Accept-Encoding`, or a cache may hand one
//! representation to a client that asked for the other.

use axum::http::{HeaderMap, header};

use crate::api::render::ENCODING_GZIP;

/// Whether the client accepts a gzip-encoded body.
///
/// gzip is served only when the client asks for it: an
/// `Accept-Encoding` that names gzip with a quality above zero, or a
/// wildcard that does while gzip itself is not named. A request that
/// does not ask for gzip gets the decompressed bytes.
///
/// The module is crate-visible, so `pub` here means "visible to the
/// whole crate" — the same reach as `pub(crate)`, which clippy would
/// report as redundant inside a crate-visible module.
pub fn accepts_gzip(headers: &HeaderMap) -> bool {
    let mut gzip_quality: Option<f32> = None;
    let mut wildcard_quality: Option<f32> = None;
    for value in headers.get_all(header::ACCEPT_ENCODING) {
        let Ok(text) = value.to_str() else {
            continue;
        };
        for item in text.split(',') {
            let (name, quality) = parse_coding(item);
            if name.eq_ignore_ascii_case(ENCODING_GZIP) {
                gzip_quality = Some(quality);
            } else if name == "*" {
                wildcard_quality = Some(quality);
            }
        }
    }
    // An explicit `gzip` entry wins over the wildcard.
    gzip_quality
        .or(wildcard_quality)
        .is_some_and(|quality| quality > 0.0)
}

/// One content coding of an `Accept-Encoding` header: its name and the
/// quality the client gave it.
///
/// A coding without a `q` parameter has the quality 1. A `q` that does
/// not parse is read as 0: the coding is then not acceptable, which is
/// the safe reading of a malformed request.
fn parse_coding(item: &str) -> (&str, f32) {
    let mut parts = item.split(';');
    let name = parts.next().unwrap_or_default().trim();
    let mut quality = 1.0_f32;
    for parameter in parts {
        let parameter = parameter.trim();
        let Some((key, value)) = parameter.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("q") {
            quality = value.trim().parse().unwrap_or(0.0);
        }
    }
    (name, quality)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    /// Whether a client that sends this `Accept-Encoding` accepts gzip.
    fn accepts(accept_encoding: Option<&str>) -> bool {
        let mut headers = HeaderMap::new();
        if let Some(value) = accept_encoding {
            headers.insert(
                header::ACCEPT_ENCODING,
                HeaderValue::from_str(value).expect("invariant: a valid header value"),
            );
        }
        accepts_gzip(&headers)
    }

    #[test]
    fn gzip_is_served_only_when_the_client_asks_for_it() {
        assert!(accepts(Some("gzip")));
        assert!(accepts(Some("gzip, deflate, br")));
        assert!(accepts(Some("deflate, gzip")));
        assert!(accepts(Some("GZIP")));
        assert!(accepts(Some("gzip;q=0.5")));
        assert!(accepts(Some("*")));
        assert!(accepts(Some("*, deflate")));
    }

    #[test]
    fn gzip_is_not_served_when_the_client_refuses_it() {
        assert!(!accepts(None));
        assert!(!accepts(Some("")));
        assert!(!accepts(Some("deflate")));
        assert!(!accepts(Some("deflate, br")));
        assert!(!accepts(Some("gzip;q=0")));
        assert!(!accepts(Some("*;q=0")));
        assert!(!accepts(Some("gzip;q=0, deflate")));
    }

    #[test]
    fn an_explicit_gzip_entry_wins_over_the_wildcard() {
        // RFC 9110 §12.5.3: gzip is listed, so the wildcard does not
        // apply to it — and a listed coding with `q=0` is refused.
        assert!(!accepts(Some("gzip;q=0, *")));
        assert!(!accepts(Some("*, gzip;q=0")));
        assert!(accepts(Some("*, gzip;q=1")));
    }
}
