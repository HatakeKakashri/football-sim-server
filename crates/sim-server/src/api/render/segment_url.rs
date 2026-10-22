//! The segment URL scheme: the path, the suffix and the parts, in
//! one place (server-API ticket 09).
//!
//! The manifest publishes a segment's URL, the router routes it and
//! the segment route reads it back into its parts. All three read
//! the scheme from here, so they cannot drift apart: the scheme is
//! one string, and the route pattern, the published URL and the
//! parser are three renderings of it.
//!
//! # The scheme
//!
//! `/match/{match_id}/segments/{build_id}/{number}.json`
//!
//! The route pattern is the scheme as it stands:
//! `/match/{match_id}/segments/{build_id}/{segment}`. The router's
//! parameter is named after the scheme's slot, so the route and the
//! published URL carry the same parts in the same order. The `.json`
//! suffix belongs to the segment's value rather than to the route,
//! because a router matches whole path segments and cannot pin a
//! suffix inside one. The parser is what rejects a path that does not
//! end in the suffix.
//!
//! # Reading a path
//!
//! [`SegmentUrl::parse`] reads a whole path and answers the segment
//! it names, or nothing. [`SegmentUrl::match_id`] reads a path
//! leniently: the match is the first part of the scheme, so a path
//! that names a match names it even when the rest of the path does
//! not match the scheme. The segment route needs both: the match's
//! status decides the answer before the path's shape does.
//!
//! # Round trip
//!
//! [`SegmentUrl::parse`] reads back what [`SegmentUrl::build`] wrote:
//! a published URL always names the segment it was built for.

use std::fmt::Write as _;

/// The scheme of a segment URL: the path, with a slot for each part.
///
/// The slots are the parts of the URL, in the order the path carries
/// them: the match, the build and the segment. The scheme is the one
/// definition of the URL — the route pattern is it as it stands, the
/// published URL is it with the slots filled in, and the parser
/// reads it back.
pub const SCHEME: &str = "/match/{match_id}/segments/{build_id}/{segment}";

/// The suffix every published segment URL ends in.
///
/// The suffix is part of the segment's value, not of the route: a
/// router matches whole path segments, so the route cannot pin a
/// suffix inside one. The parser is what rejects a path that does not
/// end in it.
pub const SEGMENT_SUFFIX: &str = ".json";

/// The scheme of the URLs the manifest publishes for a segment.
///
/// The type is a namespace: it owns the scheme and nothing else, so
/// the builder, the route pattern and the parser cannot drift apart.
#[derive(Debug, Clone, Copy)]
pub struct SegmentUrl;

impl SegmentUrl {
    /// The axum route pattern of the segment route.
    ///
    /// The pattern is the scheme as it stands: the router names the
    /// segment's parameter after the scheme's slot, so the route and
    /// the published URL carry the same parts in the same order.
    #[must_use]
    pub const fn route_pattern() -> &'static str {
        SCHEME
    }

    /// The immutable URL a segment is served from.
    ///
    /// The URL embeds the build id and the segment number, so the
    /// bytes behind it never change.
    #[must_use]
    pub fn build(match_id: &str, build_id: &str, number: u32) -> String {
        let mut url = String::with_capacity(SCHEME.len() + match_id.len() + build_id.len());
        for (index, part) in SCHEME.split('/').enumerate() {
            if index > 0 {
                url.push('/');
            }
            match part {
                "{match_id}" => url.push_str(match_id),
                "{build_id}" => url.push_str(build_id),
                "{segment}" => push_segment(&mut url, number),
                literal => url.push_str(literal),
            }
        }
        url
    }

    /// The parts a segment URL path names.
    ///
    /// The path is read against the scheme: a literal part must be
    /// equal and a slot takes the path's part. A path that is not a
    /// segment URL names no segment, and the answer is `None`.
    #[must_use]
    pub fn parse(path: &str) -> Option<SegmentPath> {
        match read(path) {
            Read::Segment {
                match_id,
                build_id,
                number,
            } => Some(SegmentPath {
                match_id: match_id.to_string(),
                build_id: build_id.to_string(),
                number,
            }),
            Read::NotSegment { .. } => None,
        }
    }

    /// The match a path names, if it names one.
    ///
    /// The match is the first part of the scheme, so a path that names
    /// a match names it even when the rest of the path does not match
    /// the scheme: a path that names no segment may still name the
    /// match it was asked about.
    #[must_use]
    pub fn match_id(path: &str) -> Option<&str> {
        match read(path) {
            Read::Segment { match_id, .. } => Some(match_id),
            Read::NotSegment { match_id } => match_id,
        }
    }
}

/// The parts a segment URL names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentPath {
    /// The match the segment belongs to.
    pub match_id: String,
    /// The build id the segment's bytes belong to.
    pub build_id: String,
    /// The segment's number, counted from zero across the match.
    pub number: u32,
}

/// What the scheme's reader found in a path.
enum Read<'a> {
    /// The path is a segment URL: it names this segment.
    Segment {
        /// The match the path names.
        match_id: &'a str,
        /// The build the path names.
        build_id: &'a str,
        /// The segment number the path names.
        number: u32,
    },
    /// The path is not a segment URL. It may still name a match.
    NotSegment {
        /// The match the path names, if it named one.
        match_id: Option<&'a str>,
    },
}

/// Read a path against the scheme.
///
/// The scheme's parts and the path's parts are read in order: a
/// literal part must be equal and a slot takes the path's part. The
/// read stops at the first part that does not match, so what was read
/// so far is what the path named.
fn read(path: &str) -> Read<'_> {
    let mut match_id = None;
    let mut build_id = None;
    let mut number = None;
    let mut parts = path.split('/');
    for scheme_part in SCHEME.split('/') {
        let Some(part) = parts.next() else {
            // The path is shorter than the scheme.
            return Read::NotSegment { match_id };
        };
        match scheme_part {
            "{match_id}" => match_id = Some(part),
            "{build_id}" => build_id = Some(part),
            "{segment}" => {
                let Some(segment) = segment_number(part) else {
                    // The part names no segment.
                    return Read::NotSegment { match_id };
                };
                number = Some(segment);
            }
            literal => {
                if literal != part {
                    // The path is not this shape.
                    return Read::NotSegment { match_id };
                }
            }
        }
    }
    if parts.next().is_some() {
        // The path has more parts than the scheme: it names no
        // segment.
        return Read::NotSegment { match_id };
    }
    match (match_id, build_id, number) {
        (Some(match_id), Some(build_id), Some(number)) => Read::Segment {
            match_id,
            build_id,
            number,
        },
        _ => Read::NotSegment { match_id },
    }
}

/// The number a segment URL's last part names.
///
/// The published URLs end in `.json`; a part that does not is not a
/// segment URL.
fn segment_number(part: &str) -> Option<u32> {
    let digits = part.strip_suffix(SEGMENT_SUFFIX)?;
    digits.parse().ok()
}

/// Write a segment's file name into a URL.
fn push_segment(url: &mut String, number: u32) {
    write!(url, "{number}{SEGMENT_SUFFIX}").expect("invariant: writing into a String cannot fail");
}
