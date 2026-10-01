//! Validation of the `simulate` trace flags, independent of clap so it can be
//! unit-tested (coding standards §7: CLI logic lives in the library).

use std::path::PathBuf;

use sim_telemetry::{TelemetryConfig, TelemetryError, parse_full_range};

/// Raw `--trace-*` flag values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceOptions {
    /// `--trace-out`: enables tracing when present.
    pub out: Option<PathBuf>,
    /// `--trace-interval-ticks` (default 60).
    pub interval_ticks: u64,
    /// `--trace-full-range <start>-<end>`.
    pub full_range: Option<String>,
}

impl Default for TraceOptions {
    fn default() -> Self {
        Self {
            out: None,
            interval_ticks: TelemetryConfig::DEFAULT_INTERVAL_TICKS,
            full_range: None,
        }
    }
}

/// A validated, ready-to-run tracing plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracePlan {
    /// Where the Chrome trace is written.
    pub path: PathBuf,
    /// Validated recording configuration.
    pub config: TelemetryConfig,
}

/// Validate the flags. `Ok(None)` means tracing is disabled.
///
/// `total_ticks` is the run length when known up front; it is `None` for
/// `--full-match`, whose length is only known once it ends (the range is then
/// left unclamped — ticks past the end simply never occur).
///
/// # Errors
///
/// Any [`TelemetryError`] describing the invalid flag combination.
pub fn plan_trace(
    options: &TraceOptions,
    total_ticks: Option<u64>,
) -> Result<Option<TracePlan>, TelemetryError> {
    let Some(path) = options.out.clone() else {
        return if options.full_range.is_some() {
            Err(TelemetryError::RangeWithoutOutput)
        } else {
            Ok(None)
        };
    };
    let full_range = options
        .full_range
        .as_deref()
        .map(parse_full_range)
        .transpose()?;
    let config = TelemetryConfig::new(options.interval_ticks, full_range)?;
    let config = match total_ticks {
        Some(total) => config.clamp_to(total),
        None => config,
    };
    Ok(Some(TracePlan { path, config }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(out: Option<&str>, interval: u64, range: Option<&str>) -> TraceOptions {
        TraceOptions {
            out: out.map(PathBuf::from),
            interval_ticks: interval,
            full_range: range.map(str::to_string),
        }
    }

    #[test]
    fn no_trace_out_means_disabled() {
        assert_eq!(plan_trace(&TraceOptions::default(), Some(1000)), Ok(None));
    }

    #[test]
    fn full_range_without_trace_out_is_rejected() {
        assert_eq!(
            plan_trace(&opts(None, 60, Some("1-2")), Some(1000)),
            Err(TelemetryError::RangeWithoutOutput)
        );
    }

    #[test]
    fn zero_interval_is_rejected_when_tracing() {
        assert_eq!(
            plan_trace(&opts(Some("t.json"), 0, None), Some(1000)),
            Err(TelemetryError::ZeroInterval)
        );
    }

    #[test]
    fn inverted_and_malformed_ranges_are_rejected() {
        assert_eq!(
            plan_trace(&opts(Some("t.json"), 60, Some("9-3")), Some(1000)),
            Err(TelemetryError::InvertedRange { start: 9, end: 3 })
        );
        assert_eq!(
            plan_trace(&opts(Some("t.json"), 60, Some("nope")), Some(1000)),
            Err(TelemetryError::MalformedRange("nope".to_string()))
        );
    }

    #[test]
    fn valid_flags_produce_a_plan() {
        let plan = plan_trace(&opts(Some("t.json"), 30, Some("100-160")), Some(1000))
            .expect("valid")
            .expect("enabled");
        assert_eq!(plan.path, PathBuf::from("t.json"));
        assert_eq!(plan.config.interval_ticks(), 30);
        assert_eq!(plan.config.full_range(), Some((100, 160)));
    }

    #[test]
    fn range_is_clamped_to_a_known_run_length_but_not_for_full_match() {
        let clamped = plan_trace(&opts(Some("t.json"), 60, Some("90-500")), Some(100))
            .expect("valid")
            .expect("enabled");
        assert_eq!(clamped.config.full_range(), Some((90, 99)));
        let unclamped = plan_trace(&opts(Some("t.json"), 60, Some("90-500")), None)
            .expect("valid")
            .expect("enabled");
        assert_eq!(unclamped.config.full_range(), Some((90, 500)));
    }
}
