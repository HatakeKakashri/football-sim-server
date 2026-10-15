//! Lifecycle tests: clock advancement, half-time transitions, kickoff
//! reset, pause/resume on stoppage, and the full 90-minute integration
//! (ignored by default; runs minutes-long).

#![allow(
    clippy::float_cmp,
    reason = "tests compare outputs of identical arithmetic against the inputs they were derived from"
)]

use crate::Simulation;
use crate::simulation::lifecycle::HalfTimeEntryTick;
use sim_components::time;
use sim_components::{Match, MatchClock, MatchState};

/// Phase 1 verification: after 60 ticks (≈ 1 s of sim time) the match
/// clock's `elapsed` should be ~1.0. Asserts the read/write-separation
/// pipeline carries the `MatchClock` component forward without losing
/// updates.
#[test]
fn test_clock_advances() {
    let mut sim = Simulation::new(42);
    let me = sim.match_entity();
    for _ in 0..60 {
        sim.tick();
    }
    let elapsed_ticks = sim
        .world()
        .entity(me)
        .get::<MatchClock>()
        .expect("match entity has MatchClock component")
        .elapsed_ticks;
    assert!(
        elapsed_ticks == 60,
        "clock should be 60 ticks, got {elapsed_ticks}"
    );
}

/// Verification: a full match (≤ 330 000 ticks — two 45-minute halves at
/// 162 000 ticks each, plus the 18-tick halftime break, matching the
/// `--full-match` CLI budget of 324 000 ticks) eventually drives `state`
/// to `FullTime`, transitions through `half == 2`, and the clock
/// pauses/resets at half-time.
/// Runs a full 90-minute match (up to 330 000 ticks at 60 Hz). This test
/// is 2-3 orders of magnitude slower than every other test in the
/// workspace — minutes vs. milliseconds — and is the dominant cost
/// in `cargo test`. It is `#[ignore]`d by default; run explicitly
/// when verifying end-to-end correctness:
///
/// ```text
/// cargo test -p sim-core --lib -- --ignored test_full_match_reaches_full_time
/// ```
#[test]
#[ignore = "full-match test is minutes-long; run only when verifying end-to-end behaviour"]
fn test_full_match_reaches_full_time() {
    let mut sim = Simulation::new(7);
    let me = sim.match_entity();
    let mut saw_half_2 = false;
    let mut clock_paused_after_half = false;

    let final_state = {
        let mut state = MatchState::PreMatch;
        for _ in 0..330_000u64 {
            sim.tick();
            let clock = sim
                .world()
                .entity(me)
                .get::<MatchClock>()
                .cloned()
                .expect("match clock");
            if clock.half == 2 && clock.elapsed_ticks > 0 && clock.elapsed_ticks < 162_100 {
                saw_half_2 = true;
            }
            // After we first observe half == 2 (i.e. 2nd half in progress),
            // check that the clock was reset toward 0 after half-time.
            if clock.half == 2 && !clock.is_running {
                clock_paused_after_half = true;
            }
            state = sim.world().resource::<Match>().state;
            if state == MatchState::FullTime {
                break;
            }
        }
        state
    };

    assert_eq!(
        final_state,
        MatchState::FullTime,
        "match never reached FullTime within 330_000 ticks"
    );
    assert!(
        saw_half_2,
        "clock.half never observed as 2 between tick 162_000 and 324_100"
    );
    assert!(
        clock_paused_after_half,
        "clock.is_running was never false while half==2 (expected pause at HalfTime)"
    );

    // The 2nd-half clock should have reset toward 0 at the HalfTime→Kickoff
    // boundary (and the HalfTime phase paused the clock for ~0.3 s). After
    // FullTime we expect elapsed to have moved past 90 (or whatever the
    // 2nd-half total was) — but importantly, after half-time was reset,
    // elapsed should be small again at some point. Verify we observed at
    // least one tick where half==2 and elapsed < 1.0 (i.e. clock was
    // restarted cleanly).
    let mut observed_low_elapsed_in_half_2 = false;
    let mut sim2 = Simulation::new(7);
    let me2 = sim2.match_entity();
    for _ in 0..170_000u64 {
        sim2.tick();
        let c = sim2
            .world()
            .entity(me2)
            .get::<MatchClock>()
            .cloned()
            .unwrap();
        if c.half == 2 && c.elapsed_ticks < 60 {
            observed_low_elapsed_in_half_2 = true;
            break;
        }
    }
    assert!(
        observed_low_elapsed_in_half_2,
        "clock did not reset toward 0 after half-time"
    );
}

/// Test: Half-time transition with added time.
/// Verifies that the match correctly transitions to `HalfTime` when
/// the first half elapsed time reaches 45 minutes + added time.
#[test]
fn test_half_time_transition_with_added_time() {
    let mut sim = Simulation::new(42);
    let me = sim.match_entity();

    // Fast-forward to just before half-time with 3 minutes added time
    // Set clock close to threshold so transition happens quickly
    let clock = MatchClock {
        elapsed_ticks: time::HALF_LENGTH_TICKS + 3 * 60 * 60 - 60, // 1 second before half-time threshold
        half: 1,
        added_time_ticks: 3 * 60 * 60, // 3 minutes added time
        is_running: true,
    };
    sim.world_mut().resource_mut::<Match>().state = MatchState::InPlay;
    // Phase F follow-up: MatchClock is a Resource (spec §3). Insert both
    // forms — the Resource is the source of truth for systems; the
    // Component on the entity is kept for backwards compat.
    sim.world_mut().insert_resource(clock.clone());
    sim.world_mut().entity_mut(me).insert(clock);

    // Run until half-time transition (needs ~60 ticks to reach threshold)
    for _ in 0..100 {
        sim.tick();
        let state = sim.world().resource::<Match>().state;
        if state == MatchState::HalfTime {
            break;
        }
    }

    let m = sim.world().resource::<Match>();
    let clock = sim.world().entity(me).get::<MatchClock>().unwrap();
    assert_eq!(m.state, MatchState::HalfTime);
    assert!(!clock.is_running);
    assert_eq!(clock.half, 1);
    assert_eq!(clock.added_time_ticks, 3 * 60 * 60);
}

/// Test: Second-half kickoff clock reset.
/// Verifies that at the start of the second half, `elapsed_ticks` resets to 0.
#[test]
fn test_second_half_kickoff_clock_reset() {
    let mut sim = Simulation::new(42);
    let me = sim.match_entity();

    // Set up match at HalfTime state
    let clock = MatchClock {
        elapsed_ticks: time::HALF_LENGTH_TICKS,
        half: 1,
        added_time_ticks: 0,
        is_running: false,
    };
    sim.world_mut().resource_mut::<Match>().state = MatchState::HalfTime;
    sim.world_mut().entity_mut(me).insert(clock);

    // Insert HalfTimeEntryTick resource to trigger transition
    let current_tick = sim.current_tick();
    sim.world_mut().insert_resource(HalfTimeEntryTick {
        tick: Some(current_tick),
    });

    // Run ticks to trigger HalfTime -> Kickoff -> InPlay (second half) transition
    // The lifecycle system transitions HalfTime -> Kickoff -> InPlay in one tick
    for _ in 0..30 {
        sim.tick();
        let state = sim.world().resource::<Match>().state;
        // After transition, state will be InPlay (second half)
        if state == MatchState::InPlay {
            break;
        }
    }

    // Verify clock was reset for second half and state is InPlay (half 2)
    // Note: tick_clock() runs after lifecycle_system(), so after one tick
    // the clock will be at 1 (it was reset to 0, then incremented).
    let m = sim.world().resource::<Match>();
    let clock = sim.world().entity(me).get::<MatchClock>().unwrap();
    assert_eq!(
        m.state,
        MatchState::InPlay,
        "should be InPlay in second half"
    );
    assert_eq!(clock.half, 2, "should be in second half");
    assert_eq!(
        clock.elapsed_ticks, 1,
        "clock should be at 1 after first tick of second half (reset to 0 then incremented)"
    );
    assert!(clock.is_running);
}

/// Test: Clock pause/resume during stoppage.
/// Verifies that the match clock pauses when `is_running` is false
/// and resumes when set back to true.
#[test]
fn test_clock_pause_resume_stoppage() {
    let mut sim = Simulation::new(42);
    let me = sim.match_entity();

    // Set to InPlay with clock running
    let clock = MatchClock {
        elapsed_ticks: 30 * 60 * 60, // 30 minutes
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    };
    sim.world_mut().resource_mut::<Match>().state = MatchState::InPlay;
    // Phase F follow-up: MatchClock is a Resource (spec §3). Insert both
    // forms — the Resource is the source of truth for systems; the
    // Component on the entity is kept for backwards compat.
    sim.world_mut().insert_resource(clock.clone());
    sim.world_mut().entity_mut(me).insert(clock);

    // Run 60 ticks (1 second) - clock should advance
    for _ in 0..60 {
        sim.tick();
    }
    let clock = sim.world().entity(me).get::<MatchClock>().unwrap();
    assert_eq!(clock.elapsed_ticks, 30 * 60 * 60 + 60);

    // Pause clock (simulate stoppage)
    if let Some(mut clock) = sim.world_mut().get_resource_mut::<MatchClock>() {
        clock.is_running = false;
    }
    if let Some(mut clock) = sim.world_mut().get_mut::<MatchClock>(me) {
        clock.is_running = false;
    }

    // Run 60 ticks - clock should NOT advance
    for _ in 0..60 {
        sim.tick();
    }
    let clock = sim.world().entity(me).get::<MatchClock>().unwrap();
    assert_eq!(
        clock.elapsed_ticks,
        30 * 60 * 60 + 60,
        "clock should not advance while paused"
    );

    // Resume clock
    if let Some(mut clock) = sim.world_mut().get_resource_mut::<MatchClock>() {
        clock.is_running = true;
    }
    if let Some(mut clock) = sim.world_mut().get_mut::<MatchClock>(me) {
        clock.is_running = true;
    }

    // Run 60 ticks - clock should advance again
    for _ in 0..60 {
        sim.tick();
    }
    let clock = sim.world().entity(me).get::<MatchClock>().unwrap();
    assert_eq!(
        clock.elapsed_ticks,
        30 * 60 * 60 + 60 + 60,
        "clock should advance after resume"
    );
}

/// Test: AI time-remaining calculation in second half.
/// Verifies that the time remaining calculation correctly handles
/// the second half (where `elapsed_ticks` resets but total elapsed continues).
#[test]
fn test_ai_time_remaining_in_second_half() {
    let mut sim = Simulation::new(42);
    let me = sim.match_entity();

    // Set up match in second half, 15 minutes elapsed (60 minutes total match time)
    let clock = MatchClock {
        elapsed_ticks: 15 * 60 * 60, // 15 minutes into second half
        half: 2,
        added_time_ticks: 0,
        is_running: true,
    };
    sim.world_mut().resource_mut::<Match>().state = MatchState::InPlay;
    sim.world_mut().entity_mut(me).insert(clock);

    // Get the clock and compute time remaining using shared utilities
    let clock = sim.world().entity(me).get::<MatchClock>().unwrap();
    let match_remaining_secs = time::match_time_remaining_secs(clock);
    let half_remaining_secs = time::half_time_remaining_secs(clock);

    // Match time remaining: 90 min - 60 min = 30 min = 1800 sec
    assert_eq!(match_remaining_secs, 30.0 * 60.0);
    // Half time remaining: 45 min - 15 min = 30 min = 1800 sec
    assert_eq!(half_remaining_secs, 30.0 * 60.0);

    // Now test at 40 minutes into second half (85 minutes total)
    let clock = MatchClock {
        elapsed_ticks: 40 * 60 * 60, // 40 minutes into second half
        half: 2,
        added_time_ticks: 0,
        is_running: true,
    };
    sim.world_mut().entity_mut(me).insert(clock);

    let clock = sim.world().entity(me).get::<MatchClock>().unwrap();
    let match_remaining_secs = time::match_time_remaining_secs(clock);
    let half_remaining_secs = time::half_time_remaining_secs(clock);

    // Match time remaining: 90 min - 85 min = 5 min = 300 sec
    assert_eq!(match_remaining_secs, 5.0 * 60.0);
    // Half time remaining: 45 min - 40 min = 5 min = 300 sec
    assert_eq!(half_remaining_secs, 5.0 * 60.0);
}
// =========================================================================
// Phase F follow-up: tests for the decomposed `decide_transition` /
// `apply_transition` pair. The pure decision function is now unit-testable
// without spinning up a full Simulation + Bevy schedule.
// =========================================================================

use crate::simulation::lifecycle::decide_transition;

/// `PreMatch` always advances to `Kickoff` with ball-center placement, no
/// kickoff impulse yet.
#[test]
fn test_decide_transition_prematch_to_kickoff() {
    let decision = decide_transition(MatchState::PreMatch, None, 0, None);
    assert_eq!(decision.next_state, MatchState::Kickoff);
    assert!(decision.place_ball_center);
    assert!(!decision.apply_kickoff_impulse);
    assert!(decision.new_clock.is_none());
}

/// `Kickoff` advances to `InPlay` with both ball placement AND kickoff
/// impulse.
#[test]
fn test_decide_transition_kickoff_to_inplay() {
    let decision = decide_transition(MatchState::Kickoff, None, 0, None);
    assert_eq!(decision.next_state, MatchState::InPlay);
    assert!(decision.place_ball_center);
    assert!(decision.apply_kickoff_impulse);
}

/// `InPlay` stays put while `elapsed_ticks` is below the half-length
/// threshold; clock unchanged, no ball ops.
#[test]
fn test_decide_transition_inplay_below_threshold() {
    let clock = MatchClock {
        elapsed_ticks: 100,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    };
    let decision = decide_transition(MatchState::InPlay, Some(clock), 0, None);
    assert_eq!(decision.next_state, MatchState::InPlay);
    assert!(decision.new_clock.is_none());
    assert!(!decision.place_ball_center);
}

/// `InPlay` half 1 transitions to `HalfTime` when elapsed crosses
/// `HALF_LENGTH_TICKS + added_time_ticks`; the new clock has `is_running`
/// set to `false`.
#[test]
fn test_decide_transition_inplay_to_halftime() {
    let clock = MatchClock {
        elapsed_ticks: time::HALF_LENGTH_TICKS,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    };
    let decision = decide_transition(MatchState::InPlay, Some(clock), 0, None);
    assert_eq!(decision.next_state, MatchState::HalfTime);
    let new_clock = decision.new_clock.expect("new_clock should be set");
    assert!(!new_clock.is_running);
    assert_eq!(new_clock.half, 1);
}

/// `InPlay` half 2 transitions to `FullTime` (not `HalfTime`) when elapsed
/// crosses threshold.
#[test]
fn test_decide_transition_inplay_to_fulltime() {
    let clock = MatchClock {
        elapsed_ticks: time::HALF_LENGTH_TICKS,
        half: 2,
        added_time_ticks: 0,
        is_running: true,
    };
    let decision = decide_transition(MatchState::InPlay, Some(clock), 0, None);
    assert_eq!(decision.next_state, MatchState::FullTime);
}

/// `HalfTime` advances to `Kickoff` only after `HALFTIME_BREAK_TICKS`
/// ticks have elapsed since the entry tick.
#[test]
fn test_decide_transition_halftime_before_break_end() {
    let clock = MatchClock {
        elapsed_ticks: 0,
        half: 1,
        added_time_ticks: 0,
        is_running: false,
    };
    let decision = decide_transition(MatchState::HalfTime, Some(clock), 5, Some(0));
    assert_eq!(decision.next_state, MatchState::HalfTime);
}

#[test]
fn test_decide_transition_halftime_to_kickoff() {
    let clock = MatchClock {
        elapsed_ticks: 0,
        half: 1,
        added_time_ticks: 0,
        is_running: false,
    };
    let decision = decide_transition(
        MatchState::HalfTime,
        Some(clock),
        time::HALFTIME_BREAK_TICKS,
        Some(0),
    );
    assert_eq!(decision.next_state, MatchState::Kickoff);
    assert!(decision.place_ball_center);
    let new_clock = decision.new_clock.expect("new_clock should be set");
    assert_eq!(new_clock.elapsed_ticks, 0);
    assert_eq!(new_clock.half, 2);
    assert_eq!(
        new_clock.added_time_ticks, 0,
        "added time is per-half (PRD §8.4): the second half starts from zero"
    );
    assert!(new_clock.is_running);
}

/// `FullTime` and `Stoppage` are terminal in the inner match-state machine;
/// `decide_transition` returns a stable decision (no transitions).
#[test]
fn test_decide_transition_terminal_states() {
    for state in [MatchState::FullTime, MatchState::Stoppage] {
        let decision = decide_transition(state, None, 0, None);
        assert_eq!(decision.next_state, state);
        assert!(!decision.place_ball_center);
        assert!(!decision.apply_kickoff_impulse);
        assert!(decision.new_clock.is_none());
    }
}

// =========================================================================
// Ticket 10: `apply_transition` emits the lifecycle events (`kickoff`
// on half starts, `half_time` at the whistle, `full_time` at the end)
// into the `EventLog`. These run `apply_transition` directly on a
// hand-built world so they stay fast; the ignored full-match test at
// the bottom covers the same events over a live match.
// =========================================================================

use crate::simulation::lifecycle::{TransitionDecision, apply_transition};
use crate::{EventLog, MatchEvent};

/// Minimal world for `apply_transition`: the transition commits match
/// state, places the ball, and appends the lifecycle event.
fn transition_test_world(
    score: (u8, u8),
    half: u8,
) -> (
    bevy_ecs::prelude::World,
    bevy_ecs::prelude::Entity,
    bevy_ecs::prelude::Entity,
) {
    use bevy_ecs::prelude::World;
    use sim_components::{
        Ball, BallMarker, BallState, MatchClock, PendingBallTouches, Position, Team, TeamId,
        Velocity,
    };

    let mut world = World::new();
    world.insert_resource(Ball {
        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touch: None,
        touch_history: [None, None],
        kick_velocity: None,
    });
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(EventLog::new());
    let clock = MatchClock {
        elapsed_ticks: 0,
        half,
        added_time_ticks: 0,
        is_running: true,
    };
    world.insert_resource(clock.clone());
    let match_entity = world.spawn(()).id();
    world.entity_mut(match_entity).insert(clock);
    world.insert_resource(Match {
        id: 1,
        home_team: match_entity,
        away_team: match_entity,
        score,
        state: MatchState::PreMatch,
        seed: 99,
    });
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world.entity_mut(ball_entity).insert(Position(sim_math::Vec2::zero()));
    world
        .entity_mut(ball_entity)
        .insert(Velocity(sim_math::Vec2::zero()));
    // Two teams with no players: the kickoff taker collapses to
    // `None`, and the attack directions fall back to the
    // construction-time convention (home `+1`, away `-1`).
    let home_players: Vec<bevy_ecs::prelude::Entity> = Vec::new();
    world.spawn(()).insert(Team::with_attack_direction(
        TeamId(0),
        "Home".to_string(),
        sim_components::Formation::FourFourTwo,
        sim_components::Mentality::Balance,
        sim_components::Tactic::Possession,
        home_players,
        Vec::new(),
        1,
    ));
    world.spawn(()).insert(Team::with_attack_direction(
        TeamId(1),
        "Away".to_string(),
        sim_components::Formation::FourFourTwo,
        sim_components::Mentality::Balance,
        sim_components::Tactic::Possession,
        Vec::new(),
        Vec::new(),
        -1,
    ));
    (world, match_entity, ball_entity)
}

/// Entering `Kickoff` from `PreMatch` emits `kickoff` with
/// `reason = match_start` and both attack directions.
#[test]
fn test_apply_transition_emits_match_start_kickoff() {
    let (mut world, match_entity, ball_entity) = transition_test_world((0, 0), 1);
    let decision = TransitionDecision {
        next_state: MatchState::Kickoff,
        new_clock: None,
        place_ball_center: true,
        apply_kickoff_impulse: false,
    };
    apply_transition(
        &decision,
        MatchState::PreMatch,
        match_entity,
        ball_entity,
        0,
        &mut world,
    );
    let log = world.resource::<EventLog>();
    assert_eq!(log.events.len(), 1);
    let MatchEvent::Kickoff { envelope, payload } = &log.events[0] else {
        panic!("expected kickoff event");
    };
    assert_eq!(envelope.team, Some(sim_components::TeamId(0)));
    assert_eq!(payload.reason, sim_components::KickoffReason::MatchStart);
    assert_eq!(payload.taker, None, "no players in this world");
    assert_eq!(
        (payload.attack_home, payload.attack_away),
        (Some(1), Some(-1))
    );
}

/// Entering `HalfTime` from `InPlay` emits `half_time` with the running
/// score.
#[test]
fn test_apply_transition_emits_half_time_with_score() {
    let (mut world, match_entity, ball_entity) = transition_test_world((2, 1), 1);
    let decision = TransitionDecision {
        next_state: MatchState::HalfTime,
        new_clock: Some(MatchClock {
            elapsed_ticks: time::HALF_LENGTH_TICKS,
            half: 1,
            added_time_ticks: 0,
            is_running: false,
        }),
        place_ball_center: false,
        apply_kickoff_impulse: false,
    };
    apply_transition(
        &decision,
        MatchState::InPlay,
        match_entity,
        ball_entity,
        time::HALF_LENGTH_TICKS,
        &mut world,
    );
    let log = world.resource::<EventLog>();
    assert_eq!(log.events.len(), 1);
    let MatchEvent::HalfTime { envelope, payload } = &log.events[0] else {
        panic!("expected half_time event");
    };
    assert_eq!(envelope.team, None, "no team is credited");
    assert_eq!(payload.score, (2, 1), "running score at the whistle");
}

/// Entering `FullTime` from `InPlay` emits `full_time` with the final
/// score, an event count including itself, and the state hash.
#[test]
fn test_apply_transition_emits_full_time_with_count_and_hash() {
    let (mut world, match_entity, ball_entity) = transition_test_world((3, 1), 2);
    let decision = TransitionDecision {
        next_state: MatchState::FullTime,
        new_clock: Some(MatchClock {
            elapsed_ticks: time::HALF_LENGTH_TICKS,
            half: 2,
            added_time_ticks: 0,
            is_running: false,
        }),
        place_ball_center: false,
        apply_kickoff_impulse: false,
    };
    apply_transition(
        &decision,
        MatchState::InPlay,
        match_entity,
        ball_entity,
        2 * time::HALF_LENGTH_TICKS,
        &mut world,
    );
    let log = world.resource::<EventLog>();
    assert_eq!(log.events.len(), 1);
    let MatchEvent::FullTime { envelope, payload } = &log.events[0] else {
        panic!("expected full_time event");
    };
    assert_eq!(envelope.team, None);
    assert_eq!(payload.score, (3, 1));
    assert_eq!(
        payload.event_count, 1,
        "event count includes the full_time event itself"
    );
    assert_eq!(
        payload.added_time_ticks,
        (0, 0),
        "no stoppages in this world, no added time"
    );
    assert_ne!(payload.state_hash, 0, "state hash is populated");
}

// =========================================================================
// Ticket 11: added time is per-half (PRD §8.4). The first half's added
// ticks are stashed at the whistle for the `full_time` report; the
// second half starts from zero with an empty stoppage queue.
// =========================================================================

/// Leaving `HalfTime` for the second-half `Kickoff` resets the clock's
/// added time to zero and clears the referee's stoppage queue, while the
/// first half's added time survives in `FirstHalfAddedTime` for the
/// `full_time` event (PRD §9.2 `added_time_ticks: [h1, h2]`).
#[test]
fn test_apply_transition_to_second_half_resets_added_time() {
    use crate::simulation::lifecycle::FirstHalfAddedTime;
    use sim_components::{Referee, StoppageEvent};

    let (mut world, match_entity, ball_entity) = transition_test_world((1, 0), 1);
    // First half accrued 1 minute of added time (1 stoppage tick).
    world.insert_resource(FirstHalfAddedTime(3_600));
    world.spawn(Referee {
        stoppage_events: vec![StoppageEvent {
            tick: 100,
            reason: "dead_ball".to_string(),
        }],
        cards: Vec::new(),
    });

    let decision = TransitionDecision {
        next_state: MatchState::Kickoff,
        new_clock: Some(MatchClock {
            elapsed_ticks: 0,
            half: 2,
            added_time_ticks: 0,
            is_running: true,
        }),
        place_ball_center: true,
        apply_kickoff_impulse: false,
    };
    apply_transition(
        &decision,
        MatchState::HalfTime,
        match_entity,
        ball_entity,
        time::HALF_LENGTH_TICKS,
        &mut world,
    );

    let clock = world.resource::<MatchClock>();
    assert_eq!(
        clock.added_time_ticks, 0,
        "second half starts with no added time"
    );
    assert_eq!(clock.half, 2);
    let mut referees = world.query::<&Referee>();
    for referee in referees.iter(&world) {
        assert!(
            referee.stoppage_events.is_empty(),
            "the stoppage queue is cleared for the second half's own accrual"
        );
    }
    assert_eq!(
        world.resource::<FirstHalfAddedTime>().0,
        3_600,
        "the first half's added time survives for the full_time report"
    );
}

/// The `full_time` event reports both halves' added time
/// (`added_time_ticks: [h1, h2]`, PRD §9.2).
#[test]
fn test_apply_transition_full_time_reports_both_halves_added_time() {
    use crate::simulation::lifecycle::FirstHalfAddedTime;

    let (mut world, match_entity, ball_entity) = transition_test_world((2, 2), 2);
    world.insert_resource(FirstHalfAddedTime(3_600));
    let decision = TransitionDecision {
        next_state: MatchState::FullTime,
        new_clock: Some(MatchClock {
            elapsed_ticks: time::HALF_LENGTH_TICKS + 7_200,
            half: 2,
            added_time_ticks: 7_200,
            is_running: false,
        }),
        place_ball_center: false,
        apply_kickoff_impulse: false,
    };
    apply_transition(
        &decision,
        MatchState::InPlay,
        match_entity,
        ball_entity,
        2 * time::HALF_LENGTH_TICKS + 7_200,
        &mut world,
    );
    let log = world.resource::<EventLog>();
    assert_eq!(log.events.len(), 1);
    let MatchEvent::FullTime { envelope, payload } = &log.events[0] else {
        panic!("expected full_time event");
    };
    assert_eq!(
        payload.added_time_ticks,
        (3_600, 7_200),
        "full_time reports per-half added time [h1, h2]"
    );
    assert_eq!(envelope.minute, 90);
    assert_eq!(
        envelope.added_minute,
        Some(2),
        "the whistle clock reads 90+2"
    );
}
