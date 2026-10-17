//! Match clock: added-time calculation and duration enforcement (Law 7).
//!
//! `added_time_calculation_system` converts the referee's accumulated
//! stoppage ticks (one [`Referee::stoppage_events`] entry per dead-ball
//! tick, plus one per OOB restart) into whole added minutes, rounded up
//! and capped at 10 min per half (PRD §8.4). The queue accumulates for
//! the whole half and is cleared at the half-time transition.
//! `match_duration_enforcement_system` transitions the match into
//! `HalfTime` / `FullTime` when a half's ticks (regular + added) are
//! exhausted.

use bevy_ecs::prelude::*;
use sim_components::{Match, MatchClock, Referee, time};
use sim_telemetry::TelemetryConfig;

use crate::CurrentTick;

/// Ticks in one whole minute at the fixed 60 Hz timestep.
const TICKS_PER_MINUTE: u64 = time::TICKS_PER_SECOND * time::SECONDS_PER_MINUTE;
/// Cap on added time for one half, in whole minutes (PRD §8.4).
const MAX_ADDED_MINUTES_PER_HALF: u64 = 10;

pub fn added_time_calculation_system(
    referee_query: Query<&Referee>,
    mut clock: ResMut<MatchClock>,
) {
    let Some(referee) = referee_query.iter().next() else {
        return;
    };
    // One queue entry equals one stoppage tick (PRD §8.4): dead-ball
    // ticks accrue one-by-one via `stoppage_tracking_system`, OOB
    // restarts credit a single tick each via `restart_system`.
    let stop_ticks = u64::try_from(referee.stoppage_events.len()).unwrap_or(u64::MAX);
    if stop_ticks == 0 {
        return;
    }
    // Round up to whole minutes, capped at the 10-minute cap.
    let added_minutes = stop_ticks
        .div_ceil(TICKS_PER_MINUTE)
        .min(MAX_ADDED_MINUTES_PER_HALF);
    let added_time_ticks = added_minutes.saturating_mul(TICKS_PER_MINUTE);
    // Monotonic within the half: never shrink what an earlier tick
    // granted (and hand-seeded test clocks keep their value).
    if added_time_ticks > clock.added_time_ticks {
        clock.added_time_ticks = added_time_ticks;
        tracing::info!(
            "Added time calculated: {added_minutes} minute(s) ({added_time_ticks} ticks)"
        );
    }
}

pub fn match_duration_enforcement_system(
    mut match_res: ResMut<Match>,
    mut clock_query: Query<&mut MatchClock>,
    current_tick: Res<CurrentTick>,
    telemetry: Option<Res<TelemetryConfig>>,
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
        let (score_home, score_away) = match_res.score;
        let is_full_time = half != 1;
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
        if telemetry.is_some() {
            sim_telemetry::emit::half_time(
                current_tick.0,
                score_home,
                score_away,
                is_full_time,
            );
        }
    }
}
