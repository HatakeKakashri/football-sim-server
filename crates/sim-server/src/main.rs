use clap::{Parser, Subcommand};

use sim_components::ManagerCommand;
use sim_core::Simulation;
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
            let mut last_ball_state = sim_components::BallState::Free;
            let mut last_possessor: Option<bevy_ecs::prelude::Entity> = None;
            let mut last_log_tick = 0;

            // --- DEBUG INSTRUMENTATION (temporary, for phase2 validation) ---
            // Neither `lifecycle_system` (sim-core, the system that actually
            // performs InPlay->HalfTime/FullTime transitions) nor the unwired
            // `match_duration_enforcement_system` (sim-rules, dead code: it's
            // never added to the schedule, which is why "Half time!"/"Full
            // time!" never print) currently report MatchState transitions.
            // Track it here from the outside so every transition is visible.
            let mut last_match_state: Option<sim_components::MatchState> = None;

            // Player-movement census cadence: every N ticks, dump an
            // aggregate view of all 22 players so "are players moving at
            // all" can be answered without a full per-player dump every tick.
            const CENSUS_INTERVAL_TICKS: u64 = 600; // ~10 sim-seconds at 60 ticks/sec
            // --- END DEBUG INSTRUMENTATION HEADER ---

            for _ in 0..effective_ticks {
                sim.tick(1.0 / 60.0);

                // --- DEBUG: MatchState transition log ---
                if let Some(m) = sim.world.get::<sim_components::Match>(match_entity) {
                    if last_match_state != Some(m.state) {
                        println!(
                            "[Tick {:06}] MatchState: {:?} -> {:?} | half={} elapsed={:.3} added_time={:.3} is_running={}",
                            sim.tick, last_match_state, m.state,
                            m.clock.half, m.clock.elapsed, m.clock.added_time, m.clock.is_running
                        );
                        last_match_state = Some(m.state);
                    }

                    // DEBUG: stop wasting ticks once the match is over. This
                    // is a functional change, not just logging -- comment out
                    // this block if you want to keep observing what happens
                    // (or doesn't) to the world state after FullTime.
                    if m.state == sim_components::MatchState::FullTime {
                        println!(
                            "[Tick {:06}] FullTime reached, stopping early ({} of {} ticks used)",
                            sim.tick, sim.tick, effective_ticks
                        );
                        break;
                    }
                }

                // --- DEBUG: periodic aggregate player-movement census ---
                if sim.tick.is_multiple_of(CENSUS_INTERVAL_TICKS) {
                    let ball_pos = sim
                        .world
                        .get::<sim_components::Position>(sim.ball_entity)
                        .map(|p| p.0);

                    let mut speed_sum = 0.0f32;
                    let mut stationary_count = 0u32;
                    let mut player_count = 0u32;
                    let mut nearest_to_ball: Option<f32> = None;
                    let mut intent_counts: std::collections::HashMap<&'static str, u32> =
                        std::collections::HashMap::new();

                    let mut query = sim.world.query::<(
                        &sim_components::Player,
                        &sim_components::Position,
                        &sim_components::Velocity,
                    )>();
                    for (player, pos, vel) in query.iter(&sim.world) {
                        player_count += 1;
                        let speed = vel.0.length();
                        speed_sum += speed;
                        if speed < 0.05 {
                            stationary_count += 1;
                        }
                        if let Some(bp) = ball_pos {
                            let d = pos.0.distance(bp);
                            nearest_to_ball = Some(nearest_to_ball.map_or(d, |cur: f32| cur.min(d)));
                        }
                        let label = match &player.intent {
                            None => "None",
                            Some(sim_components::Intent::MoveToPosition(_)) => "MoveToPosition",
                            Some(sim_components::Intent::PassTo) => "PassTo",
                            Some(sim_components::Intent::ShootAtGoal(_)) => "ShootAtGoal",
                            Some(sim_components::Intent::Tackle(_)) => "Tackle",
                            Some(sim_components::Intent::ChaseBall) => "ChaseBall",
                            Some(sim_components::Intent::MarkOpponent(_)) => "MarkOpponent",
                            Some(sim_components::Intent::Intercept) => "Intercept",
                            Some(sim_components::Intent::Press(_)) => "Press",
                            Some(sim_components::Intent::HoldPosition) => "HoldPosition",
                            Some(sim_components::Intent::SupportRun) => "SupportRun",
                            Some(sim_components::Intent::TrackBack) => "TrackBack",
                        };
                        *intent_counts.entry(label).or_insert(0) += 1;
                    }

                    if player_count > 0 {
                        println!(
                            "[Tick {:06}] CENSUS players={} avg_speed={:.3} stationary(<0.05)={}/{} nearest_to_ball={} intents={:?}",
                            sim.tick,
                            player_count,
                            speed_sum / player_count as f32,
                            stationary_count,
                            player_count,
                            nearest_to_ball.map_or_else(|| "n/a".to_string(), |d| format!("{d:.2}m")),
                            intent_counts
                        );
                    }
                }
                // --- END DEBUG: periodic census ---

                if let Some(ball) = sim.world.get::<sim_components::Ball>(sim.ball_entity)
                    && (ball.state != last_ball_state || ball.possessor != last_possessor || sim.tick - last_log_tick >= 1800) {
                        let match_comp = sim.world.get::<sim_components::Match>(match_entity);
                        let score = match_comp.map_or((0, 0), |m| m.score);
                        let time = match_comp.map_or(0.0, |m| m.clock.elapsed);
                        
                        let intent_str = if let Some(p_ent) = ball.possessor {
                            if let Some(player) = sim.world.get::<sim_components::Player>(p_ent) {
                                if let Some(intent) = &player.intent {
                                    format!("{intent:?}")
                                } else {
                                    "None".to_string()
                                }
                            } else {
                                "None".to_string()
                            }
                        } else {
                            "None".to_string()
                        };

                        println!("[Tick {:06} | {:02}:{:02}] Score: {}-{} | State: {:?} | Pos: ({:>5.1}, {:>5.1}) | Possessor: {:?} | Intent: {}",
                            sim.tick,
                            (time / 60.0) as u32,
                            (time % 60.0) as u32,
                            score.0, score.1,
                            ball.state,
                            ball.position.x, ball.position.y,
                            ball.possessor,
                            intent_str
                        );
                        
                        last_ball_state = ball.state;
                        last_possessor = ball.possessor;
                        last_log_tick = sim.tick;
                    }
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

            println!(
                "Replaying simulation with seed {}, {} commands...",
                seed,
                sorted_commands.len()
            );
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
