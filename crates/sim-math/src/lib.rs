use bevy_ecs::prelude::Resource;
use std::hash::{Hash, Hasher};
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[must_use]
    pub const fn zero() -> Self {
        Self { x: 0.0, y: 0.0 }
    }

    #[must_use]
    pub fn length_squared(self) -> f32 {
        self.y.mul_add(self.y, self.x * self.x)
    }

    #[must_use]
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    #[must_use]
    pub fn normalized(self) -> Self {
        let len = self.length();
        if len < f32::EPSILON {
            Self::zero()
        } else {
            self / len
        }
    }

    #[must_use]
    pub fn dot(self, other: Self) -> f32 {
        self.y.mul_add(other.y, self.x * other.x)
    }

    #[must_use]
    pub fn distance_squared(self, other: Self) -> f32 {
        (self - other).length_squared()
    }

    #[must_use]
    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }

    #[must_use]
    pub fn clamp_length(self, max_length: f32) -> Self {
        let len = self.length();
        if len > max_length {
            self * (max_length / len)
        } else {
            self
        }
    }
}

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

impl Add for Vec2 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub for Vec2 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl MulAssign<f32> for Vec2 {
    fn mul_assign(&mut self, rhs: f32) {
        self.x *= rhs;
        self.y *= rhs;
    }
}

impl Mul<Vec2> for f32 {
    type Output = Vec2;

    fn mul(self, rhs: Vec2) -> Self::Output {
        Vec2 {
            x: self * rhs.x,
            y: self * rhs.y,
        }
    }
}

impl Div<f32> for Vec2 {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x / rhs,
            y: self.y / rhs,
        }
    }
}

impl DivAssign<f32> for Vec2 {
    fn div_assign(&mut self, rhs: f32) {
        self.x /= rhs;
        self.y /= rhs;
    }
}

impl Eq for Vec2 {}

impl Hash for Vec2 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.x.to_bits().hash(state);
        self.y.to_bits().hash(state);
    }
}

#[derive(Debug, Clone, Copy, Resource)]
pub struct PitchDimensions {
    pub width: f32,
    pub length: f32,
    pub penalty_area: Rect,
    pub goal_area: Rect,
    pub center_circle: Circle,
}

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub min: Vec2,
    pub max: Vec2,
}

#[derive(Debug, Clone, Copy)]
pub struct Circle {
    pub center: Vec2,
    pub radius: f32,
}

impl PitchDimensions {
    #[must_use]
    pub const fn standard() -> Self {
        Self {
            width: 105.0,
            length: 68.0,
            // Standard pitch: penalty area is 16.5m from goal line, 40.32m wide.
            // The pitch is 105m long and 68m wide; goal line at x=0.
            penalty_area: Rect {
                min: Vec2::new(0.0, 13.84),
                max: Vec2::new(16.5, 54.16),
            },
            // Goal area is 5.5m from goal line, 18.32m wide.
            goal_area: Rect {
                min: Vec2::new(0.0, 24.84),
                max: Vec2::new(5.5, 43.16),
            },
            center_circle: Circle {
                center: Vec2::new(52.5, 34.0),
                radius: 9.15,
            },
        }
    }

    #[must_use]
    pub const fn width(&self) -> f32 {
        self.width
    }

    #[must_use]
    pub const fn length(&self) -> f32 {
        self.length
    }

    #[must_use]
    pub fn contains(&self, point: Vec2) -> bool {
        point.x >= 0.0 && point.x <= self.width && point.y >= 0.0 && point.y <= self.length
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests assert exact arithmetic results from integer-literal vector operands"
)]
mod tests {
    use super::*;

    #[test]
    fn test_vec2_operations() {
        let v1 = Vec2::new(1.0, 2.0);
        let v2 = Vec2::new(3.0, 4.0);

        assert_eq!(v1 + v2, Vec2::new(4.0, 6.0));
        assert_eq!(v2 - v1, Vec2::new(2.0, 2.0));
        assert_eq!(v1 * 2.0, Vec2::new(2.0, 4.0));
        assert_eq!(2.0 * v1, Vec2::new(2.0, 4.0));
        assert_eq!(v1.dot(v2), 11.0);
        assert_eq!(v1.length_squared(), 5.0);
    }

    #[test]
    fn test_swept_proximity_static_no_acquire() {
        // Player at (0,0), ball at (5,0), both stationary.
        // Distance is 5m > 1.5m acquisition radius.
        let result = swept_proximity(
            Vec2::new(0.0, 0.0),
            Vec2::zero(),
            Vec2::new(5.0, 0.0),
            Vec2::zero(),
            1.5,
        );
        assert_eq!(result.min_distance, 5.0);
        assert!(!result.acquired);
    }

    #[test]
    fn test_swept_proximity_static_acquire() {
        // Player at (0,0), ball at (1.0, 0), both stationary.
        // Distance is 1.0m < 1.5m acquisition radius.
        let result = swept_proximity(
            Vec2::new(0.0, 0.0),
            Vec2::zero(),
            Vec2::new(1.0, 0.0),
            Vec2::zero(),
            1.5,
        );
        assert_eq!(result.min_distance, 1.0);
        assert!(result.acquired);
    }

    #[test]
    fn test_swept_proximity_player_crosses_ball() {
        // Player moves from (0,0) to (5,0) — crosses the ball at (2.5, 0).
        // Ball stays at (2.5, 0). Minimum distance is 0 at t=0.5.
        let result = swept_proximity(
            Vec2::new(0.0, 0.0), // player start
            Vec2::new(5.0, 0.0), // player velocity: 5m/tick
            Vec2::new(2.5, 0.0), // ball start
            Vec2::zero(),        // ball stationary
            1.5,
        );
        assert!((result.min_distance - 0.0).abs() < 1e-6);
        assert!(result.acquired);
    }

    #[test]
    fn test_swept_proximity_player_misses_ball() {
        // Player moves from (0,0) to (5,0), ball stays at (0, 5).
        // Minimum distance is 5m (player passes 5m away).
        let result = swept_proximity(
            Vec2::new(0.0, 0.0),
            Vec2::new(5.0, 0.0),
            Vec2::new(0.0, 5.0),
            Vec2::zero(),
            1.5,
        );
        assert_eq!(result.min_distance, 5.0);
        assert!(!result.acquired);
    }

    #[test]
    fn test_swept_proximity_both_moving_towards() {
        // Player at (0,0) moving right at 3m/tick, ball at (5,0) moving left at 2m/tick.
        // They approach each other. Meeting point: t=1, position (1,0).
        // Minimum distance is 0 (they meet).
        let result = swept_proximity(
            Vec2::new(0.0, 0.0),
            Vec2::new(3.0, 0.0),
            Vec2::new(5.0, 0.0),
            Vec2::new(-2.0, 0.0),
            1.5,
        );
        assert!((result.min_distance - 0.0).abs() < 1e-6);
        assert!(result.acquired);
    }

    #[test]
    fn test_swept_proximity_ball_moving_past_stationary_player() {
        // Ball moving from (0,0) to (5,0), player stationary at (2.5, 0).
        // Ball passes directly through player at t=0.5.
        let result = swept_proximity(
            Vec2::new(2.5, 0.0),
            Vec2::zero(),
            Vec2::new(0.0, 0.0),
            Vec2::new(5.0, 0.0),
            1.5,
        );
        assert!((result.min_distance - 0.0).abs() < 1e-6);
        assert!(result.acquired);
    }

    #[test]
    fn test_swept_proximity_endpoint_acquire() {
        // Player starts at (1.4, 0) moving to (1.6, 0), ball at (0, 0) stationary.
        // Player's path comes within 1.4m at t=0, which is < 1.5m.
        let result = swept_proximity(
            Vec2::new(1.4, 0.0),
            Vec2::new(0.2, 0.0),
            Vec2::new(0.0, 0.0),
            Vec2::zero(),
            1.5,
        );
        assert!((result.min_distance - 1.4).abs() < 1e-5);
        assert!(result.acquired);
    }

    #[test]
    fn test_swept_proximity_past_endpoint_acquire() {
        // Player starts at (0, 0) moving to (3.0, 0), ball at (0, 1.4) stationary.
        // Minimum distance is 1.4 at t=0 (player passes 1.4m from ball).
        let result = swept_proximity(
            Vec2::new(0.0, 0.0),
            Vec2::new(3.0, 0.0),
            Vec2::new(0.0, 1.4),
            Vec2::zero(),
            1.5,
        );
        assert!((result.min_distance - 1.4).abs() < 1e-6);
        assert!(result.acquired);
    }

    #[test]
    fn test_swept_proximity_past_endpoint_no_acquire() {
        // Player starts at (0, 0) moving to (3.0, 0), ball at (0, 2.0) stationary.
        // Minimum distance is 2.0 at t=0, which is > 1.5m.
        let result = swept_proximity(
            Vec2::new(0.0, 0.0),
            Vec2::new(3.0, 0.0),
            Vec2::new(0.0, 2.0),
            Vec2::zero(),
            1.5,
        );
        assert!((result.min_distance - 2.0).abs() < 1e-6);
        assert!(!result.acquired);
    }

    #[test]
    fn test_swept_proximity_diagonal_motion() {
        // Player moves diagonally from (0,0) to (3,4) (length 5m at angle).
        // Ball at (2.5, 2.0). Closest approach at t=0.62:
        // player at (1.86, 2.48), distance = sqrt(0.64 + 0.2304) = sqrt(0.8704) ≈ 0.933m
        let result = swept_proximity(
            Vec2::new(0.0, 0.0),
            Vec2::new(3.0, 4.0),
            Vec2::new(2.5, 2.0),
            Vec2::zero(),
            1.5,
        );
        assert!((result.min_distance - 0.8).abs() < 1e-4);
        assert!(result.acquired);
    }

    #[test]
    fn test_swept_proximity_inner_endpoint() {
        // Player moves from (0,0) to (5,0), ball moves from (3,0) to (4,0).
        // Relative motion: player starts 3m behind and closes 1m gap.
        // They overlap at t=0.6: player at 3.0, ball at 3.6.
        // Min distance should be 0 (they cross paths).
        let result = swept_proximity(
            Vec2::new(0.0, 0.0),
            Vec2::new(5.0, 0.0),
            Vec2::new(3.0, 0.0),
            Vec2::new(1.0, 0.0),
            1.5,
        );
        assert!(result.min_distance < 1e-5);
        assert!(result.acquired);
    }
}
