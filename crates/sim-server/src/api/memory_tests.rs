//! The memory a running match holds (server-API ticket 05).
//!
//! The runner captures a frame every six ticks, so a match holds tens
//! of thousands of frames at once. Before this ticket every frame
//! named its 22 players by their fixture id — a fresh string per
//! player per frame, 1,188,066 string allocations and about 145 MB of
//! resident memory for one match. A frame now names its players by
//! their roster row, so the ids live once, in the roster the manifest
//! publishes, and a frame costs a row per player.
//!
//! # Measuring
//!
//! The measurement reads the process's resident set size from
//! `/proc/self/statm` (the second field, in pages) before and after
//! capturing a batch of frames, and divides the difference by the
//! number of frames. The batch is captured from one world, so every
//! frame is the same size and the difference is the frames' resident
//! memory.
//!
//! ```text
//! cargo test --release -p sim-server --lib -- --ignored the_resident_memory --nocapture
//! ```
//!
//! Measured on m-0001, release build, 20,000 frames: 1,641 bytes per
//! frame before this ticket, 365 bytes after — 88.6 MB against 19.7 MB
//! for a match's frames. The whole running match, frames and event log
//! together, went from 143 MB resident to 62 MB.
//!
//! The measurement is a report, not a gate: the resident set size
//! depends on the allocator, so the number moves a little between
//! runs. The gate on the frame's shape is
//! `api::tests::a_captured_frame_holds_no_player_id_string`.

use sim_core::Simulation;

use super::model::MatchFrame;
use super::render_doubles::{committed_fixture, committed_fixtures};
use super::runner::{MatchOutcome, build_roster, capture_frame, run_match_blocking};

/// The number of frames the measurement captures.
const MEASURED_FRAMES: usize = 20_000;

/// The frames a full match captures: one every six ticks of 324,018.
const MATCH_FRAMES: u64 = 54_003;

/// The size of a memory page in bytes (Linux, x86-64).
const PAGE_SIZE: u64 = 4096;

/// The process's resident set size in bytes.
///
/// `None` when the platform does not publish it.
fn resident_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let resident_pages = statm.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    Some(resident_pages * PAGE_SIZE)
}

/// The resident memory one captured frame holds.
#[test]
#[ignore = "memory measurement; run explicitly (release recommended)"]
fn the_resident_memory_of_a_captured_frame_is_measured() {
    let set = committed_fixtures();
    let fixture = committed_fixture(&set, "m-0001");
    let sim = Simulation::new_from_fixture(
        &fixture.validated_match,
        &fixture.home_team,
        &fixture.away_team,
    );
    let roster = build_roster(&sim, &fixture);

    let before = resident_bytes().expect("the resident set size must be readable");
    let frames: Vec<MatchFrame> = (0..MEASURED_FRAMES)
        .map(|_| capture_frame(&sim, &roster).expect("invariant: the frame fits in memory"))
        .collect();
    let after = resident_bytes().expect("the resident set size must be readable");

    let measured = u64::try_from(MEASURED_FRAMES).expect("invariant: the frame count fits in u64");
    let total = after.saturating_sub(before);
    let per_frame = total / measured;
    let per_match = per_frame * MATCH_FRAMES;
    println!(
        "resident memory per captured frame: {per_frame} bytes \
         ({measured} frames, {total} bytes in total, {per_match} bytes for a match)"
    );
    assert_eq!(
        frames.len(),
        MEASURED_FRAMES,
        "the measurement must capture every frame it counts"
    );
}

/// The resident memory one running match holds.
///
/// The measurement runs a whole match through the runner and reads the
/// resident set size before and after it, so the number is the match's
/// own footprint: its frames, its event log and the world it ran in.
///
/// ```text
/// cargo test --release -p sim-server --lib -- --ignored the_resident_memory_of_a_running_match --nocapture
/// ```
#[test]
#[ignore = "memory measurement; run explicitly (release recommended)"]
fn the_resident_memory_of_a_running_match_is_measured() {
    let set = committed_fixtures();
    let fixture = committed_fixture(&set, "m-0001");

    let before = resident_bytes().expect("the resident set size must be readable");
    let outcome = run_match_blocking(&fixture, 400_000);
    let after = resident_bytes().expect("the resident set size must be readable");

    let MatchOutcome::Finished(completed) = outcome else {
        panic!("m-0001 must reach full time");
    };
    let frames = u64::try_from(completed.frames.len()).expect("invariant: a frame count fits in u64");
    let total = after.saturating_sub(before);
    println!(
        "resident memory of a running match: {total} bytes \
         ({frames} frames, {} bytes per frame)",
        total / frames.max(1)
    );
    assert!(
        frames > MATCH_FRAMES / 2,
        "a full match captures a frame every six ticks"
    );
}
