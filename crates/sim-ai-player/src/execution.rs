//! Execution: turns the chosen `Intent` into actual velocity / ball motion.

use bevy_ecs::prelude::*;
use sim_components::{
    ActionIntent, Ball, Intent, NearbyEntity, PerceptionSnapshot, Player, Stamina, Skill, Velocity,
};
use sim_math::Vec2;
use sim_physics::SimRng;

/// Speed imparted to the ball when a possessing player shoots or kicks it.
///
/// Demo-scope simplification (see `kick_execution_system`): a flat
/// speed per action type, not scaled by player skill/power.
pub const KICK_SPEED_SHOT: f32 = 22.0;
pub const KICK_SPEED_PASS: f32 = 14.0;

/// Returns the `NearbyEntity` with the smallest `distance` field, or
/// `None` if the slice is empty. NaN distances are treated as equal
/// (consistent with the previous inline `unwrap_or(Ordering::Equal)`
/// pattern).
#[allow(
    dead_code,
    reason = "After F8 the production callers live in `sim_components::intent_dispatch`; this helper remains for direct unit tests in this module"
)]
fn closest_by_distance(items: &[NearbyEntity]) -> Option<&NearbyEntity> {
    items.iter().min_by(|a, b| {
        a.distance
            .partial_cmp(&b.distance)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

/// Resolve an intent to a target point and an action-specific speed,
/// then return the unit-direction × speed vector. Target entities are
/// resolved from the player's perception snapshot (`nearby_teammates` /
/// `nearby_opponents`), which carries their world position relative to the
/// player at perception time. The ball position is read from the perception
/// snapshot directly (the perception system writes it every tick).
///
/// Phase F (F8): the per-variant match moved to
/// `Intent::steer_target(&self, &PerceptionSnapshot)` in
/// `sim_components::intent_dispatch`. This function is now a thin wrapper
/// that consumes the (target, speed) pair and converts it to a velocity
/// vector. The `> 0.1` distance deadband is preserved.
fn steer(perception: &PerceptionSnapshot, intent: &Intent) -> Vec2 {
    let Some((target, speed)) = intent.steer_target(perception) else {
        return Vec2::zero();
    };
    let direction = target - perception.self_position;
    let distance = direction.length();
    if distance > 0.1 {
        (direction / distance) * speed
    } else {
        Vec2::zero()
    }
}

pub fn player_action_execution_system(
    mut query: Query<(
        Entity,
        &mut Player,
        &mut Velocity,
        &Stamina,
        &Skill,
        &PerceptionSnapshot,
    )>,
    mut rng: ResMut<SimRng>,
) {
    // Phase 2: real steering for every action. Each arm turns the abstract
    // intent into a constant-velocity steer toward (or away from) the
    // relevant world point. Acceleration is intentionally ignored — Phase 2
    // only proves the utility-AI loop drives meaningful state changes.
    //
    // Entity-target lookups use the player's own perception snapshot rather
    // than a separate world query — keeps the system signature simple and
    // avoids the Position/Velocity conflict that arises from querying both
    // the player and the ball.
    //
    // Phase 3: stochastic outcomes for tackles, passes, and shots using RNG.
    // DETERMINISM: collect entities first and sort by Entity ID to ensure
    // stable RNG draw order across processes (spec §7, rule #22).
    let mut entities: Vec<Entity> = query.iter().map(|(e, _, _, _, _, _)| e).collect();
    entities.sort_by_key(|e| e.to_bits());
    for entity in entities {
        let Ok((_entity, player, mut velocity, stamina, skill, perception)) = query.get_mut(entity)
        else {
            continue;
        };
        let Some(intent) = player.intent else {
            velocity.0 = Vec2::zero();
            continue;
        };

        // Apply stochastic modifiers based on action type
        let mut speed_modifier = 1.0;
        let mut direction_modifier = Vec2::zero();

        match &intent {
            Intent::Action(ActionIntent::Tackle(target)) => {
                // Tackle resolution: higher skill = higher success probability
                // Roll against skill-based probability
                let tackle_success_prob = skill.0.clamp(0.3, 0.9);
                let roll: f32 = rng.gen_f32();
                if roll > tackle_success_prob {
                    // Tackle fails - player stumbles, moves slower
                    speed_modifier = 0.3;
                }
                // Store target for potential later use
                let _ = target;
            }
            Intent::Action(ActionIntent::PassTo) => {
                // Pass completion: probability based on distance and skill
                let pass_distance = perception
                    .nearby_teammates
                    .first()
                    .map_or(15.0, |t| t.distance);
                // Longer passes = lower completion probability
                let pass_prob = (1.0 - (pass_distance / 50.0).min(0.8)) * skill.0;
                let roll: f32 = rng.gen_f32();
                if roll > pass_prob {
                    // Pass goes awry - add random deviation
                    speed_modifier = 0.7;
                    direction_modifier =
                        Vec2::new(rng.gen_range_f32(-2.0, 2.0), rng.gen_range_f32(-2.0, 2.0));
                }
            }
            Intent::Action(ActionIntent::ShootAtGoal(_)) => {
                // Shot accuracy: skill and stamina affect accuracy
                let accuracy = skill.0 * stamina.0.mul_add(0.5, 0.5);
                let roll: f32 = rng.gen_f32();
                if roll > accuracy {
                    // Shot misses - add deviation
                    speed_modifier = 0.8;
                    direction_modifier =
                        Vec2::new(rng.gen_range_f32(-3.0, 3.0), rng.gen_range_f32(-2.0, 2.0));
                }
            }
            Intent::Movement(_)
            | Intent::Action(ActionIntent::MarkOpponent(_) | ActionIntent::Press(_)) => {}
        }

        let new_velocity = steer(perception, &intent);
        velocity.0 = new_velocity * speed_modifier + direction_modifier;
    }
}

/// Turns a possessing player's intent into actual ball velocity.
///
/// Before this system existed, nothing in the simulation ever wrote to the
/// ball's `Velocity` component: players could gain possession via
/// `possession_resolution_system`'s proximity check, but the ball itself
/// never moved as a result of an action, so "kick/pass" was purely
/// notional. This system closes that gap for the current demo scope only —
/// it is intentionally not real ball control:
/// - `ShootAtGoal`: kicks toward the already-resolved shot target.
/// - `PassTo`: kicks toward the nearest teammate (if any); the
///   teammate lookup is encapsulated in `Intent::kick`. No
///   lead/interception modeling, no pass-completion mechanic beyond
///   that — out of scope for "just movement and kick/pass".
/// - `ChaseBall` / `Tackle` / `Press`: continues the ball in the player's
///   current heading.
///
/// Runs after `player_action_execution_system` in the same `Execution` set
/// (so it sees this tick's resolved intent and velocity) and before the
/// `Physics` set integrates the ball's velocity into its position.
///
/// Clears `ball.possessor` on every kick so the ball goes properly loose
/// (`BallState::InFlight`) rather than getting stuck reporting `Possessed`
/// by a player who has since moved away: `ball_physics_system` only
/// downgrades state to `Possessed`/`Free` from velocity + possessor, and
/// `possession_resolution_system` only re-resolves possession while
/// `ball.state == BallState::Free`. Without this, a kicked ball that
/// decelerates to rest before another player reaches it would silently
/// re-freeze as "possessed" by the original kicker.
pub fn kick_execution_system(
    player_query: Query<(Entity, &Player, &PerceptionSnapshot, &Velocity)>,
    mut ball: ResMut<Ball>,
) {
    let Some(possessor) = ball.possessor else {
        return;
    };
    let Some((_, _, perception, player_vel)) =
        player_query.iter().find(|(e, _, _, _)| *e == possessor)
    else {
        return;
    };
    let Some(intent) = player_query
        .iter()
        .find(|(e, _, _, _)| *e == possessor)
        .and_then(|(_, p, _, _)| p.intent)
    else {
        return;
    };

    // Phase F (F8): the per-variant match moved to `Intent::kick` in
    // `sim_components::intent_dispatch`. The speed multiplier depends on
    // whether the kick is a shot (uses `KICK_SPEED_SHOT`) or a pass /
    // velocity-continuation (uses `KICK_SPEED_PASS`).
    let Some(kick_dir) = intent.kick(perception, player_vel.0) else {
        return;
    };
    let kick_speed = match intent {
        Intent::Action(ActionIntent::ShootAtGoal(_)) => KICK_SPEED_SHOT,
        _ => KICK_SPEED_PASS,
    };

    // Preserve the original `dir.length() > 0.01` guard: if the
    // computed direction is effectively zero (e.g. a `ShootAtGoal`
    // target equal to the player, or a stationary player kicking on
    // `ChaseBall`/`Tackle`/`Press`), skip the kick entirely so we
    // don't touch `ball.kick_velocity` or clear `ball.possessor`.
    if kick_dir.length() > 0.01 {
        ball.kick_velocity = Some(kick_dir * kick_speed);
        ball.possessor = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closest_by_distance_empty() {
        let empty: Vec<NearbyEntity> = Vec::new();
        assert!(closest_by_distance(&empty).is_none());
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "exact equality against a literal is the point of this smoke check"
    )]
    fn closest_by_distance_single() {
        let one = vec![NearbyEntity {
            entity: bevy_ecs::prelude::Entity::from_raw(1),
            distance: 5.0,
            relative_position: Vec2::zero(),
        }];
        assert_eq!(closest_by_distance(&one).unwrap().distance, 5.0);
    }

    #[test]
    fn closest_by_distance_picks_smallest() {
        let v = vec![
            NearbyEntity {
                entity: bevy_ecs::prelude::Entity::from_raw(1),
                distance: 10.0,
                relative_position: Vec2::zero(),
            },
            NearbyEntity {
                entity: bevy_ecs::prelude::Entity::from_raw(2),
                distance: 3.0,
                relative_position: Vec2::zero(),
            },
            NearbyEntity {
                entity: bevy_ecs::prelude::Entity::from_raw(3),
                distance: 7.0,
                relative_position: Vec2::zero(),
            },
        ];
        assert_eq!(
            closest_by_distance(&v).unwrap().entity,
            bevy_ecs::prelude::Entity::from_raw(2)
        );
    }
}
