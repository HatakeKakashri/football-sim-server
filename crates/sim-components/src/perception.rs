//! Perception types: what each player knows about the world.

use bevy_ecs::prelude::*;
use sim_math::Vec2;

use crate::BallState;

/// Perception snapshot of what each player knows about the world each tick.
#[derive(Component, Debug, Clone)]
pub struct PerceptionSnapshot {
    pub self_position: Vec2,
    pub nearby_teammates: smallvec::SmallVec<[NearbyEntity; 8]>,
    pub nearby_opponents: smallvec::SmallVec<[NearbyEntity; 8]>,
    pub ball_position: Vec2,
    /// Ball state as observed by the perception system. This is a
    /// `BallState` enum value (Free / Possessed / etc.), **not** an
    /// `Entity`.  The actual possessor entity lives on `Ball.possessor`
    /// and is read by systems that need the real entity reference.
    pub ball_state: BallState,
    pub goal_position: Vec2,
    pub home_slot: Vec2,
    pub pitch_bounds: PitchBounds,
}

/// A nearby entity observed in perception.
#[derive(Debug, Clone)]
pub struct NearbyEntity {
    pub entity: Entity,
    pub distance: f32,
    pub relative_position: Vec2,
}

/// Pitch bounds from a player's perspective.
#[derive(Debug, Clone)]
pub struct PitchBounds {
    pub distance_to_left: f32,
    pub distance_to_right: f32,
    pub distance_to_top: f32,
    pub distance_to_bottom: f32,
}
