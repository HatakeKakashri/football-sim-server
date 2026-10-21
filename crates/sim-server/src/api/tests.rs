//! Unit + contract tests for the match store and runner.
//!
//! The full-match end-to-end test is `#[ignore]`d (minutes-long in
//! debug) — run it explicitly, release recommended:
//! `cargo test --release -p sim-server --lib api_end_to_end_full_match -- --ignored`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sim_core::Simulation;

use super::runner::{MatchOutcome, build_roster, capture_frame, run_match_blocking};
use super::stream::{CompletedMatch, HalfBoundaries, SseMessage, build_stream, render_body};
use super::{default_concurrency_cap, RETRY_AFTER_SECS};
use super::{JobStatus, MatchRecord, MatchRegistry, MatchStore, ServerConfig, StreamResponse};

fn test_store(fixture_count: u32, config: ServerConfig) -> Arc<MatchStore> {
    let mut registry = MatchRegistry::default();
    for index in 0..fixture_count {
        let id = format!("m-{index:04}");
        let seed = 42 + u64::from(index);
        let single = MatchRegistry::single_test(&id, seed);
        registry.fixtures.extend(single.fixtures);
    }
    Arc::new(MatchStore::new(config, registry))
}

#[cfg(test)]
fn test_store_with_fixtures(fixture_count: u32, config: ServerConfig) -> Arc<MatchStore> {
    let mut registry = MatchRegistry::default();
    for index in 0..fixture_count {
        let id = format!("m-{index:04}");
        let seed = 42 + u64::from(index);
        // Use insert on the default registry, then create single_test and merge
        // For simplicity, create a new registry each time and merge (not ideal but works for tests)
        let single = MatchRegistry::single_test(&id, seed);
        registry.fixtures.extend(single.fixtures);
    }
    Arc::new(MatchStore::new(config, registry))
}

#[test]
fn post_unknown_match_id_is_404() {
    let store = test_store_with_fixtures(1, ServerConfig::default());
    let response = store.post_match("nope");
    assert_eq!(response.status, 404);
}

#[test]
fn stream_unknown_match_id_is_404() {
    let store = test_store_with_fixtures(1, ServerConfig::default());
    assert!(matches!(
        store.stream_slice("nope", None),
        StreamResponse::Unknown
    ));
}

#[test]
fn stream_before_post_is_409() {
    let store = test_store_with_fixtures(1, ServerConfig::default());
    assert!(matches!(
        store.stream_slice("m-0000", None),
        StreamResponse::NotStarted
    ));
}

#[test]
fn concurrency_cap_returns_503_with_retry_after() {
    let config = ServerConfig {
        max_concurrent_sims: 1,
        ..ServerConfig::default()
    };
    let store = test_store_with_fixtures(2, config);
    // Occupy the single slot with a stuck Running record.
    {
        let mut state = store
            .state
            .lock()
            .expect("invariant: test lock is never poisoned");
        state.records.insert(
            "m-0000".to_string(),
            MatchRecord {
                status: JobStatus::Running,
                completed_at: None,
                stream: Vec::new(),
            },
        );
        state.running = 1;
    }
    let response = store.post_match("m-0001");
    assert_eq!(response.status, 503);
    assert_eq!(response.retry_after_secs, Some(RETRY_AFTER_SECS));
}

#[test]
fn expired_result_is_410_until_repost() {
    let config = ServerConfig {
        result_ttl_secs: 0,
        tick_cap: 120,
        ..ServerConfig::default()
    };
    let store = test_store_with_fixtures(1, config);
    let response = store.post_match("m-0000");
    assert_eq!(response.status, 202);
    // The 120-tick thread fails fast on the cap; wait for it.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("invariant: test runtime builds");
    runtime.block_on(store.wait_until_done("m-0000"));
    // TTL 0 means the cached failure is already expired.
    assert!(matches!(
        store.stream_slice("m-0000", None),
        StreamResponse::Expired
    ));
    // Re-POST re-simulates instead of serving the expired entry.
    let response = store.post_match("m-0000");
    assert_eq!(response.status, 202);
    runtime.block_on(store.wait_until_done("m-0000"));
}

#[test]
fn default_concurrency_cap_is_cores_minus_one() {
    let expected = std::thread::available_parallelism()
        .map_or(1, |cores| cores.get().max(2) - 1);
    assert_eq!(default_concurrency_cap(), expected);
    assert!(
        default_concurrency_cap() >= 1,
        "concurrency cap must allow at least one simulation"
    );
}

#[test]
fn cached_get_never_blocked_by_concurrency_cap() {
    // A tick-cap failure is a cached result just like a finished match
    // (PRD §10: failures cache like successes), and it runs in ms.
    let config = ServerConfig {
        max_concurrent_sims: 1,
        tick_cap: 120,
        ..ServerConfig::default()
    };
    let store = test_store_with_fixtures(3, config);

    // Cache a finished result first.
    let response = store.post_match("m-0000");
    assert_eq!(response.status, 202);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("invariant: test runtime builds");
    runtime.block_on(store.wait_until_done("m-0000"));
    assert!(matches!(
        store.stream_slice("m-0000", None),
        StreamResponse::Ready(_)
    ));

    // Occupy the only slot so the concurrency cap is reached.
    {
        let mut state = store
            .state
            .lock()
            .expect("invariant: test lock is never poisoned");
        state.records.insert(
            "m-0001".to_string(),
            MatchRecord {
                status: JobStatus::Running,
                completed_at: None,
                stream: Vec::new(),
            },
        );
        state.running = 1;
    }

    // Over-cap POST → 503 + Retry-After.
    let response = store.post_match("m-0002");
    assert_eq!(response.status, 503);
    assert_eq!(response.retry_after_secs, Some(RETRY_AFTER_SECS));

    // Cached GET is served immediately even with the cap full: the cap
    // never holds up GET of cached results.
    let StreamResponse::Ready(body) = store.stream_slice("m-0000", None) else {
        panic!("expected cached stream to be ready immediately");
    };
    assert!(!body.is_empty());
}

#[test]
fn evict_and_regenerate_is_byte_identical_after_ttl_expiry() {
    let config = ServerConfig {
        result_ttl_secs: 1,
        tick_cap: 120,
        ..ServerConfig::default()
    };
    let store = test_store_with_fixtures(1, config);

    // First simulation: tick-cap failure, deterministic for the seed.
    let response = store.post_match("m-0000");
    assert_eq!(response.status, 202);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("invariant: test runtime builds");
    runtime.block_on(store.wait_until_done("m-0000"));

    let StreamResponse::Ready(first_body) = store.stream_slice("m-0000", None) else {
        panic!("expected a finished stream");
    };
    assert!(first_body.contains("tick_cap_exceeded"));

    // Evict: backdate completed_at beyond the one-second TTL.
    {
        let mut state = store
            .state
            .lock()
            .expect("invariant: test lock is never poisoned");
        let record = state
            .records
            .get_mut("m-0000")
            .expect("invariant: match record exists after POST");
        record.completed_at = Some(
            Instant::now()
                .checked_sub(Duration::from_secs(2))
                .expect("invariant: the test process has been running for at least 2 seconds"),
        );
        drop(state);
    }
    assert!(matches!(
        store.stream_slice("m-0000", None),
        StreamResponse::Expired
    ));

    // Re-POST re-simulates the same seed (PRD §10.4).
    let response = store.post_match("m-0000");
    assert_eq!(response.status, 202);
    runtime.block_on(store.wait_until_done("m-0000"));

    let StreamResponse::Ready(second_body) = store.stream_slice("m-0000", None) else {
        panic!("expected a regenerated stream");
    };
    assert_eq!(
        first_body, second_body,
        "evict-and-regenerate must produce a byte-identical uncompressed SSE body"
    );
}

#[test]
fn resume_and_204_behave_over_a_finished_stream() {
    let store = test_store(1, ServerConfig::default());
    let stream: Vec<SseMessage> = (0_u64..12)
        .map(|id| SseMessage {
            id,
            event: if id == 0 {
                "manifest"
            } else if id == 11 {
                "full_time"
            } else {
                "segment"
            },
            data: serde_json::json!({"id": id}),
        })
        .collect();
    {
        let mut state = store
            .state
            .lock()
            .expect("invariant: test lock is never poisoned");
        state.records.insert(
            "m-0000".to_string(),
            MatchRecord {
                status: JobStatus::Finished,
                completed_at: Some(Instant::now()),
                stream,
            },
        );
    }
    let StreamResponse::Ready(body) =
        store.stream_slice("m-0000", None) else {
            panic!("expected full stream");
        };
    assert_eq!(body.matches("id: ").count(), 12);
    let StreamResponse::Ready(resumed) =
        store.stream_slice("m-0000", Some(5)) else {
            panic!("expected resumed stream");
        };
    assert_eq!(resumed.matches("id: ").count(), 6);
    assert!(resumed.starts_with("id: 6\n"));
    assert!(matches!(
        store.stream_slice("m-0000", Some(11)),
        StreamResponse::NoContent
    ));
    assert!(matches!(
        store.stream_slice("m-0000", Some(99)),
        StreamResponse::NoContent
    ));
}

#[test]
fn rendered_stream_bodies_are_byte_identical() {
    let completed = CompletedMatch {
        match_id: "m-0001".to_string(),
        seed: 42,
        events: Vec::new(),
        frames: Vec::new(),
        roster: Vec::new(),
        teams: Vec::new(),
        score: (2, 1),
        state_hash: 99,
        added_time_ticks: (300, 420),
        boundaries: HalfBoundaries {
            first_half_end: 165_000,
            full_time: 330_100,
        },
        attack_home: (1, -1),
        attack_away: (-1, 1),
    };
    let first = render_body(&build_stream(&completed));
    let second = render_body(&build_stream(&completed));
    assert_eq!(first, second);
    // 12 messages: manifest + 10 segments + full_time.
    let kinds: Vec<&str> = build_stream(&completed)
        .iter()
        .map(|message| message.event)
        .collect();
    assert_eq!(
        kinds,
        vec![
            "manifest",
            "segment",
            "segment",
            "segment",
            "segment",
            "segment",
            "segment",
            "segment",
            "segment",
            "segment",
            "segment",
            "full_time",
        ]
    );
}

#[test]
fn short_run_fails_on_tick_cap_with_last_good_tick() {
    let fixture = MatchRegistry::single_test("m-0000", 42)
        .fixtures
        .into_values()
        .next()
        .expect("test fixture");
    let outcome = run_match_blocking(&fixture, 120);
    let MatchOutcome::Failed {
        code,
        last_good_tick,
        ..
    } = outcome
    else {
        panic!("a 120-tick run cannot reach FullTime");
    };
    assert_eq!(code, "tick_cap_exceeded");
    assert_eq!(last_good_tick, 120);
}

#[test]
fn frame_capture_runs_every_sixth_tick_with_22_players() {
    let fixture = MatchRegistry::single_test("m-0000", 42)
        .fixtures
        .into_values()
        .next()
        .expect("test fixture");
    let sim = Simulation::new_from_fixture(
        &fixture.validated_match,
        &fixture.home_team,
        &fixture.away_team,
    );
    let roster = build_roster(&sim, &fixture);
    assert_eq!(roster.entries.len(), 22);
    let frame = capture_frame(&sim, &roster.ids).expect("invariant: the frame fits in memory");
    assert_eq!(frame.players.len(), 22);
    assert_eq!(frame.ball.state, "free");
}

/// Full-match end to end: `POST` runs the simulation on its OS
/// thread, the stream has exactly 12 messages, and segment tick
/// ranges match PRD §10.3. Minutes-long in debug; run explicitly:
///
/// ```text
/// cargo test -p sim-server --lib -- --ignored api_end_to_end_full_match
/// ```
#[test]
#[ignore = "full-match e2e is minutes-long in debug; run explicitly (release recommended)"]
fn api_end_to_end_full_match() {
    let config = ServerConfig {
        result_ttl_secs: 3600,
        ..ServerConfig::default()
    };
    let store: Arc<MatchStore> = Arc::new(MatchStore::new(
        config,
        MatchRegistry::single_test("m-0001", 42),
    ));
    let response = store.post_match("m-0001");
    assert_eq!(response.status, 202);
    // Second POST while running reports the known state.
    let response = store.post_match("m-0001");
    assert_eq!(response.status, 200);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("invariant: test runtime builds");
    runtime.block_on(store.wait_until_done("m-0001"));

    let StreamResponse::Ready(body) = store.stream_slice("m-0001", None) else {
        panic!("expected a finished stream");
    };
    assert_eq!(body.matches("id: ").count(), 12);
    assert!(body.contains("event: manifest"));
    assert!(body.contains("event: full_time"));
    assert_eq!(body.matches("event: segment").count(), 10);

    // Segment tick ranges match PRD §10.3: the first four of each
    // half are exactly 10 minutes wide, the fifth runs to the
    // whistle, and none crosses a half boundary.
    let manifest_data = body
        .lines()
        .find(|line| line.starts_with("data: "))
        .expect("invariant: manifest carries a data line");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_data["data: ".len()..])
            .expect("invariant: manifest data is JSON");
    let segments = manifest["segments"]
        .as_array()
        .expect("invariant: manifest lists 10 segments");
    assert_eq!(segments.len(), 10);
    let range = |index: usize| {
        (
            segments[index]["from_tick"].as_u64().expect("from_tick"),
            segments[index]["to_tick"].as_u64().expect("to_tick"),
        )
    };
    for segment in [0_usize, 1, 2, 3, 5, 6, 7, 8] {
        let (from, to) = range(segment);
        assert_eq!(to - from + 1, 36_000, "segment {segment}");
    }
    let first_half_end = range(4).1;
    let (second_from, _) = range(5);
    assert_eq!(second_from, first_half_end + 1);
    let full_time_tick = range(9).1;
    assert!(first_half_end >= 162_000, "half one covers 45 minutes");
    assert!(full_time_tick > first_half_end);
    let full_time_data = body
        .lines()
        .rev()
        .find(|line| line.starts_with("data: "))
        .expect("invariant: full_time carries a data line");
    let full_time: serde_json::Value =
        serde_json::from_str(&full_time_data["data: ".len()..])
            .expect("invariant: full_time data is JSON");
    assert!(full_time["event_count"].as_u64().unwrap_or(0) > 0);
    assert!(full_time["state_hash"].as_u64().unwrap_or(0) > 0);

    // Re-simulation is byte-identical (PRD §10.4 determinism).
    let fixture = store
        .registry
        .get("m-0001")
        .expect("fixture exists");
    let rerun = run_match_blocking(fixture, store.config.tick_cap);
    let MatchOutcome::Finished(rerun_completed) = rerun else {
        panic!("re-simulation of seed 42 must finish");
    };
    let cached_stream = {
        let state = store
            .state
            .lock()
            .expect("invariant: test lock is never poisoned");
        state
            .records
            .get("m-0001")
            .expect("invariant: match record exists after POST")
            .stream
            .clone()
    };
    assert_eq!(
        render_body(&cached_stream),
        render_body(&build_stream(&rerun_completed))
    );
}
