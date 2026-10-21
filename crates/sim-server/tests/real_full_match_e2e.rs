//! Real full-match end-to-end through the HTTP API (server-API ticket 12).
//!
//! The test drives a full `m-0001` match the way a browser client will:
//! `POST` starts it, the manifest is polled at the `Retry-After` pace until
//! it is ready, every listed segment is fetched with gzip, decoded, and
//! checked against the manifest's SHA-256. The run is repeated in a second
//! server process, and both runs must agree on the build id, every segment
//! hash and the final state hash.
//!
//! The server process is the real `football-sim serve` command, so the app
//! under test is built by [`sim_server::app::match_service`] — the same
//! constructor production uses — with the committed fixtures, the real
//! simulation runner and the new router. Nothing is assembled for the test.
//!
//! The test also writes the fetched manifest and first segment to
//! `docs/api-samples/` so the frontend can build against real payloads.
//!
//! Full-match tests stay out of the default suite. Run it in release:
//!
//! ```text
//! cargo test --release -p sim-server --test real_full_match_e2e -- --ignored --nocapture
//! ```

#[path = "real_full_match_e2e/support.rs"]
mod support;

use std::io::Read;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use flate2::read::GzDecoder;
use serde_json::Value;
use sha2::{Digest, Sha256};

use support::{HttpResponse, ServerProcess, free_local_port, repository_root, request};

/// The fixture the real-match test drives.
const MATCH_ID: &str = "m-0001";

/// The transfer budget the measured gzipped bytes are reported against.
const TRANSFER_BUDGET_BYTES: u64 = 5_000_000;

/// The target for receiving the manifest plus the first segment after the
/// match is ready.
const FIRST_FRAME_TARGET: Duration = Duration::from_secs(1);

/// How long the start-and-poll phase may take before the test gives up.
const START_TIMEOUT: Duration = Duration::from_mins(4);

/// Where the frontend-facing sample payloads are written.
const SAMPLE_DIR: &str = "docs/api-samples";

/// One client run against one server process.
struct RunReport {
    /// Which run this is, for the printed report.
    label: &'static str,
    /// The manifest response body exactly as the client received it.
    manifest_body: Vec<u8>,
    /// The decoded body of the first segment.
    first_segment_body: Vec<u8>,
    /// The manifest's content-derived build id.
    build_id: String,
    /// The manifest's segment hashes, in order.
    segment_hashes: Vec<String>,
    /// The deterministic state hash at full time.
    state_hash: u64,
    /// The actual gzipped bytes transferred for every segment.
    gzipped_bytes: u64,
    /// The gzipped bytes the manifest lists.
    listed_gzipped_bytes: u64,
    /// Time from starting the match to the last segment being received.
    elapsed: Duration,
    /// Time from the request that first saw `ready` to the first segment
    /// being decoded locally.
    first_frame_elapsed: Duration,
    /// How many segments the manifest listed.
    segment_count: usize,
}

#[test]
#[ignore = "real full-match over HTTP is minutes-long in debug; run with --release"]
fn a_real_full_match_is_served_segment_by_segment() {
    let root = repository_root(MATCH_ID);
    let first = run_once(&root, "run 1 of 2 (separate process)");
    let second = run_once(&root, "run 2 of 2 (separate process)");

    assert_eq!(
        first.build_id, second.build_id,
        "two server processes must publish the same build id"
    );
    assert_eq!(
        first.segment_hashes, second.segment_hashes,
        "two server processes must publish the same segment hashes"
    );
    assert_eq!(
        first.state_hash, second.state_hash,
        "two server processes must reach the same final state hash"
    );
    assert_eq!(
        first.manifest_body, second.manifest_body,
        "two server processes must answer the same manifest bytes"
    );
    assert_eq!(
        first.first_segment_body, second.first_segment_body,
        "two server processes must answer the same first segment bytes"
    );
    assert_eq!(
        first.gzipped_bytes, second.gzipped_bytes,
        "two server processes must transfer the same gzipped byte count"
    );

    println!();
    println!(
        "[ticket 12] determinism: build_id={} segments={} state_hash={}",
        first.build_id, first.segment_count, first.state_hash
    );
    println!("[ticket 12] both processes produced identical payloads and hashes");
}

/// Run one full match against a freshly spawned `football-sim serve`.
fn run_once(root: &Path, label: &'static str) -> RunReport {
    let port = free_local_port();
    let mut server = ServerProcess::start(root, port);
    let started = Instant::now();

    let start = request(port, "POST", &format!("/match/{MATCH_ID}/start"), false)
        .expect("the start request reaches the server");
    assert_eq!(
        start.status, 202,
        "starting {MATCH_ID} must be accepted: {}",
        start.text()
    );

    let (manifest_started_at, manifest_body) = poll_manifest(port, started);
    let manifest: Value = serde_json::from_slice(&manifest_body)
        .expect("invariant: the manifest response is JSON");
    assert_eq!(
        manifest["state"].as_str(),
        Some("ready"),
        "the manifest response carries the ready state"
    );
    let build_id = string_field(&manifest, "build_id").to_string();
    let state_hash = manifest["result"]["state_hash"]
        .as_u64()
        .expect("invariant: the manifest result carries a state hash");
    let segments = manifest["segments"]
        .as_array()
        .expect("invariant: the manifest carries a segment list");
    assert!(
        !segments.is_empty(),
        "a ready match must list at least one segment"
    );

    let segment_hashes: Vec<String> = segments
        .iter()
        .map(|row| string_field(row, "sha256").to_string())
        .collect();

    let first_row = segments.first().expect("the segment list is not empty");
    let first_response = request(
        port,
        "GET",
        string_field(first_row, "url"),
        true,
    )
    .expect("the first segment reaches the client");
    let first_segment_body = verify_segment(&first_response, first_row);
    let first_frame_elapsed = manifest_started_at.elapsed();
    assert!(
        first_frame_elapsed <= FIRST_FRAME_TARGET,
        "ready -> manifest + first segment took {first_frame_elapsed:?}, target {FIRST_FRAME_TARGET:?}"
    );

    let mut gzipped_bytes = byte_len(&first_response.body);
    for row in segments.iter().skip(1) {
        let response = request(port, "GET", string_field(row, "url"), true)
            .expect("every listed segment reaches the client");
        let _ = verify_segment(&response, row);
        gzipped_bytes += byte_len(&response.body);
    }

    let listed_gzipped_bytes: u64 = segments
        .iter()
        .map(|row| {
            row["gzipped_bytes"]
                .as_u64()
                .expect("invariant: every segment carries its gzipped length")
        })
        .sum();
    assert_eq!(
        gzipped_bytes, listed_gzipped_bytes,
        "the transferred bytes must equal the manifest's listed gzipped bytes"
    );

    let elapsed = started.elapsed();
    write_samples(root, &manifest_body, &first_segment_body);

    let report = RunReport {
        label,
        manifest_body,
        first_segment_body,
        build_id,
        segment_hashes,
        state_hash,
        gzipped_bytes,
        listed_gzipped_bytes,
        elapsed,
        first_frame_elapsed,
        segment_count: segments.len(),
    };
    print_report(&report);

    // Keep the server alive until the whole run is done, then shut it
    // down through `Drop` even if an assertion above panicked.
    server.stop();
    report
}

/// Poll `GET /manifest` at the server's `Retry-After` pace until ready.
///
/// Returns the instant the request that first saw `200` started, plus the
/// manifest body. The instant lets the caller measure the path from ready
/// to the manifest and first segment being received.
fn poll_manifest(port: u16, deadline_started: Instant) -> (Instant, Vec<u8>) {
    loop {
        assert!(
            deadline_started.elapsed() < START_TIMEOUT,
            "the match did not become ready within {START_TIMEOUT:?}"
        );
        let request_started = Instant::now();
        let response = request(port, "GET", &format!("/match/{MATCH_ID}/manifest"), false)
            .expect("the manifest poll reaches the server");
        match response.status {
            200 => return (request_started, response.body),
            202 => {
                let wait = retry_after(&response);
                println!(
                    "[ticket 12] {} still preparing ({}), waiting {} ms",
                    MATCH_ID,
                    response.text(),
                    wait.as_millis()
                );
                thread::sleep(wait);
            }
            other => panic!(
                "the manifest poll must be 202 or 200, got {other}: {}",
                response.text()
            ),
        }
    }
}

/// The server's `Retry-After` header as a wait.
fn retry_after(response: &HttpResponse) -> Duration {
    let seconds: u64 = response
        .header("retry-after")
        .expect("a 202 carries Retry-After")
        .parse()
        .expect("Retry-After is a whole number of seconds");
    Duration::from_secs(seconds)
}

/// Verify one segment against its manifest row and return the decoded body.
fn verify_segment(response: &HttpResponse, row: &Value) -> Vec<u8> {
    assert_eq!(
        response.status, 200,
        "segment {} must be served: {}",
        row["segment"], response.text()
    );
    assert_eq!(
        response.header("content-encoding"),
        Some("gzip"),
        "a gzip client receives the stored gzip bytes"
    );
    assert_eq!(
        byte_len(&response.body),
        row["gzipped_bytes"]
            .as_u64()
            .expect("invariant: the manifest lists the gzipped length"),
        "the transferred bytes are the length the manifest lists"
    );

    let decoded = gunzip(&response.body);
    assert_eq!(
        byte_len(&decoded),
        row["uncompressed_bytes"]
            .as_u64()
            .expect("invariant: the manifest lists the uncompressed length"),
        "the decoded bytes are the length the manifest lists"
    );
    assert_eq!(
        sha256_hex(&decoded),
        string_field(row, "sha256"),
        "segment {} decodes to the manifest's SHA-256",
        row["segment"]
    );
    // The body is JSON a frontend parses; a malformed sample fails here.
    let body: Value = serde_json::from_slice(&decoded)
        .expect("invariant: a decoded segment body is JSON");
    assert_eq!(
        body["segment"], row["segment"],
        "the body names the segment the manifest listed"
    );
    decoded
}

/// Print the report the ticket asks for: bytes against the budget and the
/// ready-to-first-frame time against the one-second target.
fn print_report(report: &RunReport) {
    let label = report.label;
    let gzipped_bytes = report.gzipped_bytes;
    let budget = if gzipped_bytes <= TRANSFER_BUDGET_BYTES {
        "within budget"
    } else {
        "OVER budget"
    };
    let over_under = gzipped_bytes.abs_diff(TRANSFER_BUDGET_BYTES);
    println!();
    println!(
        "[ticket 12] {label}: {} segments, {gzipped_bytes} gzipped bytes ({:.2} MiB) in {:.2} s",
        report.segment_count,
        mebibytes(gzipped_bytes),
        report.elapsed.as_secs_f64()
    );
    println!(
        "[ticket 12] {label}: manifest lists {} gzipped bytes; {budget} by {over_under} bytes against {TRANSFER_BUDGET_BYTES} (5 MB)",
        report.listed_gzipped_bytes
    );
    println!(
        "[ticket 12] {label}: ready -> manifest + first segment in {:.2} ms (target {} ms) — {}",
        report.first_frame_elapsed.as_secs_f64() * 1_000.0,
        FIRST_FRAME_TARGET.as_millis(),
        if report.first_frame_elapsed <= FIRST_FRAME_TARGET {
            "PASS"
        } else {
            "FAIL"
        }
    );
    println!(
        "[ticket 12] {label}: build_id={} state_hash={}",
        report.build_id, report.state_hash
    );
}

/// Write the fetched manifest and first segment as frontend samples.
fn write_samples(root: &Path, manifest_body: &[u8], first_segment_body: &[u8]) {
    let directory = root.join(SAMPLE_DIR);
    std::fs::create_dir_all(&directory).expect("the sample directory is writable");
    write_payload(
        &directory.join(format!("{MATCH_ID}-manifest.json")),
        manifest_body,
    );
    write_payload(
        &directory.join(format!("{MATCH_ID}-segment-0.json")),
        first_segment_body,
    );
}

/// Write a fetched payload exactly as the client received it, with a
/// trailing newline so the sample is a well-formed text file.
fn write_payload(path: &Path, body: &[u8]) {
    let mut payload = body.to_vec();
    if !payload.ends_with(b"\n") {
        payload.push(b'\n');
    }
    std::fs::write(path, payload)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
}

/// A field the manifest promises to carry.
fn string_field<'a>(value: &'a Value, field: &str) -> &'a str {
    value[field]
        .as_str()
        .unwrap_or_else(|| panic!("invariant: `{field}` is a string"))
}

/// Decode a gzip body.
fn gunzip(body: &[u8]) -> Vec<u8> {
    let mut decoder = GzDecoder::new(body);
    let mut uncompressed = Vec::new();
    decoder
        .read_to_end(&mut uncompressed)
        .expect("invariant: the server's segment body is a valid gzip stream");
    uncompressed
}

/// SHA-256 as lowercase hex, independent of the render step's own helper.
fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(hex, "{byte:02x}").expect("invariant: writing to a String never fails");
    }
    hex
}

/// A length as `u64`.
fn byte_len(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.len()).expect("invariant: a response body fits in u64")
}

/// Bytes as MiB, for the printed report only.
#[expect(
    clippy::cast_precision_loss,
    reason = "display-only cast; a match's transfer size stays far below 2^53"
)]
fn mebibytes(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}
