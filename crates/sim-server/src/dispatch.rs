//! CLI dispatch — drives the `Cli` subcommands to the appropriate library
//! entry points.
//!
//! Kept separate from `lib.rs` so the CLI entry point stays under the
//! 500-LoC file budget (coding standards §4).

use std::fs::File;
use std::io::{Read, Write};
use std::time::Instant;

use sim_components::ManagerCommand;
use sim_core::Simulation;

use crate::Cli;
use crate::Commands;
use crate::fixtures;
use crate::harness::{HarnessOptions, run_harness};
use crate::simulate::{run_simulate, SimulateOptions};
use crate::telemetry_cli::TraceOptions;
use crate::trace_filter::{FilterOptions, filter_file};

/// Dispatch a parsed [`Cli`] to the appropriate library entry point.
///
/// # Errors
///
/// Any error returned by the underlying `simulate`, `replay`, or
/// `trace_filter` paths is propagated as a `Box<dyn std::error::Error>` so
/// `main` can print it and exit non-zero.
pub fn dispatch(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
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
            // Abort boot if any fixture fails validation (PRD §5: fixtures
            // are loaded and validated at startup).
            let _fixtures = fixtures::load_default()?;
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
            let _fixtures = fixtures::load_default()?;
            replay(seed, commands, output)?;
        }

        Commands::Benchmark { seed, ticks, output } => {
            let _fixtures = fixtures::load_default()?;
            benchmark(seed, ticks, output)?;
        }

        Commands::TraceFilter {
            input,
            output,
            player,
            from,
            to,
            process,
            name,
        } => {
            let opts = FilterOptions {
                player,
                from,
                to,
                process,
                name,
            };
            let kept = filter_file(&input, &opts)?;
            if let Some(path) = output {
                let mut file = File::create(&path)?;
                serde_json::to_writer_pretty(&mut file, &kept)?;
            } else {
                let stdout = std::io::stdout();
                let mut handle = stdout.lock();
                serde_json::to_writer_pretty(&mut handle, &kept)?;
            }
        }

        Commands::Harness { seeds, output } => {
            let _fixtures = fixtures::load_default()?;
            let opts = HarnessOptions {
                seeds,
                output,
            };
            let summary = run_harness(&opts)?;
            tracing::info!(
                "harness: {} seeds, {} full-time, mean_goals={:.2}, max_dead_window={}, determinism_failures={}, event_order_failures={}, violations={}",
                summary.seed_count,
                summary.matches_reached_full_time,
                summary.mean_goals,
                summary.max_dead_ball_window,
                summary.determinism_failures,
                summary.event_order_failures,
                summary.violations.len(),
            );
        }

        Commands::Serve {
            bind,
            max_concurrent_sims,
            result_ttl_secs,
            api_token,
            cors_origins,
        } => {
            let _fixtures = fixtures::load_default()?;
            serve(bind, max_concurrent_sims, result_ttl_secs, api_token, cors_origins)?;
        }
    }
    Ok(())
}

/// Inlined `serve` path — builds the store and blocks on the axum
/// server (kept private so `dispatch` stays short).
fn serve(
    bind: String,
    max_concurrent_sims: Option<usize>,
    result_ttl_secs: u64,
    api_token: Option<String>,
    cors_origins: String,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::Arc;

    use crate::api::runner::SimulationRunner;
    use crate::api::{MatchRegistry, MatchStore, ServerConfig, default_concurrency_cap};
    use crate::fixtures;
    use crate::service::MatchService;

    let cors_origins: Vec<String> = cors_origins
        .split(',')
        .map(|origin| origin.trim().to_string())
        .filter(|origin| !origin.is_empty())
        .collect();
    let cap = max_concurrent_sims.unwrap_or_else(default_concurrency_cap);
    let config = ServerConfig {
        bind: bind.clone(),
        max_concurrent_sims: cap,
        result_ttl_secs,
        tick_cap: sim_core::TICK_CAP,
        api_token: api_token.clone(),
        cors_origins: cors_origins.clone(),
    };

    // Load and validate all fixtures, then build the registry.
    let fixture_set = fixtures::load_default()?;
    let mut registry = MatchRegistry::default();
    for (match_id, validated_match) in &fixture_set.matches {
        let home_team = fixture_set.teams.get(&validated_match.home)
            .expect("invariant: validated match references existing home team");
        let away_team = fixture_set.teams.get(&validated_match.away)
            .expect("invariant: validated match references existing away team");
        registry.insert(match_id, validated_match.clone(), home_team.clone(), away_team.clone());
    }

    let store = Arc::new(MatchStore::new(config, registry));
    // The match service runs the matches the new routes start: the
    // real runner, the same concurrency cap as the legacy store.
    let service = Arc::new(
        MatchService::new(fixture_set.clone(), Arc::new(SimulationRunner)).with_cap(cap),
    );
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(crate::http::serve(
            store,
            service,
            &bind,
            api_token,
            cors_origins,
        ))?;
    Ok(())
}

/// Inlined `replay` path — kept private so the dispatch function stays short.
fn replay(
    seed: u64,
    commands: String,
    output: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut sim = Simulation::new(seed);
    let match_entity = sim.match_entity();

    let mut file = File::open(commands)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let command_list: Vec<(u64, ManagerCommand)> = serde_json::from_str(&contents)?;

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
    Ok(())
}

/// Inlined benchmark path.
fn benchmark(
    seed: u64,
    ticks: u64,
    output: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut sim = Simulation::new(seed);

    tracing::info!("Benchmarking simulation with seed {seed}, {ticks} ticks...");
    let start = Instant::now();
    for _ in 0..ticks {
        sim.tick();
    }
    let duration = start.elapsed();
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
    Ok(())
}
