//! The capture of one 10 Hz animation frame.
//!
//! The capture reads the world and writes one frame: the ball and the
//! 22 players, in integer decimetres. The frame names its players by
//! their roster row, so it holds no player id string — the ids live
//! once, in the roster, and the render step borrows them from there
//! when it writes the body.

use sim_components::{Ball, MatchClock, Player, Position};
use sim_core::Simulation;

use crate::api::model::{
    FrameBall, FrameBallState, FramePlayer, MatchFrame, to_decimetres,
};

use super::ResourceError;
use super::roster::Roster;

/// Capture one 10 Hz frame: ball + all players in integer decimetres.
///
/// The frame names its players by their roster index, so it holds no
/// player id string: the ids live once, in the roster, and the render
/// step borrows them from there when it writes the body.
///
/// Every allocation the frame needs is reserved fallibly: the capture
/// is the run's largest allocation, and a host that cannot give it
/// the memory fails the match instead of aborting the process.
pub fn capture_frame(
    sim: &Simulation,
    roster: &Roster,
) -> Result<MatchFrame, ResourceError> {
    let world = sim.world();
    let ball = world
        .get_resource::<Ball>()
        .expect("invariant: Ball resource exists for the whole match");
    let ball_entity = world.entity(sim.ball_entity());
    let position = ball_entity
        .get::<Position>()
        .map_or([0, 0], |pos| [to_decimetres(pos.0.x), to_decimetres(pos.0.y)]);
    let possessor = ball
        .possessor
        .and_then(|entity| roster.rows.get(&entity.to_bits()).copied());
    let mut players: Vec<(u8, FramePlayer)> = Vec::new();
    for entity in world.iter_entities() {
        let Some(player) = entity.get::<Player>() else {
            continue;
        };
        let bits = entity.id().to_bits();
        // Every fixture player carries a roster row, so a frame never
        // names a player the manifest does not list.
        let row = roster
            .rows
            .get(&bits)
            .copied()
            .expect("invariant: every fixture player carries a roster entry");
        let (x, y) = entity.get::<Position>().map_or((0, 0), |pos| {
            (to_decimetres(pos.0.x), to_decimetres(pos.0.y))
        });
        players
            .try_reserve(1)
            .map_err(|error| ResourceError::reservation_failed("player list", &error))?;
        players.push((
            player.team_id.0,
            FramePlayer {
                roster_index: row,
                x,
                y,
            },
        ));
    }
    // Sorted by id, then by side: a mirror match puts the same fixture
    // ids on both sides, and the side breaks the tie deterministically.
    // The ids are borrowed from the roster.
    players.sort_by(|left, right| {
        roster
            .id_of(left.1.roster_index)
            .cmp(roster.id_of(right.1.roster_index))
            .then(left.0.cmp(&right.0))
    });
    let mut sorted: Vec<FramePlayer> = Vec::new();
    sorted
        .try_reserve(players.len())
        .map_err(|error| ResourceError::reservation_failed("player list", &error))?;
    for (_, player) in players {
        sorted.push(player);
    }
    let half = world
        .get_resource::<MatchClock>()
        .map_or(1, |clock| clock.half);
    Ok(MatchFrame {
        tick: sim.current_tick(),
        half,
        ball: FrameBall {
            x: position[0],
            y: position[1],
            state: FrameBallState::from(ball.state),
            possessor,
        },
        players: sorted,
    })
}
