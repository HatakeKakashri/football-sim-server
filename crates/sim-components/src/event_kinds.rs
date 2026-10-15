//! Event-kind enums for the structured match event log (PRD §9.2).
//!
//! Split out of `ball_touch.rs` so touch registration and event
//! vocabulary stay in separate modules. These enums are shared by the
//! rule-side writers (`sim-rules`, which cannot depend on `sim-core`)
//! and the event builders (`sim-core::simulation::events`).

use serde::{Deserialize, Serialize};

/// Why a kickoff was taken (PRD §9.2 `kickoff.reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KickoffReason {
    MatchStart,
    SecondHalf,
    AfterGoal,
}

impl KickoffReason {
    /// Stable string form for the wire shape (`reason` field).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MatchStart => "match_start",
            Self::SecondHalf => "second_half",
            Self::AfterGoal => "after_goal",
        }
    }
}

/// How a shot resolved (PRD §9.2 `shot.outcome`).
///
/// The `Post` variant has no producer yet (no post geometry exists);
/// it is part of the schema from the start so Phase 4 mechanics
/// (ticket 20) only add a writer, not a wire change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShotOutcome {
    Goal,
    Saved,
    Blocked,
    OffTarget,
    Post,
}

impl ShotOutcome {
    /// Stable string form for the wire shape (`outcome` field).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Goal => "goal",
            Self::Saved => "saved",
            Self::Blocked => "blocked",
            Self::OffTarget => "off_target",
            Self::Post => "post",
        }
    }
}

/// How a goalkeeper dealt with a shot (PRD §9.2 `save.kind`).
///
/// `Punch` is reserved for cross clearances (Phase 4 aerial play);
/// open-play shot saves resolve to `Catch` or `Parry` by ball speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveKind {
    Catch,
    Parry,
    Punch,
}

impl SaveKind {
    /// Stable string form for the wire shape (`kind` field).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Catch => "catch",
            Self::Parry => "parry",
            Self::Punch => "punch",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kickoff_reason_wire_names_match_prd() {
        assert_eq!(KickoffReason::MatchStart.as_str(), "match_start");
        assert_eq!(KickoffReason::SecondHalf.as_str(), "second_half");
        assert_eq!(KickoffReason::AfterGoal.as_str(), "after_goal");
    }

    #[test]
    fn shot_outcome_wire_names_match_prd() {
        assert_eq!(ShotOutcome::Goal.as_str(), "goal");
        assert_eq!(ShotOutcome::Saved.as_str(), "saved");
        assert_eq!(ShotOutcome::Blocked.as_str(), "blocked");
        assert_eq!(ShotOutcome::OffTarget.as_str(), "off_target");
        assert_eq!(ShotOutcome::Post.as_str(), "post");
    }

    #[test]
    fn save_kind_wire_names_match_prd() {
        assert_eq!(SaveKind::Catch.as_str(), "catch");
        assert_eq!(SaveKind::Parry.as_str(), "parry");
        assert_eq!(SaveKind::Punch.as_str(), "punch");
    }
}
