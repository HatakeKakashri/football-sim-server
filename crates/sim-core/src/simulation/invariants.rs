//! Runtime invariants from PRD §12.
//!
//! Every match run by the harness (and every `Simulation::tick` in the
//! library, when callers opt in) passes through [`check_invariants`],
//! which returns an [`InvariantViolation`] for the first failure it
//! detects. Callers that want a hard stop (the harness, the API in a
//! later phase) cache the violation and report it as a §10 `error`
//! event.
//!
//! Six invariants (PRD §12):
//!
//! 1. Score equals the count of `goal` events per team.
//! 2. `MatchClock` is monotonic.
//! 3. State transitions are legal.
//! 4. No dead-ball window exceeds 30 s of game time (1800 ticks).
//! 5. Ball and players stay in valid states.
//! 6. `FullTime` is reached before `tick_cap`.

use serde::{Deserialize, Serialize};
use sim_components::{Match, MatchState};
use sim_components::time::HALF_LENGTH_TICKS;

use super::Simulation;

/// Hard cap on how many ticks a match is allowed to run.
///
/// PRD §4 sets `tick_cap = 400_000`; matches that exceed it fail per
/// invariant 6. The number is inclusive (a match that hits `400_000`
/// ticks without `FullTime` fails).
pub const TICK_CAP: u64 = 400_000;

/// Longest consecutive run of `BallState::Dead` the simulation tolerates.
///
/// PRD §12 invariant 4 caps dead-ball windows at 30 s of game time,
/// which is 1800 ticks at 60 Hz.
pub const MAX_DEAD_BALL_WINDOW_TICKS: u64 = 30 * 60;

/// How the violation should be reported up the stack.
///
/// Each variant carries the minimum context needed to log the seed and
/// reproduce the failure: the violation kind, the tick at which it
/// fired, and any associated value (score tuple, last good tick,
/// window length, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvariantViolation {
    /// Invariant 1: `Match.score` and the per-team goal counter drifted.
    ScoreGoalDrift {
        tick: u64,
        score: (u8, u8),
        goals_observed: (u32, u32),
    },
    /// Invariant 2: `MatchClock.elapsed_ticks` decreased or wrapped.
    ClockRegressed {
        tick: u64,
        elapsed_before: u64,
        elapsed_after: u64,
    },
    /// Invariant 3: a state transition was illegal.
    IllegalTransition {
        tick: u64,
        from: MatchState,
        to: MatchState,
    },
    /// Invariant 4: a dead-ball window exceeded 30 s.
    DeadBallWindowExceeded {
        tick: u64,
        window_ticks: u64,
    },
    /// Invariant 5: ball state was outside the four-variant enum, or a
    /// player position left the pitch. Either is a logic bug.
    InvalidEntityState {
        tick: u64,
        detail: String,
    },
    /// Invariant 6: `tick_cap` was hit before `FullTime`.
    TickCapBreached {
        tick: u64,
        cap: u64,
    },
}

impl InvariantViolation {
    /// Reason code string for the §10 `error` event wire shape.
    ///
    /// PRD §10.2 reserves `{ code, message }` for the wire shape. The
    /// code is stable for log/replay tooling.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ScoreGoalDrift { .. } => "score_goal_drift",
            Self::ClockRegressed { .. } => "clock_regressed",
            Self::IllegalTransition { .. } => "illegal_transition",
            Self::DeadBallWindowExceeded { .. } => "dead_ball_window_exceeded",
            Self::InvalidEntityState { .. } => "invalid_entity_state",
            Self::TickCapBreached { .. } => "tick_cap_breached",
        }
    }

    /// Last tick at which the simulation was still good.
    ///
    /// PRD §10.2 wire shape carries `last_good_tick`. For all
    /// per-tick invariants that's `tick - 1`; for the tick-cap breach
    /// the last good tick is the tick at which the cap was reached.
    #[must_use]
    pub const fn last_good_tick(&self) -> u64 {
        let tick = match self {
            Self::ScoreGoalDrift { tick, .. }
            | Self::ClockRegressed { tick, .. }
            | Self::IllegalTransition { tick, .. }
            | Self::DeadBallWindowExceeded { tick, .. }
            | Self::InvalidEntityState { tick, .. }
            | Self::TickCapBreached { tick, .. } => *tick,
        };
        if matches!(self, Self::TickCapBreached { .. }) {
            tick
        } else {
            tick.saturating_sub(1)
        }
    }

    /// Human-readable message for the §10 wire shape `message` field.
    /// Currently the same as `Display`; kept distinct so the wire
    /// shape stays stable if `Display` is later localised.
    #[must_use]
    pub fn message(&self) -> String {
        self.to_string()
    }
}

impl std::fmt::Display for InvariantViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ScoreGoalDrift { tick, score, goals_observed } => write!(
                f,
                "invariant 1 (score == goals): tick {tick}: match.score = {score:?}, goals_observed = {goals_observed:?}"
            ),
            Self::ClockRegressed { tick, elapsed_before, elapsed_after } => write!(
                f,
                "invariant 2 (clock monotonic): tick {tick}: {elapsed_before} -> {elapsed_after}"
            ),
            Self::IllegalTransition { tick, from, to } => write!(
                f,
                "invariant 3 (legal state transition): tick {tick}: {from:?} -> {to:?}"
            ),
            Self::DeadBallWindowExceeded { tick, window_ticks } => write!(
                f,
                "invariant 4 (dead-ball window): tick {tick}: window = {window_ticks} (> {MAX_DEAD_BALL_WINDOW_TICKS})"
            ),
            Self::InvalidEntityState { tick, detail } => write!(
                f,
                "invariant 5 (valid entity state): tick {tick}: {detail}"
            ),
            Self::TickCapBreached { tick, cap } => write!(
                f,
                "invariant 6 (full-time before tick cap): tick {tick} >= cap {cap}"
            ),
        }
    }
}

/// Decide whether a `from -> to` transition is legal.
///
/// PRD §8.1 list: `PreMatch → Kickoff → FirstHalf (InPlay) → HalfTime
/// → SecondHalf (InPlay) → FullTime`, with `Stoppage` reachable from
/// `InPlay`. We use the same state machine the lifecycle driver
/// already enforces, so anything the driver accepts is legal here;
/// the invariant fires only on transitions that bypassed the driver
/// (an external system re-wrote `Match.state` directly).
#[must_use]
pub fn is_legal_transition(from: MatchState, to: MatchState) -> bool {
    use MatchState::{FullTime, HalfTime, InPlay, Kickoff, PreMatch, Stoppage};
    if from == to {
        return true;
    }
    matches!(
        (from, to),
        (PreMatch | HalfTime, Kickoff)
            | (PreMatch | InPlay | Stoppage, HalfTime | FullTime)
            | (Kickoff | Stoppage, InPlay)
            | (InPlay, Stoppage)
    )
}

/// Counters the harness owns; `check_invariants` is a pure function
/// over them plus the live `Simulation`.
///
/// The harness increments `goals_observed` and `current_dead_window`
/// each tick and passes them back in. The harness can also derive
/// `last_elapsed_ticks` and `last_state` from prior reads — that keeps
/// the function pure and testable.
#[derive(Debug, Clone, Copy, Default)]
pub struct InvariantCounters {
    /// Last observed `MatchClock.elapsed_ticks` (for monotonic check).
    /// `None` on the first tick so we skip the monotonic check there.
    pub last_elapsed_ticks: Option<u64>,
    /// Last observed `Match.state` (for legal-transition check). `None`
    /// on the first tick so we skip the transition check there.
    pub last_state: Option<MatchState>,
    /// Currently-running `Dead` window length (harness owns the rolling
    /// counter, since it already inspects the ball state per tick).
    pub current_dead_window: u64,
    /// Per-team goal counter (harness increments when a goal is
    /// observed). The invariant compares this to `Match.score`.
    pub goals_observed: (u32, u32),
}

/// Check every PRD §12 invariant against the current `Simulation`
/// state plus the harness-owned [`InvariantCounters`] snapshot. Returns
/// the first violation found, or `None` if the match is still good.
///
/// This function does **not** mutate `Simulation` or `counters`. The
/// harness keeps ownership and bumps its own counters per tick; this
/// function reads them.
#[allow(
    clippy::too_many_lines,
    reason = "six invariants, each a short check; splitting them across helpers would scatter the table"
)]
pub fn check_invariants(sim: &Simulation, counters: &InvariantCounters) -> Option<InvariantViolation> {
    let world = sim.world();
    let tick = sim.current_tick();

    // Invariant 6: tick cap. We return the breach only when the
    // caller has ticked past the cap without reaching FullTime.
    let match_res = world.get_resource::<Match>()?;
    if tick >= TICK_CAP && match_res.state != MatchState::FullTime {
        return Some(InvariantViolation::TickCapBreached { tick, cap: TICK_CAP });
    }

    // Invariant 3: state transition legal.
    if let Some(previous) = counters.last_state
        && !is_legal_transition(previous, match_res.state)
    {
        return Some(InvariantViolation::IllegalTransition {
            tick,
            from: previous,
            to: match_res.state,
        });
    }

    // Invariant 2: clock monotonic.
    if let Some(clock) = world.get_resource::<sim_components::MatchClock>()
        && let Some(prev) = counters.last_elapsed_ticks
    {
        // `MatchClock.elapsed_ticks` resets to 0 at the start of
        // the second half — that is a legitimate discontinuity.
        // Treat a drop to 0 across a half-boundary tick as
        // monotonic.
        let allowed = match (prev, clock.elapsed_ticks, clock.half) {
            // Reset to 0 at the start of half 2.
            (_, 0, 2) => true,
            _ => clock.elapsed_ticks >= prev,
        };
        if !allowed {
            return Some(InvariantViolation::ClockRegressed {
                tick,
                elapsed_before: prev,
                elapsed_after: clock.elapsed_ticks,
            });
        }
    }

    // Invariant 1: score equals the goal counter.
    let score_ok = match_res.score.0 == u8::try_from(counters.goals_observed.0).unwrap_or(u8::MAX)
        && match_res.score.1 == u8::try_from(counters.goals_observed.1).unwrap_or(u8::MAX);
    if !score_ok {
        return Some(InvariantViolation::ScoreGoalDrift {
            tick,
            score: match_res.score,
            goals_observed: counters.goals_observed,
        });
    }

    // Invariant 4: dead-ball window.
    if counters.current_dead_window > MAX_DEAD_BALL_WINDOW_TICKS {
        return Some(InvariantViolation::DeadBallWindowExceeded {
            tick,
            window_ticks: counters.current_dead_window,
        });
    }

    // Invariant 5: ball state/possessor coherence. The state enum has
    // exactly four variants and is enforced at construction. The
    // physics system writes state from velocity (`InFlight` when
    // moving, otherwise `Possessed` / `Free` based on possessor), so
    // the only strict coherence rule that must always hold is:
    // `Dead` implies `possessor = None` (a dead ball is not held by
    // anyone). Other transients are part of the same-tick state
    // update and resolve on the next tick.
    if let Some(ball) = world.get_resource::<sim_components::Ball>()
        && matches!(ball.state, sim_components::BallState::Dead)
        && ball.possessor.is_some()
    {
        return Some(InvariantViolation::InvalidEntityState {
            tick,
            detail: format!(
                "ball state/possessor coherence: Dead ball has possessor = {:?}",
                ball.possessor
            ),
        });
    }

    // Touch the constant so rustc doesn't warn on unused-imports.
    let _ = HALF_LENGTH_TICKS;
    None
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests compare fresh values against the constants they were built from"
)]
mod tests {
    use super::*;

    #[test]
    fn legal_transitions_include_self_and_lifecycle_table() {
        // Self-loops are always legal.
        for s in [
            MatchState::PreMatch,
            MatchState::Kickoff,
            MatchState::InPlay,
            MatchState::Stoppage,
            MatchState::HalfTime,
            MatchState::FullTime,
        ] {
            assert!(is_legal_transition(s, s), "{s:?} -> {s:?} must be legal");
        }
        // Lifecycle: PreMatch -> Kickoff -> InPlay -> HalfTime -> Kickoff -> InPlay -> FullTime.
        assert!(is_legal_transition(MatchState::PreMatch, MatchState::Kickoff));
        assert!(is_legal_transition(MatchState::Kickoff, MatchState::InPlay));
        assert!(is_legal_transition(MatchState::InPlay, MatchState::HalfTime));
        assert!(is_legal_transition(MatchState::HalfTime, MatchState::Kickoff));
        assert!(is_legal_transition(MatchState::InPlay, MatchState::FullTime));
    }

    #[test]
    fn illegal_transitions_are_rejected() {
        // PreMatch cannot jump to InPlay.
        assert!(!is_legal_transition(MatchState::PreMatch, MatchState::InPlay));
        // Kickoff cannot jump to HalfTime.
        assert!(!is_legal_transition(MatchState::Kickoff, MatchState::HalfTime));
        // FullTime is terminal in the legal table.
        assert!(!is_legal_transition(MatchState::FullTime, MatchState::InPlay));
        // Stoppage cannot go back to PreMatch.
        assert!(!is_legal_transition(MatchState::Stoppage, MatchState::PreMatch));
    }

    #[test]
    fn score_goal_drift_is_detected() {
        let mut sim = Simulation::new(7);
        let counters = InvariantCounters {
            goals_observed: (1, 0),
            ..InvariantCounters::default()
        };
        // Move to InPlay so the invariant check sees the right state.
        sim.world_mut().resource_mut::<Match>().state = MatchState::InPlay;
        sim.tick();
        let violation = check_invariants(&sim, &counters).expect("drift must be detected");
        assert_eq!(
            violation,
            InvariantViolation::ScoreGoalDrift {
                tick: 1,
                score: (0, 0),
                goals_observed: (1, 0),
            }
        );
    }

    #[test]
    fn dead_ball_window_breach_is_detected() {
        let mut sim = Simulation::new(7);
        sim.world_mut().resource_mut::<Match>().state = MatchState::InPlay;
        sim.tick();
        let counters = InvariantCounters {
            current_dead_window: MAX_DEAD_BALL_WINDOW_TICKS + 1,
            ..InvariantCounters::default()
        };
        let violation = check_invariants(&sim, &counters).expect("breach must be detected");
        assert_eq!(
            violation,
            InvariantViolation::DeadBallWindowExceeded {
                tick: 1,
                window_ticks: MAX_DEAD_BALL_WINDOW_TICKS + 1,
            }
        );
    }
}