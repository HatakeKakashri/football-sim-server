//! Bearer-token authentication shared by the legacy and service routes.
//!
//! The token is compared in constant time, so the comparison does not
//! reveal where a candidate first differs from the configured secret.

use axum::http::{HeaderMap, header};

/// Whether `headers` carry `Bearer {expected_token}`.
///
/// The token bytes are compared in constant time. A missing header, a
/// non-ASCII value or a different scheme is a rejection.
pub(super) fn bearer_token_matches(expected_token: &str, headers: &HeaderMap) -> bool {
    let Some(provided) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let expected = format!("Bearer {expected_token}");
    constant_time_eq(provided.as_bytes(), expected.as_bytes())
}

/// Compare two byte slices without stopping at the first mismatch.
///
/// The length check is the only early return: the token's length is not
/// a secret, and walking two slices of different lengths would read
/// past one of them. The secret bytes themselves are folded into one
/// accumulator, so the loop always walks the whole token and the time
/// does not depend on *where* a mismatch is.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn authorization(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(value).expect("invariant: a test token is a valid header value"),
        );
        headers
    }

    #[test]
    fn accepts_the_configured_bearer_token() {
        assert!(bearer_token_matches(
            "secret",
            &authorization("Bearer secret")
        ));
    }

    #[test]
    fn rejects_a_wrong_bearer_token() {
        assert!(!bearer_token_matches(
            "secret",
            &authorization("Bearer wrong")
        ));
    }

    #[test]
    fn rejects_a_missing_bearer_token() {
        assert!(!bearer_token_matches("secret", &HeaderMap::new()));
    }

    #[test]
    fn rejects_a_different_length_token() {
        assert!(!constant_time_eq(b"Bearer secret", b"Bearer secret-longer"));
    }

    #[test]
    fn accepts_equal_slices() {
        assert!(constant_time_eq(b"Bearer secret", b"Bearer secret"));
    }
}
