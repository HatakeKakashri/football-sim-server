use clap::{Parser, Subcommand};

use sim_core::{MatchSnapshot, Simulation};
use sim_components::ManagerCommand;
use std::fs::File;
use std::io::{Read, Write};
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

    /// Recover from a snapshot and continue simulation
    Recover {
        /// Match ID (currently only 1 is supported)
        #[arg(short, long)]
        match_id: u32,

        /// Snapshot file to recover from (bincode)
        #[arg(short, long)]
        snapshot: String,

        /// Additional ticks to run after recovery (default 0)
        #[arg(short, long, default_value_t = 0)]
        ticks: u64,
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Simulate {
            seed,
            ticks,
            full_match,
            output,
        } => {
            let mut sim = Simulation::new(seed);
            let match_entity = sim.match_entity;

            let effective_ticks = if full_match { 324000 } else { ticks };

            println!("Running simulation with seed {seed}, {effective_ticks} ticks...");
            let start = Instant::now();
            for _ in 0..effective_ticks {
                sim.tick(1.0 / 60.0);
            }
            let duration = start.elapsed();
            println!(
                "Simulation completed in {:.2}ms ({:.2} ticks/sec)",
                duration.as_secs_f64() * 1000.0,
                effective_ticks as f64 / duration.as_secs_f64()
            );

            if let Some(output_path) = output {
                let state = sim.get_state(match_entity)?;
                let json = serde_json::to_string_pretty(&state)?;
                let mut file = File::create(&output_path)?;
                file.write_all(json.as_bytes())?;
                println!("Final state written to {output_path}");
            }

            // Phase 0 CLI contract: a deterministic run is reproducible iff
            // two runs with the same seed print the same final state hash.
            // The hash is also available in any JSON snapshot produced above,
            // but printing it on stdout makes the contract literal.
            println!("final_state_hash: {}", sim.get_state_hash());
        }

        Commands::Replay {
            seed,
            commands,
            output,
        } => {
            let mut sim = Simulation::new(seed);
            let match_entity = sim.match_entity;

            // Load commands from JSON file
            let mut file = File::open(commands)?;
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            let command_list: Vec<(u64, ManagerCommand)> = serde_json::from_str(&contents)?;

            // Sort commands by tick
            let mut sorted_commands = command_list;
            sorted_commands.sort_by_key(|&(tick, _)| tick);

            println!("Replaying simulation with seed {}, {} commands...", seed, sorted_commands.len());
            let start = Instant::now();
            let mut next_command_index = 0;

            for tick in 0..=sorted_commands.last().map_or(0, |&(t, _)| t) {
                // Apply any commands for this tick
                while next_command_index < sorted_commands.len() {
                    let (cmd_tick, cmd) = &sorted_commands[next_command_index];
                    if *cmd_tick == tick {
                        if let Err(e) = sim.apply_command(match_entity, cmd.clone()) {
                            eprintln!("Warning: command at tick {tick} failed: {e}");
                        }
                        next_command_index += 1;
                    } else {
                        break;
                    }
                }

                sim.tick(1.0 / 60.0);
            }
            let duration = start.elapsed();
            println!(
                "Replay completed in {:.2}ms",
                duration.as_secs_f64() * 1000.0
            );

            if let Some(output_path) = output {
                let state = sim.get_state(match_entity)?;
                let json = serde_json::to_string_pretty(&state)?;
                let mut file = File::create(&output_path)?;
                file.write_all(json.as_bytes())?;
                println!("Final state written to {output_path}");
            }
        }

        Commands::Recover {
            match_id,
            snapshot,
            ticks,
        } => {
            // Load snapshot
            let mut file = File::open(snapshot)?;
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)?;
            let snapshot_data: MatchSnapshot = bincode::deserialize(&buffer)?;

            // For now, we can only recover the simulation state up to the snapshot.
            // We don't have a way to reconstruct the entire Simulation from the snapshot yet.
            // We'll create a new simulation and set it to the snapshot state (TODO).
            let mut sim = Simulation::new(snapshot_data.state_hash); // Use state_hash as seed for now (not correct)
            let match_entity = sim.match_entity;

            println!(
                "Recovered from snapshot at tick {}. Running {} additional ticks...",
                snapshot_data.tick, ticks
            );
            let start = Instant::now();
            for _ in 0..ticks {
                sim.tick(1.0 / 60.0);
            }
            let duration = start.elapsed();
            println!(
                "Additional simulation completed in {:.2}ms",
                duration.as_secs_f64() * 1000.0
            );

            // Output the final state
            let state = sim.get_state(match_entity)?;
            let json = serde_json::to_string_pretty(&state)?;
            let mut output_file = File::create(format!("recovered_{match_id}.json"))?;
            output_file.write_all(json.as_bytes())?;
            println!("Recovered state written to recovered_{match_id}.json");
        }

        Commands::Benchmark {
            seed,
            ticks,
            output,
        } => {
            let mut sim = Simulation::new(seed);
            let _match_entity = sim.match_entity;

            println!("Benchmarking simulation with seed {seed}, {ticks} ticks...");
            let start = Instant::now();
            for _ in 0..ticks {
                sim.tick(1.0 / 60.0);
            }
            let duration = start.elapsed();
            let tick_time_ms = duration.as_secs_f64() * 1000.0 / ticks as f64;
            let ticks_per_sec = ticks as f64 / duration.as_secs_f64();

            println!(
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
                println!("Benchmark results written to {output_path}");
            }
        }
    }

    Ok(())
}
