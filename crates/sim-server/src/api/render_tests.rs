//! Render-step tests (server-API ticket 03).
//!
//! The segmentation rules, the manifest shape and the build id are
//! pinned here. The segment hashes are golden values: they change
//! whenever the rendered bytes change, so the rendered shape cannot
//! drift unnoticed.

use super::render::{FORMAT_VERSION, SEGMENT_TICKS, RenderedMatch, render_match};
use super::render_doubles::{
    FIRST_WHISTLE, FULL_TIME, SECOND_KICKOFF, committed_fixture, committed_fixtures, fixture,
    full_length_match, hand_built_match,
};
use super::registry::MatchRegistry;

/// The pinned SHA-256 of every segment of the hand-built match.
///
/// Golden values: a change in a rendered segment's bytes changes its
/// hash and fails this test.
const GOLDEN_SEGMENT_HASHES: [&str; 5] = [
    "133125bce85cef609064750df6760227bc8fb90fd01e0fb8e670a1d505e4c0ab",
    "62f80d72e7bbcfeb8cf782dd998bb1c0ff7a13139b1efd9a43295deb61cf71b1",
    "5266343e32ef447605c011880909dbd07ce386c0da951219547ec5727a7dfe80",
    "7ddf4099bf01e8b119c5f14580979461821662e25d023a7276ebdd7e16fb0799",
    "d8ff17f1c41b9fe14034ee20df408b07237965d61e2a1ec552f0ccfaaf409909",
];

/// The event ticks every rendered segment publishes.
fn event_ticks_per_segment(rendered: &RenderedMatch) -> Vec<Vec<u64>> {
    rendered
        .segments
        .iter()
        .map(|segment| {
            let body = segment.decompress().expect("a stored segment decompresses");
            let body: serde_json::Value =
                serde_json::from_slice(&body).expect("a segment body is JSON");
            body["events"]
                .as_array()
                .expect("a segment body lists its events")
                .iter()
                .map(|event| {
                    event["tick"]
                        .as_u64()
                        .expect("a published event carries its tick")
                })
                .collect()
        })
        .collect()
}

/// The frame ticks every rendered segment publishes.
fn frame_ticks_per_segment(rendered: &RenderedMatch) -> Vec<Vec<u64>> {
    rendered
        .segments
        .iter()
        .map(|segment| {
            let body = segment.decompress().expect("a stored segment decompresses");
            let body: serde_json::Value =
                serde_json::from_slice(&body).expect("a segment body is JSON");
            body["frames"]
                .as_array()
                .expect("a segment body lists its frames")
                .iter()
                .map(|frame| {
                    frame["tick"]
                        .as_u64()
                        .expect("a published frame carries its tick")
                })
                .collect()
        })
        .collect()
}

#[test]
fn segments_are_cut_from_each_halves_first_tick_at_sixty_second_boundaries() {
    let rendered = render_match(&fixture(), &hand_built_match());

    let ranges: Vec<(u32, u8, u64, u64)> = rendered
        .manifest
        .segments
        .iter()
        .map(|segment| {
            (
                segment.segment,
                segment.half,
                segment.from_tick,
                segment.to_tick,
            )
        })
        .collect();

    assert_eq!(
        ranges,
        vec![
            (0, 1, 0, 3_599),
            (1, 1, 3_600, 7_199),
            (2, 1, 7_200, FIRST_WHISTLE),
            (3, 2, SECOND_KICKOFF, SECOND_KICKOFF + 3_599),
            (4, 2, SECOND_KICKOFF + 3_600, FULL_TIME),
        ],
        "each half is cut from its own first tick at 60 s boundaries"
    );

    // Every segment but the last one of a half is exactly 60 s wide.
    for segment in &rendered.manifest.segments[..2] {
        assert_eq!(
            segment.to_tick - segment.from_tick + 1,
            SEGMENT_TICKS,
            "segment {} is 60 s wide",
            segment.segment
        );
    }
}

#[test]
fn the_last_segment_of_each_half_runs_to_the_whistle_and_absorbs_the_remainder() {
    let rendered = render_match(&fixture(), &hand_built_match());
    let segments = &rendered.manifest.segments;

    assert_eq!(
        segments[2].to_tick, FIRST_WHISTLE,
        "the first half's last segment runs to the half-time whistle"
    );
    assert_eq!(
        segments[4].to_tick, FULL_TIME,
        "the second half's last segment runs to the full-time whistle"
    );
    assert!(
        segments[2].to_tick - segments[2].from_tick + 1 < SEGMENT_TICKS,
        "the first half's last segment absorbs a remainder shorter than 60 s"
    );
    assert!(
        segments[4].to_tick - segments[4].from_tick + 1 < SEGMENT_TICKS,
        "the second half's last segment absorbs a remainder shorter than 60 s"
    );
}

#[test]
fn no_segment_crosses_a_half_boundary() {
    let rendered = render_match(&fixture(), &hand_built_match());

    for segment in &rendered.manifest.segments {
        if segment.half == 1 {
            assert!(
                segment.to_tick <= FIRST_WHISTLE,
                "a first-half segment must not cross the half-time whistle"
            );
        } else {
            assert!(
                segment.from_tick > FIRST_WHISTLE,
                "a second-half segment must not cross the half-time whistle"
            );
            assert!(
                segment.from_tick >= SECOND_KICKOFF,
                "the second half starts at its own kickoff"
            );
        }
    }
}

#[test]
fn the_segment_count_follows_the_match_including_added_time() {
    let fixture = fixture();

    // A regular half: 45 segments, one per minute.
    let regular = render_match(&fixture, &full_length_match(0));
    assert_eq!(regular.manifest.segments.len(), 90);
    assert_eq!(
        regular
            .manifest
            .segments
            .iter()
            .filter(|segment| segment.half == 1)
            .count(),
        45
    );

    // One added minute per half: one more segment per half.
    let one_added = render_match(&fixture, &full_length_match(60 * 60));
    assert_eq!(one_added.manifest.segments.len(), 92);
    assert_eq!(
        one_added
            .manifest
            .segments
            .iter()
            .filter(|segment| segment.half == 2)
            .count(),
        46
    );

    // The referee gave ten minutes: ten more segments per half.
    let ten_added = render_match(&fixture, &full_length_match(10 * 60 * 60));
    assert_eq!(ten_added.manifest.segments.len(), 110);
    assert_eq!(
        ten_added
            .manifest
            .segments
            .iter()
            .filter(|segment| segment.half == 1)
            .count(),
        55
    );
}

#[test]
fn every_event_appears_in_exactly_one_segment() {
    let completed = hand_built_match();
    let rendered = render_match(&fixture(), &completed);
    let per_segment = event_ticks_per_segment(&rendered);

    let published: Vec<u64> = per_segment.iter().flatten().copied().collect();
    let mut expected: Vec<u64> = completed
        .events
        .iter()
        .map(|event| event.envelope().tick)
        .collect();
    expected.sort_unstable();
    let mut published_sorted = published.clone();
    published_sorted.sort_unstable();

    assert_eq!(
        published_sorted, expected,
        "every event is published exactly once, in the segment covering its tick"
    );
    assert_eq!(
        published.len(),
        expected.len(),
        "no event is published twice"
    );
}

#[test]
fn the_frames_of_the_break_belong_to_no_segment() {
    let rendered = render_match(&fixture(), &hand_built_match());
    let per_segment = frame_ticks_per_segment(&rendered);
    let published: Vec<u64> = per_segment.iter().flatten().copied().collect();

    assert!(
        !published.contains(&(FIRST_WHISTLE + 6)),
        "the half-time break is not match time, so no segment covers it"
    );
    assert!(
        published.contains(&SECOND_KICKOFF),
        "the second half's first frame is published"
    );
}

#[test]
fn the_manifest_lists_the_build_id_format_version_and_every_segment_row() {
    let rendered = render_match(&fixture(), &hand_built_match());
    let manifest = &rendered.manifest;

    assert_eq!(manifest.format_version, FORMAT_VERSION);
    assert_eq!(manifest.build_id.len(), 64, "the build id is a hex digest");
    assert!(
        manifest.build_id.chars().all(|c| c.is_ascii_hexdigit()),
        "the build id is lowercase hex"
    );

    assert_eq!(manifest.segments.len(), rendered.segments.len());
    for (row, segment) in manifest.segments.iter().zip(&rendered.segments) {
        assert_eq!(row.segment, segment.metadata.segment);
        assert_eq!(row.half, segment.metadata.half);
        assert_eq!(row.from_tick, segment.metadata.from_tick);
        assert_eq!(row.to_tick, segment.metadata.to_tick);
        assert_eq!(row.encoding, "gzip");
        assert_eq!(
            row.uncompressed_bytes,
            u64::try_from(segment.decompress().expect("a stored segment decompresses").len())
                .expect("a body length fits in u64")
        );
        assert_eq!(row.sha256.len(), 64);
        assert!(
            row.url.contains(&manifest.build_id),
            "the segment URL embeds the build id: {}",
            row.url
        );
        assert!(
            row.url.ends_with(&format!("/{}.json", row.segment)),
            "the segment URL embeds the segment number: {}",
            row.url
        );
    }
}

#[test]
fn the_manifest_keeps_the_match_metadata_and_carries_the_result() {
    let completed = hand_built_match();
    let rendered = render_match(&fixture(), &completed);
    let manifest = &rendered.manifest;

    assert_eq!(manifest.match_id, "m-test");
    assert_eq!(manifest.seed, 42);
    assert_eq!(manifest.teams, completed.teams);
    assert_eq!(manifest.roster, completed.roster);
    assert_eq!(manifest.pitch.length, 1050);
    assert_eq!(manifest.pitch.width, 680);
    assert_eq!(manifest.pitch.unit, "decimetres");
    assert_eq!(manifest.ticks_per_second, 60);
    assert_eq!(manifest.frame_every_ticks, 6);
    assert_eq!(manifest.attack_direction.home, (1, -1));
    assert_eq!(manifest.attack_direction.away, (-1, 1));

    // The result carries what the old `full_time` message carried.
    assert_eq!(manifest.result.score, (1, 0));
    assert_eq!(
        manifest.result.event_count,
        u64::try_from(completed.events.len()).expect("an event count fits in u64")
    );
    assert_eq!(manifest.result.state_hash, 99);
}

#[test]
fn rendering_the_same_input_twice_is_byte_identical() {
    let fixture = fixture();
    let completed = hand_built_match();

    let first = render_match(&fixture, &completed);
    let second = render_match(&fixture, &completed);

    assert_eq!(first.manifest.build_id, second.manifest.build_id);
    assert_eq!(first.manifest, second.manifest);
    for (left, right) in first.segments.iter().zip(&second.segments) {
        assert_eq!(
            left.gzipped_body, right.gzipped_body,
            "the stored bytes are identical"
        );
        assert_eq!(left.metadata.sha256, right.metadata.sha256);
    }
}

#[test]
fn the_golden_segment_hashes_are_pinned() {
    let rendered = render_match(&fixture(), &hand_built_match());

    let hashes: Vec<&str> = rendered
        .manifest
        .segments
        .iter()
        .map(|segment| segment.sha256.as_str())
        .collect();
    assert_eq!(
        hashes, GOLDEN_SEGMENT_HASHES,
        "the rendered bytes of a segment must not change"
    );
}

#[test]
fn the_build_id_changes_when_a_frame_changes() {    let fixture = fixture();
    let mut completed = hand_built_match();
    let before = render_match(&fixture, &completed);

    // One frame moves: the rendered bytes change, so the build id
    // must change with them.
    completed.frames[0].ball.x += 1;
    let after = render_match(&fixture, &completed);

    assert_ne!(
        before.manifest.build_id, after.manifest.build_id,
        "the build id follows the rendered bytes"
    );
    assert_ne!(
        before.manifest.segments[0].sha256, after.manifest.segments[0].sha256,
        "the changed frame is in the first segment"
    );
    assert_eq!(
        after.manifest.build_id,
        render_match(&fixture, &completed).manifest.build_id,
        "rendering the same input twice yields the same build id"
    );
}

#[test]
fn the_build_id_covers_the_fixture() {
    let completed = hand_built_match();
    let rendered = render_match(&fixture(), &completed);

    // The same match rendered against another fixture: the segments
    // are the same bytes, but the build id covers the fixture too.
    let other_fixture = MatchRegistry::single_test("m-test", 43)
        .fixtures
        .into_values()
        .next()
        .expect("the test registry holds exactly one fixture");
    let other = render_match(&other_fixture, &completed);

    assert_eq!(
        other.manifest.segments[0].sha256, rendered.manifest.segments[0].sha256,
        "the rendered segments do not depend on the fixture"
    );
    assert_ne!(
        other.manifest.build_id, rendered.manifest.build_id,
        "the build id covers the fixture's contents"
    );
}

/// A real match renders one segment per minute of match time.
///
/// The referee gave ten minutes in each half of m-0001, so the match
/// renders 55 segments per half and every one of its events lands in
/// exactly one of them. The test is minutes-long in debug; run it
/// explicitly (release recommended):
///
/// ```text
/// cargo test --release -p sim-server --lib -- --ignored a_real_match_renders
/// ```
#[test]
#[ignore = "full-match render is minutes-long in debug; run explicitly (release recommended)"]
fn a_real_match_renders_one_segment_per_minute() {
    let set = committed_fixtures();
    let fixture = committed_fixture(&set, "m-0001");
    let outcome = super::runner::run_match_blocking(&fixture, 400_000);
    let super::runner::MatchOutcome::Finished(completed) = outcome else {
        panic!("m-0001 must reach full time");
    };

    let rendered = render_match(&fixture, &completed);

    // Ten added minutes per half: 45 + 10 segments per half.
    assert_eq!(rendered.manifest.segments.len(), 110);
    assert_eq!(
        rendered
            .manifest
            .segments
            .iter()
            .filter(|segment| segment.half == 1)
            .count(),
        55
    );
    assert_eq!(
        rendered
            .manifest
            .segments
            .iter()
            .filter(|segment| segment.half == 2)
            .count(),
        55
    );

    // Every event of the match is published exactly once.
    let per_segment = event_ticks_per_segment(&rendered);
    let published: usize = per_segment.iter().map(Vec::len).sum();
    assert_eq!(published, completed.events.len());
}
