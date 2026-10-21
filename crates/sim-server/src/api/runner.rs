//! Match simulation runner: one full match on the calling OS thread.
//!
//! [`run_match_blocking`] drives a [`Simulation`][sim_core::Simulation]
//! to `FullTime` (or the tick cap), enforces the six PRD §12 invariants
//! per tick, captures 10 Hz frames, and assembles the
//! [`CompletedMatch`][crate::api::stream::CompletedMatch] the stream
//! builder consumes.

use std::collections::HashMap;
use std::collections::TryReserveError;

use sim_components::{Ball, Match, MatchClock, Player, PlayerIdentity, Position, Team};
use sim_core::{EventLog, MatchEvent, Simulation, Termination, TickHook, run_match_driver};

use super::stream::{
    CompletedMatch, FrameBall, FramePlayer, HalfBoundaries, ManifestTeam, RosterEntry, SIDE_AWAY,
    SIDE_HOME, StreamFrame, ball_state_str, to_decimetres,
};
/// Result of running one match to completion on its OS thread.
#[derive(Debug)]
pub enum MatchOutcome {
    /// Reached `FullTime` with no invariant violation.
    Finished(CompletedMatch),
    /// Invariant violation, tick-cap breach, runner panic (PRD §12) or
    /// resource exhaustion; cached like a success so retries are
    /// deterministic, except for a transient failure.
    Failed {
        /// Machine-readable reason (`tick_cap_exceeded`,
        /// `runner_panic`, `thread_spawn_failed`,
        /// `resource_exhausted` or the invariant code).
        code: String,
        /// Human-readable detail.
        message: String,
        /// Tick the simulation reached before failing.
        last_good_tick: u64,
        /// Whether the failure may be retried.
        kind: FailureKind,
    },
}

/// Why a match failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// The failure repeats: the same fixture and the same seed fail
    /// the same way, so the result is cached for the life of the
    /// process. A retry would burn the same minutes again.
    Deterministic,
    /// A resource failure: the run could not get a resource it needed.
    ///
    /// The pressure belongs to the host, not to the match, so the same
    /// match may succeed once the pressure has passed. The failure
    /// carries a retry window instead of being cached forever.
    Transient,
}

/// The failure code of a transient resource failure.
pub const RESOURCE_FAILURE_CODE: &str = "resource_exhausted";

/// A resource failure: the run could not obtain the memory it needed.
///
/// The failure is transient by nature — the memory belongs to the
/// host, not to the match — so the same match may succeed later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResourceError {
    message: String,
}

impl ResourceError {
    /// A failed reservation for `what`.
    pub(crate) fn reservation_failed(what: &str, error: &TryReserveError) -> Self {
        Self {
            message: resource_message(what, error),
        }
    }

    /// The failure message.
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

/// The message a failed reservation reports.
fn resource_message(what: &str, error: &TryReserveError) -> String {
    format!("could not allocate the {what}: {error}")
}

/// The failure a capture that could not allocate its memory reports.
///
/// The failure is transient: the pressure belongs to the host, not to
/// the match, so the same match may succeed later.
pub(crate) fn resource_failure(message: String, last_good_tick: u64) -> MatchOutcome {
    MatchOutcome::Failed {
        code: RESOURCE_FAILURE_CODE.to_string(),
        message,
        last_good_tick,
        kind: FailureKind::Transient,
    }
}

/// Copy `source` into a fresh string, reporting a resource failure
/// instead of aborting the process when the allocation fails.
///
/// `String::clone` aborts the process on an allocation failure; the
/// capture path uses this instead, so a host under memory pressure
/// loses the match rather than the whole server.
pub(crate) fn copy_string(source: &str) -> Result<String, ResourceError> {
    let mut copy = String::new();
    copy.try_reserve(source.len())
        .map_err(|error| ResourceError::reservation_failed("frame text", &error))?;
    copy.push_str(source);
    Ok(copy)
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
                // A panic repeats: the same seed panics at the same
                // place, so the failure is cached.
                kind: FailureKind::Deterministic,
            }
        }
    }
}

/// Extract a human-readable message from a panic payload.
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
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
            let frame = capture_frame(sim, self.roster_ids)
                .map_err(|error| error.message().to_string())?;
            self.frames
                .try_reserve(1)
                .map_err(|error| resource_message("frame buffer", &error))?;
            self.frames.push(frame);
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
/// The frame capture is the run's largest allocation: a full match
/// keeps about 54,000 frames of 22 players in memory at once. The
/// capture therefore reserves its memory fallibly, and a host that
/// cannot give it the memory fails the match with a transient
/// [`RESOURCE_FAILURE_CODE`] instead of aborting the process.
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
    let roster = build_roster(&sim, fixture);
    let match_id = fixture.validated_match.id.clone();
    let seed = fixture.validated_match.seed;

    let mut frames: Vec<StreamFrame> = Vec::new();
    let mut hook = RunnerHook {
        roster_ids: &roster.ids,
        frames: &mut frames,
    };
    let result = match run_match_driver(&mut sim, tick_cap, &mut hook) {
        Ok(result) => result,
        Err(message) => {
            // The hook's only side effect is capturing the frames, so
            // an error here is a resource failure: the run could not
            // get the memory it needed. The pressure belongs to the
            // host, not to the match, so the failure is transient and
            // a later start may run the same match again.
            return resource_failure(message, sim.current_tick());
        }
    };

    if let Some(violation) = result.violation {
        return MatchOutcome::Failed {
            code: violation.code().to_string(),
            message: violation.message(),
            last_good_tick: violation.last_good_tick(),
            kind: FailureKind::Deterministic,
        };
    }

    let world = sim.world();
    let match_res = world.get_resource::<Match>().cloned();
    let Some(match_res) = match_res else {
        return MatchOutcome::Failed {
            code: "no_match_resource".to_string(),
            message: "Match resource missing after simulation".to_string(),
            last_good_tick: sim.current_tick(),
            kind: FailureKind::Deterministic,
        };
    };
    if result.termination != Termination::FullTime
        || match_res.state != sim_components::MatchState::FullTime
    {
        return MatchOutcome::Failed {
            code: "tick_cap_exceeded".to_string(),
            message: format!(
                "match did not reach FullTime within {tick_cap} ticks (state {:?})",
                match_res.state
            ),
            last_good_tick: sim.current_tick(),
            kind: FailureKind::Deterministic,
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
        teams: manifest_teams(fixture),
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

/// The manifest's per-side lineup setups, taken from the fixture.
///
/// The fixture is the source of truth (PRD §5.4): the home and away
/// setups are published independently, so a match whose sides play
/// different formations or tactics says so.
pub(crate) fn manifest_teams(fixture: &crate::api::MatchFixture) -> Vec<ManifestTeam> {
    vec![
        ManifestTeam::from_setup(SIDE_HOME, &fixture.validated_match.home_setup),
        ManifestTeam::from_setup(SIDE_AWAY, &fixture.validated_match.away_setup),
    ]
}

/// Roster published in the manifest: one row per fixture player, in the
/// fixture's roster order, plus the entity → roster id map the frames
/// use.
pub(crate) struct Roster {
    pub(crate) entries: Vec<RosterEntry>,
    pub(crate) ids: HashMap<u64, String>,
}

/// Build the roster from the fixture.
///
/// The rows are the fixture's own players — their `player_id`, name and
/// squad number — in the fixture's roster order, and each row's
/// position is the formation slot the player occupies (the fixture's
/// formation table at the player's roster index).
///
/// The entity → id map comes from the world's [`PlayerIdentity`]
/// components, so a frame names the same player the manifest lists.
pub(crate) fn build_roster(sim: &Simulation, fixture: &crate::api::MatchFixture) -> Roster {
    let mut entries = side_roster(
        SIDE_HOME,
        &fixture.home_team,
        &fixture.validated_match.home_setup,
    );
    entries.extend(side_roster(
        SIDE_AWAY,
        &fixture.away_team,
        &fixture.validated_match.away_setup,
    ));

    let mut ids = HashMap::with_capacity(entries.len());
    for entity in sim.world().iter_entities() {
        if let Some(identity) = entity.get::<PlayerIdentity>() {
            ids.insert(entity.id().to_bits(), identity.id.clone());
        }
    }
    Roster { entries, ids }
}

/// One side's roster rows: the fixture's players in roster order.
fn side_roster(
    side: &str,
    team: &sim_components::fixture::ValidatedTeam,
    setup: &sim_components::fixture::ValidatedSide,
) -> Vec<RosterEntry> {
    let slots = setup.formation.table().slots;
    team.players
        .iter()
        .enumerate()
        .map(|(index, player)| RosterEntry {
            id: player.id.clone(),
            team: side.to_string(),
            name: player.name.clone(),
            number: player.number,
            position: slots
                .get(index)
                .map_or_else(|| "UNK".to_string(), |slot| slot.position.to_string()),
        })
        .collect()
}

/// Capture one 10 Hz frame: ball + all players in integer decimetres.
///
/// Every allocation the frame needs is reserved fallibly: the capture
/// is the run's largest allocation, and a host that cannot give it
/// the memory fails the match instead of aborting the process.
pub(crate) fn capture_frame(
    sim: &Simulation,
    roster_ids: &HashMap<u64, String>,
) -> Result<StreamFrame, ResourceError> {
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
        .and_then(|entity| roster_ids.get(&entity.to_bits()));
    let mut players: Vec<(u8, FramePlayer)> = Vec::new();
    for entity in world.iter_entities() {
        let Some(player) = entity.get::<Player>() else {
            continue;
        };
        let bits = entity.id().to_bits();
        // Every fixture player carries a `PlayerIdentity`, so a frame
        // never names a player with an id invented from the entity bits.
        let id = roster_ids
            .get(&bits)
            .expect("invariant: every fixture player carries a PlayerIdentity");
        let (x, y) = entity.get::<Position>().map_or((0, 0), |pos| {
            (to_decimetres(pos.0.x), to_decimetres(pos.0.y))
        });
        players
            .try_reserve(1)
            .map_err(|error| ResourceError::reservation_failed("player list", &error))?;
        players.push((player.team_id.0, FramePlayer {
            id: copy_string(id)?,
            x,
            y,
        }));
    }
    // Sorted by id, then by side: a mirror match puts the same fixture
    // ids on both sides, and the side breaks the tie deterministically.
    players.sort_by(|left, right| left.1.id.cmp(&right.1.id).then(left.0.cmp(&right.0)));
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
    Ok(StreamFrame {
        tick: sim.current_tick(),
        half,
        ball: FrameBall {
            x: position[0],
            y: position[1],
            state: copy_string(ball_state_str(ball.state))?,
            possessor: possessor.map(|id| copy_string(id)).transpose()?,
        },
        players: sorted,
    })
}
