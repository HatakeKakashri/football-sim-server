//! Thin CLI dispatcher. The CLI parsing surface lives in `lib::Cli` and the
//! per-subcommand logic lives in `simulate`, `replay`, `benchmark`, and
//! `trace_filter` modules — `main.rs` only parses args and forwards to
//! `dispatch()` per coding standards §7.

use clap::Parser;
use sim_server::Cli as ServerCli;
use sim_server::dispatch;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = ServerCli::parse();
    dispatch(cli)
}