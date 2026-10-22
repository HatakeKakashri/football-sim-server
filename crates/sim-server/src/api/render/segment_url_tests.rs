//! The segment URL scheme (server-API ticket 09).
//!
//! The scheme is pinned here: the route pattern, the published URL and
//! the parser are three renderings of one string, so these tests hold
//! them together. A change to the scheme is a change to all three, and
//! the tests below say what the scheme is.

use super::segment_url::{SCHEME, SEGMENT_SUFFIX, SegmentUrl};
use super::SegmentPath;

/// The route pattern the router routes the segment route with.
const ROUTE_PATTERN: &str = "/match/{match_id}/segments/{build_id}/{segment}";

/// The URL the manifest publishes for a segment.
const PUBLISHED_URL: &str = "/match/m-0001/segments/abc123/7.json";

/// The route pattern is the scheme as it stands.
#[test]
fn the_route_pattern_is_the_scheme() {
    assert_eq!(SegmentUrl::route_pattern(), ROUTE_PATTERN);
}

/// The published URL is the scheme with its slots filled in.
#[test]
fn the_published_url_is_the_scheme_with_its_parts_filled_in() {
    assert_eq!(SegmentUrl::build("m-0001", "abc123", 7), PUBLISHED_URL);
}

/// The parser reads the published URL back into the parts it was
/// built from.
#[test]
fn the_parser_reads_the_published_url_back() {
    assert_eq!(
        SegmentUrl::parse(PUBLISHED_URL),
        Some(SegmentPath {
            match_id: "m-0001".to_string(),
            build_id: "abc123".to_string(),
            number: 7,
        })
    );
}

/// The scheme names the parts the builder and the reader know.
///
/// The slots are the scheme's own text, so a slot the code does not
/// know would be read as a literal — and the path would name no
/// segment. This test says which slots there are.
#[test]
fn the_scheme_names_the_parts_the_definition_knows() {
    let slots: Vec<&str> = SCHEME.split('/').filter(|part| part.starts_with('{')).collect();
    assert_eq!(slots, vec!["{match_id}", "{build_id}", "{segment}"]);
    assert_eq!(SEGMENT_SUFFIX, ".json");
}

/// A path that is not a segment URL names no segment.
#[test]
fn a_path_that_is_not_a_segment_url_names_no_segment() {
    for path in [
        // No suffix.
        "/match/m-0001/segments/abc123/7",
        // A segment that is not a number.
        "/match/m-0001/segments/abc123/first.json",
        // An empty segment.
        "/match/m-0001/segments/abc123/.json",
        // An extra part.
        "/match/m-0001/segments/abc123/7.json/extra",
        // Another shape.
        "/match/m-0001/segment/abc123/7.json",
        "/segments/abc123/7.json",
        "/match/m-0001/segments/abc123",
    ] {
        assert_eq!(
            SegmentUrl::parse(path),
            None,
            "the path must name no segment: {path}"
        );
    }
}

/// A path that is not a segment URL may still name a match.
///
/// The match is the first part of the scheme, so the answer to a
/// malformed path can still name the match the client asked about.
#[test]
fn a_path_that_is_not_a_segment_url_may_still_name_a_match() {
    assert_eq!(
        SegmentUrl::match_id("/match/m-0001/segments/abc123/7"),
        Some("m-0001")
    );
    assert_eq!(
        SegmentUrl::match_id("/match/m-0001/segments/abc123/first.json"),
        Some("m-0001")
    );
    assert_eq!(
        SegmentUrl::match_id("/match/m-0001/segments/abc123/7.json/extra"),
        Some("m-0001")
    );
    assert_eq!(SegmentUrl::match_id("/segments/abc123/7.json"), None);
    assert_eq!(SegmentUrl::match_id("/matches/m-0001/segments/abc123/7.json"), None);
}

/// Every URL the builder writes is a URL the parser reads.
#[test]
fn every_built_url_is_parsed_back() {
    for number in [0, 1, 7, 89, 90, 1_000] {
        let url = SegmentUrl::build("m-0001", "abc123", number);
        let parts = SegmentUrl::parse(&url).expect("a built URL is a segment URL");
        assert_eq!(parts.match_id, "m-0001");
        assert_eq!(parts.build_id, "abc123");
        assert_eq!(parts.number, number);
    }
}
