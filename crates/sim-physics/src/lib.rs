use bevy_ecs::prelude::*;
use sim_components::{Ball, Position, Velocity};
use sim_math::PitchDimensions;

pub const MAX_PLAYER_SPEED: f32 = 10.0;
pub const MAX_BALL_SPEED: f32 = 30.0;
pub const BALL_DAMPING: f32 = 0.99;

pub fn ball_physics_system(
    pitch: Res<PitchDimensions>,
    mut query: Query<(&mut Position, &mut Velocity, &mut Ball)>,
) {
    for (mut pos, mut vel, mut ball) in query.iter_mut() {
        // Apply velocity
        pos.0 += vel.0;
        
        // Apply damping
        vel.0 *= BALL_DAMPING;
        
        // Clamp ball speed
        vel.0 = vel.0.clamp_length(MAX_BALL_SPEED);
        
        // Boundary clamping
        if pos.0.x < 0.0 {
            pos.0.x = 0.0;
            vel.0.x = -vel.0.x * 0.5; // Bounce with energy loss
        } else if pos.0.x > pitch.width {
            pos.0.x = pitch.width;
            vel.0.x = -vel.0.x * 0.5;
        }
        
        if pos.0.y < 0.0 {
            pos.0.y = 0.0;
            vel.0.y = -vel.0.y * 0.5;
        } else if pos.0.y > pitch.length {
            pos.0.y = pitch.length;
            vel.0.y = -vel.0.y * 0.5;
        }
        
        // Update ball state based on velocity
        if vel.0.length() > 0.1 {
            ball.state = sim_components::BallState::InFlight;
        } else if ball.possessor.is_some() {
            ball.state = sim_components::BallState::Possessed;
        } else {
            ball.state = sim_components::BallState::Free;
        }
    }
}

pub fn player_movement_system(
    pitch: Res<PitchDimensions>,
    mut query: Query<(&mut Position, &mut Velocity)>,
) {
    for (mut pos, mut vel) in query.iter_mut() {
        // Clamp velocity to max speed
        vel.0 = vel.0.clamp_length(MAX_PLAYER_SPEED);
        
        // Apply velocity
        pos.0 += vel.0;
        
        // Boundary clamping (players stay within pitch)
        pos.0.x = pos.0.x.clamp(0.0, pitch.width);
        pos.0.y = pos.0.y.clamp(0.0, pitch.length);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_physics_plugin() {
        // Placeholder test
        assert!(true);
    }
}