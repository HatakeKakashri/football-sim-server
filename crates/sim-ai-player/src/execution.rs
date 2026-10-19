//! Player action execution: converts a resolved `Intent` into actual
//! player velocity, stamina drain, and rule signals (tackle attempts).
//!
//! Per-entity steering math lives in `steering`; kick execution lives in
//! `kicking`; tackle recovery timers live in `recovery`.

use bevy_ecs::prelude::*;
use sim_components::{
    ActionIntent, Attribute, Ball, FormationSlot, Intent, PerceptionSnapshot, Player,
    PlayerProfile, Skill, Stamina, TackleRecovery, Team, TrainedPositions, Velocity,
    aggression_stamina_factor, fitness_drain_factor,
};
use sim_components::position::fit_multiplier;
use sim_math::{Tuning, Vec2};
use sim_physics::{PLAYER_MAX_ACCEL_PER_TICK, SimRng};
use sim_rules::{PendingRuleOutcome, TackleAttempt};

use crate::steering::{apply_acceleration_limit, steer};

/// Per-entity action execution: for each player with an `Intent`, convert
/// it into velocity (steering + arrival slow-down + accel cap), apply
/// stamina drain for costful actions, resolve stochastic tackle outcomes,
/// and write tackle attempts to `PendingRuleOutcome.tackle`.
///
/// Runs in `SimulationSet::Execution`, after `player_decision_system`
/// (which sets `Intent`) and before `kick_execution_system` (which
/// consumes resolved intents for ball kicks).
#[allow(
    clippy::too_long_first_doc_paragraph,
    reason = "single-paragraph contract description"
)]
#[allow(
    clippy::type_complexity,
    reason = "Bevy Query signature: the per-entity query carries the player state plus optional profile and recovery marker"
)]
#[allow(
    clippy::too_many_lines,
    reason = "tackle and pass stochastic resolution plus recovery and stamina bookkeeping; splitting would scatter the per-entity action flow"
)]
pub fn player_action_execution_system(
    mut query: Query<(
        Entity,
        &mut Player,
        &mut Velocity,
        &mut Stamina,
        &Skill,
        Option<&PlayerProfile>,
        Option<&TackleRecovery>,
        &PerceptionSnapshot,
        &FormationSlot,
        Option<&TrainedPositions>,
    )>,
    mut rng: ResMut<SimRng>,
    mut commands: Commands,
    ball: Res<Ball>,
    tuning: Res<Tuning>,
    team_query: Query<&Team>,
    mut pending_rule_outcome: ResMut<PendingRuleOutcome>,
) {
    // The ball's current carrier, if any. A tackle is only an attempt
    // when there is a carrier to challenge.
    let carrier_entity = ball.possessor;

    let mut entities: Vec<Entity> = query.iter().map(|(e, _, _, _, _, _, _, _, _, _)| e).collect();
    entities.sort_by_key(|e| e.to_bits());
    for entity in entities {
        let Ok((
            _entity,
            player,
            mut velocity,
            mut stamina,
            skill,
            profile,
            tackle_recovery,
            perception,
            formation_slot,
            trained_positions,
        )) = query.get_mut(entity)
        else {
            continue;
        };
        let Some(intent) = player.intent else {
            velocity.0 = Vec2::zero();
            continue;
        };

        // Compute the per-player fit multiplier (PRD §6.5): a player
        // occupying a slot they are not trained for gets a penalty
        // (0.92 at distance 1, 0.75 at distance 2+). Players without a
        // `TrainedPositions` component (placeholder profile path) get
        // 1.00, preserving the old behaviour for non-fixture worlds.
        let fit_multiplier = trained_positions.map_or(1.0, |tp| {
            fit_multiplier(formation_slot.position, tp.as_slice(), &tuning)
        });

        // Capture the previous tick's velocity BEFORE we do any work.
        // This is the "prev" the acceleration cap will clamp the new
        // velocity against.
        let prev_velocity = velocity.0;

        // Apply stochastic modifiers based on action type
        let mut speed_modifier = 1.0;
        let mut direction_modifier = Vec2::zero();

        // --- Tackle outcome (pre-computed so the match arm below stays a
        // pure dispatch over modifiers). ---
        let mut tackle_success: Option<bool> = None;
        let mut tackle_carrier: Option<Entity> = None;
        let mut tackle_stamina_cost = 0.0f32;
        let mut tackle_recovery_ticks: Option<u64> = None;

        if let Intent::Action(ActionIntent::Tackle(_)) = intent {
            // A tackle needs a ball carrier to challenge; without one it
            // is just a closing run, not a tackle attempt.
            if let Some(carrier) = carrier_entity {
                let tackling = profile
                    .and_then(|p| p.get(Attribute::Tackling))
                    .unwrap_or(100);
                let prob = sim_components::tackle_success_probability(
                    tackling,
                    fit_multiplier,
                    &tuning,
                );
                let roll: f32 = rng.gen_f32();
                let success = roll <= prob;
                // Stamina cost of the attempt, scaled by Aggression and
                // Fitness (PRD §7.3). `aggression_stamina_factor` now
                // reads the `Aggression` response curve; `fitness_drain_factor`
                // reduces the cost for fitter players.
                let aggression = profile
                    .and_then(|p| p.get(Attribute::OutfieldAggression))
                    .unwrap_or(75);
                let fitness = profile
                    .and_then(|p| p.get(Attribute::OutfieldFitness))
                    .unwrap_or(75);
                let aggression_factor =
                    aggression_stamina_factor(aggression, fit_multiplier, &tuning);
                let fitness_factor = fitness_drain_factor(fitness, fit_multiplier, &tuning);
                tackle_stamina_cost = tuning.stamina_drain_per_action
                    * tuning.tackle_drain_multiplier
                    * aggression_factor
                    * fitness_factor;
                tackle_success = Some(success);
                tackle_carrier = Some(carrier);
                if !success {
                    // Tackle fails - the tackler stumbles and is out of
                    // position for the recovery window; the carrier skips past.
                    speed_modifier = 0.3;
                    tackle_recovery_ticks = Some(tuning.tackle_recovery_ticks);
                }
            }
        }

        match &intent {
            Intent::Action(ActionIntent::PassTo | ActionIntent::DribbleForward) => {
                // Pass/Dribble completion: probability based on distance and skill.
                // The pass error spread (Passing attribute) is applied to
                // the kick direction in `kick_execution_system`; this arm
                // keeps the legacy movement deviation for the kicker.
                let pass_distance = perception
                    .nearby_teammates
                    .first()
                    .map_or(15.0, |t| t.distance);
                // Longer passes/dribbles = lower completion probability
                let pass_prob = (1.0 - (pass_distance / 50.0).min(0.8)) * skill.0;
                let roll: f32 = rng.gen_f32();
                if roll > pass_prob {
                    // Pass/Dribble goes awry - add random deviation
                    speed_modifier = 0.7;
                    // Deviation values are in m/s, converted to m/tick
                    let dev = 2.0 / 60.0;
                    direction_modifier =
                        Vec2::new(rng.gen_range_f32(-dev, dev), rng.gen_range_f32(-dev, dev));
                }
            }
            Intent::Action(ActionIntent::ShootAtGoal(_)) => {
                // Shot accuracy: skill and stamina affect accuracy
                let accuracy = skill.0 * stamina.0.mul_add(0.5, 0.5);
                let roll: f32 = rng.gen_f32();
                if roll > accuracy {
                    // Shot misses - add deviation
                    // Deviation values are in m/s, converted to m/tick
                    speed_modifier = 0.8;
                    let dev_x = 3.0 / 60.0;
                    let dev_y = 2.0 / 60.0;
                    direction_modifier = Vec2::new(
                        rng.gen_range_f32(-dev_x, dev_x),
                        rng.gen_range_f32(-dev_y, dev_y),
                    );
                }
            }
            Intent::Movement(_)
            | Intent::Action(
                ActionIntent::MarkOpponent(_)
                | ActionIntent::Press(_)
                | ActionIntent::Tackle(_),
            ) => {
                // Mark / Press have no stochastic execution effect;
                // movement intents steer; the tackle outcome was
                // pre-computed above.
            }
        }

        // Recovering tacklers are out of position: move slower while the
        // recovery window runs (inserted on a failed tackle above; the
        // deferred command becomes visible from the next tick).
        if tackle_recovery.is_some_and(|r| r.ticks_remaining > 0) {
            speed_modifier *= 0.5;
        }

        let desired = steer(perception, &intent);
        let desired_with_stochastic = desired * speed_modifier + direction_modifier;
        velocity.0 = apply_acceleration_limit(
            prev_velocity,
            desired_with_stochastic,
            PLAYER_MAX_ACCEL_PER_TICK,
        );

        // Stamina (Fitness): recover every tick, then drain for every
        // costful action (running, pressing, tackling). Fitness lowers the
        // effective running drain through its response curve; press and
        // tackle carry fixed per-action costs (scaled by Aggression, a
        // placeholder 1.0 here; ticket 25 wires it to `Aggression`).
        stamina.0 = (stamina.0 + tuning.stamina_recovery_rate).clamp(0.0, 1.0);
        // Running drain: any movement tick spends stamina (scaled by
        // Fitness). Press and tackle carry their own fixed per-action
        // costs below.
        if matches!(intent, Intent::Movement(_)) {
            let fitness = profile
                .map_or(75, |p| p.get(Attribute::OutfieldFitness).unwrap_or(75));
            let drain_factor = fitness_drain_factor(fitness, fit_multiplier, &tuning);
            stamina.0 = tuning
                .stamina_drain_per_action
                .mul_add(-drain_factor, stamina.0)
                .clamp(0.0, 1.0);
        }
        // Ticket 25: `HighPress` raises the press stamina cost
        // (PRD §7.2) via `press_stamina_factor`, a pure numeric parameter
        // of the team's tactic.
        let press_stamina_factor = team_query
            .iter()
            .find(|team| team.id == player.team_id)
            .map_or(1.0, |team| {
                sim_components::TacticParameters::from_setup(team.mentality, team.tactic)
                    .press_stamina_factor
            });

        // Press stamina cost (scaled by Aggression and Fitness,
        // PRD §7.3).
        if matches!(intent, Intent::Action(ActionIntent::Press(_))) {
            let aggression = profile
                .map_or(75, |p| p.get(Attribute::OutfieldAggression).unwrap_or(75));
            let fitness = profile
                .map_or(75, |p| p.get(Attribute::OutfieldFitness).unwrap_or(75));
            let aggression_factor =
                aggression_stamina_factor(aggression, fit_multiplier, &tuning);
            let fitness_factor = fitness_drain_factor(fitness, fit_multiplier, &tuning);
            let press_cost = tuning.stamina_drain_per_action
                * tuning.press_drain_multiplier
                * aggression_factor
                * fitness_factor
                * press_stamina_factor;
            stamina.0 = (stamina.0 - press_cost).clamp(0.0, 1.0);
        }
        // Tackle stamina drain (scaled by Aggression, placeholder 1.0 here).
        if tackle_stamina_cost > 0.0 {
            stamina.0 = (stamina.0 - tackle_stamina_cost).clamp(0.0, 1.0);
        }

        // Record the tackle attempt so the Rules set can emit the
        // `tackle` event (`tackler`, `carrier`, `success`).
        if let (Some(success), Some(carrier)) = (tackle_success, tackle_carrier) {
            pending_rule_outcome.0.tackle = Some(TackleAttempt {
                tackler: entity,
                carrier,
                success,
            });
        }

        // Recovery window on a failed tackle. Inserted via a deferred
        // command, so it becomes visible to `tackle_recovery_system`
        // from the next tick.
        if let Some(ticks) = tackle_recovery_ticks {
            commands.entity(entity).insert(TackleRecovery { ticks_remaining: ticks });
        }
    }
}
