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
}
