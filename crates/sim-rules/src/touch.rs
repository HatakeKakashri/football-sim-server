//! Touch registration consumer.
//!
//! `apply_ball_touched_system` drains `PendingBallTouches` at the
//! start of the Rules set and writes the last (latest-in-schedule)
//! `LastTouch` to `Ball.last_touch`. Writers across Execution,
//! Possession, and the earlier Rules systems push in fixed schedule
//! order, so the last entry is the deterministically-correct touch
//! for the tick.

use bevy_ecs::prelude::*;
use sim_components::{Ball, PendingBallTouches};

/// Drains `PendingBallTouches` and applies the last touch to
/// `Ball.last_touch`.
///
/// Runs at the **start** of the `Rules` set so that offside
/// detection, OOB detection, goal detection, and restart placement
/// all see the latest touch in the same tick.
///
/// If the buffer is empty (e.g., the first tick after kickoff
/// before any kick), `Ball.last_touch` is unchanged — the direct
/// write from `lifecycle_system` (kickoff placement) is the source
/// of truth for those ticks.
pub fn apply_ball_touched_system(mut pending: ResMut<PendingBallTouches>, mut ball: ResMut<Ball>) {
    if let Some(last) = pending.0.pop() {
        ball.last_touch = Some(last);
    }
    // Drain any earlier entries defensively (shouldn't happen, but
    // keeps the buffer from growing unbounded across ticks if a
    // future consumer partial-reads).
    pending.0.clear();
}
