//! Decision: each tick, a subset of players re-evaluates their
//! `UtilityBrain` and writes a new `Intent` to the `Player` component.

use bevy_ecs::prelude::*;
use sim_ai_core::geometric_mean;
use sim_components::{
    ActionIntent, Ball, Intent, MatchClock, PerceptionSnapshot, Player,
    RoleComponent, Skill, Stamina, TeamIdComponent,
};
use sim_physics::PitchControlGrid;
use sim_telemetry::TraceGate;

use crate::brain::{UtilityBrain, intent_kind};
use crate::decision_trace::{ConsiderationTrace, DecisionTraceScratch};
use crate::{
    ConsiderationContext, DECISION_CADENCE_TICKS, DecisionEvaluationCount, player_stagger_slot,
};

pub const fn consideration_scoring_system(_query: Query<(&mut Player, &Stamina, &Skill)>) {
    // Phase 2: real consideration scoring runs INSIDE `player_decision_system`
    // (so it can be gated on the per-player evaluation cadence without doing
    // redundant work). This system now exists as a no-op stub retained for
    // schedule compatibility. The defensive intent-fallback that previously
    // lived here has been removed because seeding intent to HoldPosition
    // would let the hysteresis bonus lock players into the fallback action
    // indefinitely.
}

/// Phase 2: replace the stub with real per-player Utility AI scoring.
///
/// For each player with a `UtilityBrain`:
/// 1. For every action in the brain, evaluate each consideration's raw input
///    from the player's perception snapshot, feed it through the
///    `ResponseCurve`, weight-blend via `geometric_mean`.
/// 2. Pick the highest-scoring action; add the `hysteresis` bonus if it
///    matches the player's currently-active intent.
/// 3. Commit the new intent to the Player.
///
/// Phase D (`CODEBASE_REVIEW` §7): per-player decision cadence. A player is
/// evaluated only when `(elapsed_ticks % DECISION_CADENCE_TICKS) ==
/// player_stagger_slot(entity)`. This decouples decision cost from the
/// 60 Hz physics tick (spec §10: ~10 Hz decision cadence). The
/// stagger is by `entity.to_bits()`, which spreads 22 players across
/// the 6 slots without any team-partition bias (spec §10 "individual
/// players can still be time-sliced … that staggering has no
/// team-level bias").
#[allow(
    clippy::too_many_lines,
    reason = "single linear per-player scoring pass over the brain's actions; splitting it would scatter the action-ranking logic across helpers and obscure the hysteresis / scoring flow"
)]
#[allow(clippy::type_complexity, reason = "Bevy Query signature")]
pub fn player_decision_system(
    mut query: Query<(
        Entity,
        &mut Player,
        &UtilityBrain,
        &PerceptionSnapshot,
        &Stamina,
        &Skill,
        &RoleComponent,
        &TeamIdComponent,
    )>,
    pitch_control: Res<PitchControlGrid>,
    // Phase F follow-up: MatchClock is a Resource (spec §3). Read it
    // directly via `Res<MatchClock>` instead of the previous
    // `Query<&MatchClock>` (which used a silent `.iter().next().map_or(0)`
    // fallback that masked missing-clock bugs).
    clock_res: Res<MatchClock>,
    mut eval_count: ResMut<DecisionEvaluationCount>,
    // Telemetry (feature 002): `None`/all-false when tracing is disabled, in
    // which case the scratch buffer is never touched and nothing is emitted.
    gate: Option<Res<TraceGate>>,
    mut scratch: Local<DecisionTraceScratch>,
    ball_res: Res<Ball>,
) {
    let tracing_decisions = gate.is_some_and(|g| g.decisions());
    let grid = pitch_control.into_inner();
    // Snapshot the clock once per system run. Reading from `Res<MatchClock>`
    // is a single pointer indirection — faster than the previous
    // `Query<&MatchClock>::iter().next()` lookup and free of the silent
    // default-on-missing fallback.
    let elapsed_ticks = clock_res.elapsed_ticks;
    let phase_slot = elapsed_ticks % DECISION_CADENCE_TICKS;

    for (entity, mut player, utility_brain, perception, stamina, skill, role, team_id) in &mut query
    {
        // Phase D: cadence guard. Skip if not on this player's slot.
        if player_stagger_slot(entity) != phase_slot {
            continue;
        }
        // Player passed the guard — count this evaluation.
        eval_count.0 += 1;

        let is_possessor = ball_res.possessor == Some(entity);

        // Evaluate each action.
        let mut best_action: Option<(Intent, f32)> = None;
        if tracing_decisions {
            scratch.clear();
        }

        for action in &utility_brain.actions {
            let mut consideration_scores: Vec<f32> = Vec::new();
            if tracing_decisions {
                scratch.begin_action(intent_kind(&action.intent));
            }

            for consideration in &action.considerations {
                // Compute the raw input for this consideration from the
                // player's perception + world state. The exhaustive enum
                // replaces the old string-dispatch fallthrough.
                let ctx = ConsiderationContext {
                    perception,
                    intent: &action.intent,
                    stamina: stamina.0,
                    skill: skill.0,
                    grid: Some(grid),
                    team_id: Some(team_id.0),
                    is_possessor,
                };
                let raw = consideration.raw_input(&ctx);
                let score = consideration.curve().evaluate(raw).raw();
                consideration_scores.push(score);
                if tracing_decisions {
                    scratch.push_consideration(ConsiderationTrace {
                        name: consideration.name(),
                        raw,
                        curve: consideration.curve().name(),
                        weight: consideration.weight(),
                        score,
                    });
                }
            }

            let is_kicking_action = matches!(
                action.intent,
                Intent::Action(ActionIntent::PassTo | ActionIntent::ShootAtGoal(_) | ActionIntent::DribbleForward)
            );

            let action_permitted = if is_possessor {
                is_kicking_action
            } else if ball_res.possessor.is_none() {
                !is_kicking_action
                    && !matches!(
                        action.intent,
                        Intent::Action(
                            ActionIntent::Tackle(_)
                                | ActionIntent::Press(_)
                                | ActionIntent::MarkOpponent(_)
                        )
                    )
            } else {
                !is_kicking_action
            };

            if consideration_scores.is_empty() {
                // No considerations ⇒ fixed baseline.
                let baseline = if action_permitted { 0.6 } else { 0.0 };
                if tracing_decisions {
                    scratch.finish_action(baseline);
                }
                if baseline > 0.0 {
                    if let Some((_, best)) = best_action {
                        if baseline > best {
                            best_action = Some((action.intent, baseline));
                        }
                    } else {
                        best_action = Some((action.intent, baseline));
                    }
                }
                continue;
            }

            // `geometric_mean` already clamps each input to ≥ 1e-4 inside,
            // so a 0 score collapses the aggregate toward 0 but doesn't
            // produce literal 0 (preserving numerical stability for the
            // product).
            let mut aggregate = if action_permitted {
                geometric_mean(&consideration_scores)
            } else {
                0.0
            };

            // Apply hysteresis: if this action's intent type matches the
            // player's currently-active intent, add the bonus.
            if action_permitted
                && let Some(current) = &player.intent
                && intent_kind(current) == intent_kind(&action.intent)
            {
                aggregate += utility_brain.hysteresis;
            }
            if tracing_decisions {
                scratch.finish_action(aggregate);
            }

            if aggregate > 0.0 {
                if let Some((_, best)) = best_action {
                    if aggregate > best {
                        best_action = Some((action.intent, aggregate));
                    }
                } else {
                    best_action = Some((action.intent, aggregate));
                }
            }
        }

        if tracing_decisions {
            scratch.emit(
                entity,
                team_id.0,
                role.0,
                elapsed_ticks,
                best_action.map(|(_, score)| score),
            );
        }

        if let Some((intent, _score)) = best_action {
            // Resolve sentinels from the player's perception so shots aim
            // at the team's actual goal and default movement targets the
            // player's directional home slot, not template sentinels.
            let resolved = match intent {
                sim_components::Intent::Action(
                    sim_components::ActionIntent::ShootAtGoal(_template_target),
                ) => sim_components::Intent::Action(
                    sim_components::ActionIntent::ShootAtGoal(perception.goal_position),
                ),
                sim_components::Intent::Movement(
                    sim_components::MovementIntent::MoveToPosition(target),
                ) => {
                    let dest = sim_components::MovementIntent::resolve_target(target, perception);
                    sim_components::Intent::Movement(
                        sim_components::MovementIntent::MoveToPosition(dest),
                    )
                }
                other => other,
            };
            player.intent = Some(resolved);
        } else if player.intent.is_none() {
            player.intent = Some(sim_components::Intent::Movement(
                sim_components::MovementIntent::HoldPosition,
            ));
        }
    }
}
