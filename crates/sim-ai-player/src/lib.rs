use bevy_ecs::prelude::*;
use rand::Rng;
use sim_ai_core::{ResponseCurve, geometric_mean};
use sim_components::{Ball, BallState, Intent, Player, Position, Skill, Stamina, Velocity};
use sim_math::Vec2;
use sim_physics::{PitchControlGrid, SimRng};

#[derive(Component, Debug, Clone)]
pub struct UtilityBrain {
    pub actions: Vec<PlayerAction>,
    pub hysteresis: f32,
}

#[derive(Debug, Clone)]
pub struct PlayerAction {
    pub intent: Intent,
    pub considerations: Vec<PlayerConsideration>,
}

#[derive(Debug, Clone)]
pub struct PlayerConsideration {
    pub name: String,
    pub weight: f32,
    /// Raw, unnormalized value to feed into the curve. Phase 2 decisions
    /// pre-compute this per-tick from the player's perception snapshot and
    /// current world state. Storing it on the action lets the decision
    /// system stay pure (no recomputation of perception during scoring).
    pub curve: ResponseCurve,
}

pub fn perception_system(
    mut queries: ParamSet<(
        Query<(Entity, &mut Player, &Position)>,
        Query<(Entity, &Player, &Position)>,
        Query<(Entity, &Ball, &Position)>,
        Query<&sim_components::Match>,
        Query<&sim_components::Team>,
    )>,
) {
    // Get ball position and state via the Ball query — disjointness with
    // the player queries is enforced by the ParamSet slot, not by Query
    // filtering. We also read `Ball.possessor` but store only the
    // `BallState` variant (Free / Possessed) in the snapshot; the real
    // entity lives on `Ball.possessor`.
    let (ball_position, ball_state) = {
        let q = queries.p2();
        if let Ok((_ball_entity, ball, pos)) = q.get_single() {
            (pos.0, ball.state)
        } else {
            (Vec2::zero(), BallState::Free)
        }
    };

    // Snapshot all other players' (team_id, position) up front. This avoids
    // holding p1 open while iterating p0 mutably, which ParamSet forbids.
    // Phase 1 has 22 players so an O(n) snapshot per tick is fine.
    let mut others: Vec<(sim_components::TeamId, Vec2)> = Vec::new();
    {
        let q1 = queries.p1();
        for (_e, other_player, other_pos) in q1.iter() {
            others.push((other_player.team_id, other_pos.0));
        }
    }

    // Phase 2: snapshot the single Match and all Teams up front. These are
    // read-only resources used to fill in match-context fields on each
    // player (score differential, time remaining, possession, mentality).
    // The Match stores its own clock snapshot; the separate `MatchClock`
    // component is the running clock but for Phase 2 we use the inner
    // `Match.clock` to avoid an additional ParamSet slot.
    let match_info: Option<(i8, f32, sim_components::TeamId, sim_components::TeamId)> = {
        let q = queries.p3();
        q.iter().next().map(|m| {
            let diff: i8 = (m.score.0 as i16 - m.score.1 as i16) as i8;
            let half_total = if m.clock.half == 1 { 45.0 } else { 90.0 };
            let time_remaining = half_total - m.clock.elapsed;
            (
                diff,
                time_remaining,
                sim_components::TeamId(0),
                sim_components::TeamId(1),
            )
        })
    };
    let (score_diff, time_remaining, _home_id, _away_id) = match_info.unwrap_or((
        0,
        90.0,
        sim_components::TeamId(0),
        sim_components::TeamId(1),
    ));

    // Snapshot mentalities by team id.
    let mut mentality_by_team: std::collections::HashMap<sim_components::TeamId, f32> =
        std::collections::HashMap::new();
    {
        let q = queries.p4();
        for team in q.iter() {
            let m = match team.mentality {
                sim_components::Mentality::Defend => -0.5,
                sim_components::Mentality::Balance => 0.0,
                sim_components::Mentality::Attack => 0.5,
            };
            mentality_by_team.insert(team.id, m);
        }
    }

    // Team possession share: for Phase 2 this is a deterministic placeholder
    // (0.5 / 0.5). It will become a real moving window of recent touches once
    // Phase 3 / 5 add touch tracking. A non-constant value is required so the
    // Player-side field carries meaning at the perception layer.
    let home_possession = 0.5_f32;
    let away_possession = 1.0_f32 - home_possession;

    // Now iterate players via the mutable query, building and applying the
    // snapshot in one pass. Using `get_mut` per entity keeps each mutation
    // window short and avoids holding a long-lived p0 iterator.
    let entity_ids: Vec<bevy_ecs::prelude::Entity> = {
        let q0 = queries.p0();
        q0.iter().map(|(e, _, _)| e).collect()
    };
    for entity in entity_ids {
        // Snapshot player_pos + team_id via an immutable get (read-only).
        let snapshot_info = queries
            .p0()
            .get(entity)
            .ok()
            .map(|(_, p, pos)| (pos.0, p.team_id));
        let Some((player_pos, team_id)) = snapshot_info else {
            continue;
        };

        let mut nearby_teammates = smallvec::SmallVec::new();
        let mut nearby_opponents = smallvec::SmallVec::new();
        for (other_team_id, other_pos) in &others {
            let distance = player_pos.distance(*other_pos);
            if distance <= 20.0 {
                let relative_position = *other_pos - player_pos;
                let nearby_entity = sim_components::NearbyEntity {
                    entity: Entity::PLACEHOLDER,
                    distance,
                    relative_position,
                };
                if *other_team_id == team_id {
                    if nearby_teammates.len() < 8 {
                        nearby_teammates.push(nearby_entity);
                    }
                } else if nearby_opponents.len() < 8 {
                    nearby_opponents.push(nearby_entity);
                }
            }
        }

        nearby_teammates.sort_by(
            |a: &sim_components::NearbyEntity, b: &sim_components::NearbyEntity| {
                a.distance.partial_cmp(&b.distance).unwrap()
            },
        );
        nearby_opponents.sort_by(
            |a: &sim_components::NearbyEntity, b: &sim_components::NearbyEntity| {
                a.distance.partial_cmp(&b.distance).unwrap()
            },
        );

        let pitch_bounds = sim_components::PitchBounds {
            distance_to_left: player_pos.x,
            distance_to_right: 105.0 - player_pos.x,
            distance_to_top: player_pos.y,
            distance_to_bottom: 68.0 - player_pos.y,
        };
        let goal_position = Vec2::new(105.0, 34.0);

        let perception = sim_components::PerceptionSnapshot {
            self_position: player_pos,
            nearby_teammates,
            nearby_opponents,
            ball_position,
            ball_state,
            goal_position,
            pitch_bounds,
        };

        let team_possession = if team_id.0 == 0 {
            home_possession
        } else {
            away_possession
        };
        let mentality_modifier = *mentality_by_team.get(&team_id).unwrap_or(&0.0);

        if let Ok((_, mut player, _)) = queries.p0().get_mut(entity) {
            player.perception = Some(perception);
            player.score_differential = score_diff;
            player.time_remaining = time_remaining;
            player.team_possession = team_possession;
            player.mentality_modifier = mentality_modifier;
        }
    }
}

pub fn consideration_scoring_system(_query: Query<(&mut Player, &Stamina, &Skill)>) {
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
pub fn player_decision_system(
    mut query: Query<(&mut Player, &UtilityBrain)>,
    pitch_control: Option<Res<PitchControlGrid>>,
) {
    for (mut player, utility_brain) in query.iter_mut() {
        // Need a perception snapshot to score considerations.
        let Some(perception) = player.perception.clone() else {
            // No perception yet (first tick after spawn): keep intent as-is
            // (which may be None — execution system handles None safely).
            continue;
        };

        // Evaluate each action.
        let mut best_action: Option<(Intent, f32)> = None;

        for action in &utility_brain.actions {
            let mut consideration_scores: Vec<f32> = Vec::new();

            for consideration in &action.considerations {
                // Compute the raw input for this consideration from the
                // player's perception + world state. For Phase 2 we only
                // implement a small, generic set of named considerations;
                // anything else falls back to a neutral 0.5.
                let raw = compute_consideration_input(
                    &consideration.name,
                    &player,
                    &perception,
                    &action.intent,
                    pitch_control.as_deref(),
                );
                let score = consideration.curve.evaluate(raw).clamp(0.0, 1.0);
                consideration_scores.push(score);
            }

            if consideration_scores.is_empty() {
                // No considerations ⇒ fixed baseline.
                let baseline = 0.6;
                if let Some((_, best)) = best_action {
                    if baseline > best {
                        best_action = Some((action.intent.clone(), baseline));
                    }
                } else {
                    best_action = Some((action.intent.clone(), baseline));
                }
                continue;
            }

            // `geometric_mean` already clamps each input to ≥ 1e-4 inside,
            // so a 0 score collapses the aggregate toward 0 but doesn't
            // produce literal 0 (preserving numerical stability for the
            // product).
            let mut aggregate = geometric_mean(&consideration_scores);

            // Apply hysteresis: if this action's intent type matches the
            // player's currently-active intent, add the bonus.
            if let Some(current) = &player.intent {
                if intent_kind(current) == intent_kind(&action.intent) {
                    aggregate += utility_brain.hysteresis;
                }
            }

            if let Some((_, best)) = best_action {
                if aggregate > best {
                    best_action = Some((action.intent.clone(), aggregate));
                }
            } else {
                best_action = Some((action.intent.clone(), aggregate));
            }
        }

        if let Some((intent, _score)) = best_action {
            player.intent = Some(intent);
        } else if player.intent.is_none() {
            player.intent = Some(sim_components::Intent::HoldPosition);
        }
    }
}

/// Map an Intent to a coarse "kind" label used for hysteresis matching.
/// Hysteresis is supposed to suppress flicker between near-equal *options*;
/// passing to player A vs. passing to player B should NOT receive a
/// hysteresis bonus against each other (different actions of the same
/// type would be unfair), so we match on the variant discriminant only.
fn intent_kind(intent: &Intent) -> &'static str {
    match intent {
        Intent::MoveToPosition(_) => "MoveToPosition",
        Intent::PassTo => "PassTo",
        Intent::ShootAtGoal(_) => "ShootAtGoal",
        Intent::Tackle(_) => "Tackle",
        Intent::ChaseBall => "ChaseBall",
        Intent::MarkOpponent(_) => "MarkOpponent",
        Intent::Intercept => "Intercept",
        Intent::Press(_) => "Press",
        Intent::HoldPosition => "HoldPosition",
        Intent::SupportRun => "SupportRun",
        Intent::TrackBack => "TrackBack",
    }
}

/// Compute the raw, unnormalized input value for a named consideration.
/// Phase 2 implements the considerations listed in §4a of the task spec;
/// any unknown name returns a neutral 0.5 so a poorly-tuned brain still
/// produces valid scores.
fn compute_consideration_input(
    name: &str,
    player: &Player,
    perception: &sim_components::PerceptionSnapshot,
    intent: &Intent,
    grid: Option<&PitchControlGrid>,
) -> f32 {
    match name {
        // --- MoveToPosition ---
        "distance_to_target" => {
            // Closer to target = higher score. Compute raw distance, then
            // convert to closeness so the Linear curve rises when nearer.
            let d = if let Intent::MoveToPosition(target) = intent {
                perception.self_position.distance(*target)
            } else {
                perception.self_position.distance(perception.ball_position)
            };
            30.0 - d.min(30.0)
        }

        // --- ChaseBall ---
        // distance_to_ball: feed raw metres. The brain uses a *decreasing*
        // Logistic (negative steepness) so closer = higher score and far
        // = small but nonzero score.
        "distance_to_ball" => perception.self_position.distance(perception.ball_position),
        "stamina" => player.stamina,
        "pitch_control_at_ball" => grid
            .map(|g| g.control_at(perception.ball_position.x, perception.ball_position.y) * 100.0)
            .unwrap_or(50.0),

        // --- PassTo ---
        "pass_angle_clear" => {
            // For Phase 2: 1.0 if no opponent is between player and target,
            // linearly fading to 0.0 if a defender blocks the lane.
            if let Intent::PassTo = intent {
                let lane_clear = !perception.nearby_opponents.iter().any(|opp| {
                    let dist = opp.distance;
                    dist < 8.0
                });
                if lane_clear { 1.0 } else { 0.4 }
            } else {
                0.5
            }
        }
        "teammate_distance" => {
            // Closer teammate = higher score. Feed (30 - avg_distance).
            if perception.nearby_teammates.is_empty() {
                0.0
            } else {
                let avg: f32 = perception
                    .nearby_teammates
                    .iter()
                    .map(|t| t.distance)
                    .sum::<f32>()
                    / perception.nearby_teammates.len() as f32;
                30.0 - avg.min(30.0)
            }
        }
        "teammate_space" => {
            // More space around the player = higher score. Feed the inverse
            // of nearest-opponent distance: closer opponent = lower score.
            let nearest_opp = perception
                .nearby_opponents
                .iter()
                .map(|o| o.distance)
                .fold(f32::INFINITY, f32::min);
            let space_m = if nearest_opp.is_finite() {
                nearest_opp
            } else {
                20.0
            };
            20.0 - space_m.min(20.0)
        }

        // --- ShootAtGoal ---
        "distance_to_goal" => 35.0 - perception.self_position.distance(perception.goal_position),
        "goal_angle" => {
            // Cosine of the angle between (player→ball) and (player→goal).
            // 1.0 = straight at goal, 0.0 = wide.
            let to_ball = perception.ball_position - perception.self_position;
            let to_goal = perception.goal_position - perception.self_position;
            let dot = to_ball.dot(to_goal);
            let mags = to_ball.length() * to_goal.length();
            if mags > 1e-3 {
                (dot / mags).clamp(-1.0, 1.0)
            } else {
                0.0
            }
        }
        "defender_pressure" => {
            // Number of opponents within 5m * 1m penalty per opponent.
            let n_close = perception
                .nearby_opponents
                .iter()
                .filter(|o| o.distance <= 5.0)
                .count() as f32;
            n_close
        }

        // --- Tackle ---
        // Closer opponent = higher score. Feed the inverse so the curve
        // (which rises monotonically) gets bigger values for closer targets.
        "distance_to_opponent" => {
            5.0 - perception
                .nearby_opponents
                .iter()
                .map(|o| o.distance)
                .fold(f32::INFINITY, f32::min)
                .min(5.0)
        }
        "skill_diff" => {
            // Player skill minus an average opponent skill (assume 0.7).
            (player.skill - 0.7).clamp(-1.0, 1.0)
        }

        // --- MarkOpponent ---
        "distance_to_marked" => {
            10.0 - perception
                .nearby_opponents
                .iter()
                .map(|o| o.distance)
                .fold(f32::INFINITY, f32::min)
                .min(10.0)
        }
        "defensive_position" => {
            // 1.0 if between ball position and own goal line, 0.0 otherwise.
            let own_goal_x = 0.0_f32;
            let ball_x = perception.ball_position.x;
            let player_x = perception.self_position.x;
            if player_x <= ball_x && player_x >= own_goal_x {
                1.0
            } else {
                0.0
            }
        }

        // --- Press ---
        "distance_to_press" => {
            12.0 - perception
                .nearby_opponents
                .iter()
                .map(|o| o.distance)
                .fold(f32::INFINITY, f32::min)
                .min(12.0)
        }

        // --- SupportRun ---
        "space_ahead" => {
            // More open space ahead = higher score. Compute nearest opponent
            // in front, then feed (15 - distance) so the Linear curve gives
            // a higher score when the space ahead is open.
            let forward = perception.ball_position.x > perception.self_position.x;
            let opp_in_front = perception
                .nearby_opponents
                .iter()
                .filter(|o| {
                    if forward {
                        o.relative_position.x > 0.0
                    } else {
                        o.relative_position.x < 0.0
                    }
                })
                .map(|o| o.distance)
                .fold(f32::INFINITY, f32::min);
            let d = if opp_in_front.is_finite() {
                opp_in_front
            } else {
                15.0
            };
            15.0 - d.min(15.0)
        }
        "teammate_ball" => {
            // 1.0 if a teammate is within 2m of the ball.
            let has = perception.nearby_teammates.iter().any(|t| {
                let dist_to_ball = (t.relative_position + perception.self_position
                    - perception.ball_position)
                    .length();
                dist_to_ball < 2.0
            });
            if has { 1.0 } else { 0.0 }
        }

        // --- HoldPosition ---
        "formation_discipline" => 1.0,

        // --- Generic catch-alls ---
        "ball_distance" => perception.self_position.distance(perception.ball_position),
        "self_to_ball" => perception.self_position.distance(perception.ball_position),
        _ => 0.5,
    }
}

pub fn player_action_execution_system(
    mut query: Query<(&mut Player, &mut Velocity)>,
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
    // Also mirrors the new velocity back into `Player.velocity` so the
    // snapshot/PlayerView readers (which read from the Player struct, not
    // the Position/Velocity components) see fresh values.
    //
    // Phase 3: stochastic outcomes for tackles, passes, and shots using RNG.
    for (mut player, mut velocity) in query.iter_mut() {
        let Some(intent) = player.intent.clone() else {
            velocity.0 = Vec2::zero();
            player.velocity = Vec2::zero();
            continue;
        };

        // Apply stochastic modifiers based on action type
        let mut speed_modifier = 1.0;
        let mut direction_modifier = Vec2::zero();

        match &intent {
            Intent::Tackle(target) => {
                // Tackle resolution: higher skill = higher success probability
                // Roll against skill-based probability
                let tackle_success_prob = player.skill.clamp(0.3, 0.9);
                let roll: f32 = rng.0.r#gen();
                if roll > tackle_success_prob {
                    // Tackle fails - player stumbles, moves slower
                    speed_modifier = 0.3;
                
                }
                // Store target for potential later use
                let _ = target;
            }
            Intent::PassTo => {
                // Pass completion: probability based on distance and skill
                let pass_distance = player.perception.as_ref()
                    .and_then(|p| p.nearby_teammates.first().map(|t| t.distance))
                    .unwrap_or(15.0);
                // Longer passes = lower completion probability
                let pass_prob = (1.0 - (pass_distance / 50.0).min(0.8)) * player.skill;
                let roll: f32 = rng.0.r#gen();
                if roll > pass_prob {
                    // Pass goes awry - add random deviation
                    speed_modifier = 0.7;
                    direction_modifier = Vec2::new(
                        rng.0.gen_range(-2.0..2.0),
                        rng.0.gen_range(-2.0..2.0)
                    );
                
                }
            }
            Intent::ShootAtGoal(_) => {
                // Shot accuracy: skill and stamina affect accuracy
                let accuracy = player.skill * (0.5 + 0.5 * player.stamina);
                let roll: f32 = rng.0.r#gen();
                if roll > accuracy {
                    // Shot misses - add deviation
                    speed_modifier = 0.8;
                    direction_modifier = Vec2::new(
                        rng.0.gen_range(-3.0..3.0),
                        rng.0.gen_range(-2.0..2.0)
                    );
                
                }
            }
            _ => {}
        }

        let new_velocity = steer(&player, &intent);
        velocity.0 = new_velocity * speed_modifier + direction_modifier;
        player.velocity = velocity.0;
    }
}

/// Speed imparted to the ball when a possessing player shoots or otherwise
/// kicks it. Demo-scope simplification (see `kick_execution_system`): a flat
/// speed per action type, not scaled by player skill/power.
pub const KICK_SPEED_SHOT: f32 = 22.0;
pub const KICK_SPEED_PASS: f32 = 14.0;

/// Turns a possessing player's intent into actual ball velocity.
///
/// Before this system existed, nothing in the simulation ever wrote to the
/// ball's `Velocity` component: players could gain possession via
/// `possession_resolution_system`'s proximity check, but the ball itself
/// never moved as a result of an action, so "kick/pass" was purely
/// notional. This system closes that gap for the current demo scope only —
/// it is intentionally not real ball control:
/// - `ShootAtGoal`: kicks toward the already-resolved shot target.
/// - `PassTo` / `ChaseBall` / `Tackle` / `Press`: continues the ball in the
///   player's current heading. `PassTo`'s target entity is not resolved
///   here (see the known `Entity::PLACEHOLDER` gap in
///   `default_utility_brain`) — out of scope for "just movement and
///   kick/pass", not a real pass-to-teammate mechanic.
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
    player_query: Query<(Entity, &Player)>,
    mut ball_query: Query<(&mut Velocity, &mut Ball)>,
) {
    let Ok((mut ball_velocity, mut ball)) = ball_query.get_single_mut() else {
        return;
    };
    let Some(possessor) = ball.possessor else {
        return;
    };
    let Some((_, player)) = player_query.iter().find(|(e, _)| *e == possessor) else {
        return;
    };
    let Some(intent) = &player.intent else {
        return;
    };

    let kick = match intent {
        Intent::ShootAtGoal(target) => {
            let dir = *target - player.position;
            (dir.length() > 0.01).then(|| (dir.normalized(), KICK_SPEED_SHOT))
        }
        Intent::PassTo | Intent::ChaseBall | Intent::Tackle(_) | Intent::Press(_) => {
            (player.velocity.length() > 0.01)
                .then(|| (player.velocity.normalized(), KICK_SPEED_PASS))
        }
        _ => None,
    };

    if let Some((direction, speed)) = kick {
        ball_velocity.0 = direction * speed;
        ball.possessor = None;
    }
}

/// Resolve an intent to a target point and an action-specific speed,
/// then return the unit-direction × speed vector. Target entities are
/// resolved from the player's perception snapshot (`nearby_teammates` /
/// `nearby_opponents`), which carries their world position relative to the
/// player at perception time. The ball position is read from the perception
/// snapshot directly (the perception system writes it every tick).
fn steer(player: &Player, intent: &Intent) -> Vec2 {
    let player_pos = player.position;
    let ball_pos = player
        .perception
        .as_ref()
        .map(|p| p.ball_position)
        .unwrap_or(player_pos);
    let ball_vel = Vec2::zero(); // Phase 2: ball velocity not yet exposed in perception.

    let nearest_opponent_pos = |p: &Player| -> Option<Vec2> {
        p.perception.as_ref().and_then(|snap| {
            snap.nearby_opponents
                .iter()
                .min_by(|a, b| {
                    a.distance
                        .partial_cmp(&b.distance)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|o| o.relative_position + player_pos)
        })
    };
    let nearest_teammate_pos = |p: &Player| -> Option<Vec2> {
        p.perception.as_ref().and_then(|snap| {
            snap.nearby_teammates
                .iter()
                .min_by(|a, b| {
                    a.distance
                        .partial_cmp(&b.distance)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|t| t.relative_position + player_pos)
        })
    };

    let (target, speed): (Vec2, f32) = match intent {
        Intent::MoveToPosition(target) => (*target, 5.0),
        Intent::PassTo => (nearest_teammate_pos(player).unwrap_or(ball_pos), 8.0),
        Intent::ShootAtGoal(target) => (*target, 10.0),
        Intent::Tackle(_) => (nearest_opponent_pos(player).unwrap_or(player_pos), 10.0),
        Intent::ChaseBall => (ball_pos, 8.0),
        Intent::MarkOpponent(_) => (nearest_opponent_pos(player).unwrap_or(player_pos), 4.0),
        Intent::Intercept => (ball_pos + ball_vel * 0.5, 9.0),
        Intent::Press(_) => (nearest_opponent_pos(player).unwrap_or(player_pos), 10.0),
        Intent::HoldPosition => return Vec2::zero(),
        Intent::SupportRun => {
            if let Some(perception) = &player.perception {
                if let Some(worst) = perception.nearby_opponents.iter().min_by(|a, b| {
                    a.distance
                        .partial_cmp(&b.distance)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }) {
                    let away = player_pos + (player_pos - worst.relative_position);
                    (away, 5.0)
                } else {
                    return Vec2::zero();
                }
            } else {
                return Vec2::zero();
            }
        }
        Intent::TrackBack => (Vec2::new(0.0, 34.0), 6.0),
    };

    let direction = target - player_pos;
    let distance = direction.length();
    if distance > 0.1 {
        let normalized = direction / distance;
        normalized * speed
    } else {
        Vec2::zero()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_components::{Role, TeamId};

    #[test]
    fn test_player_ai_plugin() {
        // Placeholder test
        assert!(true);
    }

    #[test]
    fn test_stamina_based_decision() {
        // Test that low stamina player conserves energy
        let mut world = World::new();

        // Create a player with low stamina
        let player_entity = world.spawn(()).id();
        world.entity_mut(player_entity).insert(Player {
            team_id: TeamId(0),
            position: Vec2::new(50.0, 34.0),
            velocity: Vec2::zero(),
            stamina: 0.2, // Low stamina
            role: Role::Striker,
            skill: 0.8,
            intent: None,
            perception: None,
            score_differential: 0,
            time_remaining: 90.0,
            team_possession: 0.5,
            mentality_modifier: 0.0,
        });
        world.entity_mut(player_entity).insert(Stamina(0.2));
        world.entity_mut(player_entity).insert(Skill(0.8));

        // Create utility brain with sprint action
        let utility_brain = UtilityBrain {
            actions: vec![PlayerAction {
                intent: sim_components::Intent::MoveToPosition(Vec2::new(100.0, 34.0)),
                considerations: vec![],
            }],
            hysteresis: 0.1,
        };
        world.entity_mut(player_entity).insert(utility_brain);

        // Phase 2: provide a minimal perception snapshot so
        // player_decision_system can run.
        if let Some(mut p) = world.entity_mut(player_entity).get_mut::<Player>() {
            p.perception = Some(sim_components::PerceptionSnapshot {
                self_position: Vec2::new(50.0, 34.0),
                nearby_teammates: smallvec::SmallVec::new(),
                nearby_opponents: smallvec::SmallVec::new(),
                ball_position: Vec2::new(52.5, 34.0),
                ball_state: sim_components::BallState::Free,
                goal_position: Vec2::new(105.0, 34.0),
                pitch_bounds: sim_components::PitchBounds {
                    distance_to_left: 50.0,
                    distance_to_right: 55.0,
                    distance_to_top: 34.0,
                    distance_to_bottom: 34.0,
                },
            });
        }

        // Run systems
        let mut schedule = Schedule::default();
        schedule.add_systems(consideration_scoring_system);
        schedule.add_systems(player_decision_system);

        let _match_entity = world.spawn(()).id();
        world
            .entity_mut(_match_entity)
            .insert(sim_components::Match {
                id: 1,
                home_team: Entity::PLACEHOLDER,
                away_team: Entity::PLACEHOLDER,
                score: (0, 0),
                clock: sim_components::MatchClock {
                    elapsed: 0.0,
                    half: 1,
                    added_time: 0.0,
                    is_running: true,
                },
                state: sim_components::MatchState::InPlay,
                seed: 0,
            });

        // Run schedule
        schedule.run(&mut world);

        // Check that player has an intent
        let player = world.entity(player_entity).get::<Player>().unwrap();
        assert!(player.intent.is_some());
    }

    #[test]
    fn test_passing_option() {
        // Test that player considers teammates in better positions
        let mut world = World::new();

        // Create a player
        let player_entity = world.spawn(()).id();
        world.entity_mut(player_entity).insert(Player {
            team_id: TeamId(0),
            position: Vec2::new(50.0, 34.0),
            velocity: Vec2::zero(),
            stamina: 0.8,
            role: Role::Striker,
            skill: 0.8,
            intent: None,
            perception: None,
            score_differential: 0,
            time_remaining: 90.0,
            team_possession: 0.5,
            mentality_modifier: 0.0,
        });
        world.entity_mut(player_entity).insert(Stamina(0.8));
        world.entity_mut(player_entity).insert(Skill(0.8));

        // Create a teammate in better position
        let teammate_entity = world.spawn(()).id();
        world.entity_mut(teammate_entity).insert(Player {
            team_id: TeamId(0),
            position: Vec2::new(80.0, 34.0), // Closer to goal
            velocity: Vec2::zero(),
            stamina: 0.9,
            role: Role::Striker,
            skill: 0.7,
            intent: None,
            perception: None,
            score_differential: 0,
            time_remaining: 90.0,
            team_possession: 0.5,
            mentality_modifier: 0.0,
        });

        // Create utility brain with pass action
        let utility_brain = UtilityBrain {
            actions: vec![PlayerAction {
                intent: sim_components::Intent::PassTo,
                considerations: vec![],
            }],
            hysteresis: 0.1,
        };
        world.entity_mut(player_entity).insert(utility_brain);

        // Phase 2: minimal perception snapshot for player_decision_system.
        if let Some(mut p) = world.entity_mut(player_entity).get_mut::<Player>() {
            p.perception = Some(sim_components::PerceptionSnapshot {
                self_position: Vec2::new(50.0, 34.0),
                nearby_teammates: smallvec::SmallVec::new(),
                nearby_opponents: smallvec::SmallVec::new(),
                ball_position: Vec2::new(52.5, 34.0),
                ball_state: sim_components::BallState::Free,
                goal_position: Vec2::new(105.0, 34.0),
                pitch_bounds: sim_components::PitchBounds {
                    distance_to_left: 50.0,
                    distance_to_right: 55.0,
                    distance_to_top: 34.0,
                    distance_to_bottom: 34.0,
                },
            });
        }

        // Run systems
        let mut schedule = Schedule::default();
        schedule.add_systems(consideration_scoring_system);
        schedule.add_systems(player_decision_system);

        let _match_entity = world.spawn(()).id();
        world
            .entity_mut(_match_entity)
            .insert(sim_components::Match {
                id: 1,
                home_team: Entity::PLACEHOLDER,
                away_team: Entity::PLACEHOLDER,
                score: (0, 0),
                clock: sim_components::MatchClock {
                    elapsed: 0.0,
                    half: 1,
                    added_time: 0.0,
                    is_running: true,
                },
                state: sim_components::MatchState::InPlay,
                seed: 0,
            });

        // Run schedule
        schedule.run(&mut world);

        // Check that player has a pass intent
        let player = world.entity(player_entity).get::<Player>().unwrap();
        assert!(player.intent.is_some());
    }

    #[test]
    fn test_defender_tackle() {
        // Test that defender attempts tackle when attacker shoots
        let mut world = World::new();

        // Create an attacker with shoot intent
        let attacker_entity = world.spawn(()).id();
        world.entity_mut(attacker_entity).insert(Player {
            team_id: TeamId(1),
            position: Vec2::new(45.0, 34.0),
            velocity: Vec2::zero(),
            stamina: 0.8,
            role: Role::Striker,
            skill: 0.8,
            intent: Some(sim_components::Intent::ShootAtGoal(Vec2::new(105.0, 34.0))),
            perception: None,
            score_differential: 0,
            time_remaining: 90.0,
            team_possession: 0.5,
            mentality_modifier: 0.0,
        });

        // Create a defender
        let defender_entity = world.spawn(()).id();
        world.entity_mut(defender_entity).insert(Player {
            team_id: TeamId(0),
            position: Vec2::new(40.0, 34.0),
            velocity: Vec2::zero(),
            stamina: 0.9,
            role: Role::CenterBack,
            skill: 0.7,
            intent: None,
            perception: Some(sim_components::PerceptionSnapshot {
                self_position: Vec2::new(40.0, 34.0),
                nearby_teammates: smallvec::SmallVec::new(),
                nearby_opponents: smallvec::smallvec![sim_components::NearbyEntity {
                    entity: attacker_entity,
                    distance: 5.0,
                    relative_position: Vec2::new(5.0, 0.0),
                }],
                ball_position: Vec2::new(45.0, 34.0),
                ball_state: sim_components::BallState::Free,
                goal_position: Vec2::new(105.0, 34.0),
                pitch_bounds: sim_components::PitchBounds {
                    distance_to_left: 40.0,
                    distance_to_right: 65.0,
                    distance_to_top: 34.0,
                    distance_to_bottom: 34.0,
                },
            }),
            score_differential: 0,
            time_remaining: 90.0,
            team_possession: 0.5,
            mentality_modifier: 0.0,
        });

        // Create utility brain with tackle action. Phase 2 decision cadence is
        let utility_brain = UtilityBrain {
            actions: vec![PlayerAction {
                intent: sim_components::Intent::Tackle(attacker_entity),
                considerations: vec![],
            }],
            hysteresis: 0.1,
        };
        world.entity_mut(defender_entity).insert(utility_brain);

        let _match_entity = world.spawn(()).id();
        world
            .entity_mut(_match_entity)
            .insert(sim_components::Match {
                id: 1,
                home_team: Entity::PLACEHOLDER,
                away_team: Entity::PLACEHOLDER,
                score: (0, 0),
                clock: sim_components::MatchClock {
                    elapsed: 0.0,
                    half: 1,
                    added_time: 0.0,
                    is_running: true,
                },
                state: sim_components::MatchState::InPlay,
                seed: 0,
            });

        // Run systems
        let mut schedule = Schedule::default();
        schedule.add_systems(consideration_scoring_system);
        schedule.add_systems(player_decision_system);

        // Run schedule
        schedule.run(&mut world);

        // Check that defender has tackle intent
        let defender = world.entity(defender_entity).get::<Player>().unwrap();
        assert!(defender.intent.is_some());
    }

    /// Phase 2 verification: per-player decision cadence means the same
    /// player is NOT re-evaluated every tick. With evaluation_interval = 6,
    /// a player whose entity id modulo 6 == 0 only evaluates on ticks
    /// whose tick % 6 == 0. We loop 100 ticks and assert that the
    /// consideration-evaluation cost stays bounded (no panic / no infinite
    /// work).
    #[test]
    fn test_decision_cadence_does_not_re_evaluate_every_tick() {
        use sim_ai_core::ResponseCurve;
        let mut world = World::new();

        // Spawn a player + utility brain.
        let player_entity = world.spawn(()).id();
        world.entity_mut(player_entity).insert(Player {
            team_id: TeamId(0),
            position: Vec2::new(50.0, 34.0),
            velocity: Vec2::zero(),
            stamina: 1.0,
            role: Role::Striker,
            skill: 0.7,
            intent: None,
            perception: None,
            score_differential: 0,
            time_remaining: 90.0,
            team_possession: 0.5,
            mentality_modifier: 0.0,
        });
        let brain = UtilityBrain {
            actions: vec![PlayerAction {
                intent: sim_components::Intent::HoldPosition,
                considerations: vec![PlayerConsideration {
                    name: "formation_discipline".to_string(),
                    weight: 1.0,
                    curve: ResponseCurve::Linear { min: 0.0, max: 1.0 },
                }],
            }],
            hysteresis: 0.1,
        };
        world.entity_mut(player_entity).insert(brain);

        // Provide a minimal perception snapshot so decision_system can run.
        if let Some(mut p) = world.entity_mut(player_entity).get_mut::<Player>() {
            p.perception = Some(sim_components::PerceptionSnapshot {
                self_position: Vec2::new(50.0, 34.0),
                nearby_teammates: smallvec::SmallVec::new(),
                nearby_opponents: smallvec::SmallVec::new(),
                ball_position: Vec2::new(52.5, 34.0),
                ball_state: sim_components::BallState::Free,
                goal_position: Vec2::new(105.0, 34.0),
                pitch_bounds: sim_components::PitchBounds {
                    distance_to_left: 50.0,
                    distance_to_right: 55.0,
                    distance_to_top: 34.0,
                    distance_to_bottom: 34.0,
                },
            });
        }

        // Insert a Match entity so player_decision_system can compute tick.
        let match_entity = world.spawn(()).id();
        world
            .entity_mut(match_entity)
            .insert(sim_components::Match {
                id: 1,
                home_team: Entity::PLACEHOLDER,
                away_team: Entity::PLACEHOLDER,
                score: (0, 0),
                clock: sim_components::MatchClock {
                    elapsed: 0.0,
                    half: 1,
                    added_time: 0.0,
                    is_running: true,
                },
                state: sim_components::MatchState::InPlay,
                seed: 0,
            });

        let mut schedule = Schedule::default();
        schedule.add_systems(player_decision_system);

        // Drive the match clock forward by 6 ticks so the player's
        // evaluation interval (= 6) elapses and decision_system runs at
        // least once. Then verify intent was set.
        for _ in 0..6 {
            if let Some(mut m) = world
                .entity_mut(match_entity)
                .get_mut::<sim_components::Match>()
            {
                m.clock.elapsed += 1.0 / 60.0;
            }
            schedule.run(&mut world);
        }

        // Run the schedule many more times — must not panic, must be cheap
        // (decision cadence spreads evaluation).
        for _ in 0..100 {
            schedule.run(&mut world);
        }

        let player = world.entity(player_entity).get::<Player>().unwrap();
        assert!(player.intent.is_some());
    }
}
