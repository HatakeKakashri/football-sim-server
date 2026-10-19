//! Swept proximity test: minimum distance between two moving points over
//! a unit time interval.

use crate::vec2::Vec2;

/// Result of a swept proximity test between two moving points over a unit
/// time interval.
///
/// The swept test checks whether two objects moving at constant velocity
/// come within `radius` meters of each other at any point during the tick.
/// This is critical for acquisition: a player moving at 5 m/tick (5 m/s at
/// 60 ticks/s) can traverse 5 m in a single tick, so a static end-of-tick
/// radius check at 1.5 m would miss the player entirely.
///
/// # Mathematical basis
///
/// For two points A and B moving at velocities `v_a` and `v_b` over
/// `t ∈ [0, 1]`:
/// - `A(t) = a_start + t * v_a`
/// - `B(t) = b_start + t * v_b`
///
/// The squared distance is `|A(t) - B(t)|² = a*t² + 2*b*t + c` where:
/// - `a = |v_a - v_b|²`  (relative speed squared)
/// - `b = (a_start - b_start) · (v_a - v_b)`  (initial separation · relative velocity)
/// - `c = |a_start - b_start|²`  (initial separation squared)
///
/// The minimum occurs at `t* = -b/a` (when `a > 0`). If `t* ∈ [0, 1]`,
/// the minimum distance is `sqrt(c - b²/a)`; otherwise the minimum is at
/// an endpoint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SweptProximity {
    /// The minimum distance between the two paths over the interval.
    pub min_distance: f32,
    /// `true` if the paths come within `radius` at any point.
    pub acquired: bool,
}

/// Compute the minimum distance between two moving points over a unit time
/// interval `[0, 1]` (one simulation tick), and report whether acquisition
/// (within `radius`) occurred.
///
/// This is a **swept collision** test: it considers the entire path each
/// object travels during the tick, not just the start or end positions.
///
/// # Parameters
///
/// - `a_start`: position of object A (e.g., player) at tick start.
/// - `a_vel`: velocity of object A (displacement per tick).
/// - `b_start`: position of object B (e.g., ball) at tick start.
/// - `b_vel`: velocity of object B (displacement per tick).
/// - `radius`: acquisition radius in metres.
///
/// # Returns
///
/// `SweptProximity { min_distance, acquired }` where `acquired` is
/// `min_distance <= radius`.
#[must_use]
pub fn swept_proximity(
    a_start: Vec2,
    a_vel: Vec2,
    b_start: Vec2,
    b_vel: Vec2,
    radius: f32,
) -> SweptProximity {
    let rel_vel = a_vel - b_vel;
    let rel_start = a_start - b_start;

    // Coefficients of |rel_start + t * rel_vel|² = a*t² + 2*b*t + c
    let a = rel_vel.length_squared();
    let b = rel_start.dot(rel_vel);
    let c = rel_start.length_squared();

    let min_distance;

    if a < f32::EPSILON {
        // Relative motion is negligible — fall back to static distance.
        // Both objects are moving together or are stationary relative to
        // each other, so the distance is essentially constant.
        min_distance = rel_start.length();
    } else {
        // Find t* = -b/a, the unconstrained minimum of the quadratic.
        let t_star = -b / a;

        let (dist_at_0, dist_at_1) = {
            let at_0 = rel_start.length();
            let at_1 = (rel_start + rel_vel).length();
            (at_0, at_1)
        };

        // If t* is outside [0, 1], the minimum is at an endpoint.
        // Otherwise, the minimum is at t* (interior).
        if (0.0..=1.0).contains(&t_star) {
            // Interior minimum: formula is min_dist² = c - b²/a.
            // This is the minimum squared distance at t = t*.
            let min_dist_sq = c - (b * b) / a;
            min_distance = if min_dist_sq > 0.0 {
                min_dist_sq.sqrt()
            } else {
                // Numerical edge case: paths intersect, distance is 0.
                0.0
            };
        } else {
            // Minimum is at whichever endpoint is closer.
            min_distance = dist_at_0.min(dist_at_1);
        }
    }

    SweptProximity {
        min_distance,
        acquired: min_distance <= radius,
    }
}
