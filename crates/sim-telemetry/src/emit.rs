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

/// Match process id (dedicated pid so goal/kickoff/foul/etc. live on one
/// timeline separate from ball / referee / players).
pub const PID_MATCH: u64 = 4;
/// Single thread inside the Match process.
pub const TID_MATCH: u64 = 1;

/// `Track` for the Match process.
#[must_use]
pub const fn match_track() -> Track<'static> {
    Track {
        pid: PID_MATCH,
        tid: TID_MATCH,
        process: "Match",
        thread: "State",
    }
}

/// Goal scored instant.
pub fn goal(tick: u64, scoring_team: u8, score_home: u8, score_away: u8) {
    let track = match_track();
    tracing::event!(
        target: TRACE_TARGET,
        Level::INFO,
        trace.kind = "instant",
        trace.pid = track.pid,
        trace.tid = track.tid,
        trace.process = track.process,
        trace.thread = track.thread,
        trace.name = "goal",
        tick = tick,
        scoring_team = scoring_team,
        score_home = score_home,
        score_away = score_away,
    );
}

/// Kickoff restart instant (after a goal or at match start).
pub fn kickoff(tick: u64) {
    let track = match_track();
    tracing::event!(
        target: TRACE_TARGET,
        Level::INFO,
        trace.kind = "instant",
        trace.pid = track.pid,
        trace.tid = track.tid,
        trace.process = track.process,
        trace.thread = track.thread,
        trace.name = "kickoff",
        tick = tick,
    );
}

/// Half-time / full-time instant.
pub fn half_time(tick: u64, score_home: u8, score_away: u8, full_time: bool) {
    let track = match_track();
    tracing::event!(
        target: TRACE_TARGET,
        Level::INFO,
        trace.kind = "instant",
        trace.pid = track.pid,
        trace.tid = track.tid,
        trace.process = track.process,
        trace.thread = track.thread,
        trace.name = if full_time { "full_time" } else { "half_time" },
        tick = tick,
        score_home = score_home,
        score_away = score_away,
    );
}

/// Offside position detected instant (debug-tier event).
pub fn offside(tick: u64, team: u8, x: f32, y: f32) {
    let track = match_track();
    tracing::event!(
        target: TRACE_TARGET,
        Level::DEBUG,
        trace.kind = "instant",
        trace.pid = track.pid,
        trace.tid = track.tid,
        trace.process = track.process,
        trace.thread = track.thread,
        trace.name = "offside",
        tick = tick,
        team = team,
        x = x,
        y = y,
    );
}

/// Foul detected instant.
#[expect(
    clippy::similar_names,
    reason = "fouler/fouled mirror the two entity ids; renaming loses meaning"
)]
pub fn foul(tick: u64, fouler: u64, fouled: u64) {
    let track = match_track();
    tracing::event!(
        target: TRACE_TARGET,
        Level::INFO,
        trace.kind = "instant",
        trace.pid = track.pid,
        trace.tid = track.tid,
        trace.process = track.process,
        trace.thread = track.thread,
        trace.name = "foul",
        tick = tick,
        fouler = fouler,
        fouled = fouled,
    );
}

/// Player count below the 7-a-side minimum instant.
pub fn low_player_count(tick: u64, team: &str, player_count: usize) {
    let track = match_track();
    tracing::event!(
        target: TRACE_TARGET,
        Level::WARN,
        trace.kind = "instant",
        trace.pid = track.pid,
        trace.tid = track.tid,
        trace.process = track.process,
        trace.thread = track.thread,
        trace.name = "low_player_count",
        tick = tick,
        team = team,
        player_count = player_count,
    );
}

/// Out-of-bounds restart instant (`kind` is `"throw_in"`, `"goal_kick"`, or
/// `"corner"`; `x`, `y` are the placement coordinates on the pitch).
pub fn restart(tick: u64, kind: &str, x: f32, y: f32) {
    let track = match_track();
    tracing::event!(
        target: TRACE_TARGET,
        Level::INFO,
        trace.kind = "instant",
        trace.pid = track.pid,
        trace.tid = track.tid,
        trace.process = track.process,
        trace.thread = track.thread,
        trace.name = "restart",
        tick = tick,
        kind = kind,
        x = x,
        y = y,
    );
}

/// Pass flow event (start at the kicker, end at the receiver).
///
/// Emits two events that share a `flow_id`: Perfetto draws an arrow from
/// the start (kicker) to the end (receiver). `from_pid` / `from_tid` /
/// `to_pid` / `to_tid` are the Chrome `pid`/`tid` of each player's
/// Decision thread. The receiver thread will not exist yet on the trace
/// timeline at the moment of the pass; that's fine — Perfetto still draws
/// the arrow from the start point onwards.
#[expect(
    clippy::similar_names,
    reason = "from_pid/from_tid and to_pid/to_tid mirror symmetric kicker/receiver identity"
)]
pub fn pass(
    flow_id: u64,
    from_pid: u64,
    from_tid: u64,
    to_pid: u64,
    to_tid: u64,
    tick: u64,
    completed: bool,
) {
    tracing::event!(
        target: TRACE_TARGET,
        Level::INFO,
        trace.kind = "flow_start",
        trace.pid = from_pid,
        trace.tid = from_tid,
        trace.flow_id = flow_id,
        tick = tick,
        to_pid = to_pid,
        to_tid = to_tid,
        completed = completed,
    );
    tracing::event!(
        target: TRACE_TARGET,
        Level::INFO,
        trace.kind = "flow_end",
        trace.pid = to_pid,
        trace.tid = to_tid,
        trace.flow_id = flow_id,
        tick = tick,
        from_pid = from_pid,
        from_tid = from_tid,
        completed = completed,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_trace;

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

    fn match_event<'a>(events: &'a [serde_json::Value], name: &str) -> &'a serde_json::Value {
        events
            .iter()
            .find(|e| {
                e["ph"] == "i"
                    && e["name"] == name
                    && e["pid"] == PID_MATCH
                    && e["tid"] == TID_MATCH
            })
            .unwrap_or_else(|| panic!("expected match event {name} on the Match process"))
    }

    #[test]
    fn match_process_id_is_distinct_from_ball_and_referee() {
        assert_ne!(PID_MATCH, PID_BALL);
        assert_ne!(PID_MATCH, PID_REFEREE);
    }

    #[test]
    fn goal_event_carries_score_and_team() {
        let events = capture_trace(|| goal(123, 0, 2, 1));
        let e = match_event(&events, "goal");
        // `tick` is consumed by the layer for the `ts` calculation; verify it
        // lands on the timestamp, while the typed payload goes into `args`.
        assert_eq!(e["ts"], 123_u64 * 1_000_000 / 60);
        assert_eq!(e["args"]["scoring_team"], 0);
        assert_eq!(e["args"]["score_home"], 2);
        assert_eq!(e["args"]["score_away"], 1);
    }

    #[test]
    fn half_time_and_full_time_share_one_helper() {
        let events = capture_trace(|| {
            half_time(60, 0, 0, false);
            half_time(120, 1, 1, true);
        });
        let ht = match_event(&events, "half_time");
        let ft = match_event(&events, "full_time");
        assert_eq!(ht["args"]["score_home"], 0);
        assert_eq!(ft["args"]["score_home"], 1);
        assert_eq!(ft["args"]["score_away"], 1);
    }

    #[test]
    fn kickoff_event_has_no_extra_args() {
        let events = capture_trace(|| kickoff(60));
        let e = match_event(&events, "kickoff");
        assert_eq!(e["ts"], 60_u64 * 1_000_000 / 60);
    }

    #[test]
    fn foul_event_carries_entity_ids() {
        let events = capture_trace(|| foul(60, 1007, 1011));
        let e = match_event(&events, "foul");
        assert_eq!(e["args"]["fouler"], 1007_u64);
        assert_eq!(e["args"]["fouled"], 1011_u64);
    }

    #[test]
    fn low_player_count_event_carries_team_name() {
        let events = capture_trace(|| low_player_count(60, "Home", 6));
        let e = match_event(&events, "low_player_count");
        assert_eq!(e["args"]["team"], "Home");
        assert_eq!(e["args"]["player_count"], 6);
    }

    #[test]
    fn restart_event_carries_kind_and_position() {
        let events = capture_trace(|| restart(60, "throw_in", 42.5, 0.0));
        let e = match_event(&events, "restart");
        assert_eq!(e["args"]["kind"], "throw_in");
        assert_eq!(e["args"]["x"], 42.5_f64);
        assert_eq!(e["args"]["y"], 0.0_f64);
    }

    #[test]
    fn pass_emits_a_flow_start_and_flow_end_with_shared_id() {
        let events = capture_trace(|| pass(42, 1007, 2, 1009, 2, 60, true));
        let starts: Vec<_> = events
            .iter()
            .filter(|e| e["ph"] == "s" && e["name"] == "pass")
            .collect();
        let ends: Vec<_> = events
            .iter()
            .filter(|e| e["ph"] == "f" && e["name"] == "pass")
            .collect();
        assert_eq!(starts.len(), 1, "one flow_start");
        assert_eq!(ends.len(), 1, "one flow_end");
        assert_eq!(starts[0]["pid"], 1007);
        assert_eq!(starts[0]["tid"], 2);
        assert_eq!(ends[0]["pid"], 1009);
        assert_eq!(ends[0]["tid"], 2);
        // Both events share the flow `id` in args so Perfetto can match them.
        assert_eq!(
            starts[0]["args"]["id"],
            42_u64,
            "flow_start args: {}",
            starts[0]["args"]
        );
        assert_eq!(ends[0]["args"]["id"], 42_u64);
        assert_eq!(starts[0]["args"]["completed"], true);
    }
}
