//! Execution: turns the chosen `Intent` into actual velocity / ball motion.

use std::sync::atomic::{AtomicU64, Ordering};

use bevy_ecs::prelude::*;
use sim_components::{
    ActionIntent, Ball, Intent, MatchClock, PerceptionSnapshot, Player, Stamina, Skill, Velocity,
};
use sim_math::Vec2;
use sim_physics::SimRng;
use sim_telemetry::TelemetryConfig;

/// Monotonic counter for `pass()` flow ids. Two passes share the same
/// process and could otherwise collide on `(tick, from_pid)`; the atomic
/// gives every pass a unique id regardless of order.
static NEXT_PASS_FLOW_ID: AtomicU64 = AtomicU64::new(1);

/// Speed imparted to the ball when a possessing player shoots or kicks it.
///
/// Demo-scope simplification (see `kick_execution_system`): a flat
/// speed per action type, not scaled by player skill/power.
pub const KICK_SPEED_SHOT: f32 = 22.0;
pub const KICK_SPEED_PASS: f32 = 14.0;

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
    match_clock: Option<Res<MatchClock>>,
    telemetry: Option<Res<TelemetryConfig>>,
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
        // Emit a `pass` flow event when telemetry is on and this was a
        // pass. Shots / tackles don't carry identity of a "receiver", so
        // we restrict to `PassTo`. `match_clock` is taken as `Option` so
        // tests can omit it.
        if let (true, Some(receiver_entity)) = (
            telemetry.is_some() && matches!(intent, Intent::Action(ActionIntent::PassTo)),
            perception.nearby_teammates.first().map(|t| t.entity),
        ) {
            let tick = match_clock.as_deref().map_or(0, |c| c.elapsed_ticks);
            let flow_id = NEXT_PASS_FLOW_ID.fetch_add(1, Ordering::Relaxed);
            sim_telemetry::emit::pass(
                flow_id,
                sim_telemetry::emit::player_pid(possessor.index()),
                sim_telemetry::emit::TID_DECISION,
                sim_telemetry::emit::player_pid(receiver_entity.index()),
                sim_telemetry::emit::TID_DECISION,
                tick,
                true,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_components::{
        Ball, BallMarker, MatchClock, NearbyEntity, PerceptionSnapshot, PitchBounds, Player, Position,
        Velocity,
    };
    use sim_telemetry::{TelemetryConfig, capture_trace};
    use smallvec::smallvec;

    type EventVec = Vec<serde_json::Value>;

    fn pos(x: f32, y: f32) -> Vec2 {
        Vec2::new(x, y)
    }

    #[test]
    fn kick_execution_emits_a_pass_flow_event_when_telemetry_is_on() {
        // Two players on the same team, 15 m apart — within `nearby_teammates`
        // range (20 m). The kicker holds the ball and has `PassTo` as its
        // committed intent. We expect one `flow_start` on the kicker's
        // process and one `flow_end` on the receiver's, sharing an id.
        let mut world = World::new();

        let kicker = world
            .spawn((
                Player {
                    team_id: sim_components::TeamId(0),
                    intent: Some(Intent::Action(ActionIntent::PassTo)),
                },
                Position(pos(50.0, 34.0)),
                Velocity(pos(0.0, 0.0)),
                PerceptionSnapshot {
                    self_position: pos(50.0, 34.0),
                    nearby_teammates: smallvec![NearbyEntity {
                        entity: Entity::from_raw(42),
                        distance: 15.0,
                        relative_position: pos(15.0, 0.0),
                    }],
                    nearby_opponents: smallvec![],
                    ball_position: pos(52.5, 34.0),
                    ball_state: sim_components::BallState::Possessed,
                    goal_position: pos(105.0, 34.0),
                    pitch_bounds: PitchBounds {
                        distance_to_left: 50.0,
                        distance_to_right: 55.0,
                        distance_to_top: 34.0,
                        distance_to_bottom: 34.0,
                    },
                },
            ))
            .id();
        // Spawn a teammate entity for completeness (the kicker only reads
        // its perception snapshot, so this is just a placeholder).
        world.spawn(BallMarker);

        world.insert_resource(Ball {
            state: sim_components::BallState::Possessed,
            possessor: Some(kicker),
            last_touched_by: None,
            kick_velocity: None,
            spin: 0.0,
        });
        world.insert_resource(MatchClock {
            elapsed_ticks: 100,
            half: 1,
            added_time_ticks: 0,
            is_running: true,
        });
        world.insert_resource(TelemetryConfig::new(60, None).expect("valid"));

        let events: EventVec = capture_trace(|| {
            let mut schedule = Schedule::default();
            schedule.add_systems(kick_execution_system);
            schedule.run(&mut world);
        });

        let starts: Vec<_> = events
            .iter()
            .filter(|e| e["ph"] == "s" && e["name"] == "pass")
            .collect();
        let ends: Vec<_> = events
            .iter()
            .filter(|e| e["ph"] == "f" && e["name"] == "pass")
            .collect();
        assert_eq!(starts.len(), 1, "one flow_start for the pass");
        assert_eq!(ends.len(), 1, "one flow_end for the pass");
        assert_eq!(starts[0]["args"]["id"], ends[0]["args"]["id"]);
        assert_eq!(starts[0]["args"]["completed"], true);
    }

    #[test]
    fn kick_execution_emits_no_flow_when_telemetry_is_off() {
        // Same setup but with telemetry disabled: no flow events.
        let mut world = World::new();

        let kicker = world
            .spawn((
                Player {
                    team_id: sim_components::TeamId(0),
                    intent: Some(Intent::Action(ActionIntent::PassTo)),
                },
                Position(pos(50.0, 34.0)),
                Velocity(pos(0.0, 0.0)),
                PerceptionSnapshot {
                    self_position: pos(50.0, 34.0),
                    nearby_teammates: smallvec![NearbyEntity {
                        entity: Entity::from_raw(42),
                        distance: 15.0,
                        relative_position: pos(15.0, 0.0),
                    }],
                    nearby_opponents: smallvec![],
                    ball_position: pos(52.5, 34.0),
                    ball_state: sim_components::BallState::Possessed,
                    goal_position: pos(105.0, 34.0),
                    pitch_bounds: PitchBounds {
                        distance_to_left: 50.0,
                        distance_to_right: 55.0,
                        distance_to_top: 34.0,
                        distance_to_bottom: 34.0,
                    },
                },
            ))
            .id();
        world.spawn(BallMarker);

        world.insert_resource(Ball {
            state: sim_components::BallState::Possessed,
            possessor: Some(kicker),
            last_touched_by: None,
            kick_velocity: None,
            spin: 0.0,
        });
        // No MatchClock, no TelemetryConfig: telemetry fully off.

        let events: EventVec = capture_trace(|| {
            let mut schedule = Schedule::default();
            schedule.add_systems(kick_execution_system);
            schedule.run(&mut world);
        });
        assert!(
            events.is_empty(),
            "no telemetry → no trace events, got {events:?}"
        );
    }
}

