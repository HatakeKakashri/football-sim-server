//! Gzip storage tests (server-API ticket 04).
//!
//! Three things are pinned here: the stored body is a gzip stream at
//! the fixed level, the manifest's hash is the hash of the
//! *uncompressed* body, and a segment's identity does not depend on
//! the compressed bytes. The last test is the size report: it plays a
//! real match and prints what the match costs to transfer.

use sha2::{Digest, Sha256};

use super::super::render_doubles::{
    committed_fixture, committed_fixtures, fixture, hand_built_match,
};
use super::digest::hex;
use super::gzip::compress_at_level;
use super::{GZIP_LEVEL, compress, render_match};

/// The transfer budget a full match must fit in (5 MB).
const TRANSFER_BUDGET_BYTES: u64 = 5_000_000;

/// The hex SHA-256 of some bytes.
///
/// The digest is taken with `sha2` directly, not with the render
/// step's own helper, so the check does not lean on the code it
/// checks.
fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The bytes of a hex digest, as the build id hashes them.
fn unhex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("a hex digest is ASCII");
            u8::from_str_radix(text, 16).expect("a hex digest is hexadecimal")
        })
        .collect()
}

/// The number of events a segment body publishes.
fn event_count(body: &[u8]) -> u64 {
    let body: serde_json::Value =
        serde_json::from_slice(body).expect("a segment body is JSON");
    let events = body["events"]
        .as_array()
        .expect("a segment body lists its events");
    u64::try_from(events.len()).expect("an event count fits in u64")
}

/// A size in megabytes, with one decimal.
fn megabytes(bytes: u64) -> String {
    format!(
        "{}.{} MB",
        bytes / 1_000_000,
        (bytes % 1_000_000) / 100_000
    )
}

/// The compressed size in tenths of a percent of the raw size.
fn ratio_permille(raw: u64, gzip: u64) -> u64 {
    gzip.saturating_mul(1_000).checked_div(raw).unwrap_or(0)
}

#[test]
fn a_stored_segment_decompresses_to_its_uncompressed_body() {
    let rendered = render_match(&fixture(), &hand_built_match());

    for segment in &rendered.segments {
        let body = segment
            .decompress()
            .expect("a stored segment decompresses");
        assert_eq!(
            u64::try_from(body.len()).expect("a body length fits in u64"),
            segment.metadata.uncompressed_bytes,
            "the decompressed body has the length the manifest lists"
        );
        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("a segment body is JSON");
        assert_eq!(
            body["segment"], segment.metadata.segment,
            "the decompressed body is the segment the manifest lists"
        );
    }
}

#[test]
fn the_manifest_hash_is_the_hash_of_the_uncompressed_body() {
    let rendered = render_match(&fixture(), &hand_built_match());

    for segment in &rendered.segments {
        let body = segment
            .decompress()
            .expect("a stored segment decompresses");
        assert_eq!(
            sha256_hex(&body),
            segment.metadata.sha256,
            "the manifest hashes the uncompressed body"
        );
    }
}

#[test]
fn the_manifest_lists_the_gzipped_byte_length_of_every_segment() {
    let rendered = render_match(&fixture(), &hand_built_match());

    for segment in &rendered.segments {
        assert_eq!(
            segment.metadata.gzipped_bytes,
            Some(
                u64::try_from(segment.gzipped_bytes().len()).expect("a body length fits in u64")
            ),
            "the manifest lists the length of the stored bytes"
        );
        assert_eq!(segment.metadata.encoding, "gzip");
    }
}

#[test]
fn the_stored_bytes_are_a_gzip_stream_at_the_fixed_level() {
    assert_eq!(GZIP_LEVEL, 6, "the gzip level is fixed and documented");
    let rendered = render_match(&fixture(), &hand_built_match());

    for segment in &rendered.segments {
        assert_eq!(
            &segment.gzipped_bytes()[..2],
            &[0x1f, 0x8b],
            "a gzip stream starts with the magic bytes"
        );
        let body = segment
            .decompress()
            .expect("a stored segment decompresses");
        assert_eq!(
            compress(&body),
            segment.gzipped_bytes(),
            "the stored bytes are gzip at the fixed level"
        );
    }
}

#[test]
fn the_segment_identity_does_not_depend_on_the_compressed_bytes() {
    let fixture = fixture();
    let rendered = render_match(&fixture, &hand_built_match());
    let segment = &rendered.segments[0];
    let body = segment
        .decompress()
        .expect("a stored segment decompresses");

    // The same body at another level compresses to other bytes.
    let other_level = compress_at_level(&body, 9);
    assert_ne!(
        other_level,
        segment.gzipped_bytes(),
        "a different level produces different bytes"
    );

    // The identity does not move with them.
    assert_eq!(
        sha256_hex(&body),
        segment.metadata.sha256,
        "the identity is the hash of the uncompressed body"
    );
    let hashes: Vec<[u8; 32]> = rendered
        .manifest
        .segments
        .iter()
        .map(|row| {
            let bytes = unhex(&row.sha256);
            let mut hash = [0_u8; 32];
            hash.copy_from_slice(&bytes);
            hash
        })
        .collect();
    assert_eq!(
        super::digest::build_id(&fixture, &hashes),
        rendered.manifest.build_id,
        "the build id is derived from the uncompressed hashes alone"
    );
}

#[test]
fn the_stored_bytes_are_shared_between_clones() {
    let rendered = render_match(&fixture(), &hand_built_match());
    let segment = &rendered.segments[0];
    let clone = segment.clone();

    assert!(
        std::sync::Arc::ptr_eq(&segment.gzipped_body, &clone.gzipped_body),
        "a clone shares the stored bytes instead of copying them"
    );
}

/// The real size of a rendered match, measured on m-0001.
///
/// The test prints the raw and the gzipped bytes of every segment,
/// the events each segment carries, and the totals: the first real
/// measurement of the gzipped transfer budget. It is minutes-long in
/// debug; run it explicitly:
///
/// ```text
/// cargo test --release -p sim-server --lib -- --ignored a_real_match_reports --nocapture
/// ```
#[test]
#[ignore = "full-match render is minutes-long in debug; run explicitly (release recommended)"]
fn a_real_match_reports_its_gzipped_size() {
    let set = committed_fixtures();
    let fixture = committed_fixture(&set, "m-0001");
    let outcome = crate::api::runner::run_match_blocking(&fixture, 400_000);
    let crate::api::runner::MatchOutcome::Finished(completed) = outcome else {
        panic!("m-0001 must reach full time");
    };

    let rendered = render_match(&fixture, &completed);

    println!("segment  half  from_tick     to_tick     raw_bytes  gzip_bytes  ratio  events");
    let mut raw_total = 0_u64;
    let mut gzip_total = 0_u64;
    let mut events_total = 0_u64;
    for segment in &rendered.segments {
        let body = segment
            .decompress()
            .expect("a stored segment decompresses");
        let events = event_count(&body);
        let raw = u64::try_from(body.len()).expect("a body length fits in u64");
        let gzip = segment
            .metadata
            .gzipped_bytes
            .expect("the gzipped length is populated");
        raw_total += raw;
        gzip_total += gzip;
        events_total += events;
        let ratio = ratio_permille(raw, gzip);
        println!(
            "{:>7}  {:>4}  {:>10}  {:>10}  {:>10}  {:>11}  {:>3}.{}%  {:>6}",
            segment.metadata.segment,
            segment.metadata.half,
            segment.metadata.from_tick,
            segment.metadata.to_tick,
            raw,
            gzip,
            ratio / 10,
            ratio % 10,
            events
        );
    }

    let listed: u64 = rendered
        .manifest
        .segments
        .iter()
        .map(|row| row.gzipped_bytes.expect("the gzipped length is populated"))
        .sum();
    assert_eq!(
        listed, gzip_total,
        "the manifest's lengths add up to the stored bytes"
    );
    assert_eq!(
        events_total,
        u64::try_from(completed.events.len()).expect("an event count fits in u64"),
        "every event of the match is published exactly once"
    );
    assert!(
        gzip_total < raw_total,
        "the stored bytes are smaller than the uncompressed ones"
    );

    let ratio = ratio_permille(raw_total, gzip_total);
    println!();
    println!("segments: {}", rendered.segments.len());
    println!("raw total:  {raw_total} bytes ({})", megabytes(raw_total));
    println!("gzip total: {gzip_total} bytes ({})", megabytes(gzip_total));
    println!("ratio: {}.{}%", ratio / 10, ratio % 10);
    println!("events: {events_total}");
    println!(
        "budget: {TRANSFER_BUDGET_BYTES} bytes (5 MB) — {}",
        if gzip_total <= TRANSFER_BUDGET_BYTES {
            "within budget"
        } else {
            "OVER BUDGET"
        }
    );
}
