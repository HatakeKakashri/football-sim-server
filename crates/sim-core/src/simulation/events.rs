//! Structured match event log (PRD §9).
//!
//! Every event in PRD §9.1 carries the same envelope —
//! `seq`, `tick`, `half`, `minute`, `added_minute`, `type`, `team` —
//! plus a payload specific to its `type`. This module owns:
//!
//! 1. The envelope + payload types ([`EventEnvelope`], [`MatchEvent`]).
//! 2. The accumulating [`EventLog`] resource.
//! 3. The `minute` / `added_minute` derivation from `MatchClock`.
//!
//! ## Wire shape
//!
//! The wire shape is a flat object: envelope fields and payload
//! fields live at the same level (PRD §9.1 sample shows the
//! envelope fields; §9.2 lists the per-type payload fields). The
//! internal `MatchEvent` representation uses a tagged enum so
//! writers can construct one in a single call; the wire
//! serialisation flattens it.
//!
//! ## Client-side event `seq` protocol
//!
//! Clients drop any event with `seq` at or below the last one seen
//! (PRD §9.1). The same rule will live in `client-protocol.md`
//! (Phase 6); documenting it here too so the implementation
//! site is unambiguous.
//!
//! ## Writer sites
//!
//! Events are appended by rule systems at the points PRD §9.3
//! describes:
//!
//! - `pass` — at every pass resolution (acquisition, OOB, goal).
//!   `kick_tick` is the tick the kick happened; `tick` is the
//!   resolution tick.
//! - `possession_change` — whenever the possessing team changes.
//!
//! The seq counter is owned by [`EventLog`] so the writers only
//! ever read `EventLog::next_seq()` and the gapless invariant
//! is a single property of the resource.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use sim_components::time::{ASSIST_WINDOW_TICKS, HALF_LENGTH_TICKS};
use sim_components::{
    KickoffReason, LastTouch, MatchClock, PlayerId, SaveKind, ShotOutcome, TeamId, TouchKind,
};
use sim_math::Vec2;

/// Discriminator for the small set of event types the MVP wires up.
///
/// Variants are added per Phase 1 ticket (08, 09, 10, 11). Each
/// variant carries its payload in `MatchEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    Pass,
    PossessionChange,
    OutOfBounds,
    /// Throw-in / goal-kick / corner restart. The concrete kind lives
    /// on the payload as [`RestartKind`]; the envelope's `type`
    /// discriminator is the single `restart` field per PRD §9.1.
    Restart,
    Goal,
    Shot,
    Save,
    Kickoff,
    HalfTime,
    FullTime,
}

impl EventType {
    /// Stable string form for the wire shape (`type` field).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::PossessionChange => "possession_change",
            Self::OutOfBounds => "out_of_bounds",
            Self::Restart => "restart",
            Self::Goal => "goal",
            Self::Shot => "shot",
            Self::Save => "save",
            Self::Kickoff => "kickoff",
            Self::HalfTime => "half_time",
            Self::FullTime => "full_time",
        }
    }
}

/// Cause of a possession change (PRD §9.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PossessionChangeCause {
    Tackle,
    Interception,
    OutOfBounds,
    Save,
    LooseBall,
    Goal,
}

impl PossessionChangeCause {
    /// Stable string form for the wire shape (`cause` field).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tackle => "tackle",
            Self::Interception => "interception",
            Self::OutOfBounds => "out_of_bounds",
            Self::Save => "save",
            Self::LooseBall => "loose_ball",
            Self::Goal => "goal",
        }
    }
}

/// Kind of restart that follows an out-of-bounds event (PRD §9.2).
///
/// The wire shape collapses `throw_in` / `goal_kick` / `corner` into
/// one envelope type (`restart`) with a per-payload `kind` field; the
/// `out_of_bounds` event's `restart` field carries the same kind so a
/// consumer can pair them up without consulting the following event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartKind {
    ThrowIn,
    GoalKick,
    Corner,
}

impl RestartKind {
    /// Stable string form for the wire shape (`kind` / `restart` fields).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ThrowIn => "throw_in",
            Self::GoalKick => "goal_kick",
            Self::Corner => "corner",
        }
    }

    /// Convert from the rule-side `BallOutOfBoundsType` discriminator
    /// the OOB detector already classifies. The names match
    /// (`ThrowIn` / `GoalKick` / `Corner`).
    #[must_use]
    pub const fn from_oob(oob: sim_components::BallOutOfBoundsType) -> Self {
        match oob {
            sim_components::BallOutOfBoundsType::ThrowIn => Self::ThrowIn,
            sim_components::BallOutOfBoundsType::GoalKick => Self::GoalKick,
            sim_components::BallOutOfBoundsType::Corner => Self::Corner,
        }
    }
}

/// Shared envelope carried by every event in PRD §9.1.
///
/// `seq` is the gapless monotonic counter from the [`EventLog`]
/// resource. `tick` is the global simulation tick (not
/// `elapsed_ticks`); events never contradict frames at the same
/// tick. `team` is `None` where the event type does not apply
/// (e.g. some `possession_change` causes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// Gapless monotonic counter (starts at 0).
    pub seq: u64,
    /// Global simulation tick at which the event was emitted.
    pub tick: u64,
    /// Current half (`1` or `2`).
    pub half: u8,
    /// Display minute (`45` for first half, `90` for second half).
    /// Always either 45 or 90 per PRD §8.3.
    pub minute: u8,
    /// Added minute in the current half (`1..=10`), or `None` while
    /// in regular time.
    pub added_minute: Option<u8>,
    /// Event type discriminator (also see `MatchEvent`).
    #[serde(rename = "type")]
    pub event_type: EventType,
    /// Owning team, or `None` when the event type does not credit
    /// a team (e.g. some `possession_change` causes).
    pub team: Option<TeamId>,
}

/// `pass` event payload (PRD §9.2).
///
/// Emitted at every resolution — the ball's next acquisition, OOB,
/// or goal. `kick_tick` is the tick the kick happened; `tick` on
/// the envelope is the resolution tick. `to` is `None` when the
/// kick was astray (no teammate received it). `success` means
/// the ball was received by a teammate of the kicker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PassEvent {
    /// Kicker (last `Play` touch).
    pub from: PlayerId,
    /// Intended receiver, or `None` if the kick was astray.
    pub to: Option<PlayerId>,
    /// `true` when the ball was received by a teammate of the kicker.
    pub success: bool,
    /// Tick at which the kick was executed.
    pub kick_tick: u64,
    /// Ball position at the kick.
    pub start: [f32; 2],
    /// Ball position at the resolution (acquisition / OOB / goal).
    pub end: [f32; 2],
}

/// `possession_change` event payload (PRD §9.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PossessionChangeEvent {
    /// Team that previously had possession.
    pub from_team: TeamId,
    /// Team that now has possession.
    pub to_team: TeamId,
    /// Why the change happened.
    pub cause: PossessionChangeCause,
    /// Last `Play` touch (kicker) on the ball, or `None` if there
    /// was no `Play` touch since the last `Restart` placement.
    pub last_touch: Option<LastTouch>,
}

/// `out_of_bounds` event payload (PRD §9.2, ticket 09).
///
/// Emitted on the tick the OOB detector fires, before the matching
/// restart event (PRD §9.3 / ticket 09 ordering invariant:
/// `out_of_bounds.seq + 1 == restart.seq`). The restart event shares
/// the same tick and carries the placement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutOfBoundsEventPayload {
    /// Last `Play` touch on the ball (the player whose touch sent it
    /// out). `None` only when the OOB detector fires without a touch
    /// on record — the rule engine debug-asserts against this case,
    /// so a real match always populates it.
    pub last_touch: Option<LastTouch>,
    /// Position the ball settled at when the boundary was crossed.
    /// Physics clamps the ball to the boundary line; this is the
    /// clamped position (i.e. the closest in-bounds point).
    pub exit_point: [f32; 2],
    /// Which restart the OOB earns (positional, per Law 9). The
    /// matching `restart` event follows with the same `kind`.
    pub restart: RestartKind,
}

/// `restart` event payload (PRD §9.2, ticket 09).
///
/// Covers `throw_in` / `goal_kick` / `corner`. The envelope type is
/// `restart`; the `kind` field on the payload distinguishes the three.
/// Emitted immediately after the matching `out_of_bounds` event
/// (seq N+1) on the same tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RestartEventPayload {
    /// Player who takes the restart (Law 9: opposite team of the last
    /// toucher). `None` in test worlds with no resolvable player on
    /// the taker team (placeholder collapse in `LastTouch::from_taker`).
    pub taker: Option<PlayerId>,
    /// Restart placement position, computed by
    /// `sim_rules::out_of_bounds::calculate_oob_restart_position`.
    pub at: [f32; 2],
    /// Concrete restart kind (`throw_in` / `goal_kick` / `corner`).
    pub kind: RestartKind,
}

/// `goal` event payload (PRD §9.2, ticket 10).
///
/// The scoring team lives on the envelope (`team`); the running score
/// after the goal lives here so a consumer can fold the table without
/// joining the two. `scorer` is `None` only when no `Play` touch is on
/// record at all (a wiring gap the rule engine debug-asserts against);
/// a live match always populates it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalEventPayload {
    /// Last `Play` toucher (PRD §8.5).
    pub scorer: Option<PlayerId>,
    /// Previous `Play` toucher on the scoring team within the last
    /// 10 s of game time, if any; always `None` for own goals.
    pub assister: Option<PlayerId>,
    /// `true` when the scorer plays for the conceding team.
    pub own_goal: bool,
    /// Running score after the goal, `(home, away)`.
    pub score: (u8, u8),
}

/// Attribution result for one goal (PRD §8.5).
///
/// Computed by [`attribute_goal`] from the scoring team and the
/// two-deep touch window the rule engine captured at the goal tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoalAttribution {
    /// Last `Play` toucher, if any.
    pub scorer: Option<PlayerId>,
    /// Previous `Play` toucher on the scoring team within
    /// [`ASSIST_WINDOW_TICKS`], if any; `None` for own goals.
    pub assister: Option<PlayerId>,
    /// `true` when the scorer plays for the conceding team.
    pub own_goal: bool,
}

/// Attribute scorer / assister / own-goal for one goal (PRD §8.5).
///
/// `touches` is the newest-first window `[last_touch, history[0],
/// history[1]]` captured at the goal tick. The scorer is the most
/// recent `Play` touch (restart placements don't shoot); when that
/// player is on the conceding team the goal is an own goal with no
/// assister. Otherwise the assister is the next most recent `Play`
/// touch on the scoring team within [`ASSIST_WINDOW_TICKS`] of
/// `goal_tick`. A touch with `tick == 0` ("unknown", from hand-built
/// records) counts as within the window rather than inventing a
/// cutoff.
#[must_use]
pub fn attribute_goal(
    scoring_team: TeamId,
    touches: [Option<LastTouch>; 3],
    goal_tick: u64,
) -> GoalAttribution {
    let mut plays = touches.into_iter().flatten().filter(|t| t.kind == TouchKind::Play);
    let Some(scorer_touch) = plays.next() else {
        return GoalAttribution {
            scorer: None,
            assister: None,
            own_goal: false,
        };
    };
    if scorer_touch.team != scoring_team {
        return GoalAttribution {
            scorer: scorer_touch.player_id,
            assister: None,
            own_goal: true,
        };
    }
    let assister = plays
        .filter(|t| t.team == scoring_team)
        .find(|t| t.tick == 0 || goal_tick.saturating_sub(t.tick) <= ASSIST_WINDOW_TICKS)
        .and_then(|t| t.player_id);
    GoalAttribution {
        scorer: scorer_touch.player_id,
        assister,
        own_goal: false,
    }
}

/// `shot` event payload (PRD §9.2, ticket 10).
///
/// Emitted at resolution (the ball's next acquisition, OOB, or goal),
/// like `pass`: the envelope `tick` is the resolution tick. The
/// envelope `team` is the shooter's team.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShotEventPayload {
    /// Player who shot.
    pub shooter: PlayerId,
    /// `true` when the shot was on target (would have scored without
    /// a stop: `goal` / `saved` / `blocked` outcomes).
    pub on_target: bool,
    /// How the shot resolved.
    pub outcome: ShotOutcome,
}

/// `save` event payload (PRD §9.2, ticket 10).
///
/// Emitted alongside the resolving `shot` event (same tick, next
/// `seq`) whenever a goalkeeper makes the stop. The envelope `team`
/// is the keeper's team.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveEventPayload {
    /// Goalkeeper who made the save.
    pub keeper: PlayerId,
    /// Player whose shot was saved.
    pub shooter: PlayerId,
    /// How the keeper dealt with the ball.
    pub kind: SaveKind,
}

/// `kickoff` event payload (PRD §9.2, ticket 10).
///
/// The envelope `team` is the taker team (Law 8: the side kicking
/// off). `attack_home` / `attack_away` carry the per-team attack
/// direction (`+1` = attacking `x = PITCH_LENGTH`, `-1` = attacking
/// `x = 0`) on half starts (`match_start`, `second_half`); they are
/// `None` for `after_goal` kickoffs, where ends don't change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KickoffEventPayload {
    /// Player taking the kickoff. `None` in test worlds with no
    /// resolvable player on the taker team.
    pub taker: Option<PlayerId>,
    /// Why the kickoff was taken.
    pub reason: KickoffReason,
    /// Home team's attack direction at the kickoff, if applicable.
    pub attack_home: Option<i8>,
    /// Away team's attack direction at the kickoff, if applicable.
    pub attack_away: Option<i8>,
}

/// `half_time` event payload (PRD §9.2, ticket 10).
///
/// The envelope `team` is `None` (no team is credited). Carries the
/// running score at the whistle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HalfTimePayload {
    /// Running score at half-time, `(home, away)`.
    pub score: (u8, u8),
}

/// `full_time` event payload (PRD §9.2, ticket 10).
///
/// The envelope `team` is `None`. Carries the final score (which the
/// A7 invariant checks against the count of `goal` events per team),
/// the total event count including this event, the deterministic
/// state hash, and per-half added time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FullTimePayload {
    /// Final score, `(home, away)`.
    pub score: (u8, u8),
    /// Total events in the log including this `full_time` event.
    pub event_count: u64,
    /// Deterministic state hash at full time (same value as
    /// `Simulation::get_state_hash`).
    pub state_hash: u64,
    /// Added time per half in ticks, `(first, second)`.
    pub added_time_ticks: (u64, u64),
}

/// Discriminated union of every event payload in the MVP log.
///
/// Internal representation: tagged enum so writers construct one
/// in a single call. Wire shape: the envelope and payload are
/// flattened into a single JSON object via a custom
/// `Serialize`/`Deserialize` impl (PRD §9.1 shows a flat envelope;
/// §9.2 lists payload fields at the same level).
#[derive(Debug, Clone, PartialEq)]
pub enum MatchEvent {
    Pass {
        envelope: EventEnvelope,
        payload: PassEvent,
    },
    PossessionChange {
        envelope: EventEnvelope,
        payload: PossessionChangeEvent,
    },
    OutOfBounds {
        envelope: EventEnvelope,
        payload: OutOfBoundsEventPayload,
    },
    Restart {
        envelope: EventEnvelope,
        payload: RestartEventPayload,
    },
    Goal {
        envelope: EventEnvelope,
        payload: GoalEventPayload,
    },
    Shot {
        envelope: EventEnvelope,
        payload: ShotEventPayload,
    },
    Save {
        envelope: EventEnvelope,
        payload: SaveEventPayload,
    },
    Kickoff {
        envelope: EventEnvelope,
        payload: KickoffEventPayload,
    },
    HalfTime {
        envelope: EventEnvelope,
        payload: HalfTimePayload,
    },
    FullTime {
        envelope: EventEnvelope,
        payload: FullTimePayload,
    },
}

impl MatchEvent {
    /// The envelope, regardless of variant.
    #[must_use]
    pub const fn envelope(&self) -> &EventEnvelope {
        match self {
            Self::Pass { envelope, .. }
            | Self::PossessionChange { envelope, .. }
            | Self::OutOfBounds { envelope, .. }
            | Self::Restart { envelope, .. }
            | Self::Goal { envelope, .. }
            | Self::Shot { envelope, .. }
            | Self::Save { envelope, .. }
            | Self::Kickoff { envelope, .. }
            | Self::HalfTime { envelope, .. }
            | Self::FullTime { envelope, .. } => envelope,
        }
    }

    /// Event type discriminator, also accessible via the envelope.
    #[must_use]
    pub const fn event_type(&self) -> EventType {
        match self {
            Self::Pass { .. } => EventType::Pass,
            Self::PossessionChange { .. } => EventType::PossessionChange,
            Self::OutOfBounds { .. } => EventType::OutOfBounds,
            Self::Restart { .. } => EventType::Restart,
            Self::Goal { .. } => EventType::Goal,
            Self::Shot { .. } => EventType::Shot,
            Self::Save { .. } => EventType::Save,
            Self::Kickoff { .. } => EventType::Kickoff,
            Self::HalfTime { .. } => EventType::HalfTime,
            Self::FullTime { .. } => EventType::FullTime,
        }
    }
}

/// Flat wire serialisation: envelope fields and payload fields
/// at the same JSON level (PRD §9.1 + §9.2).
#[allow(
    clippy::too_many_lines,
    reason = "flat-wire serialisation: one match arm per event-type with its own payload fields; splitting would scatter the field-to-wire map across helpers"
)]
impl Serialize for MatchEvent {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            Self::Pass { envelope, payload } => {
                // 7 envelope + 6 pass payload fields = 13.
                let mut map = ser.serialize_map(Some(13))?;
                map.serialize_entry("seq", &envelope.seq)?;
                map.serialize_entry("tick", &envelope.tick)?;
                map.serialize_entry("half", &envelope.half)?;
                map.serialize_entry("minute", &envelope.minute)?;
                map.serialize_entry("added_minute", &envelope.added_minute)?;
                map.serialize_entry("type", "pass")?;
                map.serialize_entry("team", &envelope.team)?;
                map.serialize_entry("from", &payload.from)?;
                map.serialize_entry("to", &payload.to)?;
                map.serialize_entry("success", &payload.success)?;
                map.serialize_entry("kick_tick", &payload.kick_tick)?;
                map.serialize_entry("start", &payload.start)?;
                map.serialize_entry("end", &payload.end)?;
                map.end()
            }
            Self::PossessionChange { envelope, payload } => {
                // 7 envelope + 4 possession_change payload fields = 11.
                let mut map = ser.serialize_map(Some(11))?;
                map.serialize_entry("seq", &envelope.seq)?;
                map.serialize_entry("tick", &envelope.tick)?;
                map.serialize_entry("half", &envelope.half)?;
                map.serialize_entry("minute", &envelope.minute)?;
                map.serialize_entry("added_minute", &envelope.added_minute)?;
                map.serialize_entry("type", "possession_change")?;
                map.serialize_entry("team", &envelope.team)?;
                map.serialize_entry("from_team", &payload.from_team)?;
                map.serialize_entry("to_team", &payload.to_team)?;
                map.serialize_entry("cause", &payload.cause)?;
                map.serialize_entry("last_touch", &payload.last_touch)?;
                map.end()
            }
            Self::OutOfBounds { envelope, payload } => {
                // 7 envelope + 3 out_of_bounds payload fields = 10.
                // The envelope's `team` field is the last-touch team
                // (the one whose touch sent the ball OOB); consumers
                // also get it via `last_touch.team` on the payload, so
                // the duplication is intentional and matches the
                // possession_change precedent (PRD §9.1).
                let mut map = ser.serialize_map(Some(10))?;
                map.serialize_entry("seq", &envelope.seq)?;
                map.serialize_entry("tick", &envelope.tick)?;
                map.serialize_entry("half", &envelope.half)?;
                map.serialize_entry("minute", &envelope.minute)?;
                map.serialize_entry("added_minute", &envelope.added_minute)?;
                map.serialize_entry("type", "out_of_bounds")?;
                map.serialize_entry("team", &envelope.team)?;
                map.serialize_entry("last_touch", &payload.last_touch)?;
                map.serialize_entry("exit_point", &payload.exit_point)?;
                map.serialize_entry("restart", &payload.restart)?;
                map.end()
            }
            Self::Restart { envelope, payload } => {
                // 7 envelope + 3 restart payload fields = 10.
                // The envelope's `team` is the restart-taker team
                // (the one that takes the throw-in / goal-kick /
                // corner), matching the §9.1 convention.
                let mut map = ser.serialize_map(Some(10))?;
                map.serialize_entry("seq", &envelope.seq)?;
                map.serialize_entry("tick", &envelope.tick)?;
                map.serialize_entry("half", &envelope.half)?;
                map.serialize_entry("minute", &envelope.minute)?;
                map.serialize_entry("added_minute", &envelope.added_minute)?;
                map.serialize_entry("type", "restart")?;
                map.serialize_entry("team", &envelope.team)?;
                map.serialize_entry("taker", &payload.taker)?;
                map.serialize_entry("at", &payload.at)?;
                map.serialize_entry("kind", &payload.kind)?;
                map.end()
            }
            Self::Goal { envelope, payload } => {
                // 7 envelope + 4 goal payload fields = 11. The
                // envelope's `team` is the scoring team (PRD §9.2
                // `team`); the running score is the payload's.
                let mut map = ser.serialize_map(Some(11))?;
                map.serialize_entry("seq", &envelope.seq)?;
                map.serialize_entry("tick", &envelope.tick)?;
                map.serialize_entry("half", &envelope.half)?;
                map.serialize_entry("minute", &envelope.minute)?;
                map.serialize_entry("added_minute", &envelope.added_minute)?;
                map.serialize_entry("type", "goal")?;
                map.serialize_entry("team", &envelope.team)?;
                map.serialize_entry("scorer", &payload.scorer)?;
                map.serialize_entry("assister", &payload.assister)?;
                map.serialize_entry("own_goal", &payload.own_goal)?;
                map.serialize_entry("score", &payload.score)?;
                map.end()
            }
            Self::Shot { envelope, payload } => {
                // 7 envelope + 3 shot payload fields = 10. The
                // envelope's `team` is the shooter's team.
                let mut map = ser.serialize_map(Some(10))?;
                map.serialize_entry("seq", &envelope.seq)?;
                map.serialize_entry("tick", &envelope.tick)?;
                map.serialize_entry("half", &envelope.half)?;
                map.serialize_entry("minute", &envelope.minute)?;
                map.serialize_entry("added_minute", &envelope.added_minute)?;
                map.serialize_entry("type", "shot")?;
                map.serialize_entry("team", &envelope.team)?;
                map.serialize_entry("shooter", &payload.shooter)?;
                map.serialize_entry("on_target", &payload.on_target)?;
                map.serialize_entry("outcome", &payload.outcome)?;
                map.end()
            }
            Self::Save { envelope, payload } => {
                // 7 envelope + 3 save payload fields = 10. The
                // envelope's `team` is the keeper's team.
                let mut map = ser.serialize_map(Some(10))?;
                map.serialize_entry("seq", &envelope.seq)?;
                map.serialize_entry("tick", &envelope.tick)?;
                map.serialize_entry("half", &envelope.half)?;
                map.serialize_entry("minute", &envelope.minute)?;
                map.serialize_entry("added_minute", &envelope.added_minute)?;
                map.serialize_entry("type", "save")?;
                map.serialize_entry("team", &envelope.team)?;
                map.serialize_entry("keeper", &payload.keeper)?;
                map.serialize_entry("shooter", &payload.shooter)?;
                map.serialize_entry("kind", &payload.kind)?;
                map.end()
            }
            Self::Kickoff { envelope, payload } => {
                // 7 envelope + 4 kickoff payload fields = 11. The
                // envelope's `team` is the taker team (Law 8).
                let mut map = ser.serialize_map(Some(11))?;
                map.serialize_entry("seq", &envelope.seq)?;
                map.serialize_entry("tick", &envelope.tick)?;
                map.serialize_entry("half", &envelope.half)?;
                map.serialize_entry("minute", &envelope.minute)?;
                map.serialize_entry("added_minute", &envelope.added_minute)?;
                map.serialize_entry("type", "kickoff")?;
                map.serialize_entry("team", &envelope.team)?;
                map.serialize_entry("taker", &payload.taker)?;
                map.serialize_entry("reason", &payload.reason)?;
                map.serialize_entry("attack_home", &payload.attack_home)?;
                map.serialize_entry("attack_away", &payload.attack_away)?;
                map.end()
            }
            Self::HalfTime { envelope, payload } => {
                // 7 envelope + 1 half_time payload field = 8. No team
                // is credited (`team` is null).
                let mut map = ser.serialize_map(Some(8))?;
                map.serialize_entry("seq", &envelope.seq)?;
                map.serialize_entry("tick", &envelope.tick)?;
                map.serialize_entry("half", &envelope.half)?;
                map.serialize_entry("minute", &envelope.minute)?;
                map.serialize_entry("added_minute", &envelope.added_minute)?;
                map.serialize_entry("type", "half_time")?;
                map.serialize_entry("team", &envelope.team)?;
                map.serialize_entry("score", &payload.score)?;
                map.end()
            }
            Self::FullTime { envelope, payload } => {
                // 7 envelope + 4 full_time payload fields = 11. No
                // team is credited (`team` is null).
                let mut map = ser.serialize_map(Some(11))?;
                map.serialize_entry("seq", &envelope.seq)?;
                map.serialize_entry("tick", &envelope.tick)?;
                map.serialize_entry("half", &envelope.half)?;
                map.serialize_entry("minute", &envelope.minute)?;
                map.serialize_entry("added_minute", &envelope.added_minute)?;
                map.serialize_entry("type", "full_time")?;
                map.serialize_entry("team", &envelope.team)?;
                map.serialize_entry("score", &payload.score)?;
                map.serialize_entry("event_count", &payload.event_count)?;
                map.serialize_entry("state_hash", &payload.state_hash)?;
                map.serialize_entry("added_time_ticks", &payload.added_time_ticks)?;
                map.end()
            }
        }
    }
}

/// Flat wire deserialisation. The `type` field is required to
/// disambiguate the variant; the remaining fields are merged
/// into the envelope + payload pair.
#[allow(
    clippy::too_many_lines,
    reason = "flat-wire deserialisation: one match arm per event-type with its own payload fields; splitting would scatter the field-to-payload map across helpers"
)]
impl<'de> Deserialize<'de> for MatchEvent {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        use serde::de::{MapAccess, Visitor};
        use std::fmt;

        /// Helper that holds the deserialised fields before
        /// assembling the right variant. The `Option<Option<T>>`
        /// pattern is intentional: the outer `Option` marks
        /// "field present in the input", and the inner `Option`
        /// marks "value is `null`". Both states are meaningful
        /// for the envelope / payload schema (e.g. `added_minute`
        /// is required and may be `null`).
        #[derive(Default)]
        #[allow(
            clippy::option_option,
            reason = "deserialize sink: outer Option = field present, inner Option = value is null"
        )]
        struct FieldSink {
            seq: Option<u64>,
            tick: Option<u64>,
            half: Option<u8>,
            minute: Option<u8>,
            added_minute: Option<Option<u8>>,
            event_type: Option<EventType>,
            team: Option<Option<TeamId>>,
            // pass payload
            from: Option<PlayerId>,
            to: Option<Option<PlayerId>>,
            success: Option<bool>,
            kick_tick: Option<u64>,
            start: Option<[f32; 2]>,
            end: Option<[f32; 2]>,
            // possession_change payload
            from_team: Option<TeamId>,
            to_team: Option<TeamId>,
            cause: Option<PossessionChangeCause>,
            last_touch: Option<Option<LastTouch>>,
            // out_of_bounds payload
            exit_point: Option<[f32; 2]>,
            restart: Option<RestartKind>,
            // restart payload
            taker: Option<Option<PlayerId>>,
            at: Option<[f32; 2]>,
            // `kind` is shared by `restart` (throw_in / goal_kick /
            // corner) and `save` (catch / parry / punch), whose value
            // domains don't overlap. It is captured raw and
            // interpreted per event type below.
            kind_raw: Option<String>,
            // goal payload
            scorer: Option<Option<PlayerId>>,
            assister: Option<Option<PlayerId>>,
            own_goal: Option<bool>,
            score: Option<(u8, u8)>,
            // shot payload
            shooter: Option<PlayerId>,
            on_target: Option<bool>,
            outcome: Option<ShotOutcome>,
            // save payload
            keeper: Option<PlayerId>,
            // kickoff payload
            reason: Option<KickoffReason>,
            attack_home: Option<Option<i8>>,
            attack_away: Option<Option<i8>>,
            // full_time payload
            event_count: Option<u64>,
            state_hash: Option<u64>,
            added_time_ticks: Option<(u64, u64)>,
        }

        struct EventVisitor;

        impl<'de> Visitor<'de> for EventVisitor {
            type Value = MatchEvent;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a MatchEvent object")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<MatchEvent, M::Error> {
                use serde::de::Error;
                let mut sink = FieldSink::default();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "seq" => sink.seq = Some(map.next_value()?),
                        "tick" => sink.tick = Some(map.next_value()?),
                        "half" => sink.half = Some(map.next_value()?),
                        "minute" => sink.minute = Some(map.next_value()?),
                        "added_minute" => sink.added_minute = Some(map.next_value()?),
                        "type" => sink.event_type = Some(map.next_value()?),
                        "team" => sink.team = Some(map.next_value()?),
                        "from" => sink.from = Some(map.next_value()?),
                        "to" => sink.to = Some(map.next_value()?),
                        "success" => sink.success = Some(map.next_value()?),
                        "kick_tick" => sink.kick_tick = Some(map.next_value()?),
                        "start" => sink.start = Some(map.next_value()?),
                        "end" => sink.end = Some(map.next_value()?),
                        "from_team" => sink.from_team = Some(map.next_value()?),
                        "to_team" => sink.to_team = Some(map.next_value()?),
                        "cause" => sink.cause = Some(map.next_value()?),
                        "last_touch" => sink.last_touch = Some(map.next_value()?),
                        "exit_point" => sink.exit_point = Some(map.next_value()?),
                        "restart" => sink.restart = Some(map.next_value()?),
                        "taker" => sink.taker = Some(map.next_value()?),
                        "at" => sink.at = Some(map.next_value()?),
                        "kind" => sink.kind_raw = Some(map.next_value()?),
                        "scorer" => sink.scorer = Some(map.next_value()?),
                        "assister" => sink.assister = Some(map.next_value()?),
                        "own_goal" => sink.own_goal = Some(map.next_value()?),
                        "score" => sink.score = Some(map.next_value()?),
                        "shooter" => sink.shooter = Some(map.next_value()?),
                        "on_target" => sink.on_target = Some(map.next_value()?),
                        "outcome" => sink.outcome = Some(map.next_value()?),
                        "keeper" => sink.keeper = Some(map.next_value()?),
                        "reason" => sink.reason = Some(map.next_value()?),
                        "attack_home" => sink.attack_home = Some(map.next_value()?),
                        "attack_away" => sink.attack_away = Some(map.next_value()?),
                        "event_count" => sink.event_count = Some(map.next_value()?),
                        "state_hash" => sink.state_hash = Some(map.next_value()?),
                        "added_time_ticks" => sink.added_time_ticks = Some(map.next_value()?),
                        _ => {
                            // Unknown fields are ignored so the
                            // wire format is forward-compatible.
                            let _: serde::de::IgnoredAny = map.next_value()?;
                        }
                    }
                }
                let envelope = EventEnvelope {
                    seq: sink.seq.ok_or_else(|| Error::missing_field("seq"))?,
                    tick: sink.tick.ok_or_else(|| Error::missing_field("tick"))?,
                    half: sink.half.ok_or_else(|| Error::missing_field("half"))?,
                    minute: sink.minute.ok_or_else(|| Error::missing_field("minute"))?,
                    added_minute: sink
                        .added_minute
                        .ok_or_else(|| Error::missing_field("added_minute"))?,
                    event_type: sink
                        .event_type
                        .ok_or_else(|| Error::missing_field("type"))?,
                    team: sink.team.ok_or_else(|| Error::missing_field("team"))?,
                };
                match envelope.event_type {
                    EventType::Pass => Ok(MatchEvent::Pass {
                        envelope,
                        payload: PassEvent {
                            from: sink.from.ok_or_else(|| Error::missing_field("from"))?,
                            to: sink.to.ok_or_else(|| Error::missing_field("to"))?,
                            success: sink
                                .success
                                .ok_or_else(|| Error::missing_field("success"))?,
                            kick_tick: sink
                                .kick_tick
                                .ok_or_else(|| Error::missing_field("kick_tick"))?,
                            start: sink.start.ok_or_else(|| Error::missing_field("start"))?,
                            end: sink.end.ok_or_else(|| Error::missing_field("end"))?,
                        },
                    }),
                    EventType::PossessionChange => Ok(MatchEvent::PossessionChange {
                        envelope,
                        payload: PossessionChangeEvent {
                            from_team: sink
                                .from_team
                                .ok_or_else(|| Error::missing_field("from_team"))?,
                            to_team: sink
                                .to_team
                                .ok_or_else(|| Error::missing_field("to_team"))?,
                            cause: sink
                                .cause
                                .ok_or_else(|| Error::missing_field("cause"))?,
                            last_touch: sink
                                .last_touch
                                .ok_or_else(|| Error::missing_field("last_touch"))?,
                        },
                    }),
                    EventType::OutOfBounds => Ok(MatchEvent::OutOfBounds {
                        envelope,
                        payload: OutOfBoundsEventPayload {
                            last_touch: sink
                                .last_touch
                                .ok_or_else(|| Error::missing_field("last_touch"))?,
                            exit_point: sink
                                .exit_point
                                .ok_or_else(|| Error::missing_field("exit_point"))?,
                            restart: sink
                                .restart
                                .ok_or_else(|| Error::missing_field("restart"))?,
                        },
                    }),
                    EventType::Restart => Ok(MatchEvent::Restart {
                        envelope,
                        payload: RestartEventPayload {
                            taker: sink.taker.ok_or_else(|| Error::missing_field("taker"))?,
                            at: sink.at.ok_or_else(|| Error::missing_field("at"))?,
                            kind: match sink
                                .kind_raw
                                .as_deref()
                                .ok_or_else(|| Error::missing_field("kind"))?
                            {
                                "throw_in" => RestartKind::ThrowIn,
                                "goal_kick" => RestartKind::GoalKick,
                                "corner" => RestartKind::Corner,
                                other => {
                                    return Err(Error::unknown_variant(
                                        other,
                                        &["throw_in", "goal_kick", "corner"],
                                    ));
                                }
                            },
                        },
                    }),
                    EventType::Goal => Ok(MatchEvent::Goal {
                        envelope,
                        payload: GoalEventPayload {
                            scorer: sink.scorer.ok_or_else(|| Error::missing_field("scorer"))?,
                            assister: sink
                                .assister
                                .ok_or_else(|| Error::missing_field("assister"))?,
                            own_goal: sink
                                .own_goal
                                .ok_or_else(|| Error::missing_field("own_goal"))?,
                            score: sink.score.ok_or_else(|| Error::missing_field("score"))?,
                        },
                    }),
                    EventType::Shot => Ok(MatchEvent::Shot {
                        envelope,
                        payload: ShotEventPayload {
                            shooter: sink
                                .shooter
                                .ok_or_else(|| Error::missing_field("shooter"))?,
                            on_target: sink
                                .on_target
                                .ok_or_else(|| Error::missing_field("on_target"))?,
                            outcome: sink
                                .outcome
                                .ok_or_else(|| Error::missing_field("outcome"))?,
                        },
                    }),
                    EventType::Save => Ok(MatchEvent::Save {
                        envelope,
                        payload: SaveEventPayload {
                            keeper: sink
                                .keeper
                                .ok_or_else(|| Error::missing_field("keeper"))?,
                            shooter: sink
                                .shooter
                                .ok_or_else(|| Error::missing_field("shooter"))?,
                            kind: match sink
                                .kind_raw
                                .as_deref()
                                .ok_or_else(|| Error::missing_field("kind"))?
                            {
                                "catch" => SaveKind::Catch,
                                "parry" => SaveKind::Parry,
                                "punch" => SaveKind::Punch,
                                other => {
                                    return Err(Error::unknown_variant(
                                        other,
                                        &["catch", "parry", "punch"],
                                    ));
                                }
                            },
                        },
                    }),
                    EventType::Kickoff => Ok(MatchEvent::Kickoff {
                        envelope,
                        payload: KickoffEventPayload {
                            taker: sink.taker.ok_or_else(|| Error::missing_field("taker"))?,
                            reason: sink
                                .reason
                                .ok_or_else(|| Error::missing_field("reason"))?,
                            attack_home: sink
                                .attack_home
                                .ok_or_else(|| Error::missing_field("attack_home"))?,
                            attack_away: sink
                                .attack_away
                                .ok_or_else(|| Error::missing_field("attack_away"))?,
                        },
                    }),
                    EventType::HalfTime => Ok(MatchEvent::HalfTime {
                        envelope,
                        payload: HalfTimePayload {
                            score: sink.score.ok_or_else(|| Error::missing_field("score"))?,
                        },
                    }),
                    EventType::FullTime => Ok(MatchEvent::FullTime {
                        envelope,
                        payload: FullTimePayload {
                            score: sink.score.ok_or_else(|| Error::missing_field("score"))?,
                            event_count: sink
                                .event_count
                                .ok_or_else(|| Error::missing_field("event_count"))?,
                            state_hash: sink
                                .state_hash
                                .ok_or_else(|| Error::missing_field("state_hash"))?,
                            added_time_ticks: sink
                                .added_time_ticks
                                .ok_or_else(|| Error::missing_field("added_time_ticks"))?,
                        },
                    }),
                }
            }
        }

        de.deserialize_map(EventVisitor)
    }
}

/// The accumulating, gapless event log for a match.
///
/// `next_seq` is the next seq to assign; it starts at 0 and is
/// monotonic. The log itself is append-only; writers call
/// [`Self::append`] which bumps the counter and pushes.
///
/// `possession_team` is a small cache the log keeps so writers
/// don't have to reach into `Ball` to detect team changes —
/// a per-tick invariant: `possession_team` matches
/// `Ball.possessor`'s team whenever the ball is `Possessed`,
/// and is `None` otherwise.
#[derive(Resource, Debug, Default, Clone)]
pub struct EventLog {
    /// The event log itself, in emission order.
    pub events: Vec<MatchEvent>,
    /// Next seq to assign on append.
    pub next_seq: u64,
    /// Last observed possessing team (`None` when the ball is dead
    /// or in flight with no possessor). Used by the
    /// possession-change detector to compare-and-emit.
    pub possession_team: Option<TeamId>,
}

impl EventLog {
    /// Create an empty log with `next_seq = 0`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            events: Vec::new(),
            next_seq: 0,
            possession_team: None,
        }
    }

    /// Append `event` and return the assigned seq.
    ///
    /// The seq stamped on the envelope is `next_seq` *before* the
    /// bump, so the first event gets seq 0. The postcondition is
    /// `next_seq == events.len()` for any sequence of appends,
    /// which is what makes the gapless invariant checkable.
    pub fn append(&mut self, mut event: MatchEvent) -> u64 {
        let seq = self.next_seq;
        match &mut event {
            MatchEvent::Pass { envelope, .. }
            | MatchEvent::PossessionChange { envelope, .. }
            | MatchEvent::OutOfBounds { envelope, .. }
            | MatchEvent::Restart { envelope, .. }
            | MatchEvent::Goal { envelope, .. }
            | MatchEvent::Shot { envelope, .. }
            | MatchEvent::Save { envelope, .. }
            | MatchEvent::Kickoff { envelope, .. }
            | MatchEvent::HalfTime { envelope, .. }
            | MatchEvent::FullTime { envelope, .. } => {
                envelope.seq = seq;
            }
        }
        self.events.push(event);
        self.next_seq = self.next_seq.wrapping_add(1);
        seq
    }

    /// Number of events appended so far.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.events.len()
    }

    /// `true` when no events have been appended.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Build the per-event envelope fields that depend on the live
/// `MatchClock` (half / minute / `added_minute`) plus the
/// global tick and the team.
///
/// `added_minute` is `None` while the match is in regular time
/// (i.e. before any `added_time_ticks` has accrued). When set,
/// it is the integer minute in `[1, 10]`.
#[must_use]
pub fn envelope_minute_fields(clock: &MatchClock) -> (u8, Option<u8>) {
    let minute = if clock.half >= 2 { 90 } else { 45 };
    // `added_time_ticks` is per-half. Convert to whole minutes,
    // rounding up: 1 added_minute starts the moment added time
    // begins (PRD §8.3).
    let added_minute = if clock.added_time_ticks == 0 {
        None
    } else {
        // ticks / 60 = seconds; seconds / 60 = minutes; round up.
        // Use ceiling division without f32 to stay integer-clean.
        let total_secs = clock.added_time_ticks.div_ceil(60);
        let mins = total_secs.div_ceil(60);
        // Cap at u8; PRD §8.4 caps added time at 10 minutes.
        Some(mins.min(255) as u8)
    };
    (minute, added_minute)
}

/// Convenience constructor for a `pass` event.
///
/// `from` is the kicker (last `Play` touch at the time of the
/// kick); `to` is the intended receiver or `None` if the kick was
/// astray; `success` is `true` when the ball was received by a
/// teammate of `from`'s team.
///
/// `start` and `end` are the ball positions at the kick and at
/// the resolution, respectively. `start` is recorded by the
/// kick site; `end` is recorded by the resolver.
#[must_use]
#[allow(
    clippy::too_many_arguments,
    reason = "pass-event constructor mirrors PRD §9.2 payload fields; a builder would scatter the field set across two calls and obscure the per-field semantics"
)]
pub fn build_pass_event(
    seq: u64,
    tick: u64,
    clock: &MatchClock,
    team: TeamId,
    from: PlayerId,
    to: Option<PlayerId>,
    success: bool,
    kick_tick: u64,
    start: Vec2,
    end: Vec2,
) -> MatchEvent {
    let (minute, added_minute) = envelope_minute_fields(clock);
    let envelope = EventEnvelope {
        seq,
        tick,
        half: clock.half,
        minute,
        added_minute,
        event_type: EventType::Pass,
        team: Some(team),
    };
    let payload = PassEvent {
        from,
        to,
        success,
        kick_tick,
        start: [start.x, start.y],
        end: [end.x, end.y],
    };
    MatchEvent::Pass { envelope, payload }
}

/// Convenience constructor for a `possession_change` event.
///
/// `from_team` is the team that previously had possession (or
/// `None` if there was no prior possessor); `to_team` is the
/// team that now has possession. `last_touch` is the `LastTouch`
/// at the moment of the change.
#[must_use]
pub fn build_possession_change_event(
    seq: u64,
    tick: u64,
    clock: &MatchClock,
    from_team: Option<TeamId>,
    to_team: TeamId,
    cause: PossessionChangeCause,
    last_touch: Option<LastTouch>,
) -> MatchEvent {
    let (minute, added_minute) = envelope_minute_fields(clock);
    // Wire shape: `from_team` and `to_team` are both required in
    // the payload. The envelope `team` field is the team credited
    // by the event; for possession_change that is the new
    // possessing team (matches the §9.2 payload direction).
    let envelope = EventEnvelope {
        seq,
        tick,
        half: clock.half,
        minute,
        added_minute,
        event_type: EventType::PossessionChange,
        team: Some(to_team),
    };
    let payload = PossessionChangeEvent {
        // PRD §9.2 lists the payload as `from_team` / `to_team`
        // both non-nullable. In practice the first change of the
        // match has no prior possessor; encode that as the team
        // losing possession in the touch history (a kickoff
        // placement credits a team, so the first change can pick
        // the opponent of `to_team`).
        from_team: from_team.unwrap_or_else(|| opponent_of(to_team)),
        to_team,
        cause,
        last_touch,
    };
    MatchEvent::PossessionChange { envelope, payload }
}

/// Convenience constructor for an `out_of_bounds` event (ticket 09).
///
/// `last_touch` is the `LastTouch` at the moment the ball crossed
/// the boundary. `exit_point` is the ball position the OOB detector
/// observed (physics clamps to the boundary line; this is the
/// clamped coord). `kind` selects which restart follows
/// (`throw_in` / `goal_kick` / `corner`).
#[must_use]
#[allow(
    clippy::too_many_arguments,
    reason = "out_of_bounds constructor mirrors PRD §9.2 payload fields; a builder would scatter the field set across two calls"
)]
pub fn build_out_of_bounds_event(
    seq: u64,
    tick: u64,
    clock: &MatchClock,
    team: TeamId,
    last_touch: Option<LastTouch>,
    exit_point: Vec2,
    kind: RestartKind,
) -> MatchEvent {
    let (minute, added_minute) = envelope_minute_fields(clock);
    // The envelope's `team` is the last-touch team (the team whose
    // touch sent the ball OOB). Consumers can read either `envelope.team`
    // or `payload.last_touch.team`; the duplication matches the
    // §9.1 envelope convention and the existing `possession_change`
    // pattern.
    let envelope = EventEnvelope {
        seq,
        tick,
        half: clock.half,
        minute,
        added_minute,
        event_type: EventType::OutOfBounds,
        team: Some(team),
    };
    let payload = OutOfBoundsEventPayload {
        last_touch,
        exit_point: [exit_point.x, exit_point.y],
        restart: kind,
    };
    MatchEvent::OutOfBounds { envelope, payload }
}

/// Convenience constructor for a `restart` event (ticket 09).
///
/// `taker_team` is the team that takes the throw-in / goal-kick /
/// corner (Law 9: opposite of the last-toucher). `taker` is the
/// `PlayerId` of the specific player chosen by `first_player_on_team`
/// (None in test worlds with no resolvable player on the taker team).
/// `at` is the restart placement position, computed by
/// `sim_rules::out_of_bounds::calculate_oob_restart_position`.
#[must_use]
#[allow(
    clippy::too_many_arguments,
    reason = "restart constructor mirrors PRD §9.2 payload fields; a builder would scatter the field set across two calls"
)]
pub fn build_restart_event(
    seq: u64,
    tick: u64,
    clock: &MatchClock,
    taker_team: TeamId,
    taker: Option<PlayerId>,
    at: Vec2,
    kind: RestartKind,
) -> MatchEvent {
    let (minute, added_minute) = envelope_minute_fields(clock);
    let envelope = EventEnvelope {
        seq,
        tick,
        half: clock.half,
        minute,
        added_minute,
        event_type: EventType::Restart,
        team: Some(taker_team),
    };
    let payload = RestartEventPayload {
        taker,
        at: [at.x, at.y],
        kind,
    };
    MatchEvent::Restart { envelope, payload }
}

/// Opponent of `team` in a two-team world. Returns `TeamId(0)`
/// when `team` is `TeamId(1)` and vice versa.
#[must_use]
pub const fn opponent_of(team: TeamId) -> TeamId {
    if team.0 == 0 { TeamId(1) } else { TeamId(0) }
}

/// Convenience constructor for a `goal` event (ticket 10).
///
/// `scoring_team` is credited on the envelope (`team`); `attribution`
/// comes from [`attribute_goal`] over the touch window the rule engine
/// captured at the goal tick. `score` is the running score after the
/// goal — the A7 invariant folds these across the match.
#[must_use]
pub fn build_goal_event(
    seq: u64,
    tick: u64,
    clock: &MatchClock,
    scoring_team: TeamId,
    attribution: GoalAttribution,
    score: (u8, u8),
) -> MatchEvent {
    let (minute, added_minute) = envelope_minute_fields(clock);
    let envelope = EventEnvelope {
        seq,
        tick,
        half: clock.half,
        minute,
        added_minute,
        event_type: EventType::Goal,
        team: Some(scoring_team),
    };
    let payload = GoalEventPayload {
        scorer: attribution.scorer,
        assister: attribution.assister,
        own_goal: attribution.own_goal,
        score,
    };
    MatchEvent::Goal { envelope, payload }
}

/// Convenience constructor for a `shot` event (ticket 10).
///
/// `shooter_team` is credited on the envelope (`team`).
#[must_use]
pub fn build_shot_event(
    seq: u64,
    tick: u64,
    clock: &MatchClock,
    shooter_team: TeamId,
    shooter: PlayerId,
    on_target: bool,
    outcome: ShotOutcome,
) -> MatchEvent {
    let (minute, added_minute) = envelope_minute_fields(clock);
    let envelope = EventEnvelope {
        seq,
        tick,
        half: clock.half,
        minute,
        added_minute,
        event_type: EventType::Shot,
        team: Some(shooter_team),
    };
    let payload = ShotEventPayload {
        shooter,
        on_target,
        outcome,
    };
    MatchEvent::Shot { envelope, payload }
}

/// Convenience constructor for a `save` event (ticket 10).
///
/// Emitted alongside the resolving `shot` event (same tick, next
/// `seq`). `keeper_team` is credited on the envelope (`team`).
#[must_use]
pub fn build_save_event(
    seq: u64,
    tick: u64,
    clock: &MatchClock,
    keeper_team: TeamId,
    keeper: PlayerId,
    shooter: PlayerId,
    kind: SaveKind,
) -> MatchEvent {
    let (minute, added_minute) = envelope_minute_fields(clock);
    let envelope = EventEnvelope {
        seq,
        tick,
        half: clock.half,
        minute,
        added_minute,
        event_type: EventType::Save,
        team: Some(keeper_team),
    };
    let payload = SaveEventPayload {
        keeper,
        shooter,
        kind,
    };
    MatchEvent::Save { envelope, payload }
}

/// Convenience constructor for a `kickoff` event (ticket 10).
///
/// `taker_team` is credited on the envelope (`team`). `attack_home` /
/// `attack_away` carry the per-team attack direction on half starts
/// (`match_start`, `second_half`); they are `None` for `after_goal`.
#[must_use]
#[allow(
    clippy::too_many_arguments,
    reason = "kickoff constructor mirrors PRD §9.2 payload fields; a builder would scatter the field set across two calls"
)]
pub fn build_kickoff_event(
    seq: u64,
    tick: u64,
    clock: &MatchClock,
    taker_team: TeamId,
    taker: Option<PlayerId>,
    reason: KickoffReason,
    attack_home: Option<i8>,
    attack_away: Option<i8>,
) -> MatchEvent {
    let (minute, added_minute) = envelope_minute_fields(clock);
    let envelope = EventEnvelope {
        seq,
        tick,
        half: clock.half,
        minute,
        added_minute,
        event_type: EventType::Kickoff,
        team: Some(taker_team),
    };
    let payload = KickoffEventPayload {
        taker,
        reason,
        attack_home,
        attack_away,
    };
    MatchEvent::Kickoff { envelope, payload }
}

/// Convenience constructor for a `half_time` event (ticket 10).
///
/// No team is credited (envelope `team` is `None`). Carries the
/// running score at the whistle.
#[must_use]
pub fn build_half_time_event(
    seq: u64,
    tick: u64,
    clock: &MatchClock,
    score: (u8, u8),
) -> MatchEvent {
    let (minute, added_minute) = envelope_minute_fields(clock);
    let envelope = EventEnvelope {
        seq,
        tick,
        half: clock.half,
        minute,
        added_minute,
        event_type: EventType::HalfTime,
        team: None,
    };
    let payload = HalfTimePayload { score };
    MatchEvent::HalfTime { envelope, payload }
}

/// Convenience constructor for a `full_time` event (ticket 10).
///
/// No team is credited (envelope `team` is `None`). `event_count` is
/// the total log length including this event; `state_hash` matches
/// `Simulation::get_state_hash` at full time.
#[must_use]
#[allow(
    clippy::too_many_arguments,
    reason = "full_time constructor mirrors PRD §9.2 payload fields plus the ticket-10 event_count/state_hash; a builder would scatter the field set across two calls"
)]
pub fn build_full_time_event(
    seq: u64,
    tick: u64,
    clock: &MatchClock,
    score: (u8, u8),
    event_count: u64,
    state_hash: u64,
    added_time_ticks: (u64, u64),
) -> MatchEvent {
    let (minute, added_minute) = envelope_minute_fields(clock);
    let envelope = EventEnvelope {
        seq,
        tick,
        half: clock.half,
        minute,
        added_minute,
        event_type: EventType::FullTime,
        team: None,
    };
    let payload = FullTimePayload {
        score,
        event_count,
        state_hash,
        added_time_ticks,
    };
    MatchEvent::FullTime { envelope, payload }
}

/// Assert the gapless-monotonic invariant on a finalized log.
///
/// `events[i].envelope.seq == i` for every index, and
/// `next_seq == events.len()`. Returns the first violation as
/// `Some((expected_seq, found_seq))`, or `None` when the log is
/// gapless.
///
/// # Panics
///
/// Panics if the event count or `next_seq` overflow `u64` (the
/// log would have to be larger than 2^64 events; the `expect`
/// calls are documentation of the invariant, not a reachable
/// failure mode in a real match).
#[must_use]
pub fn check_gapless_seq(log: &EventLog) -> Option<(u64, u64)> {
    for (idx, event) in log.events.iter().enumerate() {
        let expected = u64::try_from(idx).expect("invariant: event count fits u64");
        if event.envelope().seq != expected {
            return Some((expected, event.envelope().seq));
        }
    }
    if log.next_seq != u64::try_from(log.events.len()).expect("invariant: len fits u64") {
        return Some((
            u64::try_from(log.events.len()).expect("invariant: len fits u64"),
            log.next_seq,
        ));
    }
    None
}

/// Touch the `HALF_LENGTH_TICKS` import so it's available to
/// downstream crates without warnings.
#[allow(dead_code, reason = "constant imported for downstream display helpers")]
const _HALF_LENGTH_TICKS_TOUCH: u64 = HALF_LENGTH_TICKS;

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests compare fresh values against the constants they were built from"
)]
mod tests {
    use super::*;
    use sim_components::{PlayerId, TeamId};

    fn sample_clock(half: u8, elapsed_ticks: u64, added_time_ticks: u64) -> MatchClock {
        MatchClock {
            half,
            elapsed_ticks,
            added_time_ticks,
            is_running: true,
        }
    }

    #[test]
    fn event_log_assigns_gapless_seq() {
        let mut log = EventLog::new();
        let tick = 100_u64;
        let clock = sample_clock(1, tick, 0);
        for _ in 0..5 {
            log.append(build_pass_event(
                log.next_seq,
                tick,
                &clock,
                TeamId(0),
                PlayerId::from_bits(1),
                Some(PlayerId::from_bits(2)),
                true,
                tick,
                Vec2::new(1.0, 2.0),
                Vec2::new(3.0, 4.0),
            ));
        }
        assert_eq!(log.len(), 5);
        assert_eq!(log.next_seq, 5);
        assert!(check_gapless_seq(&log).is_none());
        for (i, event) in log.events.iter().enumerate() {
            assert_eq!(
                event.envelope().seq,
                u64::try_from(i).expect("fits"),
                "event {i} should have seq {i}"
            );
        }
    }

    #[test]
    fn event_log_detects_seq_gap() {
        // Build a log then perturb the seq of one event to confirm
        // the gapless check catches it.
        let mut log = EventLog::new();
        let clock = sample_clock(1, 100, 0);
        log.append(build_pass_event(
            log.next_seq,
            100,
            &clock,
            TeamId(0),
            PlayerId::from_bits(1),
            Some(PlayerId::from_bits(2)),
            true,
            100,
            Vec2::new(1.0, 2.0),
            Vec2::new(3.0, 4.0),
        ));
        log.append(build_pass_event(
            log.next_seq,
            200,
            &clock,
            TeamId(0),
            PlayerId::from_bits(1),
            Some(PlayerId::from_bits(2)),
            true,
            200,
            Vec2::new(1.0, 2.0),
            Vec2::new(3.0, 4.0),
        ));
        // Manually perturb the second event's seq to break the
        // invariant.
        if let MatchEvent::Pass { envelope, .. } = &mut log.events[1] {
            envelope.seq = 99;
        }
        assert_eq!(
            check_gapless_seq(&log),
            Some((1, 99)),
            "gapless check should detect the broken seq"
        );
    }

    #[test]
    fn envelope_minute_is_45_for_first_half_and_90_for_second() {
        let c1 = sample_clock(1, 100, 0);
        let (m1, a1) = envelope_minute_fields(&c1);
        assert_eq!(m1, 45);
        assert_eq!(a1, None, "no added time → added_minute is None");

        let c2 = sample_clock(2, 100, 0);
        let (m2, a2) = envelope_minute_fields(&c2);
        assert_eq!(m2, 90);
        assert_eq!(a2, None);
    }

    #[test]
    fn envelope_added_minute_rounds_up_from_added_time_ticks() {
        // 1 second of added time → first added minute is 1.
        let c = sample_clock(1, 162_000, 60);
        let (_, a) = envelope_minute_fields(&c);
        assert_eq!(a, Some(1));

        // 1 tick (a sub-second sliver) → rounds up to 1 added minute.
        let c = sample_clock(1, 162_000, 1);
        let (_, a) = envelope_minute_fields(&c);
        assert_eq!(a, Some(1));

        // 60 seconds of added time = 1 added minute.
        let c = sample_clock(1, 162_000, 60 * 60);
        let (_, a) = envelope_minute_fields(&c);
        assert_eq!(a, Some(1));

        // 3 minutes 1 second of added time = 4 added minutes (rounded up).
        let c = sample_clock(1, 162_000, (3 * 60 + 1) * 60);
        let (_, a) = envelope_minute_fields(&c);
        assert_eq!(a, Some(4));
    }

    #[test]
    fn possession_change_event_payload_round_trip() {
        let mut log = EventLog::new();
        let clock = sample_clock(1, 100, 0);
        let event = build_possession_change_event(
            log.next_seq,
            100,
            &clock,
            Some(TeamId(0)),
            TeamId(1),
            PossessionChangeCause::Interception,
            None,
        );
        log.append(event);
        // Re-serialise to verify the wire shape matches §9.2.
        let json = serde_json::to_string(&log.events[0]).expect("serialise");
        assert!(json.contains("\"type\":\"possession_change\""));
        assert!(json.contains("\"cause\":\"interception\""));
        assert!(json.contains("\"from_team\":0"));
        assert!(json.contains("\"to_team\":1"));
    }

    #[test]
    fn pass_event_payload_round_trip() {
        let mut log = EventLog::new();
        let clock = sample_clock(1, 100, 0);
        let event = build_pass_event(
            log.next_seq,
            200,
            &clock,
            TeamId(0),
            PlayerId::from_bits(7),
            Some(PlayerId::from_bits(9)),
            true,
            100,
            Vec2::new(50.0, 34.0),
            Vec2::new(60.0, 34.0),
        );
        log.append(event);
        let json = serde_json::to_string(&log.events[0]).expect("serialise");
        assert!(json.contains("\"type\":\"pass\""));
        assert!(json.contains("\"kick_tick\":100"));
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"start\":[50.0,34.0]"));
    }

    #[test]
    fn opponent_of_swaps_zero_and_one() {
        assert_eq!(opponent_of(TeamId(0)), TeamId(1));
        assert_eq!(opponent_of(TeamId(1)), TeamId(0));
    }

    /// `out_of_bounds` events carry the right payload fields.
    #[test]
    fn out_of_bounds_event_payload_round_trip() {
        let mut log = EventLog::new();
        let clock = sample_clock(1, 4_500, 0);
        let event = build_out_of_bounds_event(
            log.next_seq,
            4_500,
            &clock,
            TeamId(0),
            Some(LastTouch::by_player(
                TeamId(0),
                Entity::from_raw(7),
                sim_components::TouchKind::Play,
            )),
            Vec2::new(20.0, 0.0),
            RestartKind::ThrowIn,
        );
        log.append(event);
        let MatchEvent::OutOfBounds { envelope, payload } = &log.events[0] else {
            panic!("expected out_of_bounds event");
        };
        assert_eq!(envelope.tick, 4_500);
        assert_eq!(envelope.half, 1);
        assert_eq!(envelope.team, Some(TeamId(0)));
        assert_eq!(payload.exit_point, [20.0, 0.0]);
        assert_eq!(payload.restart, RestartKind::ThrowIn);
        assert!(payload.last_touch.is_some());

        // Wire shape matches PRD §9.2: `type`, `restart`, `exit_point`,
        // `last_touch` at the top level.
        let json = serde_json::to_string(&log.events[0]).expect("serialise");
        assert!(json.contains("\"type\":\"out_of_bounds\""));
        assert!(json.contains("\"restart\":\"throw_in\""));
        assert!(json.contains("\"exit_point\":[20.0,0.0]"));
        assert!(json.contains("\"last_touch\":{"));
    }

    /// `restart` events carry the right payload fields.
    #[test]
    fn restart_event_payload_round_trip() {
        let mut log = EventLog::new();
        let clock = sample_clock(1, 4_500, 0);
        let event = build_restart_event(
            log.next_seq,
            4_500,
            &clock,
            TeamId(1),
            Some(PlayerId::from_bits(11)),
            Vec2::new(20.0, 0.0),
            RestartKind::ThrowIn,
        );
        log.append(event);
        let MatchEvent::Restart { envelope, payload } = &log.events[0] else {
            panic!("expected restart event");
        };
        assert_eq!(envelope.tick, 4_500);
        assert_eq!(envelope.team, Some(TeamId(1)), "envelope team = taker team");
        assert_eq!(payload.taker, Some(PlayerId::from_bits(11)));
        assert_eq!(payload.at, [20.0, 0.0]);
        assert_eq!(payload.kind, RestartKind::ThrowIn);

        let json = serde_json::to_string(&log.events[0]).expect("serialise");
        assert!(json.contains("\"type\":\"restart\""));
        assert!(json.contains("\"kind\":\"throw_in\""));
        assert!(json.contains("\"at\":[20.0,0.0]"));
        assert!(json.contains("\"taker\":11"));
    }

    /// `out_of_bounds` immediately followed by `restart` carries
    /// consecutive seqs and the same tick (ticket 09 ordering
    /// invariant: `out_of_bounds.seq + 1 == restart.seq`).
    #[test]
    fn oob_then_restart_have_consecutive_seqs_and_same_tick() {
        let mut log = EventLog::new();
        let clock = sample_clock(1, 8_000, 0);
        log.append(build_out_of_bounds_event(
            log.next_seq,
            8_000,
            &clock,
            TeamId(0),
            None,
            Vec2::new(52.5, 0.0),
            RestartKind::Corner,
        ));
        log.append(build_restart_event(
            log.next_seq,
            8_000,
            &clock,
            TeamId(1),
            None,
            Vec2::new(104.0, 1.0),
            RestartKind::Corner,
        ));
        assert_eq!(log.len(), 2);
        assert_eq!(log.next_seq, 2);
        assert_eq!(log.events[0].envelope().seq, 0);
        assert_eq!(log.events[1].envelope().seq, 1);
        assert_eq!(log.events[0].envelope().tick, log.events[1].envelope().tick);
    }

    /// `RestartKind::from_oob` maps the rule-side enum to the event
    /// enum 1:1.
    #[test]
    fn restart_kind_from_oob_is_identity_mapping() {
        use sim_components::BallOutOfBoundsType;
        assert_eq!(
            RestartKind::from_oob(BallOutOfBoundsType::ThrowIn),
            RestartKind::ThrowIn
        );
        assert_eq!(
            RestartKind::from_oob(BallOutOfBoundsType::GoalKick),
            RestartKind::GoalKick
        );
        assert_eq!(
            RestartKind::from_oob(BallOutOfBoundsType::Corner),
            RestartKind::Corner
        );
    }

    fn touch_on(team: TeamId, raw: u32, kind: sim_components::TouchKind, tick: u64) -> LastTouch {
        LastTouch::by_player(team, Entity::from_raw(raw), kind).with_tick(tick)
    }

    /// PRD §8.5: scorer is the last `Play` toucher; assister is the
    /// previous `Play` toucher on the scoring team within 10 s.
    #[test]
    fn attribute_goal_picks_scorer_and_assister() {
        use sim_components::TouchKind;
        let scorer = touch_on(TeamId(0), 11, TouchKind::Play, 1_000);
        let assist = touch_on(TeamId(0), 22, TouchKind::Play, 900);
        let attr = attribute_goal(TeamId(0), [Some(scorer), Some(assist), None], 1_000);
        assert_eq!(attr.scorer, scorer.player_id, "scorer is the last Play touch");
        assert_eq!(
            attr.assister, assist.player_id,
            "assister is the previous scoring-team Play touch within 10 s"
        );
        assert!(!attr.own_goal);
    }

    /// PRD §8.5: a scorer on the conceding team is an own goal with no
    /// assister, even when a scoring-team touch sits in the window.
    #[test]
    fn attribute_goal_flags_own_goal_without_assister() {
        use sim_components::TouchKind;
        let scorer = touch_on(TeamId(1), 11, TouchKind::Play, 1_000);
        let prior = touch_on(TeamId(0), 22, TouchKind::Play, 990);
        let attr = attribute_goal(TeamId(0), [Some(scorer), Some(prior), None], 1_000);
        assert_eq!(attr.scorer, scorer.player_id);
        assert!(attr.own_goal, "conceding-team scorer is an own goal");
        assert_eq!(attr.assister, None, "own goals carry no assister");
    }

    /// PRD §8.5: restart placements never score or assist; the window
    /// slides past them to the older `Play` touch.
    #[test]
    fn attribute_goal_skips_restart_touches() {
        use sim_components::TouchKind;
        let restart = touch_on(TeamId(0), 11, TouchKind::Restart, 1_000);
        let scorer = touch_on(TeamId(0), 22, TouchKind::Play, 990);
        let assist = touch_on(TeamId(0), 33, TouchKind::Play, 900);
        let attr = attribute_goal(
            TeamId(0),
            [Some(restart), Some(scorer), Some(assist)],
            1_000,
        );
        assert_eq!(attr.scorer, scorer.player_id, "Restart touch cannot score");
        assert_eq!(attr.assister, assist.player_id);
        assert!(!attr.own_goal);
    }

    /// PRD §8.5: a previous scoring-team touch older than 10 s
    /// (600 ticks) is not an assist.
    #[test]
    fn attribute_goal_rejects_stale_assister() {
        use sim_components::TouchKind;
        let scorer = touch_on(TeamId(0), 11, TouchKind::Play, 1_000);
        let stale = touch_on(TeamId(0), 22, TouchKind::Play, 100);
        let attr = attribute_goal(TeamId(0), [Some(scorer), Some(stale), None], 1_000);
        assert_eq!(attr.scorer, scorer.player_id);
        assert_eq!(
            attr.assister, None,
            "touch 900 ticks back is outside the 10 s window"
        );
    }

    /// No `Play` touch on record: no scorer, no assister, not an own
    /// goal (the rule engine debug-asserts against this wiring gap).
    #[test]
    fn attribute_goal_without_play_touch_has_no_scorer() {
        use sim_components::TouchKind;
        let restart = touch_on(TeamId(0), 11, TouchKind::Restart, 1_000);
        let attr = attribute_goal(TeamId(0), [Some(restart), None, None], 1_000);
        assert_eq!(attr.scorer, None);
        assert_eq!(attr.assister, None);
        assert!(!attr.own_goal);
    }

    /// `goal` events carry scorer / assister / `own_goal` / score on the
    /// wire (PRD §9.2), with the scoring team on the envelope.
    #[test]
    fn goal_event_payload_round_trip() {
        let mut log = EventLog::new();
        let clock = sample_clock(1, 4_500, 0);
        let scorer = touch_on(TeamId(0), 11, sim_components::TouchKind::Play, 4_500);
        let assist = touch_on(TeamId(0), 22, sim_components::TouchKind::Play, 4_400);
        let attribution = attribute_goal(TeamId(0), [Some(scorer), Some(assist), None], 4_500);
        log.append(build_goal_event(
            log.next_seq,
            4_500,
            &clock,
            TeamId(0),
            attribution,
            (1, 0),
        ));
        let MatchEvent::Goal { envelope, payload } = &log.events[0] else {
            panic!("expected goal event");
        };
        assert_eq!(
            envelope.team,
            Some(TeamId(0)),
            "envelope team = scoring team"
        );
        assert_eq!(payload.scorer, scorer.player_id);
        assert_eq!(payload.assister, assist.player_id);
        assert!(!payload.own_goal);
        assert_eq!(payload.score, (1, 0), "running score after the goal");

        let json = serde_json::to_string(&log.events[0]).expect("serialise");
        assert!(json.contains("\"type\":\"goal\""));
        assert!(json.contains("\"own_goal\":false"));
        assert!(json.contains("\"score\":[1,0]"));
        let back: MatchEvent = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back, log.events[0], "goal event must round-trip the wire");
    }

    /// `shot` events carry shooter / `on_target` / outcome (PRD §9.2).
    #[test]
    fn shot_event_payload_round_trip() {
        let mut log = EventLog::new();
        let clock = sample_clock(1, 5_000, 0);
        log.append(build_shot_event(
            log.next_seq,
            5_000,
            &clock,
            TeamId(1),
            PlayerId::from_bits(77),
            true,
            ShotOutcome::Saved,
        ));
        let MatchEvent::Shot { envelope, payload } = &log.events[0] else {
            panic!("expected shot event");
        };
        assert_eq!(envelope.team, Some(TeamId(1)));
        assert_eq!(payload.shooter, PlayerId::from_bits(77));
        assert!(payload.on_target);
        assert_eq!(payload.outcome, ShotOutcome::Saved);

        let json = serde_json::to_string(&log.events[0]).expect("serialise");
        assert!(json.contains("\"type\":\"shot\""));
        assert!(json.contains("\"outcome\":\"saved\""));
        let back: MatchEvent = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back, log.events[0]);
    }

    /// `save` events carry keeper / shooter / kind (PRD §9.2).
    #[test]
    fn save_event_payload_round_trip() {
        let mut log = EventLog::new();
        let clock = sample_clock(2, 200_000, 0);
        log.append(build_save_event(
            log.next_seq,
            200_000,
            &clock,
            TeamId(0),
            PlayerId::from_bits(1),
            PlayerId::from_bits(77),
            SaveKind::Parry,
        ));
        let MatchEvent::Save { envelope, payload } = &log.events[0] else {
            panic!("expected save event");
        };
        assert_eq!(envelope.team, Some(TeamId(0)), "envelope team = keeper team");
        assert_eq!(payload.kind, SaveKind::Parry);

        let json = serde_json::to_string(&log.events[0]).expect("serialise");
        assert!(json.contains("\"type\":\"save\""));
        assert!(json.contains("\"kind\":\"parry\""));
        let back: MatchEvent = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back, log.events[0]);
    }

    /// `kickoff` events carry taker / reason / attack directions on
    /// half starts (PRD §9.2).
    #[test]
    fn kickoff_event_payload_round_trip() {
        let mut log = EventLog::new();
        let clock = sample_clock(1, 0, 0);
        log.append(build_kickoff_event(
            log.next_seq,
            0,
            &clock,
            TeamId(0),
            Some(PlayerId::from_bits(9)),
            KickoffReason::MatchStart,
            Some(1),
            Some(-1),
        ));
        let MatchEvent::Kickoff { envelope, payload } = &log.events[0] else {
            panic!("expected kickoff event");
        };
        assert_eq!(envelope.team, Some(TeamId(0)));
        assert_eq!(payload.reason, KickoffReason::MatchStart);
        assert_eq!((payload.attack_home, payload.attack_away), (Some(1), Some(-1)));

        let json = serde_json::to_string(&log.events[0]).expect("serialise");
        assert!(json.contains("\"type\":\"kickoff\""));
        assert!(json.contains("\"reason\":\"match_start\""));
        let back: MatchEvent = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back, log.events[0]);
    }

    /// `half_time` carries the running score; `full_time` carries
    /// score, event count, state hash and added time (ticket 10).
    #[test]
    fn half_and_full_time_events_round_trip() {
        let mut log = EventLog::new();
        let clock = sample_clock(1, 162_000, 0);
        log.append(build_half_time_event(log.next_seq, 162_000, &clock, (2, 1)));
        let clock2 = sample_clock(2, 162_000, 0);
        // `event_count` includes the `full_time` event itself.
        log.append(build_full_time_event(
            log.next_seq,
            324_018,
            &clock2,
            (3, 1),
            2,
            0xDEAD_BEEF,
            (0, 0),
        ));
        assert_eq!(log.len(), 2);
        let MatchEvent::HalfTime { payload, .. } = &log.events[0] else {
            panic!("expected half_time event");
        };
        assert_eq!(payload.score, (2, 1));
        let MatchEvent::FullTime { envelope, payload } = &log.events[1] else {
            panic!("expected full_time event");
        };
        assert_eq!(envelope.team, None, "no team is credited");
        assert_eq!(payload.score, (3, 1));
        assert_eq!(payload.event_count, 2);
        assert_eq!(payload.state_hash, 0xDEAD_BEEF);

        for event in &log.events {
            let json = serde_json::to_string(event).expect("serialise");
            let back: MatchEvent = serde_json::from_str(&json).expect("deserialise");
            assert_eq!(&back, event);
        }
    }
}
