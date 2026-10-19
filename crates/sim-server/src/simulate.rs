//! The `simulate` subcommand, extracted from `main.rs` (coding standards §7).

use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::Instant;

use sim_core::{Simulation, Termination, TickHook, run_match_driver, TICK_CAP};
use sim_telemetry::TraceMetadata;
use tracing_subscriber::EnvFilter;

use crate::telemetry_cli::{TraceOptions, plan_trace};

/// Everything the `simulate` subcommand needs.
#[derive(Debug, Clone)]
pub struct SimulateOptions {
    /// Random seed.
    pub seed: u64,
    /// Number of ticks (ignored when `full_match`).
    pub ticks: u64,
    /// Run until `FullTime`.
    pub full_match: bool,
    /// Optional JSONL output path (one `MatchSnapshot` per line, per tick).
    pub output: Option<String>,
    /// Trace flags.
    pub trace: TraceOptions,
}

/// Result of a `simulate` run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimulateReport {
    /// `Simulation::get_state_hash` at the end of the run.
    pub final_state_hash: u64,
    /// `Simulation::current_tick` at the end of the run.
    pub ticks_run: u64,
}

/// Per-tick hook for `simulate`: writes one `MatchSnapshot` per tick to
/// the JSONL writer when an output path is provided.
struct SimulateHook<'a> {
    writer: Option<&'a mut BufWriter<File>>,
}

impl TickHook for SimulateHook<'_> {
    fn on_tick(&mut self, sim: &mut Simulation) -> Result<(), String> {
        if let Some(w) = &mut self.writer {
            let state = sim.get_state()?;
            serde_json::to_writer(&mut *w, &state).map_err(|e| e.to_string())?;
            w.write_all(b"\n").map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// Run a deterministic simulation, optionally recording a Chrome trace.
///
/// # Errors
///
/// Invalid trace flags, trace/snapshot I/O failures, or snapshot serialisation.
pub fn run_simulate(
    options: &SimulateOptions,
) -> Result<SimulateReport, Box<dyn std::error::Error>> {
    // Initialize tracing subscriber if RUST_LOG is set (for debugging).
    if std::env::var("RUST_LOG").is_ok() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();
    }

    let seed = options.seed;

    // Validate trace flags before doing any work. A `--full-match` run has no
    // known length up front, so its range is not clamped.
    let total_ticks = (!options.full_match).then_some(options.ticks);
    // Crate-boundary conversion to `String` (standards §8) so the CLI prints
    // the `Display` message rather than the `Debug` variant name.
    let trace_plan = plan_trace(&options.trace, total_ticks).map_err(|e| e.to_string())?;
    let trace_guard = trace_plan
        .as_ref()
        .map(|plan| {
            // Stamp seed + interval + full range into the trace header so
            // Perfetto reads microseconds as microseconds and a developer
            // opening the file 6 months from now knows what they ran.
            let metadata =
                TraceMetadata::new(seed, plan.config.interval_ticks(), plan.config.full_range());
            sim_telemetry::install_with_metadata(&plan.path, Some(metadata))
        })
        .transpose()
        .map_err(|e| e.to_string())?;

    let mut sim = Simulation::new(seed);
    if let Some(plan) = trace_plan {
        sim.enable_telemetry(plan.config);
    }

    tracing::info!("Running simulation with seed {seed}...");
    let start = Instant::now();

    // Set up JSONL writer if output path is provided.
    let mut writer: Option<BufWriter<File>> = options
        .output
        .as_ref()
        .map(|path| Ok::<_, Box<dyn std::error::Error>>(BufWriter::new(File::create(path)?)))
        .transpose()?;

    let tick_cap = if options.full_match { TICK_CAP } else { options.ticks };
    let mut hook = SimulateHook { writer: writer.as_mut() };
    let result = run_match_driver(&mut sim, tick_cap, &mut hook)?;
    if result.termination == Termination::TickCap && options.full_match {
        tracing::warn!("Exceeded {tick_cap} ticks without reaching FullTime");
    }
    if result.termination == Termination::FullTime {
        tracing::info!("[Tick {:06}] FullTime reached", sim.current_tick());
    }
    let duration = start.elapsed();

    // Close the trace array and surface any write failure (standards §8).
    if let Some(guard) = trace_guard {
        guard.finish()?;
    }

    // Display-only conversion: 60 Hz ticks stay well below 2^53,
    // so the `u64 -> f64` cast is exact for any plausible match.
    #[expect(
        clippy::cast_precision_loss,
        reason = "display-only cast; tick counts at 60 Hz stay far below 2^53"
    )]
    let ticks_per_sec = sim.current_tick() as f64 / duration.as_secs_f64();
    tracing::info!(
        "Simulation completed in {:.2}ms ({:.2} ticks/sec)",
        duration.as_secs_f64() * 1000.0,
        ticks_per_sec
    );

    // Flush and close the JSONL file.
    if let Some(mut w) = writer {
        w.flush()?;
        if let Some(path) = &options.output {
            tracing::info!("JSONL output written to {path}");
        }
    }

    // Phase 0 CLI contract: a deterministic run is reproducible iff
    // two runs with the same seed print the same final state hash.
    let final_state_hash = sim.get_state_hash();
    tracing::info!("final_state_hash: {final_state_hash}");

    Ok(SimulateReport {
        final_state_hash,
        ticks_run: sim.current_tick(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::Simulation;

    #[test]
    fn test_simulate_runs() {
        let report = run_simulate(&SimulateOptions {
            seed: 12345,
            ticks: 100,
            full_match: false,
            output: None,
            trace: TraceOptions::default(),
        })
        .expect("simulate should succeed");
        assert_eq!(report.ticks_run, 100);
    }
}