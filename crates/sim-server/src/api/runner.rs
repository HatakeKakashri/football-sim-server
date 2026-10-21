//! Match simulation runner: one full match on the calling OS thread.
//!
//! [`run_match_blocking`] drives a [`Simulation`][sim_core::Simulation]
//! to `FullTime` (or the tick cap), enforces the six PRD §12 invariants
//! per tick, captures 10 Hz frames, and assembles the
//! [`CompletedMatch`][crate::api::model::CompletedMatch] the render
//! step consumes.
//!
//! # Frames
//!
//! A captured frame names its 22 players by their roster index — the
//! row they occupy in the manifest's roster — and holds no player id
//! string. The ids live once, in the roster the manifest publishes, so
//! a match's frames cost a row per player instead of an id string per
//! player: 1,188,066 fewer string allocations and 81 MB less resident
//! memory for one running match (143 MB against 62 MB, measured with
//! `api::memory_tests`). The render step resolves the index back to the
//! id when it writes the body.

use std::collections::TryReserveError;

use sim_components::{Match, Team};
use sim_core::{EventLog, MatchEvent, Simulation, Termination, TickHook, run_match_driver};

use super::model::{
    CompletedMatch, HalfBoundaries, ManifestTeam, MatchFrame, SIDE_AWAY, SIDE_HOME,
};
mod capture;
mod roster;

pub(crate) use capture::capture_frame;
pub(crate) use roster::{Roster, build_roster};

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
    roster: &'a Roster,
    frames: &'a mut Vec<MatchFrame>,
}

impl TickHook for RunnerHook<'_> {
    fn on_tick(&mut self, sim: &mut Simulation) -> Result<(), String> {
        if sim.current_tick().is_multiple_of(6) {
            let frame = capture_frame(sim, self.roster)
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
/// keeps about 54,000 frames of 22 players in memory at once. Each
/// frame names its players by their roster index, so a frame holds no
/// player id string — the ids live once, in the roster — and the
/// capture reserves its memory fallibly: a host that cannot give it
/// the memory fails the match with a transient
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

    let mut frames: Vec<MatchFrame> = Vec::new();
    let mut hook = RunnerHook {
        roster: &roster,
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
