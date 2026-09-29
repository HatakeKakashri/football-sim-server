use bevy_ecs::prelude::*;
use sim_components::{Ball, BallMarker, Player, Position, Velocity};
use sim_math::{PitchDimensions, Vec2};

/// Phase 2: RNG resource wrapper for stochastic gameplay outcomes.
///
/// Inserted as a Bevy Resource so systems can access it via `ResMut<SimRng>`.
/// Uses a simple, completely deterministic LCG (Linear Congruential Generator)
/// to eliminate any external crate non-determinism across processes.
#[derive(Resource)]
pub struct SimRng {
    state: u64,
}

impl SimRng {
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        // Use a non-zero seed; LCG with 0 would produce all zeros
        Self {
            state: seed.wrapping_add(0x9E37_79B9_7F4A_7C15),
        }
    }

    /// Generate a random u64 using LCG (Numerical Recipes constants)
    pub const fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    /// Generate a random f32 in [0, 1)
    ///
    /// Uses the top 24 bits of the LCG (its low bits are the weakest). 24 bits
    /// is exactly f32's mantissa width, so the conversion is exact and the
    /// result can never round up to 1.0.
    #[expect(
        clippy::cast_precision_loss,
        reason = "value is < 2^24, exactly representable in f32"
    )]
    pub fn gen_f32(&mut self) -> f32 {
        let bits = (self.next_u64() >> 40) as u32;
        bits as f32 * (1.0 / 16_777_216.0) // 2^-24, exact
    }

    /// Generate a random f32 in [min, max)
    pub fn gen_range_f32(&mut self, min: f32, max: f32) -> f32 {
        self.gen_f32().mul_add(max - min, min)
    }
}

pub const MAX_PLAYER_SPEED: f32 = 10.0;
pub const MAX_BALL_SPEED: f32 = 30.0;
pub const BALL_DAMPING: f32 = 0.99;

/// Phase 2: pitch control grid constants (16 columns × 12 rows).
///
/// The grid covers the full pitch. Each cell stores the probability that the
/// attacking team wins that cell in a footrace against the defending team.
pub const PITCH_CONTROL_COLS: usize = 16;
pub const PITCH_CONTROL_ROWS: usize = 12;
pub const PITCH_CONTROL_PLAYER_SPEED: f32 = 5.0; // m/s — controlled by sim
pub const PITCH_CONTROL_BALL_SPEED: f32 = 20.0; // m/s — for free-ball trajectories
pub const PITCH_CONTROL_STEEPNESS: f32 = 0.5; // logistic steepness (k)
pub const PITCH_CONTROL_POSSESSION_RADIUS: f32 = 1.5; // m — within this radius, ball is "possessed"

/// Convert a grid index or dimension to `f32`.
///
/// Grid sizes are tiny (16 × 12), far below 2^24, so the conversion is exact.
#[expect(
    clippy::cast_precision_loss,
    reason = "grid indices/dimensions are far below 2^24, exactly representable in f32"
)]
const fn usize_to_f32(n: usize) -> f32 {
    n as f32
}

/// Map a world coordinate onto a grid index along one axis, clamped to the grid.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value is clamped to [0, count - 1] before the cast, so it is non-negative and in range"
)]
fn cell_index(coord: f32, extent: f32, count: usize) -> usize {
    let scaled = (coord / extent) * usize_to_f32(count);
    scaled.clamp(0.0, usize_to_f32(count.saturating_sub(1))) as usize
}

/// Phase 2 resource: pitch control grid.
///
/// `cells[row][col]` stores the
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
    #[must_use]
    pub fn build_standard() -> Self {
        let pitch = PitchDimensions::standard();
        let cols = PITCH_CONTROL_COLS;
        let rows = PITCH_CONTROL_ROWS;
        let cell_width = pitch.width / usize_to_f32(cols);
        let cell_height = pitch.length / usize_to_f32(rows);
        let cells = vec![vec![0.5_f32; cols]; rows];
        Self {
            cells,
            cell_width,
            cell_height,
            cols,
            rows,
        }
    }

    /// `P_control(x`, y) = 1 / (1 + exp(-k * (`t_def` - `t_att`))) where t is the
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
                dx.hypot(dy) / *speed
            })
            .fold(f32::INFINITY, f32::min);
        let t_def = defenders
            .iter()
            .map(|(pos, speed)| {
                let dx = pos.x - x;
                let dy = pos.y - y;
                dx.hypot(dy) / *speed
            })
            .fold(f32::INFINITY, f32::min);
        let diff = t_def - t_att;
        1.0 / (1.0 + (-diff * k).exp())
    }

    /// Look up the cached control value at world coordinates `(x, y)` by
    /// mapping them onto grid indices. Out-of-bounds coordinates are
    /// clamped to the nearest cell.
    #[must_use]
    pub fn control_at(&self, x: f32, y: f32) -> f32 {
        let pitch = PitchDimensions::standard();
        let cx = cell_index(x, pitch.width, self.cols);
        let cy = cell_index(y, pitch.length, self.rows);
        self.cells[cy][cx]
    }
}

/// Phase 2 system: recompute the pitch control grid.
///
/// Runs every tick (cheap: 16 × 12 = 192 cell evaluations). Reads world state
/// (players + ball) and writes the `PitchControlGrid` resource. Does NOT
/// mutate any component, so it is safe to run before decision/execution
/// systems.
///
/// Phase C ECS-shape polish (review §2.5): now uses a typed `Query` instead
/// of `world.iter_entities()`. The ball position is queried with
/// `Query<(&Ball, &Position)>` and player positions with
/// `Query<(&Player, &Position)>`. The grid resource is mutated via
/// `ResMut<PitchControlGrid>`; downstream readers that take
/// `Res<PitchControlGrid>` are serialised correctly by the scheduler
/// (`pitch_control_system` writes, `player_decision_system` reads).
///
/// Logic per spec §3:
/// 1. Partition players by team via `Player.team_id`; pull each player's
///    world position from the `Position` component.
/// 2. Identify the ball position from the `Ball` + `Position` query.
/// 3. Sort both teams by distance to the ball; pick top-3 from each as
///    attackers/defenders (same selection rule in the free-ball case —
///    empty ball position just means stable tiebreaks).
/// 4. For each grid cell, call `cell_score` with attacker/defender sets.
/// 5. Store the result back into the resource.
pub fn pitch_control_system(
    mut grid: ResMut<PitchControlGrid>,
    ball_query: Query<&Position, With<sim_components::BallMarker>>,
    player_query: Query<(&Player, &Position)>,
) {
    // Snapshot player positions by team id (read-only).
    let mut home_players: Vec<(Vec2, f32)> = Vec::new();
    let mut away_players: Vec<(Vec2, f32)> = Vec::new();

    // Query the ball position from the typed query (filtered by `BallMarker`).
    let ball_pos_opt = ball_query.iter().next().map(|p| p.0);

    // Snapshot players by team id via Query.
    for (player, pos) in &player_query {
        let speed = PITCH_CONTROL_PLAYER_SPEED;
        if player.team_id.0 == 0 {
            home_players.push((pos.0, speed));
        } else if player.team_id.0 == 1 {
            away_players.push((pos.0, speed));
        }
    }

    // Sort by distance to the ball, then pick top-3 from each team.
    // Free-ball fallback uses the same logic (everyone is the same distance
    // away from the empty ball position, so order is arbitrary but stable).
    let sort_by_ball = |set: &mut Vec<(Vec2, f32)>, bp: Option<Vec2>| {
        if let Some(bp) = bp {
            set.sort_by(|(a, _), (b, _)| {
                let da = (a.y - bp.y).mul_add(a.y - bp.y, (a.x - bp.x) * (a.x - bp.x));
                let db = (b.y - bp.y).mul_add(b.y - bp.y, (b.x - bp.x) * (b.x - bp.x));
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

    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let x = (usize_to_f32(col) + 0.5) * grid.cell_width;
            let y = (usize_to_f32(row) + 0.5) * grid.cell_height;
            grid.cells[row][col] =
                grid.cell_score(x, y, &attackers, &defenders, PITCH_CONTROL_STEEPNESS);
        }
    }
}

pub fn ball_physics_system(
    pitch: Res<PitchDimensions>,
    mut ball_query: Query<(&mut Position, &mut Velocity), With<sim_components::BallMarker>>,
    mut ball: ResMut<sim_components::Ball>,
) {
    let Ok((mut pos, mut vel)) = ball_query.get_single_mut() else {
        return;
    };
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

    // Update ball state based on velocity (single writer: ResMut<Ball>).
    if vel.0.length() > 0.1 {
        ball.state = sim_components::BallState::InFlight;
    } else if ball.possessor.is_some() {
        ball.state = sim_components::BallState::Possessed;
    } else {
        ball.state = sim_components::BallState::Free;
    }
}

/// Apply any pending kick velocity to the ball's velocity.
///
/// Runs at the **start** of the Physics set, before `ball_physics_system`,
/// so the kick is integrated into position on the same tick the kick is
/// decided. The ordering also dodges the static query conflict with
/// `kick_execution_system` (Execution set) that would otherwise arise from
/// two systems both holding `&mut Velocity, &mut Ball` on the ball entity.
pub fn apply_kick_velocity_system(
    mut ball_query: Query<&mut Velocity, With<sim_components::BallMarker>>,
    mut ball: ResMut<sim_components::Ball>,
) {
    let Ok(mut vel) = ball_query.get_single_mut() else {
        return;
    };
    if let Some(kick_vel) = ball.kick_velocity.take() {
        vel.0 += kick_vel;
    }
}

pub fn player_movement_system(
    pitch: Res<PitchDimensions>,
    mut query: Query<(&mut Position, &mut Velocity)>,
) {
    for (mut pos, mut vel) in &mut query {
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
#[cfg(test)]
mod tests {
    use super::*;
    use sim_components::{Ball, Player, Position, RoleComponent, TeamId, Velocity};

    /// The `pitch_control_system` (Phase 2 ECS-shape polish: §2.5) must
    /// succeed when invoked as a normal Bevy system (not via
    /// `world.run_system`). It writes the `PitchControlGrid` resource and
    /// produces a 16 × 12 grid with non-default cells when players exist.
    #[test]
    fn pitch_control_system_runs_as_query_system() {
        let mut world = World::new();
        world.insert_resource(PitchControlGrid::build_standard());

        // Two players, one per team, plus a ball.
        world.spawn((
            Player {
                team_id: TeamId(0),
                intent: None,
            },
            Position(Vec2::new(20.0, 34.0)),
            Velocity(Vec2::zero()),
            RoleComponent(sim_components::Role::CentralMidfielder),
        ));
        world.spawn((
            Player {
                team_id: TeamId(1),
                intent: None,
            },
            Position(Vec2::new(85.0, 34.0)),
            Velocity(Vec2::zero()),
            RoleComponent(sim_components::Role::CentralMidfielder),
        ));
        let ball_entity = world.spawn(()).id();
        world.entity_mut(ball_entity).insert(BallMarker);
        world.insert_resource(Ball {
            spin: 0.0,
            state: sim_components::BallState::Free,
            possessor: None,
            last_touched_by: None,
            kick_velocity: None,
        });
        world
            .entity_mut(ball_entity)
            .insert(Position(Vec2::new(52.5, 34.0)));

        // Run via the schedule so the system gets the Bevy system params.
        let mut schedule = Schedule::default();
        schedule.add_systems(pitch_control_system);
        schedule.run(&mut world);

        // At least one cell should be non-default (0.5). All-home players
        // mean home should dominate the right-half of the pitch.
        let grid = world.resource::<PitchControlGrid>();
        let mut changed = 0;
        for row in 0..grid.rows {
            for col in 0..grid.cols {
                if (grid.cells[row][col] - 0.5).abs() > f32::EPSILON {
                    changed += 1;
                }
            }
        }
        assert!(changed > 0, "Grid should reflect player positions, all cells were 0.5");
    }
}
