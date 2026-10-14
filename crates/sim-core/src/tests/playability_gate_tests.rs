//! Phase 0 playability gate (PRD §15): does the loop produce football rather
//! than a 0–0 match played in one corner?
//!
//! Two layers:
//!
//! 1. [`PlayabilityTracker`] turns per-tick [`TickSample`]s into
//!    [`MatchMetrics`]. It is a pure function of its samples, so its unit
//!    tests run in default `cargo test`.
//! 2. `phase_0_playability_gate_over_10_seeds` plays ten full matches and
//!    checks the four gate criteria. It is `#[ignore]`d (minutes in debug);
//!    run it in release:
//!
//!    ```text
//!    cargo test --release -p sim-core -- --ignored phase_0_playability_gate --nocapture
//!    ```
//!
//! Interpretation of the gate: possession, halfway crossings and territory
//! must hold for **every** seed; only shots are averaged ("per match on
//! average"). "In play" means `MatchState::InPlay`.

use bevy_ecs::prelude::Entity;
use sim_components::{
    ActionIntent, Ball, Intent, Match, MatchClock, MatchState, Player, Position, TeamId, TouchKind,
    time::TOTAL_MATCH_TICKS,
};
use sim_math::PITCH_LENGTH;

use crate::Simulation;

const HALFWAY_X: f32 = PITCH_LENGTH / 2.0;

/// The ball must be this far (m) from the halfway line before its side counts,
/// so a ball resting on the line does not flicker between sides.
const SIDE_DEAD_BAND: f32 = 1.0;

/// A full match plus slack; a match that has not reached `FullTime` by then
/// is stalled.
const TICK_CAP: u64 = TOTAL_MATCH_TICKS + 1_000;

const GATE_SEEDS: [u64; 10] = [2001, 2002, 2003, 2004, 2005, 2006, 2007, 2008, 2009, 2010];

const MIN_POSSESSION_SHARE: f64 = 0.30;
const MIN_CROSSINGS_PER_HALF: u32 = 20;
const MIN_TERRITORY_SHARE: f64 = 0.25;
const MAX_TERRITORY_SHARE: f64 = 0.75;
const MIN_AVERAGE_SHOTS: f64 = 5.0;

/// What one tick looked like, reduced to what the gate measures.
#[derive(Clone, Copy)]
struct TickSample {
    in_play: bool,
    /// 1 or 2.
    half: u8,
    ball_x: f32,
    possessing_team: Option<TeamId>,
    /// A player kicked the ball this tick while holding a `ShootAtGoal` intent.
    shot_taken: bool,
}

/// Aggregate of one match.
#[derive(Debug)]
struct MatchMetrics {
    reached_full_time: bool,
    in_play_ticks: u64,
    possession_share: [f64; 2],
    crossings: [u32; 2],
    /// Share of in-play ticks with the ball in the x < 52.5 half; the other
    /// half's share is its complement.
    left_half_share: f64,
    shots: u32,
}

#[derive(Default)]
struct PlayabilityTracker {
    in_play_ticks: u64,
    possession_ticks: [u64; 2],
    left_half_ticks: u64,
    crossings: [u32; 2],
    shots: u32,
    /// Last side the ball was clearly on (`true` = x < 52.5).
    last_side_left: Option<bool>,
    was_in_play: bool,
}

#[expect(
    clippy::cast_precision_loss,
    reason = "tick counts are far below 2^52, so the f64 conversion is exact"
)]
fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

impl PlayabilityTracker {
    fn observe(&mut self, sample: TickSample) {
        if !sample.in_play {
            self.was_in_play = false;
            return;
        }
        if !self.was_in_play {
            // Restarts teleport the ball; that jump is not a crossing.
            self.last_side_left = None;
            self.was_in_play = true;
        }
        self.in_play_ticks += 1;
        if let Some(team) = sample.possessing_team {
            self.possession_ticks[usize::from(team.0).min(1)] += 1;
        }
        if sample.ball_x < HALFWAY_X {
            self.left_half_ticks += 1;
        }
        if sample.shot_taken {
            self.shots += 1;
        }

        if (sample.ball_x - HALFWAY_X).abs() > SIDE_DEAD_BAND {
            let side_left = sample.ball_x < HALFWAY_X;
            if self.last_side_left.is_some_and(|previous| previous != side_left) {
                let half_index = usize::from(sample.half.saturating_sub(1)).min(1);
                self.crossings[half_index] += 1;
            }
            self.last_side_left = Some(side_left);
        }
    }

    fn finish(self, reached_full_time: bool) -> MatchMetrics {
        MatchMetrics {
            reached_full_time,
            in_play_ticks: self.in_play_ticks,
            possession_share: self
                .possession_ticks
                .map(|ticks| ratio(ticks, self.in_play_ticks)),
            crossings: self.crossings,
            left_half_share: ratio(self.left_half_ticks, self.in_play_ticks),
            shots: self.shots,
        }
    }
}

/// One way a single match (or the multi-match average) misses the gate.
/// Variants are independent — a match can fail more than one.
#[derive(Debug, Clone, PartialEq)]
enum GateFailure {
    /// The match did not reach `FullTime` within `TICK_CAP`.
    Stalled { seed: u64 },
    /// One team held the ball under `MIN_POSSESSION_SHARE` of in-play ticks.
    Possession { seed: u64, team: usize, share: f64 },
    /// One half saw fewer than `MIN_CROSSINGS_PER_HALF` halfway crossings.
    Crossings { seed: u64, half: u8, count: u32 },
    /// Ball sat in the x < 52.5 half outside `MIN_TERRITORY_SHARE..=MAX_TERRITORY_SHARE`.
    Territory { seed: u64, share: f64 },
    /// Multi-match average shots fell below `MIN_AVERAGE_SHOTS` (this variant
    /// is independent of seed because the average is over all matches).
    AverageShots { average: f64 },
}

impl std::fmt::Display for GateFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stalled { seed } => write!(f, "seed {seed}: never reached full time (stalled)"),
            Self::Possession { seed, team, share } => write!(
                f,
                "seed {seed}: team {team} possession {share:.3} < {MIN_POSSESSION_SHARE}"
            ),
            Self::Crossings { seed, half, count } => write!(
                f,
                "seed {seed}: half {half} halfway crossings {count} < {MIN_CROSSINGS_PER_HALF}"
            ),
            Self::Territory { seed, share } => write!(
                f,
                "seed {seed}: ball in x < 52.5 half {share:.3} of in-play ticks, outside {MIN_TERRITORY_SHARE}..={MAX_TERRITORY_SHARE}"
            ),
            Self::AverageShots { average } => write!(
                f,
                "average shots per match {average:.2} < {MIN_AVERAGE_SHOTS}"
            ),
        }
    }
}

/// Every way `matches` misses the Phase 0 gate; empty means the gate holds.
fn gate_failures(seeds: &[u64], matches: &[MatchMetrics]) -> Vec<GateFailure> {
    let mut failures = Vec::new();
    for (seed, m) in seeds.iter().zip(matches) {
        if !m.reached_full_time {
            failures.push(GateFailure::Stalled { seed: *seed });
        }
        for (team, share) in m.possession_share.iter().enumerate() {
            if *share < MIN_POSSESSION_SHARE {
                failures.push(GateFailure::Possession {
                    seed: *seed,
                    team,
                    share: *share,
                });
            }
        }
        for (half, crossings) in m.crossings.iter().enumerate() {
            if *crossings < MIN_CROSSINGS_PER_HALF {
                failures.push(GateFailure::Crossings {
                    seed: *seed,
                    half: u8::try_from(half + 1).expect("invariant: half index fits u8"),
                    count: *crossings,
                });
            }
        }
        if !(MIN_TERRITORY_SHARE..=MAX_TERRITORY_SHARE).contains(&m.left_half_share) {
            failures.push(GateFailure::Territory {
                seed: *seed,
                share: m.left_half_share,
            });
        }
    }
    let total_shots: u64 = matches.iter().map(|m| u64::from(m.shots)).sum();
    let count = u64::try_from(matches.len()).expect("invariant: match count fits u64");
    let average_shots = ratio(total_shots, count);
    if average_shots < MIN_AVERAGE_SHOTS {
        failures.push(GateFailure::AverageShots {
            average: average_shots,
        });
    }
    failures
}

/// Reads [`TickSample`]s off a running `Simulation`.
#[derive(Default)]
struct PlayObserver {
    previous_possessor: Option<Entity>,
}

impl PlayObserver {
    fn sample(&mut self, sim: &Simulation) -> TickSample {
        let world = sim.world();
        let in_play = world.resource::<Match>().state == MatchState::InPlay;
        let half = world.resource::<MatchClock>().half;
        let ball = world.resource::<Ball>();
        let ball_x = world
            .get::<Position>(sim.ball_entity())
            .map_or(HALFWAY_X, |position| position.0.x);
        let possessing_team = ball
            .possessor
            .and_then(|entity| world.get::<Player>(entity))
            .map(|player| player.team_id);

        // A kick clears the possessor and registers a `Play` touch by the
        // kicker; it is a shot when the kicker's intent was `ShootAtGoal`.
        let kicker = self.previous_possessor.filter(|previous| {
            ball.possessor.is_none()
                && ball.last_touch.is_some_and(|touch| {
                    touch.kind == TouchKind::Play && touch.player == Some(*previous)
                })
        });
        let shot_taken = in_play
            && kicker
                .and_then(|entity| world.get::<Player>(entity))
                .is_some_and(|player| {
                    matches!(player.intent, Some(Intent::Action(ActionIntent::ShootAtGoal(_))))
                });
        self.previous_possessor = ball.possessor;

        TickSample {
            in_play,
            half,
            ball_x,
            possessing_team,
            shot_taken,
        }
    }
}

fn play_full_match(seed: u64) -> MatchMetrics {
    let mut sim = Simulation::new(seed);
    let mut observer = PlayObserver::default();
    let mut tracker = PlayabilityTracker::default();
    let mut reached_full_time = false;
    for _ in 0..TICK_CAP {
        sim.tick();
        tracker.observe(observer.sample(&sim));
        if sim.world().resource::<Match>().state == MatchState::FullTime {
            reached_full_time = true;
            break;
        }
    }
    tracker.finish(reached_full_time)
}

fn sample_at(x: f32, half: u8) -> TickSample {
    TickSample {
        in_play: true,
        half,
        ball_x: x,
        possessing_team: None,
        shot_taken: false,
    }
}

fn dead_ball() -> TickSample {
    TickSample {
        in_play: false,
        ..sample_at(HALFWAY_X, 1)
    }
}

#[test]
fn shares_are_fractions_of_in_play_ticks_only() {
    let mut tracker = PlayabilityTracker::default();
    for team in [Some(TeamId(0)), Some(TeamId(0)), Some(TeamId(1)), None] {
        tracker.observe(TickSample {
            possessing_team: team,
            ..sample_at(10.0, 1)
        });
    }
    for _ in 0..6 {
        tracker.observe(dead_ball());
    }
    let m = tracker.finish(true);
    assert_eq!(m.in_play_ticks, 4);
    assert_eq!(m.possession_share, [0.5, 0.25]);
    assert_eq!(m.left_half_share, 1.0);
}

#[test]
fn crossings_count_side_flips_per_half() {
    let mut tracker = PlayabilityTracker::default();
    // First half: left, left, right (1), through the centre band, left (2).
    for x in [10.0, 20.0, 80.0, 52.5, 30.0] {
        tracker.observe(sample_at(x, 1));
    }
    tracker.observe(dead_ball());
    // Second half: right, left (1).
    for x in [80.0, 20.0] {
        tracker.observe(sample_at(x, 2));
    }
    assert_eq!(tracker.finish(true).crossings, [2, 1]);
}

#[test]
fn a_restart_teleport_is_not_a_crossing() {
    let mut tracker = PlayabilityTracker::default();
    tracker.observe(sample_at(10.0, 1));
    tracker.observe(dead_ball());
    tracker.observe(sample_at(80.0, 1));
    assert_eq!(tracker.finish(true).crossings, [0, 0]);
}

#[test]
fn shots_are_counted_in_play_only() {
    let mut tracker = PlayabilityTracker::default();
    tracker.observe(TickSample {
        shot_taken: true,
        ..sample_at(90.0, 1)
    });
    tracker.observe(TickSample {
        shot_taken: true,
        ..dead_ball()
    });
    assert_eq!(tracker.finish(true).shots, 1);
}

fn healthy_match() -> MatchMetrics {
    MatchMetrics {
        reached_full_time: true,
        in_play_ticks: 300_000,
        possession_share: [0.45, 0.40],
        crossings: [30, 28],
        left_half_share: 0.5,
        shots: 12,
    }
}

#[test]
fn gate_passes_for_a_healthy_match() {
    assert!(gate_failures(&[1], &[healthy_match()]).is_empty());
}

#[test]
fn gate_names_each_failed_criterion() {
    use GateFailure::{AverageShots, Crossings, Possession, Stalled, Territory};
    let cases: [(GateFailure, MatchMetrics); 5] = [
        (Stalled { seed: 7 }, MatchMetrics { reached_full_time: false, ..healthy_match() }),
        (
            Possession { seed: 7, team: 1, share: 0.05 },
            MatchMetrics { possession_share: [0.9, 0.05], ..healthy_match() },
        ),
        (
            Crossings { seed: 7, half: 2, count: 19 },
            MatchMetrics { crossings: [30, 19], ..healthy_match() },
        ),
        (
            Territory { seed: 7, share: 0.9 },
            MatchMetrics { left_half_share: 0.9, ..healthy_match() },
        ),
        (
            // Shots is checked across the whole batch; one match with 4 shots
            // averages 4/match, which is below the 5/match floor.
            AverageShots { average: 4.0 },
            MatchMetrics { shots: 4, ..healthy_match() },
        ),
    ];
    for (expected, metrics) in cases {
        let failures = gate_failures(&[7], &[metrics]);
        assert!(
            failures.contains(&expected),
            "expected {expected:?}, got {failures:?}"
        );
    }
}

#[test]
fn shots_gate_is_an_average_across_seeds() {
    let few = MatchMetrics { shots: 2, ..healthy_match() };
    let many = MatchMetrics { shots: 10, ..healthy_match() };
    assert!(gate_failures(&[1, 2], &[few, many]).is_empty(), "(2 + 10) / 2 = 6 ≥ 5");
}

/// Phase 0 exit gate: ten full matches, reported and checked.
#[test]
#[ignore = "10 full matches (minutes). Run: cargo test --release -p sim-core -- --ignored phase_0_playability_gate --nocapture"]
fn phase_0_playability_gate_over_10_seeds() {
    let matches: Vec<MatchMetrics> = GATE_SEEDS.iter().map(|seed| play_full_match(*seed)).collect();

    println!("seed   full_time  poss_home  poss_away  cross_h1  cross_h2  left_half  shots");
    for (seed, m) in GATE_SEEDS.iter().zip(&matches) {
        println!(
            "{seed}   {:<9}  {:<9.3}  {:<9.3}  {:<8}  {:<8}  {:<9.3}  {}",
            m.reached_full_time,
            m.possession_share[0],
            m.possession_share[1],
            m.crossings[0],
            m.crossings[1],
            m.left_half_share,
            m.shots
        );
    }

    let failures = gate_failures(&GATE_SEEDS, &matches);
    let rendered: Vec<String> = failures.iter().map(std::string::ToString::to_string).collect();
    assert!(
        failures.is_empty(),
        "Phase 0 playability gate failed:\n{}",
        rendered.join("\n")
    );
}
