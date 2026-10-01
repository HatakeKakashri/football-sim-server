//! End-to-end `simulate` runs. One test function because tracing installs a
//! process-global subscriber exactly once.

use std::collections::BTreeSet;

use serde_json::Value;
use sim_server::simulate::{SimulateOptions, run_simulate};
use sim_server::telemetry_cli::TraceOptions;

const fn options(trace: TraceOptions) -> SimulateOptions {
    SimulateOptions {
        seed: 12345,
        ticks: 130,
        full_match: false,
        output: None,
        trace,
    }
}

#[test]
fn traced_and_untraced_runs_agree_and_the_trace_is_complete() {
    // 1. Untraced baseline: no file, no subscriber.
    let plain = run_simulate(&options(TraceOptions::default())).expect("untraced run");
    assert_eq!(plain.ticks_run, 130);

    // 2. Traced run with a full-resolution window.
    // V1 (verification ledger): run long enough for goals / fouls / offside
    // to fire so every Perfetto-rendered event type lands at least once.
    let path = std::env::temp_dir().join(format!("sim-server-e2e-{}.json", std::process::id()));
    let traced = run_simulate(&options(TraceOptions {
        out: Some(path.clone()),
        interval_ticks: 60,
        full_range: Some("0-600".to_string()),
    }))
    .expect("traced run");

    // SC-004: tracing is a pure side channel.
    assert_eq!(traced, plain, "final state hash and tick count must match");

    // SC-001: well-formed Chrome trace JSON.
    let text = std::fs::read_to_string(&path).expect("trace file exists");
    std::fs::remove_file(&path).expect("cleanup");
    let events: Vec<Value> = serde_json::from_str(&text).expect("trace is a JSON array");

    // SC-005 structure: 22 player processes + Ball, no Referee.
    let processes: Vec<&str> = events
        .iter()
        .filter(|e| e["name"] == "process_name")
        .filter_map(|e| e["args"]["name"].as_str())
        .collect();
    assert_eq!(
        processes.iter().filter(|n| n.starts_with("Home ")).count(),
        11
    );
    assert_eq!(
        processes.iter().filter(|n| n.starts_with("Away ")).count(),
        11
    );
    assert!(processes.contains(&"Ball"));
    assert!(processes.contains(&"Match"));
    assert!(!processes.contains(&"Referee"));

    // FR-003/SC-003: coarse snapshots plus every tick of the full range.
    // The simulation reaches FullTime at tick 130 (see `plain.ticks_run`
    // above) so ball counters stop at tick 129 even though the range is
    // 0..=600.
    let ball_ticks: BTreeSet<u64> = events
        .iter()
        .filter(|e| e["ph"] == "C" && e["pid"] == 1 && e["name"] == "x")
        .filter_map(|e| e["ts"].as_u64())
        .map(|ts| (ts * 60 + 500_000) / 1_000_000)
        .collect();
    let expected: BTreeSet<u64> = [0_u64, 60, 120]
        .into_iter()
        .chain(0..=129_u64)
        .collect();
    assert_eq!(ball_ticks, expected);

    // FR-007/008: real decisions with considerations and a marked winner.
    assert!(
        events
            .iter()
            .any(|e| e["ph"] == "B" && e["args"]["chosen"] == true)
    );
    assert!(
        events
            .iter()
            .any(|e| e["ph"] == "i" && e["args"]["curve"].is_string())
    );

    // Step 1 fix — `displayTimeUnit` lets Perfetto render µs as µs.
    let header = events
        .iter()
        .find(|e| e["name"] == "process_labels" && e["ph"] == "M")
        .expect("process_labels metadata present");
    assert_eq!(header["args"]["displayTimeUnit"], "us");
    assert_eq!(header["args"]["seed"], 12345);
    assert_eq!(header["args"]["interval_ticks"], 60);
    // Full range is clamped to `total_ticks - 1` by `plan_trace`; the run
    // was 130 ticks long, so the header records `[0, 129]`.
    assert_eq!(
        header["args"]["full_range"],
        serde_json::json!([0, 129]),
        "full_range is clamped to the run length"
    );

    // V1 (verification ledger, automated): every Perfetto-rendered event
    // type this feature promises lands at least once. If any of these
    // assertions fail, opening the trace in Perfetto will show that
    // piece missing.

    // Step 2: goals / fouls / offside / restarts / half_time / kickoff on
    // the Match process.
    let match_events = events
        .iter()
        .filter(|e| e["ph"] == "i" && e["pid"] == 4)
        .count();
    assert!(
        match_events > 0,
        "Match process emitted no events — goals/fouls/etc. missing"
    );

    // Step 3: pass flow events. Not every run has a pass, but the count
    // of `flow_start` (`s`) and `flow_end` (`f`) must match if any fired.
    let pass_starts = events
        .iter()
        .filter(|e| e["ph"] == "s" && e["name"] == "pass")
        .count();
    let pass_ends = events
        .iter()
        .filter(|e| e["ph"] == "f" && e["name"] == "pass")
        .count();
    assert_eq!(
        pass_starts, pass_ends,
        "pass flow start/end count mismatch ({pass_starts} vs {pass_ends})"
    );

    // All non-metadata events carry `ts`. Anything missing would not
    // render on Perfetto's timeline.
    let missing_ts = events
        .iter()
        .filter(|e| e["ph"] != "M" && e["ts"].is_null())
        .count();
    assert_eq!(missing_ts, 0, "events without ts will be hidden in Perfetto");
}

#[test]
fn invalid_trace_flags_fail_before_simulating() {
    let bad = |trace| {
        run_simulate(&options(trace))
            .expect_err("must be rejected")
            .to_string()
    };
    let range_only = TraceOptions {
        out: None,
        interval_ticks: 60,
        full_range: Some("1-2".to_string()),
    };
    assert!(bad(range_only).contains("--trace-out"));
    let zero = TraceOptions {
        out: Some("x.json".into()),
        interval_ticks: 0,
        full_range: None,
    };
    assert!(bad(zero).contains("greater than 0"));
}
