//! Pure render step: a fixture plus a completed match become a
//! manifest and a list of 60-second segments (server-API ticket 03).
//!
//! The step is pure: the same fixture and the same completed match
//! always render the same bytes, so a rendered match can be cached and
//! re-served unchanged (PRD §10.4 determinism).
//!
//! # Segmentation
//!
//! A half is cut from its own first tick at [`SEGMENT_TICKS`] (3,600
//! ticks = 60 s of match time) boundaries. The last segment of a half
//! runs to the whistle and absorbs the remainder, so no segment ever
//! crosses a half boundary and the segment count follows from the
//! match: a 45-minute half yields 45 segments plus one per added
//! minute. The count is never a constant.
//!
//! The two halves are anchored separately, so the second half's
//! segments are *not* aligned to multiples of 3,600 from tick 0: the
//! second half starts at its own kickoff, after the first half's last
//! tick. The half-time break between the two halves is not match time
//! and belongs to no segment.
//!
//! # Storage
//!
//! A rendered segment is stored gzipped, and its uncompressed bytes
//! are published beside it (see the publishing section below). Both are
//! shared between clones and never mutated, so serving a segment costs
//! no copy of either.
//!
//! A client that does not accept gzip is served the decompressed bytes
//! on demand; see [`RenderedSegment::decompress`].
//!
//! # Publishing
//!
//! A rendered match is published as immutable bytes: the manifest's
//! answer body and every segment's body, indexed by segment number.
//! The bytes are produced once, at render time, and are never derived
//! again, so a served manifest is a copy of bytes rather than a value
//! re-serialised per request. See [`PublishedMatch`].
//!
//! # Identity
//!
//! A segment's identity is its *uncompressed* SHA-256, never its
//! compressed bytes: the manifest's `sha256` and the [`Manifest::build_id`]
//! are computed over the uncompressed bodies, so changing the
//! compression level never changes a URL or a build id.
//!
//! # Build id
//!
//! [`Manifest::build_id`] is a content digest: SHA-256 over the
//! fixture's contents, the format version and every segment's hash. It
//! changes whenever the rendered bytes change and depends on nothing
//! else (no crate version, no git hash — neither exists here).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sim_core::MatchEvent;

use super::registry::MatchFixture;
use super::model::{CompletedMatch, ManifestTeam, MatchFrame, RosterEntry};
use digest::{build_id, hex, sha256};
use frame_body::FramesBody;
use gzip::{compress, decompress};
use ranges::{SegmentRange, segment_ranges};

pub use gzip::{GZIP_LEVEL, GzipError};
pub use published::{ManifestBody, PublishedMatch, PublishedSegment};

mod digest;
mod frame_body;
mod gzip;
mod published;
mod ranges;
#[cfg(test)]
mod frame_body_tests;
#[cfg(test)]
mod gzip_tests;
#[cfg(test)]
mod published_tests;

/// Wire format version of the manifest and the segment bodies.
///
/// The version is part of the build id, so publishing a different
/// shape produces a different build id and the old segments stay
/// reachable under the old one.
pub const FORMAT_VERSION: u32 = 1;

/// Ticks per segment: 60 s of match time at 60 ticks per second.
pub const SEGMENT_TICKS: u64 = 60 * 60;

/// The content coding of the stored segment bodies.
///
/// The bodies are stored gzipped, at the fixed level [`GZIP_LEVEL`]. A
/// client that accepts gzip is served the stored bytes; a client that
/// does not is served the decompressed bytes, so the coding is
/// negotiated per request and the URL stays the same either way.
pub const ENCODING_GZIP: &str = "gzip";

/// Pitch dimensions as published in the manifest (PRD §9.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PitchDimensions {
    /// Pitch length in decimetres.
    pub length: u32,
    /// Pitch width in decimetres.
    pub width: u32,
    /// The unit the dimensions are expressed in.
    pub unit: String,
}

/// Attack direction per side, one entry per half (PRD §10.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackDirection {
    /// Home side: `(first half, second half)`, `+1` attacks `x = length`.
    pub home: (i8, i8),
    /// Away side: `(first half, second half)`.
    pub away: (i8, i8),
}

/// The end-of-match summary the manifest publishes under `result`.
///
/// It carries what the old `full_time` stream message carried: the
/// final score, the number of events in the log and the deterministic
/// state hash at full time (PRD §10.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchSummary {
    /// Final score `(home, away)`.
    pub score: (u8, u8),
    /// Number of events in the match log.
    pub event_count: u64,
    /// Deterministic state hash at full time.
    pub state_hash: u64,
}

/// One segment as the manifest lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestSegment {
    /// The segment number, counted from zero across the whole match.
    pub segment: u32,
    /// The half the segment belongs to (`1` or `2`).
    pub half: u8,
    /// First global tick of the segment (inclusive).
    pub from_tick: u64,
    /// Last global tick of the segment (inclusive).
    pub to_tick: u64,
    /// The immutable URL the segment is served from.
    pub url: String,
    /// Content coding of the body behind the URL.
    pub encoding: String,
    /// Length of the uncompressed body in bytes.
    pub uncompressed_bytes: u64,
    /// SHA-256 of the uncompressed body, lowercase hex.
    pub sha256: String,
    /// Length of the gzipped body in bytes.
    ///
    /// Always populated: the body is stored gzipped, so the transfer
    /// budget is known before a client asks for anything.
    pub gzipped_bytes: Option<u64>,
}

/// The manifest of a rendered match.
///
/// It carries the match's metadata, the end-of-match summary, and the
/// list of segments with their immutable URLs, sizes and hashes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Content-derived build id: SHA-256 over the fixture's contents,
    /// the format version and every segment's hash.
    pub build_id: String,
    /// Wire format version of this manifest.
    pub format_version: u32,
    /// The fixture's match id.
    pub match_id: String,
    /// The fixture's seed.
    pub seed: u64,
    /// Both sides' lineup setups, home first.
    pub teams: Vec<ManifestTeam>,
    /// The roster rows, home first then away.
    pub roster: Vec<RosterEntry>,
    /// Pitch dimensions and unit.
    pub pitch: PitchDimensions,
    /// Ticks per second of match time.
    pub ticks_per_second: u64,
    /// Ticks between two animation frames.
    pub frame_every_ticks: u64,
    /// Attack direction per side per half.
    pub attack_direction: AttackDirection,
    /// The end-of-match summary.
    pub result: MatchSummary,
    /// The segments, in order.
    pub segments: Vec<ManifestSegment>,
}

/// One rendered segment: the manifest row plus the body it describes.
///
/// The body is stored gzipped and shared between clones of the
/// segment, so serving a segment from several places costs one copy of
/// the bytes. The uncompressed body is published beside it (see
/// [`PublishedSegment`]) and is reproduced on demand by
/// [`RenderedSegment::decompress`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSegment {
    /// The manifest row for this segment.
    pub metadata: ManifestSegment,
    /// The gzipped body bytes.
    pub gzipped_body: Arc<[u8]>,
}

impl RenderedSegment {
    /// The stored gzipped bytes.
    #[must_use]
    pub fn gzipped_bytes(&self) -> &[u8] {
        &self.gzipped_body
    }

    /// The uncompressed body bytes.
    ///
    /// The bytes are reproduced on demand from the stored coding. The
    /// published match keeps a copy of them too (see
    /// [`PublishedSegment`]), so a caller that serves the body itself
    /// does not have to reproduce it.
    ///
    /// # Errors
    ///
    /// Returns a [`GzipError`] when the stored bytes are not a valid
    /// gzip stream, which cannot happen for a segment this module
    /// rendered.
    pub fn decompress(&self) -> Result<Vec<u8>, GzipError> {
        decompress(&self.gzipped_body)
    }
}

/// A rendered match: the manifest and the segments it lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedMatch {
    /// The manifest.
    pub manifest: Manifest,
    /// The segments, in manifest order.
    pub segments: Vec<RenderedSegment>,
    /// The published bytes: the manifest's answer body and the
    /// segments indexed by their number.
    pub published: PublishedMatch,
}

/// The published body of one segment.
///
/// The shape carries the segment's own tick range, the events that
/// happened inside it and the animation frames it covers. Positions and
/// ticks are absolute, so a segment can be read on its own.
///
/// A frame names its players by their roster index; the body writer
/// resolves the index back to the fixture id when it writes the frame,
/// so the bytes on the wire are the bytes the earlier format produced.
#[derive(Debug, Serialize)]
struct SegmentBody<'a> {
    /// The segment number.
    segment: u32,
    /// The half the segment belongs to.
    half: u8,
    /// Display minute the segment starts in.
    from_minute: u64,
    /// Display minute the segment ends in (exclusive).
    to_minute: u64,
    /// First global tick of the segment (inclusive).
    from_tick: u64,
    /// Last global tick of the segment (inclusive).
    to_tick: u64,
    /// The events whose tick falls inside the segment.
    events: Vec<&'a MatchEvent>,
    /// The frames whose tick falls inside the segment.
    frames: FramesBody<'a>,
}

/// Render a completed match: the manifest plus its 60-second segments.
///
/// The step is pure and deterministic: the same fixture and the same
/// completed match always render the same bytes and the same hashes.
///
/// Each segment is rendered, hashed and compressed in turn, and the
/// result is published as immutable bytes: the manifest's answer body
/// and every segment's body, indexed by segment number. The bytes are
/// produced here, once, and are never derived again.
#[must_use]
pub fn render_match(fixture: &MatchFixture, completed: &CompletedMatch) -> RenderedMatch {
    let ranges = segment_ranges(completed);
    let stored: Vec<StoredSegment> = ranges
        .iter()
        .map(|range| StoredSegment::render(completed, *range))
        .collect();
    let hashes: Vec<[u8; 32]> = stored.iter().map(|segment| segment.hash).collect();
    let build_id = build_id(fixture, &hashes);

    let rows: Vec<ManifestSegment> = stored
        .iter()
        .map(|segment| ManifestSegment {
            segment: segment.range.number,
            half: segment.range.half,
            from_tick: segment.range.from_tick,
            to_tick: segment.range.to_tick,
            url: segment_url(&completed.match_id, &build_id, segment.range.number),
            encoding: ENCODING_GZIP.to_string(),
            uncompressed_bytes: segment.uncompressed_bytes,
            sha256: hex(&segment.hash),
            gzipped_bytes: Some(segment.gzipped_bytes),
        })
        .collect();

    let segments: Vec<RenderedSegment> = stored
        .iter()
        .zip(&rows)
        .map(|(segment, row)| RenderedSegment {
            metadata: row.clone(),
            gzipped_body: Arc::clone(&segment.body),
        })
        .collect();

    let manifest = Manifest {
        build_id,
        format_version: FORMAT_VERSION,
        match_id: completed.match_id.clone(),
        seed: completed.seed,
        teams: completed.teams.clone(),
        roster: completed.roster.clone(),
        pitch: PitchDimensions {
            length: 1050,
            width: 680,
            unit: "decimetres".to_string(),
        },
        ticks_per_second: 60,
        frame_every_ticks: 6,
        attack_direction: AttackDirection {
            home: completed.attack_home,
            away: completed.attack_away,
        },
        result: MatchSummary {
            score: completed.score,
            event_count: u64::try_from(completed.events.len()).unwrap_or(u64::MAX),
            state_hash: completed.state_hash,
        },
        segments: rows,
    };

    let published = PublishedMatch::publish(&manifest, &stored);

    RenderedMatch {
        manifest,
        segments,
        published,
    }
}

/// One segment as it is stored: its range, the hash of its
/// uncompressed body and the gzipped bytes themselves.
struct StoredSegment {
    /// The segment's tick range and numbering.
    range: SegmentRange,
    /// SHA-256 of the uncompressed body.
    hash: [u8; 32],
    /// Length of the uncompressed body in bytes.
    uncompressed_bytes: u64,
    /// Length of the gzipped body in bytes.
    gzipped_bytes: u64,
    /// The gzipped body, shared between clones.
    body: Arc<[u8]>,
    /// The uncompressed body, shared between clones.
    uncompressed_body: Arc<[u8]>,
}

impl StoredSegment {
    /// Render one segment, compress it and keep both bodies.
    ///
    /// The uncompressed body is kept because it is published: it is
    /// the body the manifest's hash covers, and the one a client that
    /// does not accept gzip is served.
    fn render(completed: &CompletedMatch, range: SegmentRange) -> Self {
        let body = render_body(completed, &range);
        let hash = sha256(&body);
        let gzipped = compress(&body);
        Self {
            range,
            hash,
            uncompressed_bytes: u64::try_from(body.len()).unwrap_or(u64::MAX),
            gzipped_bytes: u64::try_from(gzipped.len()).unwrap_or(u64::MAX),
            body: Arc::from(gzipped),
            uncompressed_body: Arc::from(body),
        }
    }
}

/// Render the body of one segment: its events and its frames.
///
/// The frames are written from the captured frames, resolved against
/// the roster the manifest publishes: the ids are borrowed from the
/// roster, never copied into the body.
fn render_body(completed: &CompletedMatch, range: &SegmentRange) -> Vec<u8> {
    let events: Vec<&MatchEvent> = completed
        .events
        .iter()
        .filter(|event| {
            let tick = event.envelope().tick;
            tick >= range.from_tick && tick <= range.to_tick
        })
        .collect();
    let frames: Vec<&MatchFrame> = completed
        .frames
        .iter()
        .filter(|frame| frame.tick >= range.from_tick && frame.tick <= range.to_tick)
        .collect();
    let body = SegmentBody {
        segment: range.number,
        half: range.half,
        from_minute: range.from_minute,
        to_minute: range.to_minute,
        from_tick: range.from_tick,
        to_tick: range.to_tick,
        events,
        frames: FramesBody::new(&frames, &completed.roster),
    };
    serde_json::to_vec(&body).expect("invariant: a segment body is JSON-serialisable")
}

/// The immutable URL a segment is served from.
///
/// The URL embeds the build id and the segment number, so the bytes
/// behind it never change.
fn segment_url(match_id: &str, build_id: &str, number: u32) -> String {
    format!("/match/{match_id}/segments/{build_id}/{number}.json")
}
