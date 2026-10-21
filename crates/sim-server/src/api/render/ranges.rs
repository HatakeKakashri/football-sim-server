//! The tick ranges of a match's segments.
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

use sim_components::time::{HALF_LENGTH_TICKS, TICKS_PER_SECOND};
use sim_components::KickoffReason;
use sim_core::MatchEvent;

use super::super::model::CompletedMatch;
use super::SEGMENT_TICKS;

/// Display minutes per half (PRD §8.3).
///
/// The second half's display minutes continue after the first half's,
/// so a segment of the second half starts at minute 45.
const MINUTES_PER_HALF: u64 = HALF_LENGTH_TICKS / (TICKS_PER_SECOND * 60);

/// The tick range of one segment, before it is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SegmentRange {
    /// The segment number, counted from zero across the whole match.
    pub(super) number: u32,
    /// The half the segment belongs to.
    pub(super) half: u8,
    /// First global tick of the segment (inclusive).
    pub(super) from_tick: u64,
    /// Last global tick of the segment (inclusive).
    pub(super) to_tick: u64,
    /// Display minute the segment starts in.
    pub(super) from_minute: u64,
    /// Display minute the segment ends in (exclusive).
    pub(super) to_minute: u64,
}

/// The tick ranges of every segment of a completed match.
///
/// Each half is cut from its own first tick at [`SEGMENT_TICKS`]
/// boundaries and its last segment runs to the whistle, so no segment
/// crosses a half boundary. The number of segments follows from the
/// length of the half: 45 segments for the regular 45 minutes plus
/// one per added minute.
pub(super) fn segment_ranges(completed: &CompletedMatch) -> Vec<SegmentRange> {
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
