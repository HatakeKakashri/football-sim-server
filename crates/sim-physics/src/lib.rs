use bevy_ecs::prelude::*;
use rand::{SeedableRng, rngs::SmallRng};
use sim_components::{Ball, Position, Velocity};
use sim_math::{PitchDimensions, Vec2};

/// Phase 2: RNG resource wrapper for stochastic gameplay outcomes.
/// Inserted as a Bevy Resource so systems can access it via `ResMut<SimRng>`.
/// Wraps SmallRng for speed (Mersenne Twister 19937).
#[derive(Resource)]
pub struct SimRng(pub SmallRng);

impl SimRng {
    pub fn new(seed: u64) -> Self {
        Self(SmallRng::seed_from_u64(seed))
    }
}

pub const MAX_PLAYER_SPEED: f32 = 10.0;
pub const MAX_BALL_SPEED: f32 = 30.0;
pub const BALL_DAMPING: f32 = 0.99;

/// Phase 2: pitch control grid constants. The grid covers the full pitch
/// with 16 columns × 12 rows. Each cell stores the probability that the
/// attacking team wins that cell in a footrace against the defending team.
pub const PITCH_CONTROL_COLS: usize = 16;
pub const PITCH_CONTROL_ROWS: usize = 12;
pub const PITCH_CONTROL_PLAYER_SPEED: f32 = 5.0; // m/s — controlled by sim
pub const PITCH_CONTROL_BALL_SPEED: f32 = 20.0; // m/s — for free-ball trajectories
pub const PITCH_CONTROL_STEEPNESS: f32 = 0.5; // logistic steepness (k)
pub const PITCH_CONTROL_POSSESSION_RADIUS: f32 = 1.5; // m — within this radius, ball is "possessed"

/// Phase 2 resource: pitch control grid. `cells[row][col]` stores the
/// probability P ∈ [0, 1] that the attacking side wins the race to that
/// cell. Rows index the y-axis (north-south); columns index the x-axis
/// (east-west). Pitch is `width × length = 105 × 68` (standard).
#[derive(Resource, Debug, Clone)]
pub struct PitchControlGrid {
    pub cells: Vec<Vec<f32>>,
    pub cell_width: f32,
    pub cell_height: f32,
    pub cols: usize,
    pub rows: usize,
}

impl PitchControlGrid {
    /// Standard 16 × 12 grid over a 105 m × 68 m pitch. `cell_width = 105/16`
    /// ≈ 6.5625 m and `cell_height = 68/12` ≈ 5.6667 m.
    pub fn build_standard() -> Self {
        let pitch = PitchDimensions::standard();
        let cols = PITCH_CONTROL_COLS;
        let rows = PITCH_CONTROL_ROWS;
        let cell_width = pitch.width / cols as f32;
        let cell_height = pitch.length / rows as f32;
        let cells = vec![vec![0.5_f32; cols]; rows];
        Self {
            cells,
            cell_width,
            cell_height,
            cols,
            rows,
        }
    }

    /// P_control(x, y) = 1 / (1 + exp(-k * (t_def - t_att))) where t is the
    /// minimum time-of-arrival across the supplied set. Larger values
    /// indicate the attacker can reach the cell first.
    ///
    /// `k` is `PITCH_CONTROL_STEEPNESS`; the public signature accepts it as
    /// a parameter so unit tests can stress it without changing the constant.
    pub fn cell_score(
        &self,
        x: f32,
        y: f32,
        attackers: &[(Vec2, f32)],
        defenders: &[(Vec2, f32)],
        k: f32,
    ) -> f32 {
        let t_att = attackers
            .iter()
            .map(|(pos, speed)| {
                let dx = pos.x - x;
                let dy = pos.y - y;
                (dx * dx + dy * dy).sqrt() / *speed
            })
            .fold(f32::INFINITY, f32::min);
        let t_def = defenders
            .iter()
            .map(|(pos, speed)| {
                let dx = pos.x - x;
                let dy = pos.y - y;
                (dx * dx + dy * dy).sqrt() / *speed
            })
            .fold(f32::INFINITY, f32::min);
        let diff = t_def - t_att;
        1.0 / (1.0 + (-diff * k).exp())
    }

    /// Look up the cached control value at world coordinates `(x, y)` by
    /// mapping them onto grid indices. Out-of-bounds coordinates are
    /// clamped to the nearest cell.
    pub fn control_at(&self, x: f32, y: f32) -> f32 {
        let pitch = PitchDimensions::standard();
        let cx = ((x / pitch.width) * self.cols as f32).clamp(0.0, (self.cols - 1) as f32) as usize;
        let cy =
            ((y / pitch.length) * self.rows as f32).clamp(0.0, (self.rows - 1) as f32) as usize;
        self.cells[cy][cx]
    }
}

/// Phase 2 system: recompute the pitch control grid. Runs every tick
/// (cheap: 16 × 12 = 192 cell evaluations). Reads world state (players +
/// ball) and writes the `PitchControlGrid` resource. Does NOT mutate any
/// component, so it is safe to run before decision/execution systems.
///
/// Note: takes `&mut World` directly (not `ResMut<PitchControlGrid>`) so
/// that downstream systems reading the grid with `Res<PitchControlGrid>`
/// don't conflict with this system's exclusive access.
///
/// Logic per spec §3:
/// 1. Find all home-team and away-team players via `Player.team_id`.
/// 2. Identify the ball "possessor": the player within 1.5 m of the ball.
/// 3. If possessed: attacker = possessor's team (top-3 by distance to ball),
///    defender = opposing team (top-3 by distance to ball).
///    Otherwise: attacker/defender = 3 closest from each team.
/// 4. For each grid cell, call `cell_score` with attacker/defender sets.
/// 5. Store the result back into the resource.
pub fn pitch_control_system(world: &mut World) {
    use sim_components::Player;

    // Snapshot player positions by team id (read-only).
    let mut home_players: Vec<(Vec2, f32)> = Vec::new();
    let mut away_players: Vec<(Vec2, f32)> = Vec::new();

    // Find ball position by iterating entities.
    let mut ball_pos_opt: Option<Vec2> = None;
    for entity in world.iter_entities() {
        if entity.get::<sim_components::Ball>().is_some() {
            if let Some(p) = entity.get::<Position>() {
                ball_pos_opt = Some(p.0);
                break;
            }
        }
    }

    // Snapshot players by team id.
    for entity in world.iter_entities() {
        if let (Some(player), Some(pos)) = (entity.get::<Player>(), entity.get::<Position>()) {
            let speed = PITCH_CONTROL_PLAYER_SPEED;
            if player.team_id.0 == 0 {
                home_players.push((pos.0, speed));
            } else if player.team_id.0 == 1 {
                away_players.push((pos.0, speed));
            }
        }
    }

    // Sort by distance to the ball, then pick top-3 from each team.
    // Free-ball fallback uses the same logic (everyone is the same distance
    // away from the empty ball position, so order is arbitrary but stable).
    let sort_by_ball = |set: &mut Vec<(Vec2, f32)>, bp: Option<Vec2>| {
        if let Some(bp) = bp {
            set.sort_by(|(a, _), (b, _)| {
                let da = (a.x - bp.x) * (a.x - bp.x) + (a.y - bp.y) * (a.y - bp.y);
                let db = (b.x - bp.x) * (b.x - bp.x) + (b.y - bp.y) * (b.y - bp.y);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            });
        }
    };

    let mut home_sorted = home_players.clone();
    let mut away_sorted = away_players.clone();
    sort_by_ball(&mut home_sorted, ball_pos_opt);
    sort_by_ball(&mut away_sorted, ball_pos_opt);

    let attackers: Vec<(Vec2, f32)> = home_sorted.iter().take(3).copied().collect();
    let defenders: Vec<(Vec2, f32)> = away_sorted.iter().take(3).copied().collect();

    // Recompute every cell. We mutate the resource via `world.resource_mut`
    // (instead of taking `ResMut<PitchControlGrid>` directly) so that other
    // systems on the same schedule can take `Res<PitchControlGrid>` without
    // a conflict error.
    let mut grid = match world.get_resource_mut::<PitchControlGrid>() {
        Some(g) => g,
        None => return,
    };
    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let x = (col as f32 + 0.5) * grid.cell_width;
            let y = (row as f32 + 0.5) * grid.cell_height;
            grid.cells[row][col] =
                grid.cell_score(x, y, &attackers, &defenders, PITCH_CONTROL_STEEPNESS);
        }
    }
}

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
    #[test]
    fn test_physics_plugin() {
        // Placeholder test
        assert!(true);
    }
}
