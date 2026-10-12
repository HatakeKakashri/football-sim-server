//! Touch registration consumer.
//!
//! `apply_ball_touched_system` drains `PendingBallTouches` and writes
//! the last (latest-in-schedule) `LastTouch` to `Ball.last_touch`.
//! Writers across Execution, Possession, and the earlier Rules systems
//! push in fixed schedule order, so the last entry is the
//! deterministically-correct touch for the tick.
//!
//! **Two call sites**, both load-bearing — see the `PendingBallTouches`
//! doc in `sim-components` for the full contract:
//!
//! 1. `sim-core`'s `register_systems` registers it first in the `Rules`
//!    set, so the rules writers read the same-tick touch.
//! 2. `sim_core::Simulation::tick` invokes it via
//!    `world.run_system_once` after the post-schedule `lifecycle_system`
//!    returns, because `lifecycle_system`'s kickoff placement is
//!    otherwise invisible to site 1 on the tick it happens.
//!
//! Pinned by `sim-core/src/tests/touch_drain_tests.rs`.

use bevy_ecs::prelude::*;
use sim_components::{Ball, PendingBallTouches};

/// Drains `PendingBallTouches` and applies the last touch to
/// `Ball.last_touch`.
///
/// Registered first in the `Rules` set so that OOB detection, goal
/// detection, foul detection, and restart placement all see the latest
/// touch in the same tick. Also invoked post-schedule from
/// `Simulation::tick` to drain `lifecycle_system`'s kickoff placement.
///
/// If the buffer is empty, `Ball.last_touch` is unchanged.
pub fn apply_ball_touched_system(mut pending: ResMut<PendingBallTouches>, mut ball: ResMut<Ball>) {
    if let Some(last) = pending.0.pop() {
        ball.last_touch = Some(last);
    }
    // Drain any earlier entries defensively (shouldn't happen, but
    // keeps the buffer from growing unbounded across ticks if a
    // future consumer partial-reads).
    pending.0.clear();
}
