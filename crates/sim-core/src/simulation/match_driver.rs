//! `MatchDriver`: single source of truth for the per-tick driving loop.
//!
//! Consumed by `simulate`, `harness`, and `api/runner` so the tick
//! contract (invariant order, dead-ball window, goal counting,
//! termination) lives in exactly one place.

use crate::{EventLog, InvariantCounters, InvariantViolation, MatchEvent, Simulation, check_invariants};
use sim_components::{Ball, BallState, Match, MatchClock, MatchState, TeamId};

/// What stopped the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Termination {
    FullTime,
    TickCap,
    InvariantViolated,
}

/// Per-tick observability hook.
///
/// Called once per tick, after `Simulation::tick()` and before the driver
/// updates its internal counters. The hook can perform side effects
/// (JSONL write, frame capture, logging) but cannot abort the loop —
/// termination is owned by the driver.
pub trait TickHook {
    /// Called after each `sim.tick()`.
    ///
    /// Return `Err` to stop the loop and propagate the error to the
    /// caller (e.g. I/O failure on JSONL write). The driver's counters
    /// are not updated on the failing tick.
    ///
    /// # Errors
    ///
    /// Returns an error when the caller's side effect (e.g. JSONL write)
    /// fails. The driver propagates this error to `run_match_driver`'s
    /// caller and does not update internal counters for the failing tick.
    fn on_tick(&mut self, sim: &Simulation) -> Result<(), String>;
}

/// Observable result of a run.
#[derive(Debug, Clone)]
pub struct MatchResult {
    pub termination: Termination,
    pub ticks_run: u64,
    pub final_state_hash: u64,
    pub violation: Option<InvariantViolation>,
    /// Longest consecutive `BallState::Dead` window during the run (PRD §13.1 A2).
    pub max_dead_ball_window: u64,
}

/// Run `sim` to `FullTime`, tick-cap, or invariant violation.
///
/// The loop body is fixed: tick → hook → maintain shared counters →
/// `check_invariants` → `FullTime` check. Callers customize via `hook`.
///
/// `tick_cap` is inclusive — a `tick_cap` of 0 runs zero ticks and
/// returns `Termination::TickCap` immediately.
///
/// # Errors
///
/// Returns an error when `hook.on_tick` returns `Err` (e.g. I/O failure
/// on JSONL write). On error, `sim` has been advanced by some number of
/// ticks (at least one, since `hook.on_tick` runs after `sim.tick()`)
/// but no `MatchResult` is produced.
pub fn run_match_driver(
    sim: &mut Simulation,
    tick_cap: u64,
    hook: &mut impl TickHook,
) -> Result<MatchResult, String> {
    /// Internal per-tick bookkeeping. Not exposed to callers — the driver's
    /// `MatchResult` carries the final aggregate values.
    #[derive(Default)]
    struct DriverCounters {
        // Fields for check_invariants (InvariantCounters superset)
        last_elapsed_ticks: Option<u64>,
        last_state: Option<MatchState>,
        current_dead_window: u64,
        goals_observed: (u32, u32),
        // Internal tracking
        events_seen: usize,
        max_dead_ball_window: u64,
    }

    let mut counters = DriverCounters::default();
    let mut violation: Option<InvariantViolation> = None;

    for _ in 0..tick_cap {
        sim.tick();

        // Caller side effects (JSONL write, frame capture, logging).
        hook.on_tick(sim)?;

        // --- Shared bookkeeping (owned by the driver) ---
        let world = sim.world();
        counters.last_elapsed_ticks = world.get_resource::<MatchClock>().map(|c| c.elapsed_ticks);
        counters.last_state = world.get_resource::<Match>().map(|m| m.state);

        if world.get_resource::<Ball>().is_some_and(|b| b.state == BallState::Dead) {
            counters.current_dead_window += 1;
            if counters.current_dead_window > counters.max_dead_ball_window {
                counters.max_dead_ball_window = counters.current_dead_window;
            }
        } else {
            counters.current_dead_window = 0;
        }

        // Count goals incrementally from `EventLog`. Only events appended
        // since the last tick are scanned, so per-tick cost stays constant.
        if let Some(log) = world.get_resource::<EventLog>() {
            for event in log.events.iter().skip(counters.events_seen) {
                if let MatchEvent::Goal { envelope, .. } = event {
                    match envelope.team {
                        Some(TeamId(0)) => counters.goals_observed.0 += 1,
                        _ => counters.goals_observed.1 += 1,
                    }
                }
            }
            counters.events_seen = log.events.len();
        }

        // --- Termination checks ---
        if let Some(v) = check_invariants(
            sim,
            &InvariantCounters {
                last_elapsed_ticks: counters.last_elapsed_ticks,
                last_state: counters.last_state,
                current_dead_window: counters.current_dead_window,
                goals_observed: counters.goals_observed,
            },
        ) {
            violation = Some(v);
            break;
        }
        if world.get_resource::<Match>().is_some_and(|m| m.state == MatchState::FullTime) {
            break;
        }
    }

    let termination = if violation.is_some() {
        Termination::InvariantViolated
    } else if sim.world().get_resource::<Match>().is_some_and(|m| m.state == MatchState::FullTime) {
        Termination::FullTime
    } else {
        Termination::TickCap
    };

    Ok(MatchResult {
        termination,
        ticks_run: sim.current_tick(),
        final_state_hash: sim.get_state_hash(),
        violation,
        max_dead_ball_window: counters.max_dead_ball_window,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TICK_CAP;

    /// Minimal hook that counts how many times it was invoked.
    #[derive(Default)]
    struct CountingHook(u64);

    impl TickHook for CountingHook {
        fn on_tick(&mut self, _sim: &Simulation) -> Result<(), String> {
            self.0 += 1;
            Ok(())
        }
    }

    /// Hook that always fails on the first tick.
    struct FailingHook(&'static str);

    impl TickHook for FailingHook {
        fn on_tick(&mut self, _sim: &Simulation) -> Result<(), String> {
            Err(self.0.to_string())
        }
    }

    #[test]
    fn tick_cap_returns_tick_cap() {
        let mut sim = Simulation::new(7);
        let mut hook = CountingHook::default();
        let result = run_match_driver(&mut sim, 10, &mut hook).expect("run should succeed");
        assert_eq!(result.termination, Termination::TickCap);
        assert_eq!(result.ticks_run, 10);
        assert_eq!(hook.0, 10);
        assert!(result.violation.is_none());
    }

    #[test]
    fn hook_error_is_propagated() {
        let mut sim = Simulation::new(7);
        let mut hook = FailingHook("boom");
        let err = run_match_driver(&mut sim, 100, &mut hook).unwrap_err();
        assert_eq!(err, "boom");
    }

    #[test]
    fn hook_called_once_per_tick() {
        let mut sim = Simulation::new(7);
        let mut hook = CountingHook::default();
        let _ = run_match_driver(&mut sim, 5, &mut hook);
        assert_eq!(hook.0, 5);
    }

    #[test]
    fn max_dead_ball_window_is_zero_in_normal_run() {
        let mut sim = Simulation::new(7);
        let mut hook = CountingHook::default();
        let result = run_match_driver(&mut sim, 4, &mut hook).expect("run should succeed");
        // Ball is never Dead in a normal short run.
        assert_eq!(result.max_dead_ball_window, 0);
    }

    #[test]
    fn invariant_violation_terminates_run() {
        let mut sim = Simulation::new(7);
        // Mismatch score vs goals_observed(0,0): invariant 1 fires at tick 1.
        sim.world_mut().resource_mut::<Match>().score = (1, 0);
        let mut hook = CountingHook::default();
        let result = run_match_driver(&mut sim, 100, &mut hook).expect("run should succeed");
        assert_eq!(result.termination, Termination::InvariantViolated);
        assert!(result.violation.is_some());
        assert_eq!(hook.0, 1);
    }

    #[test]
    #[ignore = "full-match test is minutes-long; run explicitly when verifying termination"]
    fn full_match_reaches_full_time() {
        let mut sim = Simulation::new(12345);
        let mut hook = CountingHook::default();
        let result = run_match_driver(&mut sim, TICK_CAP, &mut hook).expect("run should succeed");
        assert_eq!(result.termination, Termination::FullTime);
        assert!(result.violation.is_none());
        assert!(result.ticks_run > 0);
    }
}
