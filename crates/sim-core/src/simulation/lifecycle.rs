//! `lifecycle_system`: the `MatchState` transition driver, ball placement
//! at restart, and `apply_player_slot` (formation reset helper).

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

/// Match state machine driver. Runs once per tick after `schedule.run()` and
/// before `tick_clock`.
/// Owns `PreMatch` → Kickoff → `InPlay` → `HalfTime` → Kickoff(2nd half) →
/// `InPlay` → `FullTime`. Also responsible for placing the ball at the center
/// spot, zeroing ball velocity, applying the kickoff impulse, and resetting
/// the 4-4-2 formation at every kickoff transition.
///
/// Phase 1 deliberately hardcodes formation slots and intent — proving the
/// read/write-separation pipeline and perception-snapshot shape before any
/// utility-AI variable comes online.
///
/// Phase 1 implementation note: the `PreMatch` → Kickoff → `InPlay` chain is
/// instantaneous (all three states settle on tick 1), so the driver loops
/// through the state machine until the state stabilises. Without this,
/// `PreMatch` would only advance one step per tick and the kickoff impulse
/// wouldn't apply until tick 2 (clobbering anything else that ran in
/// between).
// `MatchClock.elapsed_ticks` accumulates in simulation ticks (60 ticks = 1 second).
// A half is 45 match-minutes = 45 * 60 * 60 = 162,000 ticks (HALF_LENGTH_TICKS).
// This is the SAME threshold for both halves: `elapsed_ticks` resets to 0 at the
// second-half kickoff (see `lifecycle_system` below), so half 2 needs its own
// 162,000-tick budget, not the cumulative 90-minute match length.
#[allow(
    clippy::too_long_first_doc_paragraph,
    reason = "single doc paragraph documents the entire state-machine lifecycle; \
              splitting it would scatter a tightly-coupled description"
)]
pub fn lifecycle_system(
    match_entity: Entity,
    ball_entity: Entity,
    sim_tick: u64,
    world: &mut World,
) {
    const MAX_TRANSITIONS_PER_TICK: usize = 8;

    for _iteration in 0..MAX_TRANSITIONS_PER_TICK {
        // Phase C §4.3: Match is now a Resource, read it from `world.resource`.
        let current_state = world.resource::<Match>().state;

        let mut next_state = current_state;
        let mut new_clock: Option<MatchClock> = None;
        let mut place_ball_center = false;
        let mut apply_kickoff_impulse = false;

        match current_state {
            MatchState::PreMatch => {
                next_state = MatchState::Kickoff;
                place_ball_center = true;
            }
            MatchState::Kickoff => {
                next_state = MatchState::InPlay;
                apply_kickoff_impulse = true;
                place_ball_center = true;
            }
            MatchState::InPlay => {
                let clock = world.entity(match_entity).get::<MatchClock>().cloned();
                if let Some(c) = clock {
                    // Both halves use the same per-half length: `elapsed_ticks` is
                    // reset to 0 at the second-half kickoff, so there is no
                    // "cumulative 90 minutes" quantity to compare against here.
                    let in_play_threshold = HALF_LENGTH_TICKS + c.added_time_ticks;
                    let is_final_half = c.half >= 2;
                    if c.elapsed_ticks >= in_play_threshold {
                        if is_final_half {
                            next_state = MatchState::FullTime;
                        } else {
                            next_state = MatchState::HalfTime;
                        }
                        let mut updated = c;
                        updated.is_running = false;
                        new_clock = Some(updated);
                    }
                }
            }
            MatchState::HalfTime => {
                let clock = world.entity(match_entity).get::<MatchClock>().cloned();
                let mut entry = world
                    .get_resource::<HalfTimeEntryTick>()
                    .copied()
                    .unwrap_or_default();
                if entry.tick.is_none() {
                    entry.tick = Some(sim_tick);
                    world.insert_resource(entry);
                }
                let entry_tick = entry.tick.unwrap_or(sim_tick);
                let in_halftime_for = sim_tick.saturating_sub(entry_tick);
                if in_halftime_for >= HALFTIME_BREAK_TICKS {
                    next_state = MatchState::Kickoff;
                    if let Some(c) = clock {
                        new_clock = Some(MatchClock {
                            elapsed_ticks: 0,
                            half: 2,
                            added_time_ticks: c.added_time_ticks,
                            is_running: true,
                        });
                    }
                    place_ball_center = true;
                    world.insert_resource(HalfTimeEntryTick { tick: None });
                }
            }
            MatchState::FullTime | MatchState::Stoppage => {}
        }

        // Apply clock mutation.
        if let Some(clock) = new_clock {
            world.entity_mut(match_entity).insert(clock);
        }

        // Apply ball placement / kickoff impulse.
        if place_ball_center {
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
        if apply_kickoff_impulse
            && let Some(mut vel) = world.entity_mut(ball_entity).get_mut::<Velocity>()
        {
            vel.0 = Vec2::new(2.0, 0.0);
        }

        // Reset 4-4-2 formation whenever we enter Kickoff.
        if next_state == MatchState::Kickoff && current_state != MatchState::Kickoff {
            reset_formation_to_4_4_2(world);
        }

        // Commit state mutation.
        if next_state == current_state {
            // No further transitions this tick; stop iterating.
            return;
        }
        world.resource_mut::<Match>().state = next_state;
    }
}