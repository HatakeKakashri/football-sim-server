//! SSE stream layout for one fixture match (PRD §10.2–§10.3).
//!
//! A finished match streams as exactly 12 messages: id 0 `manifest`,
//! ids 1–10 `segment` (five per half, never crossing a half boundary),
//! id 11 `full_time`. A failed match streams a single `error` message
//! (id 0) that replaces the remaining messages (PRD §10.2).
//!
//! The rendering is canonical [`serde_json::to_string`] with struct
//! field order, and carries no wall-clock timestamps, so re-simulating
//! the same fixture produces a byte-identical uncompressed body
//! (PRD §10.4).

use serde::{Deserialize, Serialize};
use sim_core::MatchEvent;

/// Ticks per 10-minute segment: `10 min × 60 s × 60 ticks`.
pub const SEGMENT_TICKS: u64 = 10 * 60 * 60;

/// The home side, as published on the wire.
pub const SIDE_HOME: &str = "home";

/// The away side, as published on the wire.
pub const SIDE_AWAY: &str = "away";

/// One SSE message: the `id` is the message index the client sends
/// back as `Last-Event-ID` to resume after message *n* (PRD §10.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseMessage {
    /// Message index (`manifest` = 0 … `full_time` = 11).
    pub id: u64,
    /// SSE `event:` field (`manifest` | `segment` | `full_time` | `error`).
    pub event: &'static str,
    /// JSON `data:` payload.
    pub data: serde_json::Value,
}

impl SseMessage {
    /// Render one SSE frame (`id` / `event` / `data` + blank line).
    ///
    /// # Panics
    ///
    /// Panics if the payload is not JSON-serialisable, which cannot
    /// happen for [`serde_json::Value`] (the only `data` type).
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "id: {}\nevent: {}\ndata: {}\n\n",
            self.id,
            self.event,
            serde_json::to_string(&self.data)
                .expect("invariant: SSE payloads are JSON-serialisable"),
        )
    }
}

/// Render a whole stream body: the concatenation of every message.
/// This is the byte sequence PRD §10.4 requires to be identical
/// across re-simulation (uncompressed).
#[must_use]
pub fn render_body(messages: &[SseMessage]) -> String {
    let mut body = String::new();
    for message in messages {
        body.push_str(&message.render());
    }
    body
}

/// Half boundaries in global ticks, located from the event log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HalfBoundaries {
    /// Global tick of the `half_time` event (last tick of half one).
    pub first_half_end: u64,
    /// Global tick of the `full_time` event (last tick of the match).
    pub full_time: u64,
}

/// Tick range of one of the 10 segments (PRD §10.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentRange {
    /// Segment index 0–9 (message id is `segment + 1`).
    pub segment: u8,
    /// First global tick in the segment (inclusive).
    pub from_tick: u64,
    /// Last global tick in the segment (inclusive).
    pub to_tick: u64,
}

/// Split a match into 10 half-aligned segments (PRD §10.3).
///
/// The first half `[0, first_half_end]` yields five segments: the
/// first four are exactly [`SEGMENT_TICKS`] wide (0–10 … 30–40) and
/// the fifth runs to the half whistle (40–45+added). The second half
/// `[first_half_end + 1, full_time]` is split the same way, so the
/// half-time break ticks stream inside segment 5 — they are in the
/// stream but are not match time (PRD §8.3). No segment crosses a
/// half boundary by construction.
#[must_use]
pub fn segment_ranges(boundaries: &HalfBoundaries) -> [SegmentRange; 10] {
    let mut ranges = [SegmentRange {
        segment: 0,
        from_tick: 0,
        to_tick: 0,
    }; 10];
    let mut half_start = 0_u64;
    let mut half_end = boundaries.first_half_end;
    for segment in 0_u8..10 {
        if segment == 5 {
            half_start = boundaries.first_half_end.saturating_add(1);
            half_end = boundaries.full_time;
        }
        let from_tick = if segment % 5 == 0 {
            half_start
        } else {
            half_start.saturating_add(u64::from(segment % 5) * SEGMENT_TICKS)
        };
        // The last segment of each half runs to the whistle
        // (40–45+added / 85–90+added); the others are exactly
        // `SEGMENT_TICKS` wide. Clamp defensively so a short half
        // yields empty (not inverted) ranges.
        let natural_end = half_start.saturating_add(u64::from(segment % 5 + 1) * SEGMENT_TICKS);
        let to_tick = if segment % 5 == 4 {
            half_end
        } else {
            natural_end.saturating_sub(1).min(half_end)
        };
        ranges[segment as usize] = SegmentRange {
            segment,
            from_tick,
            to_tick: to_tick.max(from_tick.saturating_sub(1)),
        };
    }
    ranges
}

/// Display-minute span of one segment (PRD §10.3).
#[must_use]
pub const fn segment_minutes(segment: u8) -> (u64, u64) {
    match segment {
        0 => (0, 10),
        1 => (10, 20),
        2 => (20, 30),
        3 => (30, 40),
        4 => (40, 45),
        5 => (45, 55),
        6 => (55, 65),
        7 => (65, 75),
        8 => (75, 85),
        _ => (85, 90),
    }
}

/// Metres → signed integer decimetres, rounded half away from zero
/// (PRD §9.4; maximum error 0.05 m).
#[must_use]
pub fn to_decimetres(metres: f32) -> i32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "pitch coordinates stay within ±100_000 dm; the clamp makes the `as` cast exact"
    )]
    let decimetres = (metres * 10.0).round().clamp(-100_000.0, 100_000.0) as i32;
    decimetres
}

/// Ball state discriminator on the wire (`free` | `possessed` |
/// `in_flight` | `dead`, PRD §9.4).
#[must_use]
pub const fn ball_state_str(state: sim_components::BallState) -> &'static str {
    match state {
        sim_components::BallState::Free => "free",
        sim_components::BallState::Possessed => "possessed",
        sim_components::BallState::InFlight => "in_flight",
        sim_components::BallState::Dead => "dead",
    }
}

/// One 10 Hz animation frame (PRD §9.4): pure positions, no extra
/// frames at event ticks. Coordinates are integer decimetres.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamFrame {
    /// Global tick of the frame.
    pub tick: u64,
    /// Half in progress when the frame was captured.
    pub half: u8,
    /// Ball position, state and possessor roster id (if any).
    pub ball: FrameBall,
    /// All 22 players: roster id + position.
    pub players: Vec<FramePlayer>,
}

/// Ball projection inside a [`StreamFrame`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameBall {
    /// Ball x in decimetres.
    pub x: i32,
    /// Ball y in decimetres.
    pub y: i32,
    /// Ball state discriminator.
    pub state: String,
    /// Roster id of the possessor, if any.
    pub possessor: Option<String>,
}

/// Player projection inside a [`StreamFrame`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FramePlayer {
    /// Stable roster id: the fixture's `player_id`, the same id the
    /// manifest's roster row carries.
    pub id: String,
    /// Player x in decimetres.
    pub x: i32,
    /// Player y in decimetres.
    pub y: i32,
}

/// One roster row published in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterEntry {
    /// Stable roster id: the fixture's `player_id`.
    pub id: String,
    /// Team side (`home` | `away`).
    pub team: String,
    /// Display name from the fixture.
    pub name: String,
    /// Squad number from the fixture.
    pub number: u8,
    /// Formation slot (`GK`, `DC`, …).
    pub position: String,
}

/// One side's lineup setup as published in the manifest (PRD §10.2).
///
/// The three values come from the fixture's per-side setup
/// (`[match.home_setup]` / `[match.away_setup]`, PRD §5.4), so the
/// manifest describes the match that actually ran instead of a
/// hardcoded default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestTeam {
    /// Side (`home` | `away`).
    pub id: String,
    /// Formation name, e.g. `4-3-3`.
    pub formation: String,
    /// Mentality name, e.g. `attack`.
    pub mentality: String,
    /// Tactic name, e.g. `counter_attack`.
    pub tactic: String,
}

impl ManifestTeam {
    /// Build the manifest row for one side from the fixture's setup.
    ///
    /// The setup's spellings are the fixture's own (PRD §5.4), so the
    /// published names round-trip through `Formation::from_name` and
    /// friends.
    #[must_use]
    pub fn from_setup(side: &str, setup: &sim_components::fixture::ValidatedSide) -> Self {
        Self {
            id: side.to_string(),
            formation: setup.formation.name().to_string(),
            mentality: setup.mentality.name().to_string(),
            tactic: setup.tactic.name().to_string(),
        }
    }
}

/// A completed match: everything [`build_stream`] needs. Built by
/// the simulation runner on the match's OS thread.
#[derive(Debug, Clone)]
pub struct CompletedMatch {
    /// Fixture match id (PRD §10.2 `match_id`).
    pub match_id: String,
    /// Fixture seed.
    pub seed: u64,
    /// Full event log in emission order.
    pub events: Vec<MatchEvent>,
    /// 10 Hz frames in tick order.
    pub frames: Vec<StreamFrame>,
    /// 22 roster rows: home rows first, then away rows, each in the
    /// fixture's roster order.
    pub roster: Vec<RosterEntry>,
    /// The two sides' lineup setups, home first.
    pub teams: Vec<ManifestTeam>,
    /// Final score `(home, away)`.
    pub score: (u8, u8),
    /// Deterministic state hash at full time.
    pub state_hash: u64,
    /// Added time per half in ticks `(first, second)`.
    pub added_time_ticks: (u64, u64),
    /// Half boundaries located from the event log.
    pub boundaries: HalfBoundaries,
    /// Attack direction per half: `(first_half, second_half)`.
    pub attack_home: (i8, i8),
    /// Attack direction per half: `(first_half, second_half)`.
    pub attack_away: (i8, i8),
}

/// Build the 12-message stream for a finished match: manifest (0),
/// 10 segments (1–10), `full_time` (11).
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "single manifest/segment/full_time assembly: one JSON object per message kind; splitting would scatter the §10.2 field map"
)]
pub fn build_stream(completed: &CompletedMatch) -> Vec<SseMessage> {
    let ranges = segment_ranges(&completed.boundaries);
    let segment_summaries: Vec<serde_json::Value> = ranges
        .iter()
        .map(|range| {
            serde_json::json!({
                "segment": range.segment,
                "from_tick": range.from_tick,
                "to_tick": range.to_tick,
            })
        })
        .collect();

    let mut messages = Vec::with_capacity(12);
    messages.push(SseMessage {
        id: 0,
        event: "manifest",
        data: serde_json::json!({
            "match_id": completed.match_id,
            "seed": completed.seed,
            "teams": completed.teams,
            "roster": completed.roster,
            "pitch": {"length": 1050, "width": 680, "unit": "decimetres"},
            "ticks_per_second": 60,
            "frame_every_ticks": 6,
            "attack_direction": {
                "home": [completed.attack_home.0, completed.attack_home.1],
                "away": [completed.attack_away.0, completed.attack_away.1],
            },
            "segments": segment_summaries,
        }),
    });

    for range in &ranges {
        let (from_minute, to_minute) = segment_minutes(range.segment);
        let half = if range.segment < 5 { 1 } else { 2 };
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
        messages.push(SseMessage {
            id: u64::from(range.segment) + 1,
            event: "segment",
            data: serde_json::json!({
                "segment": range.segment,
                "half": half,
                "from_minute": from_minute,
                "to_minute": to_minute,
                "from_tick": range.from_tick,
                "to_tick": range.to_tick,
                "events": events,
                "frames": frames,
            }),
        });
    }

    messages.push(SseMessage {
        id: 11,
        event: "full_time",
        data: serde_json::json!({
            "score": completed.score,
            "event_count": completed.events.len(),
            "state_hash": completed.state_hash,
        }),
    });
    messages
}

/// Build the failure stream: a single `error` message (id 0) that
/// replaces the remaining messages (PRD §10.2).
#[must_use]
pub fn build_error_stream(
    code: &str,
    message: &str,
    last_good_tick: u64,
) -> Vec<SseMessage> {
    vec![SseMessage {
        id: 0,
        event: "error",
        data: serde_json::json!({
            "code": code,
            "message": message,
            "last_good_tick": last_good_tick,
        }),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boundaries() -> HalfBoundaries {
        HalfBoundaries {
            first_half_end: 165_000,
            full_time: 330_100,
        }
    }

    #[test]
    fn segment_ranges_cover_both_halves_without_crossing() {
        let ranges = segment_ranges(&boundaries());
        assert_eq!(ranges.len(), 10);
        // First four segments of each half are exactly 10 minutes wide.
        for segment in [0_usize, 1, 2, 3, 5, 6, 7, 8] {
            let width = ranges[segment].to_tick - ranges[segment].from_tick + 1;
            assert_eq!(width, SEGMENT_TICKS, "segment {segment}");
        }
        // Last segment of each half runs to the whistle (incl. added).
        assert_eq!(ranges[4].to_tick, 165_000);
        assert_eq!(ranges[9].to_tick, 330_100);
        // Contiguous coverage, no half-boundary crossing.
        assert_eq!(ranges[0].from_tick, 0);
        assert_eq!(ranges[5].from_tick, 165_001);
        for window in ranges.windows(2) {
            let (left, right) = (window[0], window[1]);
            if left.segment == 4 {
                continue; // half boundary: right starts after the whistle.
            }
            assert_eq!(left.to_tick + 1, right.from_tick);
        }
        for range in &ranges[..5] {
            assert!(range.to_tick <= 165_000);
        }
        for range in &ranges[5..] {
            assert!(range.from_tick > 165_000);
        }
    }

    #[test]
    fn segment_minutes_match_prd_table() {
        let minutes: Vec<(u64, u64)> = (0_u8..10).map(segment_minutes).collect();
        assert_eq!(
            minutes,
            vec![
                (0, 10),
                (10, 20),
                (20, 30),
                (30, 40),
                (40, 45),
                (45, 55),
                (55, 65),
                (65, 75),
                (75, 85),
                (85, 90),
            ]
        );
    }

    #[test]
    fn sse_message_renders_id_event_data_framing() {
        let message = SseMessage {
            id: 3,
            event: "segment",
            data: serde_json::json!({"segment": 2}),
        };
        assert_eq!(
            message.render(),
            "id: 3\nevent: segment\ndata: {\"segment\":2}\n\n"
        );
    }

    #[test]
    fn decimetres_round_half_away_from_zero() {
        assert_eq!(to_decimetres(1.25), 13);
        assert_eq!(to_decimetres(-1.25), -13);
        assert_eq!(to_decimetres(52.5), 525);
        assert_eq!(to_decimetres(0.0), 0);
    }

    #[test]
    fn error_stream_is_a_single_id_zero_message() {
        let messages = build_error_stream("tick_cap_exceeded", "boom", 77);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, 0);
        assert_eq!(messages[0].event, "error");
        assert_eq!(messages[0].data["last_good_tick"], 77);
    }
}
