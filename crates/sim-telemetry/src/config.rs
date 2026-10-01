//! Recording configuration and the pure "is this tick recorded?" predicates.

use std::fmt;

use bevy_ecs::prelude::Resource;

/// Recording configuration: a coarse snapshot cadence plus an optional
/// inclusive tick range recorded at every tick.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct TelemetryConfig {
    interval_ticks: u64,
    full_range: Option<(u64, u64)>,
}

/// Invalid telemetry configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelemetryError {
    /// `--trace-interval-ticks 0` is meaningless (not "every tick").
    ZeroInterval,
    /// `--trace-full-range` with `start > end`.
    InvertedRange { start: u64, end: u64 },
    /// `--trace-full-range` not of the form `<start>-<end>`.
    MalformedRange(String),
    /// `--trace-full-range` was supplied without `--trace-out`.
    RangeWithoutOutput,
    /// The trace output file could not be created.
    Io(String),
    /// A global tracing subscriber was already installed.
    SubscriberAlreadySet,
}

impl fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroInterval => write!(f, "trace interval must be greater than 0"),
            Self::InvertedRange { start, end } => {
                write!(f, "trace full range start {start} is after end {end}")
            }
            Self::MalformedRange(raw) => {
                write!(f, "trace full range `{raw}` must look like <start>-<end>")
            }
            Self::RangeWithoutOutput => {
                write!(f, "--trace-full-range requires --trace-out")
            }
            Self::Io(msg) => write!(f, "cannot create trace output: {msg}"),
            Self::SubscriberAlreadySet => write!(f, "a tracing subscriber is already installed"),
        }
    }
}

impl std::error::Error for TelemetryError {}

impl TelemetryConfig {
    /// Default coarse cadence: one snapshot every 10 seconds at 60 Hz.
    ///
    /// 60 (one per second) produced ~660 MB for a full match, mostly decision
    /// spans; 600 keeps a full-match trace around 67 MB.
    pub const DEFAULT_INTERVAL_TICKS: u64 = 600;

    /// Validated constructor.
    ///
    /// # Errors
    ///
    /// `ZeroInterval` when `interval_ticks == 0`; `InvertedRange` when `start > end`.
    pub const fn new(
        interval_ticks: u64,
        full_range: Option<(u64, u64)>,
    ) -> Result<Self, TelemetryError> {
        if interval_ticks == 0 {
            return Err(TelemetryError::ZeroInterval);
        }
        if let Some((start, end)) = full_range
            && start > end
        {
            return Err(TelemetryError::InvertedRange { start, end });
        }
        Ok(Self {
            interval_ticks,
            full_range,
        })
    }

    /// Coarse cadence in ticks (always `> 0`).
    #[must_use]
    pub const fn interval_ticks(&self) -> u64 {
        self.interval_ticks
    }

    /// Inclusive full-resolution range, if any.
    #[must_use]
    pub const fn full_range(&self) -> Option<(u64, u64)> {
        self.full_range
    }

    /// Clamp the full range into `0..total_ticks`; a range starting at or past
    /// `total_ticks` (or any range when `total_ticks == 0`) is dropped.
    #[must_use]
    pub fn clamp_to(self, total_ticks: u64) -> Self {
        let full_range = match self.full_range {
            Some((start, end)) if start < total_ticks => Some((start, end.min(total_ticks - 1))),
            _ => None,
        };
        Self { full_range, ..self }
    }
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            interval_ticks: Self::DEFAULT_INTERVAL_TICKS,
            full_range: None,
        }
    }
}

/// Parse `<start>-<end>` into an inclusive range.
///
/// # Errors
///
/// `MalformedRange` for anything else; `InvertedRange` when `start > end`.
pub fn parse_full_range(raw: &str) -> Result<(u64, u64), TelemetryError> {
    let malformed = || TelemetryError::MalformedRange(raw.to_string());
    let (start, end) = raw.split_once('-').ok_or_else(malformed)?;
    let start: u64 = start.parse().map_err(|_| malformed())?;
    let end: u64 = end.parse().map_err(|_| malformed())?;
    if start > end {
        return Err(TelemetryError::InvertedRange { start, end });
    }
    Ok((start, end))
}

/// Whether `tick` (a `MatchClock` tick) is a recorded snapshot tick.
#[must_use]
pub fn should_record(tick: u64, config: &TelemetryConfig) -> bool {
    tick.is_multiple_of(config.interval_ticks)
        || config
            .full_range
            .is_some_and(|(start, end)| (start..=end).contains(&tick))
}

/// Whether a decision evaluated on `tick` belongs to a recorded window.
///
/// True when some recorded tick `t` satisfies `t <= tick < t + window`.
/// `window` is the decision cadence, so every player is evaluated exactly
/// once per window.
#[must_use]
pub fn should_record_decisions(tick: u64, config: &TelemetryConfig, window: u64) -> bool {
    if window == 0 {
        return false;
    }
    let lookback = window - 1;
    // Newest interval multiple at or before `tick`.
    let on_interval = tick % config.interval_ticks <= lookback;
    // Some tick of `[tick - lookback, tick]` lies in the full range.
    let in_range = config
        .full_range
        .is_some_and(|(start, end)| tick >= start && tick.saturating_sub(lookback) <= end);
    on_interval || in_range
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(interval: u64, range: Option<(u64, u64)>) -> TelemetryConfig {
        TelemetryConfig::new(interval, range).expect("valid config")
    }

    #[test]
    fn rejects_zero_interval() {
        assert_eq!(
            TelemetryConfig::new(0, None),
            Err(TelemetryError::ZeroInterval)
        );
    }

    #[test]
    fn rejects_inverted_range_but_accepts_single_tick_range() {
        assert_eq!(
            TelemetryConfig::new(60, Some((10, 9))),
            Err(TelemetryError::InvertedRange { start: 10, end: 9 })
        );
        assert!(TelemetryConfig::new(60, Some((10, 10))).is_ok());
    }

    #[test]
    fn default_is_600_ticks_no_range() {
        let c = TelemetryConfig::default();
        assert_eq!((c.interval_ticks(), c.full_range()), (600, None));
        assert_eq!(c.interval_ticks(), TelemetryConfig::DEFAULT_INTERVAL_TICKS);
    }

    #[test]
    fn parses_ranges_and_rejects_garbage() {
        assert_eq!(parse_full_range("100-160"), Ok((100, 160)));
        assert_eq!(
            parse_full_range("160-100"),
            Err(TelemetryError::InvertedRange {
                start: 160,
                end: 100
            })
        );
        for bad in ["", "100", "a-b", "1-2-3", "-5", "5-"] {
            assert_eq!(
                parse_full_range(bad),
                Err(TelemetryError::MalformedRange(bad.to_string())),
                "input {bad:?}"
            );
        }
    }

    #[test]
    fn interval_boundaries() {
        let c = cfg(60, None);
        assert!(should_record(0, &c));
        assert!(!should_record(1, &c));
        assert!(!should_record(59, &c));
        assert!(should_record(60, &c));
        assert!(!should_record(61, &c));
        assert!(should_record(120, &c));
    }

    #[test]
    fn default_interval_records_a_small_minority_of_ticks() {
        let c = TelemetryConfig::default();
        let recorded = (0..6000).filter(|t| should_record(*t, &c)).count();
        assert_eq!(recorded, 10);
    }

    #[test]
    fn full_range_edges_are_inclusive_and_not_double_counted() {
        let c = cfg(60, Some((100, 160)));
        assert!(!should_record(99, &c));
        assert!(should_record(100, &c));
        assert!(should_record(160, &c));
        assert!(!should_record(161, &c));
        // 120 is both an interval multiple and inside the range: still one boolean.
        let in_range = (100..=160).filter(|t| should_record(*t, &c)).count();
        assert_eq!(in_range, 61);
    }

    #[test]
    fn clamp_trims_or_drops_out_of_bounds_ranges() {
        assert_eq!(
            cfg(60, Some((10, 500))).clamp_to(100).full_range(),
            Some((10, 99))
        );
        assert_eq!(cfg(60, Some((100, 200))).clamp_to(100).full_range(), None);
        assert_eq!(cfg(60, Some((0, 5))).clamp_to(0).full_range(), None);
        assert_eq!(
            cfg(60, Some((10, 20))).clamp_to(100).full_range(),
            Some((10, 20))
        );
        assert_eq!(cfg(60, None).clamp_to(100).interval_ticks(), 60);
    }

    #[test]
    fn decision_window_covers_six_ticks_per_recorded_tick() {
        let c = cfg(60, None);
        let on: Vec<u64> = (0..130)
            .filter(|t| should_record_decisions(*t, &c, 6))
            .collect();
        let expected: Vec<u64> = (0..6).chain(60..66).chain(120..126).collect();
        assert_eq!(on, expected);
    }

    #[test]
    fn decision_window_with_full_range_extends_window_past_range_end() {
        let c = cfg(1000, Some((100, 110)));
        assert!(!should_record_decisions(99, &c, 6));
        assert!(should_record_decisions(100, &c, 6));
        assert!(should_record_decisions(115, &c, 6));
        assert!(!should_record_decisions(116, &c, 6));
    }

    #[test]
    fn zero_window_never_records_decisions() {
        assert!(!should_record_decisions(0, &cfg(60, None), 0));
    }

    #[test]
    fn window_larger_than_interval_records_every_tick() {
        let c = cfg(3, None);
        assert!((0..30).all(|t| should_record_decisions(t, &c, 6)));
    }
}
