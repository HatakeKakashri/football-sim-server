pub mod simulate;
pub mod telemetry_cli;
pub mod trace_filter;

use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand};
use sim_components::ManagerCommand;
use sim_core::Simulation;
use sim_telemetry::TelemetryConfig;

use crate::simulate::{SimulateOptions, run_simulate};
use crate::telemetry_cli::TraceOptions;
use crate::trace_filter::{FilterOptions, filter_file};

/// CLI for the football-sim binary.
///
/// `main.rs` parses this and forwards to [`dispatch`]; the dispatch function
/// lives here (in the library) per coding standards §7 so unit tests can
/// exercise it.
#[derive(Parser)]
#[command(name = "football-sim", about = "Football Simulation Server")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// Subcommands. Keep the bodies in `lib`; this enum only describes the CLI.
#[derive(Subcommand)]
pub enum Commands {
    /// Run a deterministic simulation.
    Simulate {
        /// Random seed.
        #[arg(short, long, default_value_t = 12345)]
        seed: u64,

        /// Number of ticks to simulate (ignored if --full-match).
        #[arg(short, long, default_value_t = 1000)]
        ticks: u64,

        /// Run a full 90-minute match (324000 ticks).
        #[arg(long)]
        full_match: bool,

        /// Output file for the final state (JSON).
        #[arg(short, long)]
        output: Option<String>,

        /// Write a Perfetto-compatible Chrome JSON trace to this path.
        #[arg(long)]
        trace_out: Option<PathBuf>,

        /// Record a snapshot every N ticks (requires --trace-out to have effect).
        #[arg(long, default_value_t = TelemetryConfig::DEFAULT_INTERVAL_TICKS)]
        trace_interval_ticks: u64,

        /// Record every tick in the inclusive range `<start>-<end>` (requires --trace-out).
        #[arg(long)]
        trace_full_range: Option<String>,
    },

    /// Replay a simulation with manager commands.
    Replay {
        /// Random seed.
        #[arg(short, long)]
        seed: u64,

        /// JSON file containing manager commands (list of {tick, command}).
        #[arg(short, long)]
        commands: String,

        /// Output file for the event sequence (JSON).
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Run a performance benchmark.
    Benchmark {
        /// Random seed.
        #[arg(short, long, default_value_t = 12345)]
        seed: u64,

        /// Number of ticks to simulate.
        #[arg(short, long, default_value_t = 6000)]
        ticks: u64,

        /// Output file for the benchmark results (JSON).
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Filter a recorded Chrome trace down to one player / tick window.
    TraceFilter {
        /// Path to the trace file (a JSON array of Chrome events).
        #[arg(short, long)]
        input: PathBuf,

        /// Output file (defaults to stdout).
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Keep events for the given entity index (translates to `pid = 1000 + idx`).
        #[arg(long)]
        player: Option<u32>,

        /// Inclusive lower tick bound.
        #[arg(long)]
        from: Option<u64>,

        /// Inclusive upper tick bound.
        #[arg(long)]
        to: Option<u64>,

        /// Keep events on a named process (`Ball`, `Referee`, `Match`, ...).
        #[arg(long)]
        process: Option<String>,

        /// Keep events with this exact `name`.
        #[arg(long)]
        name: Option<String>,
    },
}

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
        } => replay(seed, commands, output)?,

        Commands::Benchmark {
            seed,
            ticks,
            output,
        } => benchmark(seed, ticks, output)?,

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
    }
    Ok(())
}

/// Inlined replay path — kept private so the dispatch function stays short.
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

use sim_core::MatchSnapshot;

pub use sim_components::CommandError;

pub struct ServerSimulation {
    simulation: Simulation,
    command_queue: CommandQueue,
}

impl ServerSimulation {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            simulation: Simulation::new(seed),
            command_queue: CommandQueue::new(),
        }
    }

    pub fn tick(&mut self) {
        // Apply queued commands at tick boundaries
        let commands = self.command_queue.apply_at_tick(self.simulation.current_tick());
        for command in commands {
            // Apply command (validation already done when queueing)
            self.apply_command_immediately(command);
        }

        // Advance simulation
        self.simulation.tick();
    }

    /// Validate a `ManagerCommand` against the current match state and
    /// enqueue it for application at the next tick boundary.
    ///
    /// Validation is performed by
    /// `sim_core::Simulation::validate_command` (the single source of
    /// truth for command legality). On success the command is enqueued
    /// for execution when `simulation.tick` reaches
    /// `simulation.tick + 1`. The queued command is applied by `tick()`
    /// via [`Self::apply_command_immediately`].
    ///
    /// # Errors
    ///
    /// Returns a [`CommandError::InvalidForState`] if the command is not
    /// legal in the current match state (e.g. formation/mentality/tactic
    /// changes during `PreMatch` / `HalfTime` / `FullTime`).
    pub fn apply_command(
        &mut self,
        command: ManagerCommand,
    ) -> Result<(), CommandError> {
        // Phase F §F5: validation lives in sim-core; sim-server delegates
        // so the rules are defined in exactly one place. The local wrapper
        // only handles command queueing for next-tick dispatch. The apply
        // happens at the queue dispatch (`apply_command_immediately`) so
        // that command application is aligned with the tick boundary.
        self.simulation
            .validate_command(self.simulation.match_entity(), &command)?;
        self.command_queue
            .enqueue(command, self.simulation.current_tick() + 1);
        Ok(())
    }

    /// Apply a previously-validated `ManagerCommand` to the simulation now.
    ///
    /// Callers must have already run validation through [`Self::apply_command`];
    /// this path only exists because commands queued at tick T-1 are
    /// dispatched at tick T's start, after validation has already happened.
    fn apply_command_immediately(&mut self, command: ManagerCommand) {
        // Phase F §F5 fix: the apply lives in sim-core. sim-server delegates
        // so the mutation is defined in exactly one place. This path is
        // home-only; side-aware apply is a future API addition.
        if let Err(e) = self
            .simulation
            .apply_validated_command(self.simulation.match_entity(), command)
        {
            tracing::warn!("command failed to apply: {e:?}");
        }
    }

    /// Snapshot the current state of the wrapped simulation.
    ///
    /// # Panics
    ///
    /// Panics if the underlying `Simulation::get_state` returns `Err`,
    /// which only happens when the `Match` resource has not been
    /// registered (i.e. `Simulation::new` was bypassed). All paths that
    /// construct a `ServerSimulation` via [`Self::new`] are guaranteed
    /// to have the resource available.
    pub fn get_state(&self) -> MatchSnapshot {
        self.simulation.get_state().unwrap()
    }
}

pub struct CommandQueue {
    commands: Vec<QueuedCommand>,
}

pub struct QueuedCommand {
    pub(crate) command: ManagerCommand,
    pub(crate) tick: u64,
}

impl Default for CommandQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandQueue {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn enqueue(&mut self, command: ManagerCommand, tick: u64) {
        self.commands.push(QueuedCommand { command, tick });
    }

    pub fn apply_at_tick(&mut self, tick: u64) -> Vec<ManagerCommand> {
        self.commands
            .drain(..)
            .filter(|cmd| cmd.tick <= tick)
            .map(|cmd| cmd.command)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_simulation() {
        let server = ServerSimulation::new(12345);
        assert_eq!(server.simulation.current_tick(), 0);
    }

    #[test]
    fn test_command_queue() {
        let mut queue = CommandQueue::new();
        queue.enqueue(
            ManagerCommand::ChangeFormation(sim_components::Formation::FourFourTwo),
            100,
        );
        let commands = queue.apply_at_tick(100);
        assert_eq!(commands.len(), 1);
    }

    #[test]
    fn test_validation_invalid_command_rejected() {
        let mut server = ServerSimulation::new(12345);

        // Try to change formation during PreMatch state (should fail)
        let result = server.apply_command(ManagerCommand::ChangeFormation(
            sim_components::Formation::FourThreeThree,
        ));
        assert!(result.is_err());

        match result {
            Err(CommandError::InvalidForState {
                current_state,
                required_state,
            }) => {
                assert_eq!(current_state, sim_components::MatchState::PreMatch);
                assert_eq!(required_state, sim_components::MatchState::InPlay);
            }
            _ => panic!("Expected InvalidForState error"),
        }
    }

    #[test]
    fn test_validation_valid_command_accepted() {
        let mut server = ServerSimulation::new(12345);

        // Set match state to InPlay (Phase C §4.3: Match is now a Resource).
        server
            .simulation
            .world_mut()
            .resource_mut::<sim_components::Match>()
            .state = sim_components::MatchState::InPlay;

        // Try to change formation during InPlay state (should succeed)
        let result = server.apply_command(ManagerCommand::ChangeFormation(
            sim_components::Formation::FourThreeThree,
        ));
        assert!(result.is_ok());
    }

    /// After F5's `apply_command` refactor, commands target the home team.
    /// `active_manager_side` was removed because it had no consumers.
    /// If away-management is added back, this test is the deliberate-API-change pin.
    ///
    /// Note: `apply_command` enqueues at `tick + 1` for next-tick dispatch.
    /// We enqueue manually at the current tick so a single `tick()` drains
    /// and applies the command — testing the apply path directly without
    /// depending on the queue's enqueue-tick semantics.
    #[test]
    fn apply_command_targets_home_team_only() {
        let mut server = ServerSimulation::new(12345);
        // Move to InPlay so the command validates.
        server.simulation.world_mut().resource_mut::<sim_components::Match>().state = sim_components::MatchState::InPlay;

        let home = server.simulation.world().resource::<sim_components::Match>().home_team;
        let away = server.simulation.world().resource::<sim_components::Match>().away_team;

        // Enqueue directly at the current tick and tick once. This exercises
        // `tick()`'s drain-and-apply path (`apply_command_immediately` →
        // `sim_core::apply_validated_command`) without depending on
        // `apply_command`'s `tick + 1` enqueue semantics.
        server.command_queue.enqueue(
            ManagerCommand::ChangeFormation(sim_components::Formation::FourThreeThree),
            server.simulation.current_tick(),
        );
        server.tick();

        let home_formation = server.simulation.world().entity(home).get::<sim_components::Team>().unwrap().formation;
        let away_formation = server.simulation.world().entity(away).get::<sim_components::Team>().unwrap().formation;

        assert_eq!(home_formation, sim_components::Formation::FourThreeThree, "home team should have new formation");
        assert_eq!(away_formation, sim_components::Formation::FourFourTwo, "away team should still have default formation");
    }

    #[test]
    fn test_state_query_all_state_from_server() {
        let server = ServerSimulation::new(12345);

        // Get state snapshot
        let state = server.get_state();

        // Verify state contains expected fields
        assert_eq!(state.tick, 0);
        assert!(state.match_state.state == sim_components::MatchState::PreMatch);
        assert_eq!(state.score, (0, 0));
        assert!(state.clock.is_running);
        assert!(state.state_hash > 0);

        // Verify players are present (22 players total)
        assert_eq!(state.players.len(), 22);
    }

    /// Runs a full 90-minute match (324,000 ticks at 60 Hz). This test is
    /// 2-3 orders of magnitude slower than every other test in the
    /// workspace — minutes vs. milliseconds — and is the dominant cost
    /// in `cargo test`. It is `#[ignore]`d by default; run explicitly
    /// when verifying end-to-end correctness:
    ///
    /// ```text
    /// cargo test -p sim-server --lib -- --ignored test_end_to_end_full_match_simulation
    /// ```
    #[test]
    #[ignore = "full-match test is minutes-long; run only when verifying end-to-end behaviour"]
    fn test_end_to_end_full_match_simulation() {
        let mut server = ServerSimulation::new(42);

        // Phase C §4.3: Match is a Resource.
        server
            .simulation
            .world_mut()
            .resource_mut::<sim_components::Match>()
            .state = sim_components::MatchState::Kickoff;

        let full_match_ticks: u64 = 324_000;
        for _ in 0..full_match_ticks {
            server.tick();
        }

        let state = server.get_state();
        assert_eq!(state.tick, full_match_ticks);
        assert_eq!(state.players.len(), 22);
        assert!(state.state_hash > 0);
    }

    #[test]
    fn test_determinism_two_seeds_differ() {
        let mut s1 = ServerSimulation::new(111);
        for _ in 0..1000 {
            s1.tick();
        }
        let h1 = s1.get_state().state_hash;

        let mut s2 = ServerSimulation::new(222);
        for _ in 0..1000 {
            s2.tick();
        }
        let h2 = s2.get_state().state_hash;

        assert_ne!(h1, h2);
    }
}
