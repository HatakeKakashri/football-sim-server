//! Smoke tests for issues 02 and 03: per-team, per-half attack direction
//! with half-time swap, and 4-4-2 home slots.
//!
//! Active tests pin the contracts those issues deliver:
//!
//! 1. `attack_direction` flips at `HalfTime → Kickoff`, and home slots mirror
//!    with it.
//! 2. Perception's `goal_position` follows each team's per-half direction.
//! 3. A mirror-seed match (sides swapped) is an exact point reflection of the
//!    canonical match over a 3-minute window.
//! 4. Home slots are distinct and spread across the pitch while a match runs.
//!
//! The PRD §14 windowed smoke test (both teams *play* the ball, ball crosses
//! halfway, at least one shot, within 3 minutes) runs in default `cargo test`
//! for the canonical seed and a spread of others (issue 06).

use bevy_ecs::prelude::Entity;
use sim_components::{
    ActionIntent, FormationSlot, HomeSlot, Intent, Match, MatchState, PerceptionSnapshot, Player,
    Team, TeamId, TouchKind,
};
use sim_math::{PITCH_LENGTH, PITCH_WIDTH, Vec2, goal_position_for};

use crate::Simulation;
use crate::simulation::formation::reset_formation;
use crate::simulation::lifecycle::HalfTimeEntryTick;

/// Halfway line along the pitch length.
const HALFWAY_X: f32 = PITCH_LENGTH / 2.0;

/// The 3-minute window used by the PRD §14 smoke criteria (60 ticks/s).
const THREE_MINUTES_TICKS: u64 = 3 * 60 * 60;

fn team_entities(sim: &Simulation) -> (Entity, Entity) {
    let m = sim.world().resource::<Match>();
    (m.home_team, m.away_team)
}

fn attack_direction(sim: &Simulation, team: Entity) -> i8 {
    sim.world()
        .entity(team)
        .get::<Team>()
        .expect("invariant: team entity has a Team component")
        .attack_direction()
}

/// Force the half-time break and tick until the second half is in play.
fn advance_to_second_half(sim: &mut Simulation) {
    sim.world_mut().resource_mut::<Match>().state = MatchState::HalfTime;
    sim.world_mut().insert_resource(HalfTimeEntryTick { tick: Some(0) });

    // The break is HALFTIME_BREAK_TICKS (18), so HalfTime → Kickoff → InPlay
    // settles within 20 ticks.
    for _ in 0..20 {
        sim.tick();
        if sim.world().resource::<Match>().state == MatchState::InPlay {
            return;
        }
    }
    panic!("invariant: match must reach InPlay within 20 ticks of the half-time break");
}

/// Issue 02: `attack_direction` is +1 for Home and -1 for Away in the first
/// half and flips after `HalfTime → Kickoff`; issue 03: each player's home
/// slot follows the flip, so the same player sits at the opposite end.
#[test]
fn test_attack_direction_half_time_swap() {
    let mut sim = Simulation::new(12345);
    let (home, away) = team_entities(&sim);

    assert_eq!(attack_direction(&sim, home), 1, "home attacks +x in the first half");
    assert_eq!(attack_direction(&sim, away), -1, "away attacks -x in the first half");

    sim.tick();
    let first_half_slots: Vec<(Entity, Vec2)> = sim
        .world_mut()
        .query::<(Entity, &HomeSlot)>()
        .iter(sim.world())
        .map(|(e, slot)| (e, slot.position()))
        .collect();
    assert_eq!(first_half_slots.len(), 22, "every player carries a HomeSlot");

    advance_to_second_half(&mut sim);

    assert_eq!(attack_direction(&sim, home), -1, "home attacks -x in the second half");
    assert_eq!(attack_direction(&sim, away), 1, "away attacks +x in the second half");

    // Each player's slot is the exact 180° reflection of its first-half slot.
    for (entity, before) in first_half_slots {
        let after = sim
            .world()
            .entity(entity)
            .get::<HomeSlot>()
            .expect("invariant: player keeps its HomeSlot")
            .position();
        assert!(
            (after.x - (PITCH_LENGTH - before.x)).abs() < 1e-3
                && (after.y - (PITCH_WIDTH - before.y)).abs() < 1e-3,
            "second-half slot {after:?} must mirror first-half slot {before:?}"
        );
    }
}

/// Issue 02: each team's `PerceptionSnapshot.goal_position` is derived from
/// `attack_direction` in both halves, not from a hardcoded constant.
#[test]
fn test_perception_goal_position_follows_attack_direction_in_both_halves() {
    let mut sim = Simulation::new(12345);

    for second_half in [false, true] {
        if second_half {
            advance_to_second_half(&mut sim);
        }
        // Run one more tick so perception sees the current directions.
        sim.tick();

        let (home_dir, away_dir) = if second_half { (-1, 1) } else { (1, -1) };
        let mut seen = [false; 2];
        for (player, snapshot) in sim
            .world_mut()
            .query::<(&Player, &PerceptionSnapshot)>()
            .iter(sim.world())
        {
            let expected = if player.team_id == TeamId(0) {
                goal_position_for(home_dir)
            } else {
                goal_position_for(away_dir)
            };
            assert_eq!(
                snapshot.goal_position, expected,
                "team {:?} goal (second_half = {second_half})",
                player.team_id
            );
            seen[usize::from(player.team_id.0)] = true;
        }
        assert_eq!(seen, [true, true], "both teams have perception snapshots");
    }
}

/// Issue 03: while a match runs, the 22 home slots are distinct, well apart,
/// and none sits on the centre spot — the opposite of "all players on top of
/// each other at the centre".
#[test]
fn test_slot_positions_distinct_and_spread_in_match() {
    const MIN_SEPARATION: f32 = 3.0;
    let mut sim = Simulation::new(12345);
    let centre = Vec2::new(HALFWAY_X, PITCH_WIDTH / 2.0);

    for _ in 0..300 {
        sim.tick();

        let slots: Vec<Vec2> = sim
            .world_mut()
            .query::<(&FormationSlot, &HomeSlot)>()
            .iter(sim.world())
            .map(|(_, slot)| slot.position())
            .collect();
        assert_eq!(slots.len(), 22, "every player has a formation slot");

        for (i, a) in slots.iter().enumerate() {
            assert!(a.distance(centre) > 5.0, "slot {a:?} must not sit on the centre spot");
            for b in &slots[i + 1..] {
                assert!(
                    a.distance(*b) >= MIN_SEPARATION,
                    "slots {a:?} and {b:?} are closer than {MIN_SEPARATION} m"
                );
            }
        }
    }
}

/// Issue 02 (mirror-seed): the same squads with sides swapped play out as the
/// exact point reflection `(x, y) → (105 - x, 68 - y)` of the canonical match
/// over the 3-minute window, so the ball's attack-camp end follows the team's
/// per-half direction. The canonical ball crosses halfway into x > 52.5 and the
/// mirrored ball into x < 52.5.
#[test]
fn test_mirror_match_is_exact_reflection_over_three_minutes() {
    const TOLERANCE: f32 = 0.01;

    let mut canonical = Simulation::new(12345);

    let mut mirrored = Simulation::new(12345);
    let (home, away) = team_entities(&mirrored);
    for (team, direction) in [(home, -1_i8), (away, 1_i8)] {
        mirrored
            .world_mut()
            .entity_mut(team)
            .get_mut::<Team>()
            .expect("invariant: team entity has a Team component")
            .set_attack_direction(direction);
    }
    reset_formation(mirrored.world_mut());

    let (mut canonical_max_x, mut mirrored_min_x) = (f32::NEG_INFINITY, f32::INFINITY);
    for tick in 0..THREE_MINUTES_TICKS {
        canonical.tick();
        mirrored.tick();
        let c = canonical.get_state().expect("get_state succeeded");
        let m = mirrored.get_state().expect("get_state succeeded");

        let [cx, cy] = c.ball.position;
        let [mx, my] = m.ball.position;
        if tick < 200 {
            assert!(
                (PITCH_LENGTH - cx - mx).abs() < TOLERANCE && (PITCH_WIDTH - cy - my).abs() < TOLERANCE,
                "tick {tick}: mirrored ball ({mx}, {my}) must reflect canonical ({cx}, {cy})"
            );
        }
        canonical_max_x = canonical_max_x.max(cx);
        mirrored_min_x = mirrored_min_x.min(mx);
    }

    assert!(canonical_max_x > HALFWAY_X + 1.0, "home (+x) ball must cross halfway into x > {HALFWAY_X}");
    assert!(mirrored_min_x < HALFWAY_X - 1.0, "home (-x) ball must cross halfway into x < {HALFWAY_X}");
}

/// What a windowed smoke run saw: the three PRD §14 criteria.
struct SmokeOutcome {
    /// Per team: did it *play* the ball (`Play` touches only — the kickoff
    /// `Restart` placement does not count)?
    played: [bool; 2],
    crossed_halfway: bool,
    shot_occurred: bool,
}

impl SmokeOutcome {
    const fn complete(&self) -> bool {
        self.played[0] && self.played[1] && self.crossed_halfway && self.shot_occurred
    }
}

/// Run `seed` for at most the 3-minute window, stopping early once every
/// criterion has been seen.
fn run_windowed_smoke(seed: u64) -> SmokeOutcome {
    let mut sim = Simulation::new(seed);
    let mut outcome = SmokeOutcome {
        played: [false; 2],
        crossed_halfway: false,
        shot_occurred: false,
    };

    for _ in 0..THREE_MINUTES_TICKS {
        sim.tick();
        let state = sim.get_state().expect("get_state succeeded");

        if (state.ball.position[0] - HALFWAY_X).abs() > 1.0 {
            outcome.crossed_halfway = true;
        }
        if let Some(touch) = state.ball.last_touch
            && touch.kind == TouchKind::Play
        {
            outcome.played[usize::from(touch.team.0)] = true;
        }
        for player in sim.world_mut().query::<&Player>().iter(sim.world()) {
            if let Some(Intent::Action(ActionIntent::ShootAtGoal(_))) = player.intent {
                outcome.shot_occurred = true;
            }
        }
        if outcome.complete() {
            break;
        }
    }
    outcome
}

fn assert_smoke_criteria(seed: u64, outcome: &SmokeOutcome) {
    assert!(outcome.crossed_halfway, "seed {seed}: ball must cross halfway within 3 minutes");
    assert!(outcome.played[0], "seed {seed}: team 0 must play the ball within 3 minutes");
    assert!(outcome.played[1], "seed {seed}: team 1 must play the ball within 3 minutes");
    assert!(outcome.shot_occurred, "seed {seed}: at least one shot must occur within 3 minutes");
}

/// Full PRD §14 windowed smoke test on the canonical seed: both teams *play*
/// the ball, the ball crosses halfway, and at least one shot is chosen, all
/// within 3 minutes.
#[test]
fn test_windowed_smoke_test_prd_14() {
    assert_smoke_criteria(12345, &run_windowed_smoke(12345));
}

/// Issue 06: the same criteria hold for every seed in a small spread, so the
/// goal-direction bug cannot hide behind one lucky seed.
#[test]
fn test_windowed_smoke_test_prd_14_multi_seed() {
    for seed in [1001, 1002, 1003, 1004, 1005] {
        assert_smoke_criteria(seed, &run_windowed_smoke(seed));
    }
}

/// Issue 05: Nearest player moves toward free ball and acquires possession
/// within a bounded number of ticks (<= 90 ticks / 1.5s).
#[test]
fn test_free_ball_retrieval_bounded_ticks() {
    let mut sim = Simulation::new(12345);
    let mut ticks_free = 0_u64;
    let mut acquired_after_free_ticks = None;

    for _ in 1..=300 {
        sim.tick();
        let state = sim.get_state().expect("get_state succeeded");
        if state.ball.state == sim_components::BallState::Free {
            ticks_free += 1;
        } else if ticks_free > 0 && state.ball.possessor.is_some() {
            acquired_after_free_ticks = Some(ticks_free);
            break;
        }
    }

    assert!(
        acquired_after_free_ticks.is_some_and(|t| t <= 90),
        "free ball must be acquired within 90 ticks (1.5s); took {acquired_after_free_ticks:?} free ticks"
    );
}

/// Issue 05 / PRD §13 A2: Measurement over 10 seeds shows no free-ball window
/// exceeding PRD §13 A2 (<= 30s = 1800 ticks).
#[test]
fn test_multi_seed_free_ball_window_prd_13_a2() {
    const WINDOW_TICKS: u64 = 1800; // 30s game time (PRD §13 A2)
    const TEST_TICKS: u64 = 3600;   // 60s test window per seed
    let seeds = [1001, 1002, 1003, 1004, 1005, 1006, 1007, 1008, 1009, 1010];

    for seed in seeds {
        let mut sim = Simulation::new(seed);
        let mut consecutive_free_ticks = 0_u64;
        let mut max_free_ticks = 0_u64;

        for _ in 0..TEST_TICKS {
            sim.tick();
            let state = sim.get_state().expect("get_state succeeded");
            if state.ball.state == sim_components::BallState::Free {
                consecutive_free_ticks += 1;
                max_free_ticks = max_free_ticks.max(consecutive_free_ticks);
            } else {
                consecutive_free_ticks = 0;
            }
        }

        assert!(
            max_free_ticks <= WINDOW_TICKS,
            "seed {seed}: max free-ball window {max_free_ticks} ticks exceeded PRD §13 A2 threshold {WINDOW_TICKS} ticks"
        );
    }
}


