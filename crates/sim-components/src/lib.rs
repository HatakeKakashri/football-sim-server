//! `sim-components`: shared domain types for the football-sim-server
//! workspace.
//!
//! Public surface: all Components, Resources, enums, and pure-domain
//! helpers. Every item is re-exported from its focused submodule.

pub mod attribute;
pub mod attribute_groups;
pub mod ball_touch;
pub mod command;
pub mod components;
pub mod enums;
pub mod event_kinds;
pub mod entities;
pub mod fixture;
pub mod formation_slot;
pub mod goalkeeper_mechanics;
pub mod intent;
pub mod intent_dispatch;
pub mod outfield_mechanics;
pub mod perception;
pub mod physical_mechanics;
pub mod position;
pub mod profile;
pub mod quality;
pub mod resources;
pub mod shot_pipeline;
pub mod tactic_params;
pub mod time;

#[cfg(test)]
mod attribute_tests;

pub use attribute::Attribute;
pub use attribute_groups::{Goalkeeping, OutfieldAttack, OutfieldDefence, PhysicalMental};
pub use ball_touch::{
    BallAirborne, BallOutOfBoundsType, BallState, FoulType, LastTouch, PendingBallTouches,
    PlayerId, RuleEvent, TouchKind,
};
pub use command::{CommandError, ManagerCommand};
pub use components::{
    BallMarker, Player, PlayerIdentity, Position, RoleComponent, Skill, Stamina, TackleRecovery,
    Team, TeamIdComponent, Velocity,
};
pub use enums::{
    CardColor, Formation, MatchState, Mentality, Role, StrengthMode, Tactic,
};
pub use entities::{
    Card, DecisionFactor, Manager, Referee, StoppageEvent, TeamId, WeightedDecisionTable,
};
pub use event_kinds::{KickoffReason, SaveKind, ShotOutcome};
pub use formation_slot::{FormationSlot, HomeSlot, TrainedPositions};
pub use intent::{ActionIntent, Intent, IntentKind, MovementIntent};
pub use perception::{NearbyEntity, PerceptionSnapshot, PitchBounds};
pub use profile::{
    AttributeError, PlayerProfile, ProfileKind, ATTRIBUTE_COUNT, ATTRIBUTE_MAX, ATTRIBUTE_MIN,
};
pub use position::{PosSlot, PositionError};
pub use quality::quality;
pub use resources::{Ball, Match, MatchClock};
pub use tactic_params::{TeamSetup, TacticParameters};
pub use shot_pipeline::{
    finishing_conversion, keeper_positioning_error, keeper_save_chance, resolve_shot_outcome,
    rush_out_quality, save_kind_from_speed, save_probability, shot_accuracy, shot_power,
    SAVE_CATCH_MAX_SPEED_MPS, ShotResolution,
};
pub use outfield_mechanics::{
    aerial_duel_attacker_win_probability, bravery_contest_weight, cross_accuracy,
    dribble_retention_probability, header_accuracy, header_power, key_pass_weight,
    mark_distance_error, pass_error_spread, target_position_error, tackle_success_probability,
};
pub use goalkeeper_mechanics::{
    agility_dive_reach, aerial_reach_radius, claim_propensity, communication_defender_error_factor,
    concentration_save_lapse_probability, distribution_weight, kicking_accuracy, kicking_range,
    punching_clearance_probability, rush_out_propensity, throwing_accuracy, throwing_range,
};
pub use physical_mechanics::{
    aggression_stamina_factor, fitness_drain_factor, held_ball_retention_probability, press_frequency,
    press_trigger_distance, sprint_speed, tackle_propensity,
};
pub use sim_math::{effective_attribute, fit_multiplier, QualityGroup, QualityWeights};
