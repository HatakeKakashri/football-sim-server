use crate::MatchClock;

/// Simulation ticks per second (fixed timestep: 60 Hz).
pub const TICKS_PER_SECOND: u64 = 60;

/// Seconds per minute.
pub const SECONDS_PER_MINUTE: u64 = 60;

/// Minutes per half.
pub const MINUTES_PER_HALF: u64 = 45;

/// Simulation ticks per half (45 minutes * 60 seconds * 60 ticks/sec = 162,000).
pub const HALF_LENGTH_TICKS: u64 = MINUTES_PER_HALF * SECONDS_PER_MINUTE * TICKS_PER_SECOND;

/// Halftime break duration in simulation ticks.
pub const HALFTIME_BREAK_TICKS: u64 = 18;

/// Total match length in ticks (2 halves + halftime).
pub const TOTAL_MATCH_TICKS: u64 = 2 * HALF_LENGTH_TICKS + HALFTIME_BREAK_TICKS;

/// Convert ticks to seconds (floor division).
#[must_use]
pub const fn ticks_to_secs(ticks: u64) -> u64 {
    ticks / TICKS_PER_SECOND
}

/// Convert ticks to seconds as f32.
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "f32 is exact up to 2^24 ticks (~77 h at 60 Hz); a match is ~324k ticks"
)]
pub fn ticks_to_secs_f32(ticks: u64) -> f32 {
    ticks as f32 / TICKS_PER_SECOND as f32
}

/// Convert ticks to minutes as f32.
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "SECONDS_PER_MINUTE is 60, exactly representable in f32"
)]
pub fn ticks_to_mins_f32(ticks: u64) -> f32 {
    ticks_to_secs_f32(ticks) / SECONDS_PER_MINUTE as f32
}

/// Convert seconds to ticks.
#[must_use]
pub const fn secs_to_ticks(secs: u64) -> u64 {
    secs * TICKS_PER_SECOND
}

/// Compute total match time elapsed in ticks from the match clock.
///
/// The `elapsed_ticks` field resets to 0 at the start of each half.
/// This function returns the cumulative elapsed time across both halves.
#[must_use]
pub const fn total_match_elapsed_ticks(clock: &MatchClock) -> u64 {
    if clock.half == 1 {
        clock.elapsed_ticks
    } else {
        HALF_LENGTH_TICKS + clock.elapsed_ticks
    }
}

/// Compute match time remaining in ticks.
///
/// Returns the total match ticks remaining (including added time for current half).
#[must_use]
pub const fn match_time_remaining_ticks(clock: &MatchClock) -> u64 {
    let total_elapsed = total_match_elapsed_ticks(clock);
    let effective_match_ticks = 2 * HALF_LENGTH_TICKS + clock.added_time_ticks;
    effective_match_ticks.saturating_sub(total_elapsed)
}

/// Compute match time remaining in seconds.
#[must_use]
pub fn match_time_remaining_secs(clock: &MatchClock) -> f32 {
    ticks_to_secs_f32(match_time_remaining_ticks(clock))
}

/// Compute match time remaining in minutes as f32.
#[must_use]
pub fn match_time_remaining_mins(clock: &MatchClock) -> f32 {
    ticks_to_mins_f32(match_time_remaining_ticks(clock))
}

/// Compute half time remaining in ticks.
///
/// Returns the current half's remaining ticks (including added time).
#[must_use]
pub const fn half_time_remaining_ticks(clock: &MatchClock) -> u64 {
    let half_elapsed = clock.elapsed_ticks;
    let half_total_with_added = HALF_LENGTH_TICKS + clock.added_time_ticks;
    half_total_with_added.saturating_sub(half_elapsed)
}

/// Compute half time remaining in seconds.
#[must_use]
pub fn half_time_remaining_secs(clock: &MatchClock) -> f32 {
    ticks_to_secs_f32(half_time_remaining_ticks(clock))
}

/// Compute half time remaining in minutes as f32.
#[must_use]
pub fn half_time_remaining_mins(clock: &MatchClock) -> f32 {
    ticks_to_mins_f32(half_time_remaining_ticks(clock))
}

/// Check if the match clock is in the first half.
#[must_use]
pub const fn is_first_half(clock: &MatchClock) -> bool {
    clock.half == 1
}

/// Check if the match clock is in the second half.
#[must_use]
pub const fn is_second_half(clock: &MatchClock) -> bool {
    clock.half >= 2
}

/// Check if the match has reached half-time (first half ended, not yet second half kickoff).
#[must_use]
pub const fn is_half_time(clock: &MatchClock) -> bool {
    clock.half == 1 && clock.elapsed_ticks >= HALF_LENGTH_TICKS + clock.added_time_ticks
}

/// Check if the match has reached full-time.
#[must_use]
pub const fn is_full_time(clock: &MatchClock) -> bool {
    clock.half >= 2 && clock.elapsed_ticks >= HALF_LENGTH_TICKS + clock.added_time_ticks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(TICKS_PER_SECOND, 60);
        assert_eq!(HALF_LENGTH_TICKS, 45 * 60 * 60); // 162,000
        assert_eq!(
            TOTAL_MATCH_TICKS,
            2 * HALF_LENGTH_TICKS + HALFTIME_BREAK_TICKS
        );
    }

    #[test]
    fn test_ticks_to_secs() {
        assert_eq!(ticks_to_secs(60), 1);
        assert_eq!(ticks_to_secs(120), 2);
        assert_eq!(ticks_to_secs(162000), 2700); // 45 minutes
    }

    #[test]
    fn test_ticks_to_secs_f32() {
        assert!((ticks_to_secs_f32(60) - 1.0).abs() < 0.001);
        assert!((ticks_to_secs_f32(30) - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_total_match_elapsed_ticks() {
        // First half, 30 minutes elapsed
        let clock = MatchClock {
            elapsed_ticks: 30 * 60 * 60,
            half: 1,
            added_time_ticks: 0,
            is_running: true,
        };
        assert_eq!(total_match_elapsed_ticks(&clock), 30 * 60 * 60);

        // Second half, 20 minutes elapsed (total = 45 + 20 = 65 minutes)
        let clock = MatchClock {
            elapsed_ticks: 20 * 60 * 60,
            half: 2,
            added_time_ticks: 0,
            is_running: true,
        };
        assert_eq!(
            total_match_elapsed_ticks(&clock),
            HALF_LENGTH_TICKS + 20 * 60 * 60
        );
    }

    #[test]
    fn test_match_time_remaining_ticks() {
        // Start of match
        let clock = MatchClock {
            elapsed_ticks: 0,
            half: 1,
            added_time_ticks: 0,
            is_running: true,
        };
        assert_eq!(match_time_remaining_ticks(&clock), 2 * HALF_LENGTH_TICKS);

        // 30 minutes into first half
        let clock = MatchClock {
            elapsed_ticks: 30 * 60 * 60,
            half: 1,
            added_time_ticks: 0,
            is_running: true,
        };
        assert_eq!(
            match_time_remaining_ticks(&clock),
            2 * HALF_LENGTH_TICKS - 30 * 60 * 60
        );

        // 20 minutes into second half
        let clock = MatchClock {
            elapsed_ticks: 20 * 60 * 60,
            half: 2,
            added_time_ticks: 0,
            is_running: true,
        };
        assert_eq!(
            match_time_remaining_ticks(&clock),
            HALF_LENGTH_TICKS - 20 * 60 * 60
        );

        // With added time in first half
        let clock = MatchClock {
            elapsed_ticks: 45 * 60 * 60,
            half: 1,
            added_time_ticks: 3 * 60 * 60,
            is_running: true,
        };
        assert_eq!(
            match_time_remaining_ticks(&clock),
            HALF_LENGTH_TICKS + 3 * 60 * 60
        );
    }

    #[test]
    fn test_half_time_remaining_ticks() {
        // Start of first half
        let clock = MatchClock {
            elapsed_ticks: 0,
            half: 1,
            added_time_ticks: 0,
            is_running: true,
        };
        assert_eq!(half_time_remaining_ticks(&clock), HALF_LENGTH_TICKS);

        // 30 minutes into first half
        let clock = MatchClock {
            elapsed_ticks: 30 * 60 * 60,
            half: 1,
            added_time_ticks: 0,
            is_running: true,
        };
        assert_eq!(half_time_remaining_ticks(&clock), 15 * 60 * 60);

        // With 3 minutes added time, at 45 minutes
        let clock = MatchClock {
            elapsed_ticks: 45 * 60 * 60,
            half: 1,
            added_time_ticks: 3 * 60 * 60,
            is_running: true,
        };
        assert_eq!(half_time_remaining_ticks(&clock), 3 * 60 * 60);

        // Second half, 20 minutes elapsed
        let clock = MatchClock {
            elapsed_ticks: 20 * 60 * 60,
            half: 2,
            added_time_ticks: 0,
            is_running: true,
        };
        assert_eq!(half_time_remaining_ticks(&clock), 25 * 60 * 60);
    }

    #[test]
    fn test_half_time_flags() {
        // Normal first half
        let clock = MatchClock {
            elapsed_ticks: 30 * 60 * 60,
            half: 1,
            added_time_ticks: 0,
            is_running: true,
        };
        assert!(!is_half_time(&clock));
        assert!(!is_full_time(&clock));

        // At half-time threshold (first half ended)
        let clock = MatchClock {
            elapsed_ticks: HALF_LENGTH_TICKS,
            half: 1,
            added_time_ticks: 0,
            is_running: false,
        };
        assert!(is_half_time(&clock));
        assert!(!is_full_time(&clock));

        // Second half, not yet full time
        let clock = MatchClock {
            elapsed_ticks: 30 * 60 * 60,
            half: 2,
            added_time_ticks: 0,
            is_running: true,
        };
        assert!(!is_half_time(&clock));
        assert!(!is_full_time(&clock));

        // At full-time
        let clock = MatchClock {
            elapsed_ticks: HALF_LENGTH_TICKS,
            half: 2,
            added_time_ticks: 0,
            is_running: false,
        };
        assert!(!is_half_time(&clock));
        assert!(is_full_time(&clock));
    }

    #[test]
    fn test_half_time_transition_with_added_time() {
        // First half ends exactly at 45:00 + 3 min added time
        let clock = MatchClock {
            elapsed_ticks: HALF_LENGTH_TICKS + 3 * 60 * 60,
            half: 1,
            added_time_ticks: 3 * 60 * 60,
            is_running: false,
        };
        assert!(is_half_time(&clock));
        assert!(!is_full_time(&clock));
        assert_eq!(half_time_remaining_ticks(&clock), 0);

        // Match time remaining should be one half + added time of second half (none yet)
        assert_eq!(match_time_remaining_ticks(&clock), HALF_LENGTH_TICKS);
    }

    #[test]
    fn test_second_half_kickoff_clock_reset() {
        // At second half kickoff, elapsed_ticks should be 0
        let clock = MatchClock {
            elapsed_ticks: 0,
            half: 2,
            added_time_ticks: 0,
            is_running: true,
        };
        assert_eq!(total_match_elapsed_ticks(&clock), HALF_LENGTH_TICKS);
        assert_eq!(half_time_remaining_ticks(&clock), HALF_LENGTH_TICKS);
        assert_eq!(match_time_remaining_ticks(&clock), HALF_LENGTH_TICKS);
        assert!(is_second_half(&clock));
        assert!(!is_first_half(&clock));
    }

    #[test]
    fn test_ai_time_remaining_in_second_half() {
        // 15 minutes into second half (60 minutes total match time elapsed)
        let clock = MatchClock {
            elapsed_ticks: 15 * 60 * 60,
            half: 2,
            added_time_ticks: 0,
            is_running: true,
        };
        // Match time remaining = 90 min - 60 min = 30 min
        assert_eq!(match_time_remaining_secs(&clock), 30.0 * 60.0);
        // Half time remaining = 45 min - 15 min = 30 min
        assert_eq!(half_time_remaining_secs(&clock), 30.0 * 60.0);

        // 40 minutes into second half (85 minutes total match time elapsed)
        let clock = MatchClock {
            elapsed_ticks: 40 * 60 * 60,
            half: 2,
            added_time_ticks: 0,
            is_running: true,
        };
        // Match time remaining = 90 min - 85 min = 5 min
        assert_eq!(match_time_remaining_secs(&clock), 5.0 * 60.0);
        // Half time remaining = 45 min - 40 min = 5 min
        assert_eq!(half_time_remaining_secs(&clock), 5.0 * 60.0);
    }

    #[test]
    fn test_clock_pause_resume_stoppage() {
        // Clock running normally
        let mut clock = MatchClock {
            elapsed_ticks: 30 * 60 * 60,
            half: 1,
            added_time_ticks: 0,
            is_running: true,
        };
        assert!(clock.is_running);

        // Pause for stoppage (e.g., injury, VAR)
        clock.is_running = false;
        assert!(!clock.is_running);

        // Time should not advance while paused
        let elapsed_before = clock.elapsed_ticks;
        // Simulate tick_clock not advancing because is_running is false
        if clock.is_running {
            clock.elapsed_ticks += 1;
        }
        assert_eq!(clock.elapsed_ticks, elapsed_before);

        // Resume after stoppage
        clock.is_running = true;
        assert!(clock.is_running);

        // Now time advances
        if clock.is_running {
            clock.elapsed_ticks += 1;
        }
        assert_eq!(clock.elapsed_ticks, elapsed_before + 1);
    }

    #[test]
    fn test_match_time_remaining_with_added_time_both_halves() {
        // First half with 2 min added time, at 45 min
        let clock = MatchClock {
            elapsed_ticks: HALF_LENGTH_TICKS,
            half: 1,
            added_time_ticks: 2 * 60 * 60,
            is_running: true,
        };
        // Match time remaining = 90 min + 2 min = 92 min
        assert_eq!(
            match_time_remaining_ticks(&clock),
            HALF_LENGTH_TICKS + 2 * 60 * 60
        );

        // Second half with 4 min added time, at 45 min
        let clock = MatchClock {
            elapsed_ticks: HALF_LENGTH_TICKS,
            half: 2,
            added_time_ticks: 4 * 60 * 60,
            is_running: true,
        };
        // Match time remaining = 4 min
        assert_eq!(match_time_remaining_ticks(&clock), 4 * 60 * 60);
    }

    #[test]
    fn test_ticks_to_mins_f32() {
        assert!((ticks_to_mins_f32(60 * 60) - 1.0).abs() < 0.001);
        assert!((ticks_to_mins_f32(30 * 60) - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_match_time_remaining_mins() {
        let clock = MatchClock {
            elapsed_ticks: 0,
            half: 1,
            added_time_ticks: 0,
            is_running: true,
        };
        assert!((match_time_remaining_mins(&clock) - 90.0).abs() < 0.001);

        let clock = MatchClock {
            elapsed_ticks: 30 * 60 * 60,
            half: 1,
            added_time_ticks: 0,
            is_running: true,
        };
        assert!((match_time_remaining_mins(&clock) - 60.0).abs() < 0.001);

        let clock = MatchClock {
            elapsed_ticks: 45 * 60 * 60,
            half: 1,
            added_time_ticks: 3 * 60 * 60,
            is_running: true,
        };
        assert!((match_time_remaining_mins(&clock) - 48.0).abs() < 0.001);

        let clock = MatchClock {
            elapsed_ticks: 15 * 60 * 60,
            half: 2,
            added_time_ticks: 0,
            is_running: true,
        };
        assert!((match_time_remaining_mins(&clock) - 30.0).abs() < 0.001);
    }

    #[test]
    fn test_half_time_remaining_mins() {
        let clock = MatchClock {
            elapsed_ticks: 0,
            half: 1,
            added_time_ticks: 0,
            is_running: true,
        };
        assert!((half_time_remaining_mins(&clock) - 45.0).abs() < 0.001);

        let clock = MatchClock {
            elapsed_ticks: 15 * 60 * 60,
            half: 2,
            added_time_ticks: 0,
            is_running: true,
        };
        assert!((half_time_remaining_mins(&clock) - 30.0).abs() < 0.001);
    }

    #[test]
    fn test_stoppage_time_carryover_half1_to_half2() {
        // Scenario: 3 minutes added time in first half
        // At start of second half (elapsed_ticks = 0, half = 2)
        // Match time remaining should be 45 min (second half) + 0 min (half 2 added time)
        // = 45 min, NOT 48 min (which would include half 1's added time)
        // Per FIFA Law 7: added time is per-half, doesn't carry over
        let clock = MatchClock {
            elapsed_ticks: 0,
            half: 2,
            added_time_ticks: 0, // No added time in half 2 yet
            is_running: true,
        };
        // Total match time: 90 min + 3 min (half 1 added) = 93 min
        // Time elapsed: 45 min (half 1) + 3 min (half 1 added) = 48 min
        // Time remaining: 93 - 48 = 45 min = second half only
        assert_eq!(match_time_remaining_secs(&clock), 45.0 * 60.0);
        assert_eq!(match_time_remaining_mins(&clock), 45.0);

        // Now with 2 minutes added time in second half
        let clock = MatchClock {
            elapsed_ticks: 0,
            half: 2,
            added_time_ticks: 2 * 60 * 60,
            is_running: true,
        };
        // Total match time: 90 min + 3 min (half 1) + 2 min (half 2) = 95 min
        // Time elapsed: 48 min
        // Time remaining: 95 - 48 = 47 min
        assert_eq!(match_time_remaining_secs(&clock), 47.0 * 60.0);
        assert_eq!(match_time_remaining_mins(&clock), 47.0);

        // Half time remaining should only consider current half's added time
        assert_eq!(half_time_remaining_secs(&clock), 47.0 * 60.0);
        assert_eq!(half_time_remaining_mins(&clock), 47.0);
    }

    #[test]
    fn test_total_match_elapsed_with_half1_added_time() {
        // First half, 45 min elapsed (no added time counted in elapsed)
        let clock = MatchClock {
            elapsed_ticks: HALF_LENGTH_TICKS,
            half: 1,
            added_time_ticks: 3 * 60 * 60,
            is_running: false,
        };
        // total_match_elapsed_ticks only counts elapsed_ticks, not added_time_ticks
        // Added time is extra budget for the half, not elapsed time
        assert_eq!(total_match_elapsed_ticks(&clock), HALF_LENGTH_TICKS);

        // Second half starts, elapsed_ticks resets to 0
        let clock = MatchClock {
            elapsed_ticks: 0,
            half: 2,
            added_time_ticks: 0,
            is_running: true,
        };
        // Total elapsed = 45 min (half 1 elapsed, not including its added time)
        assert_eq!(total_match_elapsed_ticks(&clock), HALF_LENGTH_TICKS);

        // 15 min into second half
        let clock = MatchClock {
            elapsed_ticks: 15 * 60 * 60,
            half: 2,
            added_time_ticks: 0,
            is_running: true,
        };
        // Total elapsed = 45 min (half 1) + 15 min (half 2) = 60 min
        assert_eq!(
            total_match_elapsed_ticks(&clock),
            HALF_LENGTH_TICKS + 15 * 60 * 60
        );
    }
}
