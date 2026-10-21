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
//! # Build id
//!
//! [`Manifest::build_id`] is a content digest: SHA-256 over the
//! fixture's contents, the format version and every segment's hash. It
//! changes whenever the rendered bytes change and depends on nothing
//! else (no crate version, no git hash — neither exists here).

use serde::{Deserialize, Serialize};
use sim_components::time::{HALF_LENGTH_TICKS, TICKS_PER_SECOND};
use sim_components::KickoffReason;
use sim_core::MatchEvent;

use super::registry::MatchFixture;
use super::stream::{CompletedMatch, ManifestTeam, RosterEntry, StreamFrame};
use digest::{build_id, hex, sha256};

mod digest;

/// Wire format version of the manifest and the segment bodies.
///
/// The version is part of the build id, so publishing a different
/// shape produces a different build id and the old segments stay
/// reachable under the old one.
pub const FORMAT_VERSION: u32 = 1;

/// Ticks per segment: 60 s of match time at 60 ticks per second.
pub const SEGMENT_TICKS: u64 = 60 * 60;

/// The content coding of the published segment bodies.
///
/// The bodies are published as rendered: plain JSON, no compression.
/// Ticket 04 stores them pre-gzipped and fills the manifest's gzipped
/// byte length.
pub const ENCODING_IDENTITY: &str = "identity";

/// Display minutes per half (PRD §8.3).
///
/// The second half's display minutes continue after the first half's,
/// so a segment of the second half starts at minute 45.
const MINUTES_PER_HALF: u64 = HALF_LENGTH_TICKS / (TICKS_PER_SECOND * 60);

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
    /// Always `null` for now: the segments are published uncompressed
    /// and ticket 04 stores them pre-gzipped.
    pub gzipped_bytes: Option<u64>,
}

/// The manifest of a rendered match.
///
/// It carries the match's metadata (as the old SSE `manifest` message
/// did), the end-of-match summary, and the list of segments with their
/// immutable URLs, sizes and hashes.
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSegment {
    /// The manifest row for this segment.
    pub metadata: ManifestSegment,
    /// The uncompressed body bytes.
    pub body: Vec<u8>,
}

/// A rendered match: the manifest and the segments it lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedMatch {
    /// The manifest.
    pub manifest: Manifest,
    /// The segments, in manifest order.
    pub segments: Vec<RenderedSegment>,
}

/// The tick range of one segment, before it is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SegmentRange {
    /// The segment number, counted from zero across the whole match.
    number: u32,
    /// The half the segment belongs to.
    half: u8,
    /// First global tick of the segment (inclusive).
    from_tick: u64,
    /// Last global tick of the segment (inclusive).
    to_tick: u64,
    /// Display minute the segment starts in.
    from_minute: u64,
    /// Display minute the segment ends in (exclusive).
    to_minute: u64,
}

/// The published body of one segment.
///
/// The shape is the one the SSE stream used: the segment's own tick
/// range, the events that happened inside it and the animation frames
/// it covers. Positions and ticks are absolute, so a segment can be
/// read on its own.
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
    frames: Vec<&'a StreamFrame>,
}

/// Render a completed match: the manifest plus its 60-second segments.
///
/// The step is pure and deterministic: the same fixture and the same
/// completed match always render the same bytes and the same hashes.
#[must_use]
pub fn render_match(fixture: &MatchFixture, completed: &CompletedMatch) -> RenderedMatch {
    let ranges = segment_ranges(completed);
    let bodies: Vec<Vec<u8>> = ranges
        .iter()
        .map(|range| render_body(completed, range))
        .collect();
    let hashes: Vec<[u8; 32]> = bodies.iter().map(|body| sha256(body)).collect();
    let build_id = build_id(fixture, &hashes);

    let segments: Vec<RenderedSegment> = ranges
        .iter()
        .zip(bodies.iter())
        .zip(hashes.iter())
        .map(|((range, body), hash)| {
            let metadata = ManifestSegment {
                segment: range.number,
                half: range.half,
                from_tick: range.from_tick,
                to_tick: range.to_tick,
                url: segment_url(&completed.match_id, &build_id, range.number),
                encoding: ENCODING_IDENTITY.to_string(),
                uncompressed_bytes: u64::try_from(body.len()).unwrap_or(u64::MAX),
                sha256: hex(hash),
                gzipped_bytes: None,
            };
            RenderedSegment {
                metadata,
                body: body.clone(),
            }
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
        segments: segments
            .iter()
            .map(|segment| segment.metadata.clone())
            .collect(),
    };

    RenderedMatch { manifest, segments }
}

/// The tick ranges of every segment of a completed match.
///
/// Each half is cut from its own first tick at [`SEGMENT_TICKS`]
/// boundaries and its last segment runs to the whistle, so no segment
/// crosses a half boundary. The number of segments follows from the
/// length of the half: 45 segments for the regular 45 minutes plus
/// one per added minute.
fn segment_ranges(completed: &CompletedMatch) -> Vec<SegmentRange> {
    let first_half_end = completed.boundaries.first_half_end;
    let second_half_start = second_half_start(completed);
    let first = cut_half(1, 0, first_half_end, 0, 0);
    let first_count = u32::try_from(first.len()).unwrap_or(u32::MAX);
    let second = cut_half(
        2,
        second_half_start,
        completed.boundaries.full_time,
        first_count,
        MINUTES_PER_HALF,
    );
    first.into_iter().chain(second).collect()
}

/// Cut one half into 60-second segments.
///
/// `start` is the half's first tick and `end` the whistle that ends
/// it. The segments are cut at [`SEGMENT_TICKS`] boundaries from
/// `start`; the last one runs to the whistle and absorbs the
/// remainder, so the half's tick range is covered exactly once.
fn cut_half(
    half: u8,
    start: u64,
    end: u64,
    first_number: u32,
    minute_base: u64,
) -> Vec<SegmentRange> {
    let mut ranges = Vec::new();
    if end < start {
        return ranges;
    }
    let span = end - start;
    let count = span.div_ceil(SEGMENT_TICKS).max(1);
    for index in 0..count {
        let from_tick = start + index * SEGMENT_TICKS;
        let to_tick = if index + 1 == count {
            end
        } else {
            (from_tick + SEGMENT_TICKS - 1).min(end)
        };
        ranges.push(SegmentRange {
            number: first_number + u32::try_from(index).unwrap_or(u32::MAX),
            half,
            from_tick,
            to_tick,
            from_minute: minute_base + (from_tick - start) / SEGMENT_TICKS,
            to_minute: minute_base + (to_tick - start + 1) / SEGMENT_TICKS,
        });
    }
    ranges
}

/// The tick at which the second half's match clock starts.
///
/// The second half begins at its own kickoff, where the clock resets
/// to zero, so the second half's segments are anchored there and every
/// one of them covers exactly 60 s of match time. A match whose event
/// log carries no second-half kickoff falls back to the tick after the
/// first half's whistle.
fn second_half_start(completed: &CompletedMatch) -> u64 {
    let after_whistle = completed.boundaries.first_half_end.saturating_add(1);
    completed
        .events
        .iter()
        .find_map(|event| match event {
            MatchEvent::Kickoff { envelope, payload }
                if payload.reason == KickoffReason::SecondHalf =>
            {
                Some(envelope.tick)
            }
            _ => None,
        })
        // The second half always starts after the first half's whistle,
        // whatever the event log says.
        .map_or(after_whistle, |tick| tick.max(after_whistle))
}

/// Render the body of one segment: its events and its frames.
fn render_body(completed: &CompletedMatch, range: &SegmentRange) -> Vec<u8> {
    let events: Vec<&MatchEvent> = completed
        .events
        .iter()
        .filter(|event| {
            let tick = event.envelope().tick;
            tick >= range.from_tick && tick <= range.to_tick
        })
        .collect();
    let frames: Vec<&StreamFrame> = completed
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
        frames,
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
