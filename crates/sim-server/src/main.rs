use clap::{Parser, Subcommand};

use sim_components::ManagerCommand;
use sim_core::Simulation;
use sim_server::simulate::{SimulateOptions, run_simulate};
use sim_server::telemetry_cli::TraceOptions;
use sim_telemetry::TelemetryConfig;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser)]
#[command(name = "football-sim", about = "Football Simulation Server")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a deterministic simulation
    Simulate {
        /// Random seed
        #[arg(short, long, default_value_t = 12345)]
        seed: u64,

        /// Number of ticks to simulate (ignored if --full-match)
        #[arg(short, long, default_value_t = 1000)]
        ticks: u64,

        /// Run a full 90-minute match (324000 ticks)
        #[arg(long)]
        full_match: bool,

        /// Output file for the final state (JSON)
        #[arg(short, long)]
        output: Option<String>,

        /// Write a Perfetto-compatible Chrome JSON trace to this path
        #[arg(long)]
        trace_out: Option<PathBuf>,

        /// Record a snapshot every N ticks (requires --trace-out to have effect)
        #[arg(long, default_value_t = TelemetryConfig::DEFAULT_INTERVAL_TICKS)]
        trace_interval_ticks: u64,

        /// Record every tick in the inclusive range `<start>-<end>` (requires --trace-out)
        #[arg(long)]
        trace_full_range: Option<String>,
    },

    /// Replay a simulation with manager commands
    Replay {
        /// Random seed
        #[arg(short, long)]
        seed: u64,

        /// JSON file containing manager commands (list of {tick, command})
        #[arg(short, long)]
        commands: String,

        /// Output file for the event sequence (JSON)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Run a performance benchmark
    Benchmark {
        /// Random seed
        #[arg(short, long, default_value_t = 12345)]
        seed: u64,

        /// Number of ticks to simulate
        #[arg(short, long, default_value_t = 6000)]
        ticks: u64,

        /// Output file for the benchmark results (JSON)
        #[arg(short, long)]
        output: Option<String>,
    },
}

#[expect(
    clippy::too_many_lines,
    reason = "single-file CLI dispatcher; splitting would scatter the three subcommand flows across modules"
)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Simulate {
            seed,
            ticks,
            full_match,
            output,
            trace_out,
            trace_interval_ticks,
            trace_full_range,
        } => {
            let report = run_simulate(&SimulateOptions {
                seed,
                ticks,
                full_match,
                output,
                trace: TraceOptions {
                    out: trace_out,
                    interval_ticks: trace_interval_ticks,
                    full_range: trace_full_range,
                },
            })?;
            tracing::debug!(
                "simulate finished at tick {} (hash {})",
                report.ticks_run,
                report.final_state_hash
            );
        }

        Commands::Replay {
            seed,
            commands,
            output,
        } => {
            let mut sim = Simulation::new(seed);
            let match_entity = sim.match_entity();

            // Load commands from JSON file
            let mut file = File::open(commands)?;
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            let command_list: Vec<(u64, ManagerCommand)> = serde_json::from_str(&contents)?;

            // Sort commands by tick
            let mut sorted_commands = command_list;
            sorted_commands.sort_by_key(|&(tick, _)| tick);

            tracing::info!(
                "Replaying simulation with seed {}, {} commands...",
                seed,
                sorted_commands.len()
            );
            let start = Instant::now();
            let mut next_command_index = 0;

            for tick in 0..=sorted_commands.last().map_or(0, |&(t, _)| t) {
                // Apply any commands for this tick. Replay runs against
                // a bare `Simulation` (no command queue), so we validate
                // then apply directly. Phase F §F5 split: `apply_command`
                // was the single (validate + apply) entry point; replay
                // needs both, so it calls them in order.
                while next_command_index < sorted_commands.len() {
                    let (cmd_tick, cmd) = &sorted_commands[next_command_index];
                    if *cmd_tick == tick {
                        if let Err(e) = sim.validate_command(match_entity, cmd) {
                            tracing::warn!("command at tick {tick} failed: {e:?}");
                        } else if let Err(e) = sim.apply_validated_command(match_entity, cmd.clone()) {
                            tracing::warn!("command at tick {tick} failed to apply: {e:?}");
                        }
                        next_command_index += 1;
                    } else {
                        break;
                    }
                }

                sim.tick();
            }
            let duration = start.elapsed();
            tracing::info!(
                "Replay completed in {:.2}ms",
                duration.as_secs_f64() * 1000.0
            );

            if let Some(output_path) = output {
                let state = sim.get_state()?;
                let json = serde_json::to_string_pretty(&state)?;
                let mut file = File::create(&output_path)?;
                file.write_all(json.as_bytes())?;
                tracing::info!("Final state written to {output_path}");
            }
        }

        Commands::Benchmark {
            seed,
            ticks,
            output,
        } => {
            let mut sim = Simulation::new(seed);

            tracing::info!("Benchmarking simulation with seed {seed}, {ticks} ticks...");
            let start = Instant::now();
            for _ in 0..ticks {
                sim.tick();
            }
            let duration = start.elapsed();
            // Display-only conversion: 60 Hz ticks stay well below 2^53,
            // so the `u64 -> f64` cast is exact for any plausible run.
            #[expect(
                clippy::cast_precision_loss,
                reason = "display-only cast; tick counts at 60 Hz stay far below 2^53"
            )]
            let tick_time_ms = duration.as_secs_f64() * 1000.0 / ticks as f64;
            #[expect(
                clippy::cast_precision_loss,
                reason = "display-only cast; tick counts at 60 Hz stay far below 2^53"
            )]
            let ticks_per_sec = ticks as f64 / duration.as_secs_f64();

            tracing::info!(
                "Benchmark completed: {tick_time_ms:.2}ms per tick, {ticks_per_sec:.2} ticks/sec"
            );

            if let Some(output_path) = output {
                let benchmark_result = serde_json::json!({
                    "seed": seed,
                    "ticks": ticks,
                    "duration_ms": duration.as_secs_f64() * 1000.0,
                    "average_tick_ms": tick_time_ms,
                    "ticks_per_sec": ticks_per_sec,
                });
                let mut file = File::create(&output_path)?;
                file.write_all(serde_json::to_string_pretty(&benchmark_result)?.as_bytes())?;
                tracing::info!("Benchmark results written to {output_path}");
            }
        }
    }

    Ok(())
}
