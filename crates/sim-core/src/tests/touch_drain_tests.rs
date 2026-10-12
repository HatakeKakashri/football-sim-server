//! Tests for the per-tick touch-drain contract owned by `Simulation::tick`.
//!
//! `apply_ball_touched_system` has **two** invocation sites, and both
//! matter:
//!
//! 1. Inside the `Rules` set (`simulation/construct.rs`), so the OOB /
//!    goal / foul / restart writers all read the same-tick
//!    touch written by the `Execution` and `Possession` sets.
//! 2. In `Simulation::tick` (`simulation/tick.rs`), via
//!    `world.run_system_once`, immediately after `lifecycle_system`
//!    returns. `lifecycle_system` is a post-schedule driver, so the
//!    kickoff placement it performs is invisible to the `Rules` set on
//!    the tick it happens — the second drain is what makes that touch
//!    land on `Ball.last_touch` before the tick ends.
//!
//! Without site 2 the kickoff `Restart` touch would not reach
//! `Ball.last_touch` until the *next* tick's `Rules` set, and the
//! OOB / goal pre-touch guards (`out_of_bounds_system`,
//! `goal_detection_system`) would see a stale or absent touch for the
//! first tick of each half. These tests pin that contract.

use crate::Simulation;
use sim_components::{Ball, PendingBallTouches, TouchKind};

/// The kickoff placement pushed by `lifecycle_system` must be drained
/// onto `Ball.last_touch` **within the same tick** that created it —
/// i.e. the post-schedule drain in `Simulation::tick` must run. Without
/// it, `Ball.last_touch` would still be `None` after tick 1.
#[test]
fn kickoff_touch_lands_on_ball_in_the_tick_it_is_created() {
    let mut sim = Simulation::new(12345);

    // One tick drives `PreMatch → Kickoff` (ball placed at the centre
    // spot, `lifecycle_system` pushes the kickoff `Restart` touch).
    sim.tick();

    let ball = sim.world().resource::<Ball>();
    let touch = ball
        .last_touch
        .expect("invariant: kickoff placement must register a touch within tick 1");

    assert_eq!(
        touch.kind,
        TouchKind::Restart,
        "kickoff placement is a Restart placement (Law 8), not a Play touch"
    );
    assert_eq!(
        touch.team,
        sim_components::TeamId(0),
        "first-half kickoff is taken by the home team (Law 8)"
    );
    assert!(
        touch.player.is_some(),
        "a real squad must yield a concrete kickoff taker"
    );
}

/// The buffer must be empty once the tick ends. A leftover entry means
/// a writer pushed but neither drain site ran — the same stale-touch
/// class of bug the drains exist to prevent.
#[test]
fn pending_touches_are_fully_drained_by_the_end_of_the_tick() {
    let mut sim = Simulation::new(12345);

    // Tick 1 places the ball (kickoff touch). Tick 2 transitions to
    // InPlay and applies the kickoff impulse. Ticks 3+ are ordinary
    // in-play ticks with kicks and possession changes flowing through
    // the buffer.
    for _ in 0..60 {
        sim.tick();
        assert!(
            sim.world().resource::<PendingBallTouches>().0.is_empty(),
            "PendingBallTouches must be drained by the end of every tick"
        );
    }
}

/// A `Kickoff` transition is the *only* place `lifecycle_system`
/// writes a touch, and it happens on the first tick of each half. If
/// the post-schedule drain were removed, the second-half kickoff touch
/// would lag by one tick and the `half >= 2` team flip
/// (`apply_transition`) would be masked by the previous half's touch.
/// Pin that the second-half kickoff attributes to the away team.
#[test]
fn second_half_kickoff_touch_attributes_to_the_away_team() {
    use sim_components::{MatchClock, MatchState};

    let mut sim = Simulation::new(12345);

    // Jump the clock to just before half-time. `decide_transition`
    // compares `elapsed_ticks` against `HALF_LENGTH_TICKS +
    // added_time_ticks`; setting `elapsed_ticks` directly drives
    // `InPlay → HalfTime` on the next tick, and then
    // `HalfTime → Kickoff` once the break elapses.
    if let Some(mut clock) = sim.world_mut().get_resource_mut::<MatchClock>() {
        clock.elapsed_ticks = sim_components::time::HALF_LENGTH_TICKS;
    }

    // Drive past half-time and through the break. The loop is bounded:
    // the break is a fixed tick count, and the kickoff lands shortly
    // after it.
    let mut saw_second_half_kickoff = false;
    for _ in 0..(sim_components::time::HALFTIME_BREAK_TICKS + 4_000) {
        sim.tick();
        let ball = sim.world().resource::<Ball>();
        let state = sim.world().resource::<sim_components::Match>().state;
        if state == MatchState::Kickoff || ball.last_touch.is_none() {
            continue;
        }
        let Some(touch) = ball.last_touch else {
            continue;
        };
        if touch.kind == TouchKind::Restart
            && sim
                .world()
                .get_resource::<MatchClock>()
                .is_some_and(|c| c.half >= 2)
        {
            assert_eq!(
                touch.team,
                sim_components::TeamId(1),
                "second-half kickoff is taken by the away team (Law 8 half swap)"
            );
            saw_second_half_kickoff = true;
            break;
        }
    }

    assert!(
        saw_second_half_kickoff,
        "the simulation must reach a second-half kickoff within the bounded loop"
    );
}
