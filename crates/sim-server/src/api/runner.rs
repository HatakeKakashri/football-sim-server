//! Match simulation runner: one full match on the calling OS thread.
//!
//! [`run_match_blocking`] drives a [`Simulation`][sim_core::Simulation]
//! to `FullTime` (or the tick cap), enforces the six PRD §12 invariants
//! per tick, captures 10 Hz frames, and assembles the
//! [`CompletedMatch`][crate::api::stream::CompletedMatch] the stream
//! builder consumes.

use std::collections::HashMap;

use sim_components::{Ball, FormationSlot, Match, MatchClock, Player, Position, Team};
use sim_core::{EventLog, MatchEvent, Simulation, Termination, TickHook, run_match_driver};

use super::stream::{
    CompletedMatch, FrameBall, FramePlayer, HalfBoundaries, RosterEntry, StreamFrame,
    ball_state_str, to_decimetres,
};
/// Result of running one match to completion on its OS thread.
#[derive(Debug)]
pub enum MatchOutcome {
    /// Reached `FullTime` with no invariant violation.
    Finished(CompletedMatch),
    /// Invariant violation, tick-cap breach or runner panic (PRD §12);
    /// cached like a success so retries are deterministic.
    Failed {
        /// Machine-readable reason (`tick_cap_exceeded`,
        /// `runner_panic`, `thread_spawn_failed` or the invariant
        /// code).
        code: String,
        /// Human-readable detail.
        message: String,
        /// Tick the simulation reached before failing.
        last_good_tick: u64,
    },
}

/// Injection seam over the blocking match runner.
///
/// The real adapter ([`SimulationRunner`]) drives the simulation;
/// tests supply adapters that panic or fail so the store's failure
/// handling can be exercised without relying on the tick cap.
pub trait MatchRunner: Send + Sync + std::fmt::Debug {
    /// Run one match to completion on the calling thread.
    fn run(&self, fixture: &crate::api::MatchFixture, tick_cap: u64) -> MatchOutcome;
}

/// Real adapter: runs the simulation through the shared match driver.
#[derive(Debug, Default)]
pub struct SimulationRunner;

impl MatchRunner for SimulationRunner {
    fn run(&self, fixture: &crate::api::MatchFixture, tick_cap: u64) -> MatchOutcome {
        run_match_blocking(fixture, tick_cap)
    }
}

/// Run the runner, turning a panic into a cached failure outcome.
///
/// A panic on the simulation thread must not skip the store's
/// completion bookkeeping: the concurrency slot would leak, the record
/// would stay `Running` forever, and every later `GET` would wait
/// for a match that never finishes. The panic is caught here and
/// reported as a `runner_panic` failure, so the slot is released and
/// the failure is cached like any other (PRD §12).
#[must_use]
pub fn run_catching_panic(
    runner: &dyn MatchRunner,
    fixture: &crate::api::MatchFixture,
    tick_cap: u64,
) -> MatchOutcome {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runner.run(fixture, tick_cap)
    }));
    match outcome {
        Ok(outcome) => outcome,
        Err(payload) => {
            let message = panic_message(payload.as_ref());
            tracing::error!(
                match_id = %fixture.validated_match.id,
                seed = fixture.validated_match.seed,
                "simulation runner panicked: {message}"
            );
            MatchOutcome::Failed {
                code: "runner_panic".to_string(),
                message,
                // The panic payload carries no tick count: the run is
                // failed at tick 0 (unknown progress).
                last_good_tick: 0,
            }
        }
    }
}

/// Extract a human-readable message from a panic payload.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload.downcast_ref::<&str>().map_or_else(
        || {
            payload
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| "runner panicked".to_string())
        },
        |text| (*text).to_string(),
    )
}

/// Per-tick hook for the API runner: captures 10 Hz frames every 6 ticks.
struct RunnerHook<'a> {
    roster_ids: &'a HashMap<u64, String>,
    frames: &'a mut Vec<StreamFrame>,
}

impl TickHook for RunnerHook<'_> {
    fn on_tick(&mut self, sim: &mut Simulation) -> Result<(), String> {
        if sim.current_tick().is_multiple_of(6) {
            self.frames.push(capture_frame(sim, self.roster_ids));
        }
        Ok(())
    }
}

/// Run one match on the calling thread using validated fixture data.
///
/// Drives the simulation to `FullTime` (or the tick cap), enforces
/// the six PRD §12 invariants per tick via [`check_invariants`],
/// captures 10 Hz frames, and assembles the [`CompletedMatch`].
///
/// # Panics
///
/// Panics if `RunnerHook::on_tick` returns `Err`, which it never does
/// (it only captures frames). The `.expect()` is a defensive guard.
#[must_use]
pub fn run_match_blocking(
    fixture: &crate::api::MatchFixture,
    tick_cap: u64,
) -> MatchOutcome {
    let mut sim = Simulation::new_from_fixture(
        &fixture.validated_match,
        &fixture.home_team,
        &fixture.away_team,
    );
    let attack_start = attack_directions(&sim);
    let roster = build_roster(&sim);
    let match_id = fixture.validated_match.id.clone();
    let seed = fixture.validated_match.seed;

    let mut frames: Vec<StreamFrame> = Vec::new();
    let mut hook = RunnerHook {
        roster_ids: &roster.ids,
        frames: &mut frames,
    };
    let result = run_match_driver(&mut sim, tick_cap, &mut hook)
        .expect("invariant: RunnerHook never errors");

    if let Some(violation) = result.violation {
        return MatchOutcome::Failed {
            code: violation.code().to_string(),
            message: violation.message(),
            last_good_tick: violation.last_good_tick(),
        };
    }

    if result.termination == Termination::TickCap {
        let world = sim.world();
        let match_res = world.get_resource::<Match>().cloned();
        let Some(match_res) = match_res else {
            return MatchOutcome::Failed {
                code: "no_match_resource".to_string(),
                message: "Match resource missing after simulation".to_string(),
                last_good_tick: sim.current_tick(),
            };
        };
        return MatchOutcome::Failed {
            code: "tick_cap_exceeded".to_string(),
            message: format!(
                "match did not reach FullTime within {tick_cap} ticks (state {:?})",
                match_res.state
            ),
            last_good_tick: sim.current_tick(),
        };
    }

    let world = sim.world();
    let match_res = world.get_resource::<Match>().cloned();
    let Some(match_res) = match_res else {
        return MatchOutcome::Failed {
            code: "no_match_resource".to_string(),
            message: "Match resource missing after simulation".to_string(),
            last_good_tick: sim.current_tick(),
        };
    };
    if match_res.state != sim_components::MatchState::FullTime {
        return MatchOutcome::Failed {
            code: "tick_cap_exceeded".to_string(),
            message: format!(
                "match did not reach FullTime within {tick_cap} ticks (state {:?})",
                match_res.state
            ),
            last_good_tick: sim.current_tick(),
        };
    }

    let events: Vec<MatchEvent> = world
        .get_resource::<EventLog>()
        .map_or(Vec::new(), |log| log.events.clone());
    let full_time_tick = events
        .iter()
        .rev()
        .find(|event| matches!(event, MatchEvent::FullTime { .. }))
        .map_or_else(|| sim.current_tick(), |event| event.envelope().tick);
    let first_half_end = events
        .iter()
        .find(|event| matches!(event, MatchEvent::HalfTime { .. }))
        .map_or_else(|| full_time_tick, |event| event.envelope().tick);
    let added_time_ticks = events
        .iter()
        .rev()
        .find_map(|event| match event {
            MatchEvent::FullTime { payload, .. } => Some(payload.added_time_ticks),
            _ => None,
        })
        .unwrap_or((0, 0));
    let attack_end = attack_directions(&sim);

    MatchOutcome::Finished(CompletedMatch {
        match_id,
        seed,
        events,
        frames,
        roster: roster.entries,
        score: match_res.score,
        state_hash: sim.get_state_hash(),
        added_time_ticks,
        boundaries: HalfBoundaries {
            first_half_end,
            full_time: full_time_tick,
        },
        attack_home: (attack_start.0, attack_end.0),
        attack_away: (attack_start.1, attack_end.1),
    })
}

/// Per-team attack directions as `(home, away)`.
pub(crate) fn attack_directions(sim: &Simulation) -> (i8, i8) {
    let mut home_dir = 1;
    let mut away_dir = -1;
    for entity in sim.world().iter_entities() {
        if let Some(team) = entity.get::<Team>() {
            if team.id == sim_components::TeamId(0) {
                home_dir = team.attack_direction();
            } else if team.id == sim_components::TeamId(1) {
                away_dir = team.attack_direction();
            }
        }
    }
    (home_dir, away_dir)
}

/// Roster built from the live world: players sorted by entity bits,
/// indexed per team to stable `p-home-NN` / `p-away-NN` ids.
pub(crate) struct Roster {
    pub(crate) entries: Vec<RosterEntry>,
    pub(crate) ids: HashMap<u64, String>,
}

pub(crate) fn build_roster(sim: &Simulation) -> Roster {
    let mut players: Vec<(u64, sim_components::TeamId, String)> = Vec::new();
    for entity in sim.world().iter_entities() {
        if let Some(player) = entity.get::<Player>() {
            let slot = entity
                .get::<FormationSlot>()
                .map_or_else(|| "UNK".to_string(), |slot| format!("{}", slot.position));
            players.push((entity.id().to_bits(), player.team_id, slot));
        }
    }
    players.sort_by_key(|(bits, _, _)| *bits);
    let mut home_seen = 0_u8;
    let mut away_seen = 0_u8;
    let mut entries = Vec::with_capacity(players.len());
    let mut ids = HashMap::with_capacity(players.len());
    for (bits, team, slot) in players {
        let (side, number) = if team == sim_components::TeamId(0) {
            home_seen += 1;
            ("home", home_seen)
        } else {
            away_seen += 1;
            ("away", away_seen)
        };
        let index = u32::from(number) - 1;
        let id = format!("p-{side}-{index:02}");
        let side_name = if side == "home" { "Home" } else { "Away" };
        entries.push(RosterEntry {
            id: id.clone(),
            team: side.to_string(),
            name: format!("{side_name} {number}"),
            number,
            position: slot,
        });
        ids.insert(bits, id);
    }
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    Roster { entries, ids }
}

/// Capture one 10 Hz frame: ball + all players in integer decimetres.
pub(crate) fn capture_frame(sim: &Simulation, roster_ids: &HashMap<u64, String>) -> StreamFrame {
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
        .and_then(|entity| roster_ids.get(&entity.to_bits()).cloned());
    let mut players: Vec<FramePlayer> = Vec::new();
    for entity in world.iter_entities() {
        if entity.get::<Player>().is_none() {
            continue;
        }
        let bits = entity.id().to_bits();
        let id = roster_ids.get(&bits).map_or_else(
            || format!("e-{bits}"),
            std::clone::Clone::clone,
        );
        let (x, y) = entity.get::<Position>().map_or((0, 0), |pos| {
            (to_decimetres(pos.0.x), to_decimetres(pos.0.y))
        });
        players.push(FramePlayer { id, x, y });
    }
    players.sort_by(|a, b| a.id.cmp(&b.id));
    let half = world
        .get_resource::<MatchClock>()
        .map_or(1, |clock| clock.half);
    StreamFrame {
        tick: sim.current_tick(),
        half,
        ball: FrameBall {
            x: position[0],
            y: position[1],
            state: ball_state_str(ball.state).to_string(),
            possessor,
        },
        players,
    }
}
