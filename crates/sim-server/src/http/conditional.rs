//! Conditional requests: the client's validator against the resource's.
//!
//! One rule, shared by every route that may answer a conditional
//! request:
//!
//! - `If-None-Match` names the validators the client already holds.
//!   The request matches when one of them is the resource's current
//!   validator (RFC 9110 §13.1.2).
//! - The comparison is the weak one (RFC 9110 §8.8.3.2): two entity
//!   tags are equivalent when their opaque tags match, whether or not
//!   either is weak. A client that sends `W/"<build_id>"` therefore
//!   matches the manifest's strong `"<build_id>"`.
//! - `If-None-Match: *` matches whenever the server has a current
//!   representation of the resource (RFC 9110 §13.1.2). The caller
//!   only asks the question when it has one.
//! - A header that is absent, empty, or names no validator matches
//!   nothing: the resource is served in full.
//!
//! The rule lives here, once, so a route that answers a conditional
//! request uses this module instead of parsing the header itself. A
//! route whose bytes never change behind their URL needs no rule of
//! its own: there is nothing to revalidate.

use axum::http::{HeaderMap, header};

/// Whether the client's `If-None-Match` names the resource's current
/// validator.
///
/// The validators are compared with the weak comparison function
/// (RFC 9110 §8.8.3.2), so a weak `W/"..."` validator matches a strong
/// one, and a list of validators matches when any one of them does.
///
/// The module is crate-visible, so `pub` here means "visible to the
/// whole crate" — the same reach as `pub(crate)`, which clippy would
/// report as redundant inside a crate-visible module.
pub fn if_none_match_matches(headers: &HeaderMap, validator: &str) -> bool {
    headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|candidate| candidate_matches(candidate.trim(), validator))
}

/// Whether one validator of the header names the resource's validator.
fn candidate_matches(candidate: &str, validator: &str) -> bool {
    // `*` names any current representation of the resource
    // (RFC 9110 §13.1.2).
    if candidate == "*" {
        return true;
    }
    opaque_tag(candidate) == opaque_tag(validator)
}

/// The opaque tag of an entity tag, without its weakness.
///
/// The weak comparison function compares the opaque tags and ignores
/// whether either tag is weak (RFC 9110 §8.8.3.2). The `W/` prefix is
/// case-sensitive, as the grammar says (RFC 9110 §8.8.3).
fn opaque_tag(entity_tag: &str) -> &str {
    entity_tag.strip_prefix("W/").unwrap_or(entity_tag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    /// The validator the manifest publishes: the quoted build id.
    const VALIDATOR: &str = "\"abc123\"";

    /// Whether a client that sends this `If-None-Match` matches the
    /// validator.
    fn matches(if_none_match: Option<&str>) -> bool {
        let mut headers = HeaderMap::new();
        if let Some(value) = if_none_match {
            headers.insert(
                header::IF_NONE_MATCH,
                HeaderValue::from_str(value).expect("invariant: a valid header value"),
            );
        }
        if_none_match_matches(&headers, VALIDATOR)
    }

    #[test]
    fn the_current_validator_matches() {
        assert!(matches(Some("\"abc123\"")));
    }

    #[test]
    fn another_validator_does_not_match() {
        assert!(!matches(Some("\"other\"")));
        assert!(!matches(Some("abc123")));
        assert!(!matches(Some("\"abc1234\"")));
    }

    #[test]
    fn an_absent_or_empty_header_matches_nothing() {
        assert!(!matches(None));
        assert!(!matches(Some("")));
    }

    #[test]
    fn the_weak_form_matches_the_strong_one() {
        // RFC 9110 §8.8.3.2: the weak comparison function ignores
        // whether either tag is weak.
        assert!(matches(Some("W/\"abc123\"")));
    }

    #[test]
    fn the_weak_prefix_is_case_sensitive() {
        // RFC 9110 §8.8.3: the prefix is `W/`, so `w/` is not a weak
        // entity tag and names nothing.
        assert!(!matches(Some("w/\"abc123\"")));
    }

    #[test]
    fn a_list_matches_when_one_of_its_validators_does() {
        assert!(matches(Some("\"one\", \"abc123\"")));
        assert!(matches(Some("\"one\", W/\"abc123\", \"two\"")));
        assert!(!matches(Some("\"one\", \"two\"")));
    }

    #[test]
    fn the_wildcard_matches_a_current_representation() {
        assert!(matches(Some("*")));
    }
}
