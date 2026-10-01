//! Post-match tick observability: recording configuration, per-tick gate,
//! `tracing` call-site helpers and a deterministic Chrome/Perfetto JSON layer.
#![deny(clippy::unwrap_used)]

mod capture;
mod chrome;
mod config;
pub mod emit;
mod gate;
mod metadata;
mod output;

pub use capture::{capture_trace, capture_trace_with};
pub use chrome::{ChromeTraceLayer, TraceGuard};
pub use config::parse_full_range;
pub use config::{TelemetryConfig, TelemetryError, should_record, should_record_decisions};
pub use gate::TraceGate;
pub use metadata::TraceMetadata;
pub use output::{install, install_with_metadata, open, open_with_metadata};
