//! The `simulate` subcommand, extracted from `main.rs` (coding standards §7).

use std::fs::File;
use std::io::Write;
use std::time::Instant;

use sim_core::Simulation;

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
    /// Optional final-state JSON path.
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

/// Run a deterministic simulation, optionally recording a Chrome trace.
///
/// # Errors
///
/// Invalid trace flags, trace/snapshot I/O failures, or snapshot serialisation.
pub fn run_simulate(
    options: &SimulateOptions,
) -> Result<SimulateReport, Box<dyn std::error::Error>> {
    let seed = options.seed;

    // Validate trace flags before doing any work. A `--full-match` run has no
    // known length up front, so its range is not clamped.
    let total_ticks = (!options.full_match).then_some(options.ticks);
    // Crate-boundary conversion to `String` (standards §8) so the CLI prints
    // the `Display` message rather than the `Debug` variant name.
    let trace_plan = plan_trace(&options.trace, total_ticks).map_err(|e| e.to_string())?;
    let trace_guard = trace_plan
        .as_ref()
        .map(|plan| sim_telemetry::install(&plan.path))
        .transpose()
        .map_err(|e| e.to_string())?;

    let mut sim = Simulation::new(seed);
    if let Some(plan) = trace_plan {
        sim.enable_telemetry(plan.config);
    }

    tracing::info!("Running simulation with seed {seed}...");
    let start = Instant::now();

    if options.full_match {
        loop {
            sim.tick();
            let m_state = sim.world().resource::<sim_components::Match>().state;
            if m_state == sim_components::MatchState::FullTime {
                tracing::info!("[Tick {:06}] FullTime reached", sim.current_tick());
                break;
            }
            // Safety net
            if sim.current_tick() > 400_000 {
                tracing::warn!("Exceeded 400k ticks without reaching FullTime");
                break;
            }
        }
    } else {
        for _ in 0..options.ticks {
            sim.tick();
            if sim.world().resource::<sim_components::Match>().state
                == sim_components::MatchState::FullTime
            {
                break;
            }
        }
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

    if let Some(output_path) = &options.output {
        let state = sim.get_state()?;
        let json = serde_json::to_string_pretty(&state)?;
        let mut file = File::create(output_path)?;
        file.write_all(json.as_bytes())?;
        tracing::info!("Final state written to {output_path}");
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
