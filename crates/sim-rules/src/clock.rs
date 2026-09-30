//! Match clock: added-time calculation and duration enforcement (Law 7).
//!
//! `added_time_calculation_system` converts the referee's stoppage-events
//! queue into added-time minutes (capped at 10 min per half).
//! `match_duration_enforcement_system` transitions the match into
//! `HalfTime` / `FullTime` when a half's ticks (regular + added) are
//! exhausted.

use bevy_ecs::prelude::*;
use sim_components::{Match, MatchClock, Referee, time};

/// Added time granted per stoppage event, in seconds (0.5 min).
const ADDED_SECS_PER_STOPPAGE_EVENT: u64 = 30;
/// Cap on added time for one half, in seconds (10 min).
const MAX_ADDED_SECS_PER_HALF: u64 = 600;

pub fn added_time_calculation_system(
    mut referee_query: Query<&mut Referee>,
    mut clock_query: Query<&mut MatchClock>,
) {
    for mut referee in &mut referee_query {
        if referee.stoppage_events.is_empty() {
            continue;
        }
        // 0.5 minutes (30 s) per stoppage event, capped at 10 minutes
        let events = u64::try_from(referee.stoppage_events.len()).unwrap_or(u64::MAX);
        let added_secs = events
            .saturating_mul(ADDED_SECS_PER_STOPPAGE_EVENT)
            .min(MAX_ADDED_SECS_PER_HALF);
        let added_time_ticks = time::secs_to_ticks(added_secs);
        let added_time_min = time::ticks_to_mins_f32(added_time_ticks);

        // Apply to match clock
        if let Ok(mut clock) = clock_query.get_single_mut() {
            clock.added_time_ticks = added_time_ticks;
            tracing::info!(
                "Added time calculated: {added_time_min} minutes ({added_time_ticks} ticks)"
            );
        }

        referee.stoppage_events.clear();
    }
}

pub fn match_duration_enforcement_system(
    mut match_res: ResMut<Match>,
    mut clock_query: Query<&mut MatchClock>,
) {
    // First check if clock is running
    let is_running = clock_query.get_single().is_ok_and(|c| c.is_running);
    if !is_running {
        return;
    }

    // Check half time remaining
    let half_remaining_ticks = clock_query
        .get_single()
        .map_or(0, time::half_time_remaining_ticks);

    if half_remaining_ticks == 0 {
        if let Ok(mut clock) = clock_query.get_single_mut() {
            clock.is_running = false;
        }
        // Need to re-query to get half
        let half = clock_query.get_single().map_or(1, |c| c.half);
        if half == 1 {
            match_res.state = sim_components::MatchState::HalfTime;
            tracing::info!(
                "Half time! Score: {}-{}",
                match_res.score.0,
                match_res.score.1
            );
        } else {
            match_res.state = sim_components::MatchState::FullTime;
            tracing::info!(
                "Full time! Score: {}-{}",
                match_res.score.0,
                match_res.score.1
            );
        }
    }
}