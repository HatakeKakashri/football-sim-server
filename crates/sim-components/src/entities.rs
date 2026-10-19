//! Entity types: teams, managers, referees, and related records.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

/// Stable player identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TeamId(pub u8);
/// Manager component: weighted decision table and cooldown.
#[derive(Component, Debug, Clone)]
pub struct Manager {
    pub decision_table: WeightedDecisionTable,
    pub last_decision_tick: u64,
    pub decision_cooldown: u64,
}

/// Weighted decision table for the manager.
#[derive(Debug, Clone)]
pub struct WeightedDecisionTable {
    pub factors: Vec<DecisionFactor>,
}

/// One weighted factor in the manager's decision table.
#[derive(Debug, Clone)]
pub struct DecisionFactor {
    pub name: String,
    pub weight: f32,
}

/// Referee component: stoppage events and cards.
#[derive(Component, Debug, Clone)]
pub struct Referee {
    pub stoppage_events: Vec<StoppageEvent>,
    pub cards: Vec<Card>,
}

/// A stoppage event recorded by the referee.
#[derive(Debug, Clone)]
pub struct StoppageEvent {
    pub tick: u64,
    pub reason: String,
}

/// A card shown to a player.
#[derive(Debug, Clone)]
pub struct Card {
    pub player: Entity,
    pub color: super::CardColor,
    pub tick: u64,
}
