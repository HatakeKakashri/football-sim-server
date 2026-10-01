//! Per-tick recording flags, computed once per tick by `Simulation::tick`.

use bevy_ecs::prelude::Resource;

use crate::{TelemetryConfig, should_record, should_record_decisions};

/// What the current tick should record. All-false by default, which is the
/// state of a simulation with telemetry disabled.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TraceGate {
    tick: u64,
    snapshot: bool,
    decisions: bool,
    last_snapshot_tick: Option<u64>,
    last_decision_tick: Option<u64>,
}

impl TraceGate {
    /// Recompute the flags for `tick` (a `MatchClock` tick). A snapshot is
    /// emitted at most once per clock tick even if the clock is paused; the same
    /// holds for decisions.
    pub fn advance(&mut self, tick: u64, config: &TelemetryConfig, decision_window: u64) {
        self.tick = tick;
        self.snapshot = should_record(tick, config) && self.last_snapshot_tick != Some(tick);
        if self.snapshot {
            self.last_snapshot_tick = Some(tick);
        }
        // The match clock stops during the pre-match and half-time breaks while
        // the schedule keeps running, so the same clock tick can be seen many
        // times. Decisions are traced once per clock tick (as snapshots are):
        // the spans are named `decision @ tick <n>`, and repeating them would
        // flood the trace and the per-tick timestamp budget.
        self.decisions = should_record_decisions(tick, config, decision_window)
            && self.last_decision_tick != Some(tick);
        if self.decisions {
            self.last_decision_tick = Some(tick);
        }
    }

    /// Clock tick the flags were computed for (used as the trace timestamp).
    #[must_use]
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// Emit position/ball/referee snapshots this tick.
    #[must_use]
    pub const fn snapshot(&self) -> bool {
        self.snapshot
    }

    /// Emit decision spans for players evaluated this tick.
    #[must_use]
    pub const fn decisions(&self) -> bool {
        self.decisions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_gate_records_nothing() {
        let g = TraceGate::default();
        assert!(!g.snapshot() && !g.decisions());
    }

    #[test]
    fn advance_sets_snapshot_on_interval_and_decisions_over_window() {
        let cfg = TelemetryConfig::new(60, None).expect("valid");
        let mut g = TraceGate::default();
        g.advance(0, &cfg, 6);
        assert!(g.snapshot() && g.decisions() && g.tick() == 0);
        g.advance(3, &cfg, 6);
        assert!(!g.snapshot() && g.decisions() && g.tick() == 3);
        g.advance(6, &cfg, 6);
        assert!(!g.snapshot() && !g.decisions());
    }

    #[test]
    fn paused_clock_records_snapshot_and_decisions_once() {
        let cfg = TelemetryConfig::new(60, None).expect("valid");
        let mut g = TraceGate::default();
        g.advance(60, &cfg, 6);
        assert!(g.snapshot() && g.decisions());
        // The clock is paused: the same clock tick is seen again.
        for _ in 0..1000 {
            g.advance(60, &cfg, 6);
            assert!(
                !g.snapshot() && !g.decisions(),
                "a repeated clock tick must not be recorded again"
            );
        }
        // The clock resumes: the next tick of the window records decisions again.
        g.advance(61, &cfg, 6);
        assert!(!g.snapshot() && g.decisions());
    }
}
