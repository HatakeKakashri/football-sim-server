//! Scratch collection + emission of per-decision traces.
//!
//! The scoring loop in `player_decision_system` copies the values it already
//! computes into a reusable scratch buffer (only when tracing is gated on).
//! Once the winner is known, spans/events are emitted in properly nested
//! order: decision → action → consideration. Scoring itself is untouched.

use bevy_ecs::prelude::Entity;
use sim_components::{Role, TeamId};
use sim_telemetry::emit::{
    self, TID_DECISION, Track, action_span, decision_span, player_pid, player_process_name,
};

/// One evaluated consideration, copied from the scoring loop.
pub struct ConsiderationTrace {
    pub(crate) name: &'static str,
    pub(crate) raw: f32,
    pub(crate) curve: &'static str,
    pub(crate) weight: f32,
    pub(crate) score: f32,
}

struct ActionTrace {
    kind: &'static str,
    aggregate: f32,
    first: usize,
    end: usize,
}

/// Reusable per-system buffer (held in a bevy `Local`, never allocated per tick).
/// Opaque: only the decision system fills and emits it.
#[derive(Default)]
pub struct DecisionTraceScratch {
    actions: Vec<ActionTrace>,
    considerations: Vec<ConsiderationTrace>,
}

impl DecisionTraceScratch {
    pub(crate) fn clear(&mut self) {
        self.actions.clear();
        self.considerations.clear();
    }

    pub(crate) fn begin_action(&mut self, kind: &'static str) {
        let at = self.considerations.len();
        self.actions.push(ActionTrace {
            kind,
            aggregate: 0.0,
            first: at,
            end: at,
        });
    }

    pub(crate) fn push_consideration(&mut self, trace: ConsiderationTrace) {
        self.considerations.push(trace);
    }

    /// Record the action's final (post-hysteresis) aggregate score.
    pub(crate) fn finish_action(&mut self, aggregate: f32) {
        let end = self.considerations.len();
        if let Some(action) = self.actions.last_mut() {
            action.aggregate = aggregate;
            action.end = end;
        }
    }

    /// Index of the action whose aggregate equals the winning score. The
    /// scorer picks the first maximum (strict `>`), so the first bit-equal
    /// aggregate is the winner.
    fn winner(&self, best_score: Option<f32>) -> Option<usize> {
        let best = best_score?;
        self.actions
            .iter()
            .position(|a| a.aggregate.to_bits() == best.to_bits())
    }

    /// Emit the decision span tree for one player.
    pub(crate) fn emit(
        &self,
        entity: Entity,
        team: TeamId,
        role: Role,
        tick: u64,
        best_score: Option<f32>,
    ) {
        let index = entity.index();
        let process = player_process_name(team.0, index, &role);
        let track = Track {
            pid: player_pid(index),
            tid: TID_DECISION,
            process: &process,
            thread: "Decision",
        };
        let root = decision_span(track, tick);
        let winner = self.winner(best_score);
        for (i, action) in self.actions.iter().enumerate() {
            let span = action_span(&root, action.kind, winner == Some(i), action.aggregate);
            for c in &self.considerations[action.first..action.end] {
                emit::consideration(&span, c.name, c.raw, c.curve, c.weight, c.score);
            }
        }
    }
}
