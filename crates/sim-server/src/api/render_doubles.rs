//! The hand-built match the render tests render (ticket 03).
//!
//! The doubles are built by hand rather than simulated, so the
//! segmentation's boundary cases — the segment edges, the half-time
//! whistle, the break and the full-time whistle — land on exact,
//! readable ticks.

use sim_components::time::HALF_LENGTH_TICKS;
use sim_components::{KickoffReason, MatchClock, PlayerId, TeamId};
use sim_core::{
    MatchEvent, build_full_time_event, build_half_time_event, build_kickoff_event, build_pass_event,
};
use sim_math::Vec2;

use super::registry::{MatchFixture, MatchRegistry};
use super::stream::{
    CompletedMatch, FrameBall, FramePlayer, HalfBoundaries, ManifestTeam, RosterEntry, StreamFrame,
};

/// Ticks between the first half's whistle and the second half's kickoff.
pub const BREAK_TICKS: u64 = 18;

/// The first half's whistle in the hand-built match.
pub const FIRST_WHISTLE: u64 = 7_201;

/// The second half's kickoff in the hand-built match.
pub const SECOND_KICKOFF: u64 = FIRST_WHISTLE + BREAK_TICKS;

/// The full-time whistle in the hand-built match.
pub const FULL_TIME: u64 = SECOND_KICKOFF + 5_401;

/// The fixture the hand-built match belongs to.
pub fn fixture() -> MatchFixture {
    MatchRegistry::single_test("m-test", 42)
        .fixtures
        .into_values()
        .next()
        .expect("the test registry holds exactly one fixture")
}

/// A running clock at a point in a half.
fn clock(half: u8, elapsed_ticks: u64, added_time_ticks: u64) -> MatchClock {
    MatchClock {
        elapsed_ticks,
        half,
        added_time_ticks,
        is_running: true,
    }
}

/// A pass event at a tick.
fn pass(seq: u64, tick: u64, clock: &MatchClock) -> MatchEvent {
    build_pass_event(
        seq,
        tick,
        clock,
        TeamId(0),
        PlayerId::from_bits(1),
        Some(PlayerId::from_bits(2)),
        true,
        tick,
        Vec2::new(10.0, 20.0),
        Vec2::new(30.0, 40.0),
    )
}

/// One animation frame at a tick.
fn frame(tick: u64, half: u8) -> StreamFrame {
    StreamFrame {
        tick,
        half,
        ball: FrameBall {
            x: 525,
            y: 340,
            state: "free".to_string(),
            possessor: None,
        },
        players: vec![
            FramePlayer {
                id: "test-home-p00".to_string(),
                x: 100,
                y: 200,
            },
            FramePlayer {
                id: "test-away-p00".to_string(),
                x: 900,
                y: 400,
            },
        ],
    }
}

/// The two roster rows the hand-built match publishes.
fn roster() -> Vec<RosterEntry> {
    vec![
        RosterEntry {
            id: "test-home-p00".to_string(),
            team: "home".to_string(),
            name: "Home Keeper".to_string(),
            number: 1,
            position: "GK".to_string(),
        },
        RosterEntry {
            id: "test-away-p00".to_string(),
            team: "away".to_string(),
            name: "Away Keeper".to_string(),
            number: 1,
            position: "GK".to_string(),
        },
    ]
}

/// The two lineup setups the hand-built match publishes.
fn teams() -> Vec<ManifestTeam> {
    vec![
        ManifestTeam {
            id: "home".to_string(),
            formation: "4-4-2".to_string(),
            mentality: "balance".to_string(),
            tactic: "possession".to_string(),
        },
        ManifestTeam {
            id: "away".to_string(),
            formation: "4-3-3".to_string(),
            mentality: "attack".to_string(),
            tactic: "counter_attack".to_string(),
        },
    ]
}

/// A hand-built completed match that hits the boundary cases.
///
/// The first half runs from tick 0 to its whistle at 7,201, so its
/// three segments are `[0, 3599]`, `[3600, 7199]` and `[7200, 7201]`
/// — the last one absorbs the two-tick remainder. The break lasts 18
/// ticks and the second half starts at its own kickoff; it runs to the
/// full-time whistle in two segments, the last one absorbing the
/// remainder.
pub fn hand_built_match() -> CompletedMatch {
    let first_clock = clock(1, FIRST_WHISTLE, 0);
    let second_clock = clock(2, FULL_TIME - SECOND_KICKOFF, 0);
    let events = vec![
        build_kickoff_event(
            0,
            0,
            &clock(1, 0, 0),
            TeamId(0),
            None,
            KickoffReason::MatchStart,
            Some(1),
            Some(-1),
        ),
        pass(1, 3_599, &clock(1, 3_599, 0)),
        pass(2, 3_600, &clock(1, 3_600, 0)),
        build_half_time_event(3, FIRST_WHISTLE, &first_clock, (1, 0)),
        build_kickoff_event(
            4,
            SECOND_KICKOFF,
            &clock(2, 0, 0),
            TeamId(1),
            None,
            KickoffReason::SecondHalf,
            Some(-1),
            Some(1),
        ),
        pass(5, FULL_TIME, &second_clock),
        build_full_time_event(6, FULL_TIME, &second_clock, (1, 0), 7, 99, (0, 0)),
    ];
    let frames = vec![
        frame(6, 1),
        frame(3_600, 1),
        // Inside the half-time break: not match time, so no segment
        // covers it.
        frame(FIRST_WHISTLE + 6, 1),
        frame(SECOND_KICKOFF, 2),
        frame(FULL_TIME, 2),
    ];
    CompletedMatch {
        match_id: "m-test".to_string(),
        seed: 42,
        events,
        frames,
        roster: roster(),
        teams: teams(),
        score: (1, 0),
        state_hash: 99,
        added_time_ticks: (0, 0),
        boundaries: HalfBoundaries {
            first_half_end: FIRST_WHISTLE,
            full_time: FULL_TIME,
        },
        attack_home: (1, -1),
        attack_away: (-1, 1),
    }
}

/// A full-length match with the given added time per half.
///
/// No frames and no events except the lifecycle ones: the test only
/// needs the half lengths.
pub fn full_length_match(added_time_ticks: u64) -> CompletedMatch {
    let first_whistle = HALF_LENGTH_TICKS + added_time_ticks;
    let second_kickoff = first_whistle + BREAK_TICKS;
    let full_time = second_kickoff + HALF_LENGTH_TICKS + added_time_ticks;
    let events = vec![
        build_kickoff_event(
            0,
            0,
            &clock(1, 0, 0),
            TeamId(0),
            None,
            KickoffReason::MatchStart,
            Some(1),
            Some(-1),
        ),
        build_half_time_event(
            1,
            first_whistle,
            &clock(1, first_whistle, added_time_ticks),
            (0, 0),
        ),
        build_kickoff_event(
            2,
            second_kickoff,
            &clock(2, 0, 0),
            TeamId(1),
            None,
            KickoffReason::SecondHalf,
            Some(-1),
            Some(1),
        ),
        build_full_time_event(
            3,
            full_time,
            &clock(2, HALF_LENGTH_TICKS + added_time_ticks, added_time_ticks),
            (0, 0),
            4,
            1,
            (added_time_ticks, added_time_ticks),
        ),
    ];
    CompletedMatch {
        match_id: "m-test".to_string(),
        seed: 42,
        events,
        frames: Vec::new(),
        roster: roster(),
        teams: teams(),
        score: (0, 0),
        state_hash: 1,
        added_time_ticks: (added_time_ticks, added_time_ticks),
        boundaries: HalfBoundaries {
            first_half_end: first_whistle,
            full_time,
        },
        attack_home: (1, -1),
        attack_away: (-1, 1),
    }
}
