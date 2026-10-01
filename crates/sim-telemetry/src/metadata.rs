//! Run-level metadata written into the Chrome trace's `M` event.
//!
//! Without this metadata, Perfetto reads the layer's microsecond `ts` values
//! as milliseconds and renders the timeline 1000x too slow. The metadata is
//! also the only place a developer opening the file 6 months from now sees
//! the run identity (seed, interval, full range).
//!
//! [`crate::ChromeTraceLayer::with_metadata`] writes one
//! `process_labels` `M` event at startup; without metadata the layer writes
//! nothing new (preserving the pre-feature output for unit tests and library
//! consumers that haven't opted in).

/// What the CLI / library hands the layer so it can stamp the trace header.
///
/// Cheap to clone; embedded into the layer once at construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceMetadata {
    seed: u64,
    interval_ticks: u64,
    full_range: Option<(u64, u64)>,
}

impl TraceMetadata {
    /// Construct a metadata record. `interval_ticks` must be `> 0`; the caller
    /// (`TelemetryConfig`) has already enforced that, so we trust it.
    #[must_use]
    pub const fn new(
        seed: u64,
        interval_ticks: u64,
        full_range: Option<(u64, u64)>,
    ) -> Self {
        Self {
            seed,
            interval_ticks,
            full_range,
        }
    }

    /// Seed used for the deterministic RNG.
    #[must_use]
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// Coarse cadence in ticks.
    #[must_use]
    pub const fn interval_ticks(&self) -> u64 {
        self.interval_ticks
    }

    /// Inclusive full-resolution range, if any.
    #[must_use]
    pub const fn full_range(&self) -> Option<(u64, u64)> {
        self.full_range
    }
}

impl From<&super::TelemetryConfig> for TraceMetadata {
    fn from(config: &super::TelemetryConfig) -> Self {
        Self::new(0, config.interval_ticks(), config.full_range())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_what_was_put_in() {
        let m = TraceMetadata::new(7, 600, Some((100, 200)));
        assert_eq!(m.seed(), 7);
        assert_eq!(m.interval_ticks(), 600);
        assert_eq!(m.full_range(), Some((100, 200)));
    }

    #[test]
    fn from_config_carries_them_without_seed() {
        let c = super::super::TelemetryConfig::new(60, Some((100, 200))).expect("valid");
        let m = TraceMetadata::from(&c);
        assert_eq!(m.seed(), 0);
        assert_eq!(m.interval_ticks(), 60);
        assert_eq!(m.full_range(), Some((100, 200)));
    }
}