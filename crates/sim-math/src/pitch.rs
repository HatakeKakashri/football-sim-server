//! Pitch geometry constants and shapes.

use bevy_ecs::prelude::Resource;
use crate::vec2::Vec2;

/// Standard pitch length in metres (along the x-axis).
pub const PITCH_LENGTH: f32 = 105.0;

/// Standard pitch width in metres (along the y-axis).
pub const PITCH_WIDTH: f32 = 68.0;

/// Standard goal centre y-coordinate (half of pitch width).
pub const GOAL_Y_CENTER: f32 = 34.0;

/// Returns the goal position `Vec2` for a team attacking in `attack_direction`.
///
/// - `attack_direction > 0` (+1): team attacks toward `x = PITCH_LENGTH` (right goal).
/// - `attack_direction <= 0` (-1): team attacks toward `x = 0.0` (left goal).
#[must_use]
pub const fn goal_position_for(attack_direction: i8) -> Vec2 {
    if attack_direction > 0 {
        Vec2::new(PITCH_LENGTH, GOAL_Y_CENTER)
    } else {
        Vec2::new(0.0, GOAL_Y_CENTER)
    }
}

/// Pitch dimensions and key zones.
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
