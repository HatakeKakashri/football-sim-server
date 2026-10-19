//! The `harness` subcommand (issue 07).
//!
//! `sim-server harness --seeds <file> -o <json>` runs a fixed list of
//! seeds to `FullTime` in release mode, never serialises per-tick
//! snapshots, and reports the PRD §13.1 metrics that do not require
//! events:
//!
//! - A1: matches that reach `FullTime` within `tick_cap`.
//! - A2: longest dead-ball window across all seeds.
//! - A3: mean goals per match.
//! - A4: share of 0–0 draws.
//! - A6: determinism (byte-equal hashes for repeated seeds).
//!
//! A5 (strong-side win rate), A8 (event order), and A9 (tactic
//! signatures) need fixtures the harness does not yet load; the
//! harness records raw counts so later fixture-tiers can upgrade those
//! metrics without re-running. A7 (goals vs score) is testable from
//! ticket 10 onward: per-tick goal counts fold the `goal` events in
//! the `EventLog` (PRD §9.3) and invariant 1 compares them to
//! `Match.score`.
//!
//! The six PRD §12 invariants are checked per tick (via
//! `sim_core::check_invariants`); any violation fails the match, logs
//! the seed, and caches the failure.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sim_components::Match;
use sim_core::{
    InvariantViolation, Simulation, Termination, TickHook, run_match_driver, TICK_CAP,
};

/// Hard cap on how many ticks the harness will drive a single match
/// before declaring it stalled. Mirrors `sim_core::TICK_CAP`.
pub const HARNESS_TICK_CAP: u64 = TICK_CAP;

/// Command-line configuration for `harness`.
#[derive(Debug, Clone)]
pub struct HarnessOptions {
    /// Path to a file of newline-separated seeds.
    pub seeds: PathBuf,
    /// Where the harness writes its JSON summary.
    pub output: Option<PathBuf>,
}

/// Per-seed metrics the harness reports.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SeedReport {
    pub seed: u64,
    pub reached_full_time: bool,
    pub ticks_run: u64,
    pub final_score: (u8, u8),
    pub max_dead_ball_window: u64,
    /// All violations detected during this seed's run. The harness
    /// stops the run on the first one but keeps the list open at length 1
    /// so callers don't have to read a separate log.
    pub violations: Vec<InvariantViolation>,
    pub final_state_hash: u64,
    pub determinism_agreed: bool,
    /// A8 event-ordering check (PRD §13.1 A8, §9.1). `true` if the
    /// final `EventLog` passes `check_gapless_seq` (strictly increasing
    /// and gapless `seq`; non-decreasing `tick`). Defaults to `false`
    /// because the harness only flips it to `true` after a successful
    /// check; a `false` value therefore always means the gate failed.
    pub event_order_ok: bool,
}

/// Aggregate metrics across all seeds.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HarnessSummary {
    pub seed_count: usize,
    pub matches_reached_full_time: usize,
    pub full_time_rate: f64,
    pub mean_goals: f64,
    pub zero_zero_draws: usize,
    pub zero_zero_rate: f64,
    pub max_dead_ball_window: u64,
    pub determinism_failures: usize,
    /// A8 event-ordering failures across the batch (PRD §13.1 A8,
    /// §9.1). Counts seeds whose `SeedReport.event_order_ok` is `false`
    /// — i.e. the final `EventLog` is not gapless-monotonic.
    pub event_order_failures: usize,
    pub violations: Vec<ViolationRecord>,
    /// Per-seed reports in the order the seeds were read. Useful for
    /// post-hoc inspection when an aggregate metric flags a failure.
    pub per_seed: Vec<SeedReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationRecord {
    pub seed: u64,
    pub violation: InvariantViolation,
}

impl HarnessSummary {
    fn add(&mut self, report: &SeedReport) {
        self.seed_count += 1;
        if report.reached_full_time {
            self.matches_reached_full_time += 1;
        }
        let total_goals = u64::from(report.final_score.0) + u64::from(report.final_score.1);
        #[expect(
            clippy::cast_precision_loss,
            reason = "match goal-counts are far below 2^52, so the f64 conversion is exact"
        )]
        let total_goals_f64 = total_goals as f64;
        self.mean_goals = update_mean(self.mean_goals, self.seed_count, total_goals_f64);
        if report.final_score == (0, 0) {
            self.zero_zero_draws += 1;
        }
        if report.max_dead_ball_window > self.max_dead_ball_window {
            self.max_dead_ball_window = report.max_dead_ball_window;
        }
        if !report.determinism_agreed {
            self.determinism_failures += 1;
        }
        if !report.event_order_ok {
            self.event_order_failures += 1;
        }
        for v in &report.violations {
            self.violations.push(ViolationRecord {
                seed: report.seed,
                violation: v.clone(),
            });
        }
        self.per_seed.push(report.clone());
    }

    fn finalize(&mut self) {
        if self.seed_count > 0 {
            let n = f64::from(u32::try_from(self.seed_count).expect("invariant: count fits u32"));
            self.full_time_rate = f64::from(
                u32::try_from(self.matches_reached_full_time)
                    .expect("invariant: matches_reached_full_time fits u32"),
            ) / n;
            self.zero_zero_rate = f64::from(
                u32::try_from(self.zero_zero_draws).expect("invariant: zero_zero_draws fits u32"),
            ) / n;
        }
    }
}

/// Run the harness over the configured seeds and write a JSON summary.
///
/// # Errors
///
/// I/O failures reading the seeds file or writing the summary, or
/// unrecoverable simulation errors. Per-match invariant failures are
/// *not* errors here — they're recorded in the summary so the caller
/// can inspect them.
pub fn run_harness(options: &HarnessOptions) -> Result<HarnessSummary, Box<dyn std::error::Error>> {
    let seeds = read_seeds(&options.seeds)?;
    let mut cache = FailureCache::default();
    let mut summary = HarnessSummary::default();

    for (index, seed) in seeds.iter().enumerate() {
        tracing::info!(
            "[{}/{}] running seed {} (cached? {})",
            index + 1,
            seeds.len(),
            seed,
            cache.contains(*seed)
        );
        let (report, _runtime) = run_seed_with_invariants(*seed);
        if !report.violations.is_empty() {
            for violation in &report.violations {
                tracing::error!(
                    seed = report.seed,
                    violation_code = violation.code(),
                    tick = report.ticks_run,
                    "invariant violation: {}",
                    violation
                );
            }
        }
        cache.record(*seed, &report);
        summary.add(&report);
    }

    summary.finalize();

    let json = serde_json::to_string_pretty(&summary)?;
    if let Some(path) = &options.output {
        let mut writer = BufWriter::new(File::create(path)?);
        writer.write_all(json.as_bytes())?;
        writer.flush()?;
    } else {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        handle.write_all(json.as_bytes())?;
        handle.flush()?;
    }

    Ok(summary)
}

fn read_seeds(path: &PathBuf) -> Result<Vec<u64>, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut seeds = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let seed: u64 = trimmed.parse()?;
        seeds.push(seed);
    }
    Ok(seeds)
}

fn update_mean(previous: f64, count: usize, new_value: f64) -> f64 {
    if count == 0 {
        return new_value;
    }
    let weight = 1.0 / f64::from(u32::try_from(count).expect("invariant: count fits u32"));
    new_value.mul_add(weight, previous * (1.0 - weight))
}

#[derive(Debug, Default)]
struct FailureCache {
    map: HashMap<u64, SeedReport>,
}

impl FailureCache {
    fn contains(&self, seed: u64) -> bool {
        self.map.contains_key(&seed)
    }
    fn record(&mut self, seed: u64, report: &SeedReport) {
        self.map.insert(seed, report.clone());
    }
}

/// Play one seed to `FullTime` (or the invariant / tick-cap stop), then
/// play it again to confirm determinism.
fn run_seed_with_invariants(seed: u64) -> (SeedReport, std::time::Duration) {
    let start = Instant::now();
    let (mut report, hash_first) = play_one(seed);
    let (_second_report, hash_second) = play_one(seed);
    report.determinism_agreed = hash_first == hash_second;
    report.final_state_hash = hash_first;
    (report, start.elapsed())
}

/// Play one seed. Returns the per-seed metrics and the final hash.
fn play_one(seed: u64) -> (SeedReport, u64) {
    let mut sim = Simulation::new(seed);
    let mut hook = HarnessHook;
    let result = run_match_driver(&mut sim, HARNESS_TICK_CAP, &mut hook)
        .expect("invariant: HarnessHook never errors");

    let mut report = SeedReport {
        seed,
        ..SeedReport::default()
    };
    let final_score = sim.world().get_resource::<Match>().map_or((0, 0), |m| m.score);
    report.final_score = final_score;
    report.ticks_run = result.ticks_run;
    report.reached_full_time = result.termination == Termination::FullTime;
    report.max_dead_ball_window = result.max_dead_ball_window;
    if let Some(v) = result.violation {
        report.violations.push(v);
    }
    // A8 event ordering (PRD §13.1 A8): seq strictly increasing and
    // gapless across the match; ticks non-decreasing within each
    // event envelope. The engine emits envelopes in tick order, so
    // the seq check (which is gapless-monotonic) is the load-bearing
    // invariant. `check_gapless_seq` returns `None` on success.
    report.event_order_ok = sim
        .world()
        .get_resource::<sim_core::EventLog>()
        .is_some_and(|log| sim_core::check_gapless_seq(log).is_none());
    let hash = sim.get_state_hash();
    (report, hash)
}

/// Per-tick hook for the harness: no per-tick side effects; all metrics
/// are collected from `MatchResult` after the run.
struct HarnessHook;

impl TickHook for HarnessHook {
    fn on_tick(&mut self, _sim: &Simulation) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests compare freshly-computed metrics against the constants they were built from"
)]
mod tests {
    use super::*;

    #[test]
    fn read_seeds_skips_blanks_and_comments() {
        let dir = std::env::temp_dir();
        let path = dir.join("harness_seeds_smoke.txt");
        std::fs::write(&path, "1\n# comment\n\n2\n  3  \n").expect("write");
        let seeds = read_seeds(&path).expect("read");
        assert_eq!(seeds, vec![1, 2, 3]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn update_mean_tracks_running_average() {
        // First sample: previous = 0.0, count = 0, new = 4.0 → returns 4.0.
        let m = update_mean(0.0, 0, 4.0);
        assert_eq!(m, 4.0);
        // Second sample: previous = 4.0, count = 1, new = 0.0 →
        // mean = 4.0 * 0.0 + 0.0 * 1.0 = 0.0.
        let m = update_mean(4.0, 1, 0.0);
        assert!((m - 0.0).abs() < 1e-9, "expected 0.0, got {m}");
        // Third sample (count = 2): previous = 0.0, new = 4.0 →
        // 0.0 * 0.5 + 4.0 * 0.5 = 2.0.
        let m = update_mean(0.0, 2, 4.0);
        assert!((m - 2.0).abs() < 1e-9, "expected 2.0, got {m}");
    }

    #[test]
    fn tick_cap_constant_is_400_000() {
        assert_eq!(TICK_CAP, 400_000);
    }

    /// `event_order_ok` is true for the canonical seed (12345), since
    /// the event-log invariant holds across the existing tests.
    /// The seed's score / state are deterministic.
    ///
    /// `#[ignore]`'d (PRD §14: full-match tests run manually in
    /// release). Run with:
    ///
    /// ```text
    /// cargo test --release -p sim-server --lib -- --ignored event_order_ok_for_canonical_seed_12345
    /// ```
    #[test]
    #[ignore = "full-match test is minutes-long; run explicitly when verifying the A8 metric"]
    fn event_order_ok_for_canonical_seed_12345() {
        let (report, _hash) = play_one(12345);
        // The harness plays to FullTime, then checks `check_gapless_seq`
        // on the final EventLog. PRD §9.1 / §13.1 A8 requires gapless
        // monotonic seq; if the harness ever reports a non-OK value
        // here, the engine has a seq-ordering regression.
        assert!(report.event_order_ok, "A8: seq must be gapless");
        assert!(report.reached_full_time, "A1: full-time within cap");
    }
}
