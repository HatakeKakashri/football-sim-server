//! `tracing` call-site helpers and the field-name contract shared with the
//! Chrome layer. Call sites never spell field names by hand.

use std::fmt::Debug;

use tracing::{Level, Span};

/// Only events/spans with this `target` reach the Chrome layer.
pub const TRACE_TARGET: &str = "sim_trace";

/// Thread ids inside a player process.
pub const TID_POSITION: u64 = 1;
/// Decision thread id inside a player process.
pub const TID_DECISION: u64 = 2;
/// Ball process id.
pub const PID_BALL: u64 = 1;
/// Referee process id.
pub const PID_REFEREE: u64 = 2;
/// First player process id; a player's pid is `PID_PLAYER_BASE + entity index`.
pub const PID_PLAYER_BASE: u64 = 1000;

/// Chrome `pid` of the player process for a given entity index.
#[must_use]
pub fn player_pid(entity_index: u32) -> u64 {
    PID_PLAYER_BASE + u64::from(entity_index)
}

/// Process display name: `"<Team> <entity index> — <Role>"`.
#[must_use]
pub fn player_process_name(team: u8, entity_index: u32, role: &dyn Debug) -> String {
    let team_name = if team == 0 { "Home" } else { "Away" };
    format!("{team_name} {entity_index} — {role:?}")
}

/// Where an event lives in the Perfetto process/thread hierarchy.
#[derive(Debug, Clone, Copy)]
pub struct Track<'a> {
    /// Chrome `pid`.
    pub pid: u64,
    /// Chrome `tid`.
    pub tid: u64,
    /// Process display name (emitted once as `process_name` metadata).
    pub process: &'a str,
    /// Thread display name (emitted once as `thread_name` metadata).
    pub thread: &'static str,
}

/// Counter sample (`ph: "C"`), e.g. a player's `x`.
pub fn counter(track: Track<'_>, tick: u64, name: &'static str, value: f32) {
    tracing::event!(
        target: TRACE_TARGET,
        Level::INFO,
        trace.kind = "counter",
        trace.pid = track.pid,
        trace.tid = track.tid,
        trace.process = track.process,
        trace.thread = track.thread,
        trace.name = name,
        tick = tick,
        value = value,
    );
}

/// Root span for one player's decision at `tick`: `decision @ tick <n>`.
#[must_use]
pub fn decision_span(track: Track<'_>, tick: u64) -> Span {
    tracing::span!(
        target: TRACE_TARGET,
        Level::INFO,
        "decision",
        trace.pid = track.pid,
        trace.tid = track.tid,
        trace.process = track.process,
        trace.thread = track.thread,
        trace.name = display(format_args!("decision @ tick {tick}")),
        tick = tick,
    )
}

/// Child span for one evaluated action, carrying its final result.
#[must_use]
pub fn action_span(parent: &Span, name: &'static str, chosen: bool, aggregate_score: f32) -> Span {
    tracing::span!(
        target: TRACE_TARGET,
        parent: parent,
        Level::INFO,
        "action",
        trace.name = name,
        chosen = chosen,
        aggregate_score = aggregate_score,
    )
}

/// Instant event for one consideration, emitted inside its action span.
pub fn consideration(
    action: &Span,
    name: &'static str,
    raw: f32,
    curve: &'static str,
    weight: f32,
    score: f32,
) {
    tracing::event!(
        target: TRACE_TARGET,
        parent: action,
        Level::INFO,
        trace.kind = "instant",
        trace.name = name,
        raw = raw,
        curve = curve,
        weight = weight,
        score = score,
    );
}

/// Ball `state_change` instant event.
pub fn state_change(track: Track<'_>, tick: u64, from: &str, to: &str) {
    tracing::event!(
        target: TRACE_TARGET,
        Level::INFO,
        trace.kind = "instant",
        trace.pid = track.pid,
        trace.tid = track.tid,
        trace.process = track.process,
        trace.thread = track.thread,
        trace.name = "state_change",
        tick = tick,
        from = from,
        to = to,
    );
}

/// Ball `possession_change` instant event (`"none"` when unowned).
pub fn possession_change(track: Track<'_>, tick: u64, from: &str, to: &str) {
    tracing::event!(
        target: TRACE_TARGET,
        Level::INFO,
        trace.kind = "instant",
        trace.pid = track.pid,
        trace.tid = track.tid,
        trace.process = track.process,
        trace.thread = track.thread,
        trace.name = "possession_change",
        tick = tick,
        from = from,
        to = to,
    );
}

/// Referee `card` instant event.
pub fn card(track: Track<'_>, tick: u64, player: u64, color: &str, issued_tick: u64) {
    tracing::event!(
        target: TRACE_TARGET,
        Level::INFO,
        trace.kind = "instant",
        trace.pid = track.pid,
        trace.tid = track.tid,
        trace.process = track.process,
        trace.thread = track.thread,
        trace.name = "card",
        tick = tick,
        player = player,
        color = color,
        issued_tick = issued_tick,
    );
}

/// Referee `stoppage` instant event.
pub fn stoppage(track: Track<'_>, tick: u64, reason: &str, stoppage_tick: u64) {
    tracing::event!(
        target: TRACE_TARGET,
        Level::INFO,
        trace.kind = "instant",
        trace.pid = track.pid,
        trace.tid = track.tid,
        trace.process = track.process,
        trace.thread = track.thread,
        trace.name = "stoppage",
        tick = tick,
        reason = reason,
        stoppage_tick = stoppage_tick,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    enum TestRole {
        Striker,
        Winger,
    }

    #[test]
    fn player_identity_helpers() {
        assert_eq!(player_pid(0), 1000);
        assert_eq!(player_pid(23), 1023);
        assert_eq!(
            player_process_name(0, 7, &TestRole::Striker),
            "Home 7 — Striker"
        );
        assert_eq!(
            player_process_name(1, 12, &TestRole::Winger),
            "Away 12 — Winger"
        );
    }
}
