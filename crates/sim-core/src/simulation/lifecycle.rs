//! `lifecycle_system`: the `MatchState` transition driver, ball placement
//! at restart, and `apply_player_slot` (formation reset helper).
//!
//! Decomposed (Phase F follow-up): the per-tick driver loops `decide_transition`
//! and `apply_transition` until the state stabilises. The split isolates the
//! pure decision from the side effects (commit the new state, place the
//! ball, update the clock, reset formation).

use bevy_ecs::prelude::*;
use sim_components::{Match, MatchClock, MatchState, Position, Velocity};
use sim_components::time::{HALF_LENGTH_TICKS, HALFTIME_BREAK_TICKS};
use sim_math::Vec2;

use super::formation::reset_formation_to_4_4_2;

/// Tracks the simulation tick at which the match entered the `HalfTime`
/// state. `None` means we are not currently in `HalfTime`. Used to time out
/// the half-time break using the sim-tick counter rather than the paused
/// match clock (which doesn't accumulate `elapsed` while frozen).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct HalfTimeEntryTick {
    pub tick: Option<u64>,
}

/// Place a single player at `target`, zero velocity, reset intent to
/// `HoldPosition`. Mirrors the Position into Player.position so any system
/// that reads the player component sees the same value.
///
/// Phase 2: leave intent as `None` so the player's first decision cycle
/// isn't dominated by a hysteresis bonus on `HoldPosition`. The
/// `player_decision_system` will set the first real intent on its cadence
/// tick.
pub fn apply_player_slot(world: &mut World, player_entity: Entity, target: Vec2) {
    if let Some(mut pos) = world.entity_mut(player_entity).get_mut::<Position>() {
        pos.0 = target;
    }
    if let Some(mut vel) = world.entity_mut(player_entity).get_mut::<Velocity>() {
        vel.0 = Vec2::zero();
    }
    if let Some(mut p) = world.entity_mut(player_entity).get_mut::<sim_components::Player>() {
        p.intent = None;
    }
}

/// Pure decision: given the current state + clock + sim-tick + half-time entry,
/// compute the next state and the side-effect descriptors (clock mutation,
/// ball placement, kickoff impulse). Does not mutate the world.
#[derive(Debug, Clone)]
pub struct TransitionDecision {
    pub next_state: MatchState,
    pub new_clock: Option<MatchClock>,
    pub place_ball_center: bool,
    pub apply_kickoff_impulse: bool,
}

impl TransitionDecision {
    const fn stable(next_state: MatchState) -> Self {
        Self {
            next_state,
            new_clock: None,
            place_ball_center: false,
            apply_kickoff_impulse: false,
        }
    }
}

/// Decide whether the match state machine should advance from `current_state`
/// given the current clock + sim-tick. Pure function — no world mutation.
#[allow(
    clippy::too_long_first_doc_paragraph,
    reason = "single doc paragraph documents the entire state-machine decision table"
)]
pub fn decide_transition(
    current_state: MatchState,
    clock: Option<MatchClock>,
    sim_tick: u64,
    half_time_entry_tick: Option<u64>,
) -> TransitionDecision {
    match current_state {
        MatchState::PreMatch => TransitionDecision {
            next_state: MatchState::Kickoff,
            new_clock: None,
            place_ball_center: true,
            apply_kickoff_impulse: false,
        },
        MatchState::Kickoff => TransitionDecision {
            next_state: MatchState::InPlay,
            new_clock: None,
            place_ball_center: true,
            apply_kickoff_impulse: true,
        },
        MatchState::InPlay => {
            let Some(c) = clock else {
                return TransitionDecision::stable(current_state);
            };
            // Both halves use the same per-half length: `elapsed_ticks` is
            // reset to 0 at the second-half kickoff, so there is no
            // "cumulative 90 minutes" quantity to compare against here.
            let in_play_threshold = HALF_LENGTH_TICKS + c.added_time_ticks;
            let is_final_half = c.half >= 2;
            if c.elapsed_ticks < in_play_threshold {
                return TransitionDecision::stable(current_state);
            }
            let next_state = if is_final_half {
                MatchState::FullTime
            } else {
                MatchState::HalfTime
            };
            let mut updated = c;
            updated.is_running = false;
            TransitionDecision {
                next_state,
                new_clock: Some(updated),
                place_ball_center: false,
                apply_kickoff_impulse: false,
            }
        }
        MatchState::HalfTime => {
            // If we just entered HalfTime, this is the first tick of the break.
            // Caller is responsible for persisting the entry tick on the resource
            // (see the driver); here we only read it.
            let entry_tick = half_time_entry_tick.unwrap_or(sim_tick);
            let in_halftime_for = sim_tick.saturating_sub(entry_tick);
            if in_halftime_for < HALFTIME_BREAK_TICKS {
                return TransitionDecision::stable(current_state);
            }
            let new_clock = clock.map(|c| MatchClock {
                elapsed_ticks: 0,
                half: 2,
                added_time_ticks: c.added_time_ticks,
                is_running: true,
            });
            TransitionDecision {
                next_state: MatchState::Kickoff,
                new_clock,
                place_ball_center: true,
                apply_kickoff_impulse: false,
            }
        }
        MatchState::FullTime | MatchState::Stoppage => TransitionDecision::stable(current_state),
    }
}

/// Apply a `TransitionDecision` to the world: write the new clock, place the
/// ball at the center spot if requested, apply the kickoff impulse if
/// requested, reset the formation when entering `Kickoff`, and commit the
/// new match state.
pub fn apply_transition(
    decision: &TransitionDecision,
    current_state: MatchState,
    match_entity: Entity,
    ball_entity: Entity,
    world: &mut World,
) {
    // Apply clock mutation. Phase F follow-up: MatchClock is a Resource
    // (spec §3). Write to the Resource form (source of truth); the
    // Component form on the match entity is kept in sync for backwards
    // compat with tests + external consumers.
    if let Some(clock) = decision.new_clock.clone() {
        if let Some(mut existing) = world.get_resource_mut::<MatchClock>() {
            *existing = clock.clone();
        } else {
            world.insert_resource(clock.clone());
        }
        world.entity_mut(match_entity).insert(clock);
    }

    // Apply ball placement / kickoff impulse.
    if decision.place_ball_center {
        if let Some(mut pos) = world.entity_mut(ball_entity).get_mut::<Position>() {
            pos.0 = Vec2::new(52.5, 34.0);
        }
        if let Some(mut vel) = world.entity_mut(ball_entity).get_mut::<Velocity>() {
            vel.0 = Vec2::zero();
        }
        // Phase C §4.3: Ball state lives on the Resource, not on the entity.
        world.resource_mut::<sim_components::Ball>().state = sim_components::BallState::Free;
        // Drop any pending kick velocity — placement at the center
        // spot must not be combined with an inbound kick impulse.
        world.resource_mut::<sim_components::Ball>().kick_velocity = None;
    }
    if decision.apply_kickoff_impulse
        && let Some(mut vel) = world.entity_mut(ball_entity).get_mut::<Velocity>()
    {
        vel.0 = Vec2::new(2.0, 0.0);
    }

    // Reset 4-4-2 formation whenever we enter Kickoff.
    if decision.next_state == MatchState::Kickoff && current_state != MatchState::Kickoff {
        reset_formation_to_4_4_2(world);
    }

    // Clear the HalfTime entry-tick resource once we've left HalfTime.
    if current_state == MatchState::HalfTime && decision.next_state != MatchState::HalfTime {
        world.insert_resource(HalfTimeEntryTick { tick: None });
    }

    // Commit state mutation.
    world.resource_mut::<Match>().state = decision.next_state;
}

/// Match state machine driver. Runs once per tick after `schedule.run()` and
/// before `tick_clock`. Loops `decide_transition` + `apply_transition` until
/// the state stabilises.
///
/// Note: the `PreMatch` -> `Kickoff` -> `InPlay` chain settles on tick 1,
/// so the loop is bounded.
pub fn lifecycle_system(
    match_entity: Entity,
    ball_entity: Entity,
    sim_tick: u64,
    world: &mut World,
) {
    const MAX_TRANSITIONS_PER_TICK: usize = 8;

    for _iteration in 0..MAX_TRANSITIONS_PER_TICK {
        let current_state = world.resource::<Match>().state;
        // Phase F follow-up: MatchClock is a Resource. Read from the
        // Resource form (source of truth).
        let clock = world.get_resource::<MatchClock>().cloned();

        // HalfTime entry-tick persistence: on the first HalfTime tick the
        // entry resource is None; the decision reads it to compute
        // `in_halftime_for`. We persist here in the driver (not in the pure
        // decision) so `decide_transition` stays a pure function.
        if current_state == MatchState::HalfTime {
            let mut entry = world
                .get_resource::<HalfTimeEntryTick>()
                .copied()
                .unwrap_or_default();
            if entry.tick.is_none() {
                entry.tick = Some(sim_tick);
                world.insert_resource(entry);
                continue;
            }
        }

        let entry_tick = world
            .get_resource::<HalfTimeEntryTick>()
            .and_then(|e| e.tick);

        let decision = decide_transition(current_state, clock, sim_tick, entry_tick);

        if decision.next_state == current_state {
            return;
        }

        apply_transition(&decision, current_state, match_entity, ball_entity, world);
    }
}