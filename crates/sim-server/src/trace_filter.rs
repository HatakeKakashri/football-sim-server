//! Post-hoc filtering of Chrome trace files.
//!
//! The full-match trace at the default interval is ~67 MB; with full-range
//! recording enabled for every tick it can balloon past Perfetto's 250 MB
//! upload limit. This module lets the developer slice a trace down to one
//! player or a tick window before opening it.
//!
//! Filters supported (all combinable, all `AND`ed together):
//! - `--player <entity_index>`: events for that player's process
//! - `--from <tick>` / `--to <tick>`: inclusive tick range
//! - `--process <name>`: events on a named process (e.g `"Ball"`, `"Match"`)
//! - `--name <event_name>`: events with a given `name` field
//!
//! The output is a JSON array of the matching events. Pass-through events
//! (the `process_labels` metadata and process/thread names) are kept so
//! Perfetto can still resolve process names.
//!
//! # `jq` recipes
//!
//! For ad-hoc inspection without re-running the filter command:
//!
//! ```text
//! # Count events by phase
//! jq '[.[] | .ph] | group_by(.) | map({phase: .[0], count: length})' trace.json
//!
//! # Show every goal event
//! jq '[.[] | select(.name == "goal")]' trace.json
//!
//! # All instant events on the Match process between tick 1000 and 2000
//! jq '[.[] | select(.pid == 4 and .ph == "i") | select(.ts >= 1000*(1_000_000/60) and .ts <= 2000*(1_000_000/60))]' trace.json
//!
//! # The chosen actions at a specific tick (replace 600 with any tick)
//! jq '[.[] | select(.name != null and (.args.chosen == true) and (.ts >= 600*(1_000_000/60) and .ts < 601*(1_000_000/60)))]' trace.json
//! ```

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use serde_json::Value;

use sim_telemetry::emit;

/// Filter a parsed JSON array of Chrome events in-place and write the result
/// to `out`.
///
/// # Errors
///
/// Returns any I/O error from `out`, or a JSON serialization failure.
pub fn run_filter(
    events: &[Value],
    options: &FilterOptions,
    out: &mut dyn Write,
) -> io::Result<()> {
    let filtered: Vec<&Value> = events
        .iter()
        .filter(|e| keep_event(e, options))
        .collect();
    // Write the filtered array compactly: `[` ... `]` with one event per line.
    // Perfetto doesn't care about whitespace inside the array.
    let mut buf = Vec::with_capacity(filtered.len() * 100);
    buf.push(b'[');
    for (i, event) in filtered.iter().enumerate() {
        if i > 0 {
            buf.push(b',');
        }
        buf.push(b'\n');
        serde_json::to_writer(&mut buf, event).map_err(io::Error::other)?;
    }
    if !filtered.is_empty() {
        buf.push(b'\n');
    }
    buf.push(b']');
    buf.push(b'\n');
    out.write_all(&buf)
}

/// Parse a trace file from `path` and apply `options`.
///
/// # Errors
///
/// Returns an I/O error if `path` cannot be read, or a JSON parse error if
/// the file is not a well-formed Chrome trace array.
pub fn filter_file(
    path: &Path,
    options: &FilterOptions,
) -> io::Result<Vec<Value>> {
    let text = fs::read_to_string(path)?;
    let events: Vec<Value> = serde_json::from_str(&text)
        .map_err(|e| io::Error::other(format!("trace JSON: {e}")))?;
    let kept: Vec<Value> = events
        .into_iter()
        .filter(|e| keep_event(&e.clone(), options))
        .collect();
    Ok(kept)
}

/// CLI options for the `trace-filter` subcommand.
#[derive(Debug, Default, Clone)]
pub struct FilterOptions {
    /// Filter to a single player by entity index (translates to `pid = 1000 + idx`).
    pub player: Option<u32>,
    /// Inclusive lower tick bound.
    pub from: Option<u64>,
    /// Inclusive upper tick bound.
    pub to: Option<u64>,
    /// Match `args.process_name` (`process_name` metadata events) or `pid`.
    pub process: Option<String>,
    /// Match `name` exactly.
    pub name: Option<String>,
}

/// Decide whether a single Chrome event survives the filters.
///
/// Pass-through rules (always kept, so Perfetto still has process names):
/// - `process_labels` metadata (the run header)
/// - `process_name` metadata
/// - `thread_name` metadata
fn keep_event(event: &Value, options: &FilterOptions) -> bool {
    // Always keep the run header so a developer opening the filtered file
    // still knows what they ran.
    if event["name"] == "process_labels" && event["ph"] == "M" {
        return true;
    }
    // Keep process / thread name metadata so the filtered file's events
    // can still resolve their `pid`/`tid` to a human-readable name.
    if event["ph"] == "M"
        && (event["name"] == "process_name" || event["name"] == "thread_name")
    {
        return true;
    }

    // Tick-range filter (works on `ts` which encodes the tick).
    let ts = event.get("ts").and_then(Value::as_u64);
    if let Some(ts) = ts {
        let tick = tick_of(ts);
        if let Some(lo) = options.from
            && tick < lo
        {
            return false;
        }
        if let Some(hi) = options.to
            && tick > hi
        {
            return false;
        }
    }

    // Player filter: match the event's pid against PID_PLAYER_BASE + idx.
    if let Some(idx) = options.player {
        let pid = event.get("pid").and_then(Value::as_u64);
        if let Some(pid) = pid
            && pid != emit::player_pid(idx)
        {
            return false;
        }
    }

    // Process filter: match against either the metadata name or the pid.
    if let Some(proc_name) = &options.process {
        let name_match = event.get("args")
            .and_then(|a| a.get("name"))
            .and_then(Value::as_str)
            .is_some_and(|n| n == proc_name);
        if !name_match && !passes_process_pid(event, proc_name) {
            return false;
        }
    }

    // Name filter: exact match on the event's `name`.
    if let Some(name) = &options.name
        && event.get("name").and_then(Value::as_str) != Some(name.as_str())
    {
        return false;
    }

    true
}

/// Resolve a process name filter to a pid (for built-in processes like Ball).
fn passes_process_pid(event: &Value, proc_name: &str) -> bool {
    let pid = event.get("pid").and_then(Value::as_u64);
    let Some(pid) = pid else {
        return false;
    };
    match proc_name {
        "Ball" => pid == emit::PID_BALL,
        "Referee" => pid == emit::PID_REFEREE,
        "Match" => pid == emit::PID_MATCH,
        _ => false,
    }
}

/// Convert a Chrome `ts` (µs of sim time) back to the tick it belongs to.
/// Identical to the helper in `sim_core::tests::telemetry_tests`.
const fn tick_of(ts: u64) -> u64 {
    (ts * 60 + 500_000) / 1_000_000
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_events() -> Vec<Value> {
        vec![
            json!({"name": "process_labels", "ph": "M", "pid": 0, "tid": 0, "args": {}}),
            json!({"name": "process_name", "ph": "M", "pid": 1007, "tid": 0, "args": {"name": "Home 7 — Striker"}}),
            json!({"name": "x", "ph": "C", "pid": 1007, "tid": 1, "ts": 60 * 1_000_000 / 60, "args": {"value": 1.0}}),
            json!({"name": "x", "ph": "C", "pid": 1007, "tid": 1, "ts": 120 * 1_000_000 / 60, "args": {"value": 2.0}}),
            json!({"name": "x", "ph": "C", "pid": 1008, "tid": 1, "ts": 60 * 1_000_000 / 60, "args": {"value": 3.0}}),
            json!({"name": "goal", "ph": "i", "pid": 4, "tid": 1, "ts": 60 * 1_000_000 / 60, "args": {"scoring_team": 0}}),
        ]
    }

    #[test]
    fn no_filters_keeps_everything() {
        let opts = FilterOptions::default();
        let events = make_events();
        let kept = events.iter().filter(|e| keep_event(e, &opts)).count();
        assert_eq!(kept, events.len());
    }

    #[test]
    fn player_filter_keeps_only_that_pids_events_plus_metadata() {
        let opts = FilterOptions {
            player: Some(7),
            ..FilterOptions::default()
        };
        let events = make_events();
        let kept: Vec<_> = events.iter().filter(|e| keep_event(e, &opts)).collect();
        // Header + Home 7's process_name + both Home 7 counters.
        assert_eq!(kept.len(), 4);
        assert!(kept.iter().any(|e| e["name"] == "process_name" && e["pid"] == 1007));
        assert!(!kept.iter().any(|e| e["pid"] == 1008));
        assert!(!kept.iter().any(|e| e["pid"] == 4));
    }

    #[test]
    fn from_and_to_constrain_by_tick() {
        let opts = FilterOptions {
            from: Some(61),
            to: Some(119),
            ..FilterOptions::default()
        };
        let events = make_events();
        let kept: Vec<_> = events.iter().filter(|e| keep_event(e, &opts)).collect();
        // Header + process_name + the tick-60 counters are out, tick-120 is in.
        assert!(kept.iter().all(|e| e["name"] == "process_labels"
            || e["name"] == "process_name"
            || e["ts"].as_u64().is_none_or(|ts| (61..=119).contains(&tick_of(ts)))));
        assert!(!kept.iter().any(|e| e["name"] == "x" && e["ts"] == 60 * 1_000_000 / 60));
    }

    #[test]
    fn name_filter_is_exact_match() {
        let opts = FilterOptions {
            name: Some("goal".to_string()),
            ..FilterOptions::default()
        };
        let events = make_events();
        let kept: Vec<_> = events.iter().filter(|e| keep_event(e, &opts)).collect();
        // Header + the goal event (and process_name metadata still kept).
        assert!(kept.iter().any(|e| e["name"] == "goal"));
        assert!(!kept.iter().any(|e| e["name"] == "x"));
    }

    #[test]
    fn process_filter_resolves_ball_match_and_referee_to_pids() {
        let events = [
            json!({"name": "process_labels", "ph": "M", "pid": 0, "tid": 0, "args": {}}),
            json!({"name": "x", "ph": "C", "pid": 1, "tid": 1, "ts": 0, "args": {}}),
            json!({"name": "goal", "ph": "i", "pid": 4, "tid": 1, "ts": 0, "args": {}}),
            json!({"name": "x", "ph": "C", "pid": 1007, "tid": 1, "ts": 0, "args": {}}),
        ];
        for (proc, expected_count) in [
            ("Ball", 2),    // header + ball counter
            ("Match", 2),   // header + match event
            ("Referee", 1), // header only (no referee events in this slice)
        ] {
            let opts = FilterOptions {
                process: Some(proc.to_string()),
                ..FilterOptions::default()
            };
            let kept = events.iter().filter(|e| keep_event(e, &opts)).count();
            assert_eq!(kept, expected_count, "process={proc}");
        }
    }

    #[test]
    fn filters_combine_with_and_semantics() {
        // Player 7 (pid 1007) AND name "x": both tick-60 and tick-120 survive.
        // Plus header + process_name metadata (which carries pid 1007 so it
        // passes the player filter). The pid-1008 counter and pid-4 goal
        // event are excluded by pid *and* by name.
        let opts = FilterOptions {
            player: Some(7),
            name: Some("x".to_string()),
            ..FilterOptions::default()
        };
        let events = make_events();
        let kept: Vec<_> = events.iter().filter(|e| keep_event(e, &opts)).collect();
        assert_eq!(kept.len(), 4); // header + process_name + 2 counters
        assert!(kept.iter().all(|e| e["pid"] == 0 || e["pid"] == 1007));
    }
}