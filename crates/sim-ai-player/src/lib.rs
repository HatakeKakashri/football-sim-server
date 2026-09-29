use bevy_ecs::prelude::*;
use sim_ai_core::{ResponseCurve, geometric_mean};
use sim_components::{
    Intent, MatchClock, PerceptionSnapshot, Player, Position, RoleComponent, Skill, Stamina,
    TeamIdComponent, Velocity, time,
};
use sim_math::Vec2;
use sim_physics::{PitchControlGrid, SimRng};

/// Default decision cadence: every N physics ticks a given player is
/// re-evaluated. Spec §13 #15, default N=6 → ~10 Hz at 60 Hz physics.
/// Each player maps to a deterministic slot in `0..N` (see
/// `player_stagger_slot`), so the guard is
/// `(elapsed_ticks % N) == player_stagger_slot(entity)`.
pub const DECISION_CADENCE_TICKS: u64 = 6;

/// Counter incremented by `player_decision_system` each time a player
/// is actually evaluated (i.e. passed the cadence guard). Used by tests
/// to assert the cadence is in effect; not used by gameplay code.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct DecisionEvaluationCount(pub u64);

/// Map an entity to one of `DECISION_CADENCE_TICKS` decision-cadence
/// slots. Uses a `SplitMix64` finalize on the entity's packed id bits so
/// 22 players distribute roughly evenly across the N slots without a
/// roster-index pass. Stable for a given entity id (Bevy entity ids are
/// dense u32 indices, allocated deterministically).
const fn player_stagger_slot(entity: Entity) -> u64 {
    let bits = entity.to_bits();
    // SplitMix64 finalize — well-trodden; uniform in [0, 2^64).
    let z = (bits ^ (bits >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    let z = z ^ (z >> 31);
    z % DECISION_CADENCE_TICKS
}

#[derive(Component, Debug, Clone)]
pub struct UtilityBrain {
    pub actions: Vec<PlayerAction>,
    pub hysteresis: f32,
}

/// One consideration in a player's utility brain. Each variant carries its
/// own `weight` and `ResponseCurve`. The dispatcher (`raw_input`) is
/// exhaustive: adding a new variant forces a match-arm update at compile
/// time, so a brain template that references a typo'd consideration fails
/// at compile time instead of silently scoring 0.5 (the old string-dispatch
/// behaviour).
#[derive(Debug, Clone)]
pub enum Consideration {
    DistanceToTarget { weight: f32, curve: ResponseCurve },
    DistanceToBall { weight: f32, curve: ResponseCurve },
    Stamina { weight: f32, curve: ResponseCurve },
    PitchControlAtBall { weight: f32, curve: ResponseCurve },
    PassAngleClear { weight: f32, curve: ResponseCurve },
    TeammateDistance { weight: f32, curve: ResponseCurve },
    TeammateSpace { weight: f32, curve: ResponseCurve },
    DistanceToGoal { weight: f32, curve: ResponseCurve },
    GoalAngle { weight: f32, curve: ResponseCurve },
    DefenderPressure { weight: f32, curve: ResponseCurve },
    DistanceToOpponent { weight: f32, curve: ResponseCurve },
    SkillDiff { weight: f32, curve: ResponseCurve },
    DistanceToMarked { weight: f32, curve: ResponseCurve },
    DefensivePosition { weight: f32, curve: ResponseCurve },
    DistanceToPress { weight: f32, curve: ResponseCurve },
    SpaceAhead { weight: f32, curve: ResponseCurve },
    TeammateBall { weight: f32, curve: ResponseCurve },
    FormationDiscipline { weight: f32, curve: ResponseCurve },
}

impl Consideration {
    #[must_use]
    pub fn weight(&self) -> f32 {
        match self {
            Self::DistanceToTarget { weight, .. }
            | Self::DistanceToBall { weight, .. }
            | Self::Stamina { weight, .. }
            | Self::PitchControlAtBall { weight, .. }
            | Self::PassAngleClear { weight, .. }
            | Self::TeammateDistance { weight, .. }
            | Self::TeammateSpace { weight, .. }
            | Self::DistanceToGoal { weight, .. }
            | Self::GoalAngle { weight, .. }
            | Self::DefenderPressure { weight, .. }
            | Self::DistanceToOpponent { weight, .. }
            | Self::SkillDiff { weight, .. }
            | Self::DistanceToMarked { weight, .. }
            | Self::DefensivePosition { weight, .. }
            | Self::DistanceToPress { weight, .. }
            | Self::SpaceAhead { weight, .. }
            | Self::TeammateBall { weight, .. }
            | Self::FormationDiscipline { weight, .. } => *weight,
        }
    }

    #[must_use]
    pub fn curve(&self) -> ResponseCurve {
        match self {
            Self::DistanceToTarget { curve, .. }
            | Self::DistanceToBall { curve, .. }
            | Self::Stamina { curve, .. }
            | Self::PitchControlAtBall { curve, .. }
            | Self::PassAngleClear { curve, .. }
            | Self::TeammateDistance { curve, .. }
            | Self::TeammateSpace { curve, .. }
            | Self::DistanceToGoal { curve, .. }
            | Self::GoalAngle { curve, .. }
            | Self::DefenderPressure { curve, .. }
            | Self::DistanceToOpponent { curve, .. }
            | Self::SkillDiff { curve, .. }
            | Self::DistanceToMarked { curve, .. }
            | Self::DefensivePosition { curve, .. }
            | Self::DistanceToPress { curve, .. }
            | Self::SpaceAhead { curve, .. }
            | Self::TeammateBall { curve, .. }
            | Self::FormationDiscipline { curve, .. } => curve.clone(),
        }
    }
}

/// Bundles the data the dispatcher would otherwise take as five
/// parameters. Held behind a reference so callers can keep their
/// perception snapshots on the stack.
pub struct ConsiderationContext<'a> {
    pub perception: &'a PerceptionSnapshot,
    pub intent: &'a Intent,
    pub stamina: f32,
    pub skill: f32,
    pub grid: Option<&'a PitchControlGrid>,
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

/// Convert a small count (nearby entities, at most a handful) to `f32`.
#[expect(
    clippy::cast_precision_loss,
    reason = "counts here are tiny (<= 8 nearby entities), far below 2^24, so the cast is exact"
)]
const fn count_to_f32(n: usize) -> f32 {
    n as f32
}

#[expect(
    clippy::type_complexity,
    clippy::too_many_lines,
    reason = "Bevy ParamSet/Query signatures are inherently verbose; this system is one linear snapshot-then-build pass"
)]
pub fn perception_system(
    mut commands: Commands,
    mut queries: ParamSet<(
        Query<(Entity, &Position, &TeamIdComponent)>,
        Query<(Entity, &Position, &TeamIdComponent)>,
        Query<&Position, With<sim_components::BallMarker>>,
        Query<&sim_components::MatchClock>,
        Query<&sim_components::Team>,
    )>,
    match_res: Res<sim_components::Match>,
    ball_res: Res<sim_components::Ball>,
) {
    // Phase C §4.3: Ball state lives on the `Ball` Resource. Ball position
    // is queried from the entity carrying `BallMarker`.
    let ball_state = ball_res.state;
    let ball_position = {
        let q = queries.p2();
        if let Ok(pos) = q.get_single() {
            pos.0
        } else {
            Vec2::zero()
        }
    };

    // Snapshot all other players' (entity, team_id, position) up front. This
    // avoids holding p1 open while iterating p0 mutably, which ParamSet
    // forbids. Phase 1 has 22 players so an O(n) snapshot per tick is fine.
    // The entity is needed so `NearbyEntity.entity` can carry the real id
    // (downstream `Tackle` / `MarkOpponent` / `Press` intents reference it).
    let mut others: Vec<(Entity, sim_components::TeamId, Vec2)> = Vec::new();
    {
        let q1 = queries.p1();
        for (other_entity, other_pos, other_team) in q1.iter() {
            others.push((other_entity, other_team.0, other_pos.0));
        }
    }

    // Phase 2: snapshot the single MatchClock up front (read-only resource
    // used to fill in match-context fields on each player — time remaining,
    // etc.). Phase C §4.3: `Match` itself is now a Resource and accessed
    // directly via the `match_res` parameter above.
    let clock = {
        let clock_q = queries.p3();
        clock_q.iter().next().cloned().unwrap_or(MatchClock {
            elapsed_ticks: 0,
            half: 1,
            added_time_ticks: 0,
            is_running: true,
        })
    };
    // Derive match-context fields from the resource.
    let (_score_diff, _time_remaining_secs, _home_id, _away_id) = {
        let diff = i16::from(match_res.score.0) - i16::from(match_res.score.1);
        let time_remaining_secs = time::match_time_remaining_secs(&clock);
        (
            diff,
            time_remaining_secs,
            sim_components::TeamId(0),
            sim_components::TeamId(1),
        )
    };

    // Snapshot mentalities by team id — two teams only, so a fixed array
    // replaces the HashMap (no allocation, no iteration order nondeterminism).
    let mut mentality_by_team: [f32; 2] = [0.0, 0.0];
    {
        let q = queries.p4();
        for team in q.iter() {
            let m = match team.mentality {
                sim_components::Mentality::Defend => -0.5,
                sim_components::Mentality::Balance => 0.0,
                sim_components::Mentality::Attack => 0.5,
            };
            if team.id.0 < 2 {
                mentality_by_team[team.id.0 as usize] = m;
            }
        }
    }

    // Team possession share: for Phase 2 this is a deterministic placeholder
    // (0.5 / 0.5). It will become a real moving window of recent touches once
    // Phase 3 / 5 add touch tracking.

    // Now iterate players via the mutable query, building and inserting
    // PerceptionSnapshot as a Component.
    let entity_data: Vec<(Entity, Vec2, sim_components::TeamId)> = {
        let q0 = queries.p0();
        q0.iter().map(|(e, pos, team)| (e, pos.0, team.0)).collect()
    };

    for (entity, player_pos, team_id) in entity_data {
        let mut nearby_teammates = smallvec::SmallVec::new();
        let mut nearby_opponents = smallvec::SmallVec::new();
        for (other_entity, other_team_id, other_pos) in &others {
            let distance = player_pos.distance(*other_pos);
            if distance <= 20.0 {
                let relative_position = *other_pos - player_pos;
                let nearby_entity = sim_components::NearbyEntity {
                    entity: *other_entity,
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
                a.distance
                    .partial_cmp(&b.distance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            },
        );
        nearby_opponents.sort_by(
            |a: &sim_components::NearbyEntity, b: &sim_components::NearbyEntity| {
                a.distance
                    .partial_cmp(&b.distance)
                    .unwrap_or(std::cmp::Ordering::Equal)
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

        // Insert or update the PerceptionSnapshot component for this player
        commands.entity(entity).insert(perception);
    }
}

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
    pitch_control: Option<Res<PitchControlGrid>>,
    clock_q: Query<&sim_components::MatchClock>,
    mut eval_count: ResMut<DecisionEvaluationCount>,
) {
    // Snapshot the clock once per system run (Bevy lets us borrow the
    // Query here even though `query` is a separate Query — disjoint
    // access on different component sets).
    let elapsed_ticks = clock_q.iter().next().map_or(0, |c| c.elapsed_ticks);
    let phase_slot = elapsed_ticks % DECISION_CADENCE_TICKS;

    for (entity, mut player, utility_brain, perception, stamina, skill, _role, _team_id) in
        &mut query
    {
        // Phase D: cadence guard. Skip if not on this player's slot.
        if player_stagger_slot(entity) != phase_slot {
            continue;
        }
        // Player passed the guard — count this evaluation.
        eval_count.0 += 1;

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
                    perception,
                    &action.intent,
                    stamina.0,
                    skill.0,
                    pitch_control.as_deref(),
                );
                let score = consideration.curve.evaluate(raw).raw();
                consideration_scores.push(score);
            }

            if consideration_scores.is_empty() {
                // No considerations ⇒ fixed baseline.
                let baseline = 0.6;
                if let Some((_, best)) = best_action {
                    if baseline > best {
                        best_action = Some((action.intent, baseline));
                    }
                } else {
                    best_action = Some((action.intent, baseline));
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
            if let Some(current) = &player.intent
                && intent_kind(current) == intent_kind(&action.intent)
            {
                aggregate += utility_brain.hysteresis;
            }

            if let Some((_, best)) = best_action {
                if aggregate > best {
                    best_action = Some((action.intent, aggregate));
                }
            } else {
                best_action = Some((action.intent, aggregate));
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
const fn intent_kind(intent: &Intent) -> &'static str {
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
#[expect(
    clippy::too_many_lines,
    reason = "flat name -> input lookup table; splitting it would only scatter the mapping"
)]
fn compute_consideration_input(
    name: &str,
    perception: &sim_components::PerceptionSnapshot,
    intent: &Intent,
    stamina: f32,
    skill: f32,
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
        // (`ball_distance` / `self_to_ball` are aliases kept for older brains.)
        "distance_to_ball" | "ball_distance" | "self_to_ball" => {
            perception.self_position.distance(perception.ball_position)
        }
        "stamina" => stamina,
        "pitch_control_at_ball" => grid.map_or(50.0, |g| {
            g.control_at(perception.ball_position.x, perception.ball_position.y) * 100.0
        }),

        // --- PassTo ---
        "pass_angle_clear" => {
            // For Phase 2: 1.0 if no opponent is between player and target,
            // linearly fading to 0.0 if a defender blocks the lane.
            if matches!(intent, Intent::PassTo) {
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
                    / count_to_f32(perception.nearby_teammates.len());
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
            count_to_f32(
                perception
                    .nearby_opponents
                    .iter()
                    .filter(|o| o.distance <= 5.0)
                    .count(),
            )
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
            (skill - 0.7).clamp(-1.0, 1.0)
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
        _ => 0.5,
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
            Intent::Tackle(target) => {
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
            Intent::PassTo => {
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
            Intent::ShootAtGoal(_) => {
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
            _ => {}
        }

        let new_velocity = steer(perception, &intent);
        velocity.0 = new_velocity * speed_modifier + direction_modifier;
    }
}

/// Speed imparted to the ball when a possessing player shoots or kicks it.
///
/// Demo-scope simplification (see `kick_execution_system`): a flat
/// speed per action type, not scaled by player skill/power.
pub const KICK_SPEED_SHOT: f32 = 22.0;
pub const KICK_SPEED_PASS: f32 = 14.0;

/// Nearest teammate's absolute position from the player's perception
/// snapshot, if any. Used to aim a `PassTo` kick at an actual teammate
/// rather than just continuing the ball in the passer's current heading.
/// Mirrors the equivalent lookup `steer()` already uses to *move* a player
/// toward the same teammate for a `PassTo` intent.
fn nearest_teammate_position(perception: &PerceptionSnapshot) -> Option<Vec2> {
    perception
        .nearby_teammates
        .iter()
        .min_by(|a, b| {
            a.distance
                .partial_cmp(&b.distance)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|t| t.relative_position + perception.self_position)
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
/// - `PassTo`: kicks toward the nearest teammate (if any), via
///   `nearest_teammate_position`. No lead/interception modeling, no
///   pass-completion mechanic beyond that — out of scope for "just
///   movement and kick/pass".
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
    mut ball: ResMut<sim_components::Ball>,
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

    let kick = match &intent {
        Intent::ShootAtGoal(target) => {
            let dir = *target - perception.self_position;
            (dir.length() > 0.01).then(|| (dir.normalized(), KICK_SPEED_SHOT))
        }
        Intent::PassTo => nearest_teammate_position(perception).and_then(|target| {
            let dir = target - perception.self_position;
            (dir.length() > 0.01).then(|| (dir.normalized(), KICK_SPEED_PASS))
        }),
        Intent::ChaseBall | Intent::Tackle(_) | Intent::Press(_) => {
            (player_vel.0.length() > 0.01).then(|| (player_vel.0.normalized(), KICK_SPEED_PASS))
        }
        _ => None,
    };

    if let Some((direction, speed)) = kick {
        ball.kick_velocity = Some(direction * speed);
        ball.possessor = None;
    }
}

/// Resolve an intent to a target point and an action-specific speed,
/// then return the unit-direction × speed vector. Target entities are
/// resolved from the player's perception snapshot (`nearby_teammates` /
/// `nearby_opponents`), which carries their world position relative to the
/// player at perception time. The ball position is read from the perception
/// snapshot directly (the perception system writes it every tick).
fn steer(perception: &PerceptionSnapshot, intent: &Intent) -> Vec2 {
    let player_pos = perception.self_position;
    let ball_pos = perception.ball_position;
    let ball_vel = Vec2::zero(); // Phase 2: ball velocity not yet exposed in perception.

    let nearest_opponent_pos = || -> Option<Vec2> {
        perception
            .nearby_opponents
            .iter()
            .min_by(|a, b| {
                a.distance
                    .partial_cmp(&b.distance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|o| o.relative_position + player_pos)
    };
    let nearest_teammate_pos = || -> Option<Vec2> {
        perception
            .nearby_teammates
            .iter()
            .min_by(|a, b| {
                a.distance
                    .partial_cmp(&b.distance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|t| t.relative_position + player_pos)
    };

    let (target, speed): (Vec2, f32) = match intent {
        Intent::MoveToPosition(target) => (*target, 5.0),
        Intent::PassTo => (nearest_teammate_pos().unwrap_or(ball_pos), 8.0),
        Intent::ShootAtGoal(target) => (*target, 10.0),
        Intent::Tackle(_) | Intent::Press(_) => {
            (nearest_opponent_pos().unwrap_or(player_pos), 10.0)
        }
        Intent::ChaseBall => (ball_pos, 8.0),
        Intent::MarkOpponent(_) => (nearest_opponent_pos().unwrap_or(player_pos), 4.0),
        Intent::Intercept => (ball_pos + ball_vel * 0.5, 9.0),
        Intent::HoldPosition => return Vec2::zero(),
        Intent::SupportRun => {
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

    /// Phase D helper: run a schedule for one full cadence window,
    /// incrementing the match clock each iteration. Every player is
    /// guaranteed at least one evaluation regardless of which slot
    /// their entity id hashes into. Returns the final `elapsed_ticks`.
    fn run_one_cadence_window(
        world: &mut World,
        schedule: &mut Schedule,
        match_entity: Entity,
    ) -> u64 {
        for _ in 0..DECISION_CADENCE_TICKS {
            if let Some(mut clock) = world
                .entity_mut(match_entity)
                .get_mut::<sim_components::MatchClock>()
            {
                clock.elapsed_ticks += 1;
            }
            schedule.run(world);
        }
        world
            .entity(match_entity)
            .get::<sim_components::MatchClock>()
            .map_or(0, |c| c.elapsed_ticks)
    }

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
            intent: None,
        });
        world
            .entity_mut(player_entity)
            .insert(Position(Vec2::new(50.0, 34.0)));
        world
            .entity_mut(player_entity)
            .insert(Velocity(Vec2::zero()));
        world.entity_mut(player_entity).insert(Stamina(0.2));
        world.entity_mut(player_entity).insert(Skill(0.8));
        world
            .entity_mut(player_entity)
            .insert(RoleComponent(Role::Striker));
        world
            .entity_mut(player_entity)
            .insert(TeamIdComponent(TeamId(0)));

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
        world
            .entity_mut(player_entity)
            .insert(sim_components::PerceptionSnapshot {
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

        // Run systems
        let mut schedule = Schedule::default();
        schedule.add_systems(consideration_scoring_system);
        schedule.add_systems(player_decision_system);

        let match_entity = world.spawn(()).id();
        world.insert_resource(sim_components::Match {
            id: 1,
            home_team: Entity::PLACEHOLDER,
            away_team: Entity::PLACEHOLDER,
            score: (0, 0),
            state: sim_components::MatchState::InPlay,
            seed: 0,
        });
        // Phase D: player_decision_system requires this resource.
        world.insert_resource(DecisionEvaluationCount::default());
        world
            .entity_mut(match_entity)
            .insert(sim_components::MatchClock {
                elapsed_ticks: 0,
                half: 1,
                added_time_ticks: 0,
                is_running: true,
            });

        // Run schedule.
        // Phase D: drive a full cadence window (6 ticks) so the player
        // is guaranteed at least one evaluation regardless of which
        // slot their entity id hashes into.
        run_one_cadence_window(&mut world, &mut schedule, match_entity);

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
            intent: None,
        });
        world
            .entity_mut(player_entity)
            .insert(Position(Vec2::new(50.0, 34.0)));
        world
            .entity_mut(player_entity)
            .insert(Velocity(Vec2::zero()));
        world.entity_mut(player_entity).insert(Stamina(0.8));
        world.entity_mut(player_entity).insert(Skill(0.8));
        world
            .entity_mut(player_entity)
            .insert(RoleComponent(Role::Striker));
        world
            .entity_mut(player_entity)
            .insert(TeamIdComponent(TeamId(0)));

        // Create a teammate in better position
        let teammate_entity = world.spawn(()).id();
        world.entity_mut(teammate_entity).insert(Player {
            team_id: TeamId(0),
            intent: None,
        });
        world
            .entity_mut(teammate_entity)
            .insert(Position(Vec2::new(80.0, 34.0)));
        world
            .entity_mut(teammate_entity)
            .insert(Velocity(Vec2::zero()));
        world.entity_mut(teammate_entity).insert(Stamina(0.9));
        world.entity_mut(teammate_entity).insert(Skill(0.7));
        world
            .entity_mut(teammate_entity)
            .insert(RoleComponent(Role::Striker));

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
        world
            .entity_mut(player_entity)
            .insert(sim_components::PerceptionSnapshot {
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

        // Run systems
        let mut schedule = Schedule::default();
        schedule.add_systems(consideration_scoring_system);
        schedule.add_systems(player_decision_system);

        let match_entity = world.spawn(()).id();
        world.insert_resource(sim_components::Match {
            id: 1,
            home_team: Entity::PLACEHOLDER,
            away_team: Entity::PLACEHOLDER,
            score: (0, 0),
            state: sim_components::MatchState::InPlay,
            seed: 0,
        });
        // Phase D: player_decision_system requires this resource.
        world.insert_resource(DecisionEvaluationCount::default());
        world
            .entity_mut(match_entity)
            .insert(sim_components::MatchClock {
                elapsed_ticks: 0,
                half: 1,
                added_time_ticks: 0,
                is_running: true,
            });

        // Run schedule.
        // Phase D: drive a full cadence window (6 ticks) so the player
        // is guaranteed at least one evaluation regardless of which
        // slot their entity id hashes into.
        run_one_cadence_window(&mut world, &mut schedule, match_entity);

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
            intent: Some(sim_components::Intent::ShootAtGoal(Vec2::new(105.0, 34.0))),
        });
        world
            .entity_mut(attacker_entity)
            .insert(Position(Vec2::new(45.0, 34.0)));
        world
            .entity_mut(attacker_entity)
            .insert(Velocity(Vec2::zero()));
        world.entity_mut(attacker_entity).insert(Stamina(0.8));
        world.entity_mut(attacker_entity).insert(Skill(0.8));
        world
            .entity_mut(attacker_entity)
            .insert(RoleComponent(Role::Striker));
        world
            .entity_mut(attacker_entity)
            .insert(TeamIdComponent(TeamId(1)));

        // Create a defender
        let defender_entity = world.spawn(()).id();
        world.entity_mut(defender_entity).insert(Player {
            team_id: TeamId(0),
            intent: None,
        });
        world
            .entity_mut(defender_entity)
            .insert(Position(Vec2::new(40.0, 34.0)));
        world
            .entity_mut(defender_entity)
            .insert(Velocity(Vec2::zero()));
        world.entity_mut(defender_entity).insert(Stamina(0.9));
        world.entity_mut(defender_entity).insert(Skill(0.7));
        world
            .entity_mut(defender_entity)
            .insert(RoleComponent(Role::CenterBack));
        world
            .entity_mut(defender_entity)
            .insert(TeamIdComponent(TeamId(0)));
        world
            .entity_mut(defender_entity)
            .insert(sim_components::PerceptionSnapshot {
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

        let match_entity = world.spawn(()).id();
        world.insert_resource(sim_components::Match {
            id: 1,
            home_team: Entity::PLACEHOLDER,
            away_team: Entity::PLACEHOLDER,
            score: (0, 0),
            state: sim_components::MatchState::InPlay,
            seed: 0,
        });
        // Phase D: player_decision_system requires this resource.
        world.insert_resource(DecisionEvaluationCount::default());
        world
            .entity_mut(match_entity)
            .insert(sim_components::MatchClock {
                elapsed_ticks: 0,
                half: 1,
                added_time_ticks: 0,
                is_running: true,
            });

        // Run systems
        let mut schedule = Schedule::default();
        schedule.add_systems(consideration_scoring_system);
        schedule.add_systems(player_decision_system);

        // Run schedule.
        // Phase D: drive a full cadence window (6 ticks) so the player
        // is guaranteed at least one evaluation regardless of which
        // slot their entity id hashes into.
        run_one_cadence_window(&mut world, &mut schedule, match_entity);

        // Check that defender has tackle intent
        let defender = world.entity(defender_entity).get::<Player>().unwrap();
        assert!(defender.intent.is_some());
    }

    /// Phase D (CODEBASE_REVIEW §7): per-player decision cadence.
    ///
    /// The decision system MUST evaluate a given player only on the ticks
    /// where `(elapsed_ticks % DECISION_CADENCE_TICKS) == player_slot(entity)`,
    /// per spec §13 #15 (default N=6 → ~10 Hz at 60 Hz physics).
    ///
    /// We drive 60 ticks with a single player. The player's slot is
    /// deterministic (depends only on entity id), so over 60 ticks the
    /// player should be evaluated exactly 10 times (60 / cadence 6).
    /// Before Phase D, the guard was missing and the count was 60.
    #[test]
    fn test_decision_cadence_does_not_re_evaluate_every_tick() {
        use sim_ai_core::ResponseCurve;
        let mut world = World::new();

        // Spawn a player + utility brain.
        let player_entity = world.spawn(()).id();
        world.entity_mut(player_entity).insert(Player {
            team_id: TeamId(0),
            intent: None,
        });
        world
            .entity_mut(player_entity)
            .insert(Position(Vec2::new(50.0, 34.0)));
        world
            .entity_mut(player_entity)
            .insert(Velocity(Vec2::zero()));
        world.entity_mut(player_entity).insert(Stamina(1.0));
        world
            .entity_mut(player_entity)
            .insert(RoleComponent(Role::Striker));
        world.entity_mut(player_entity).insert(Skill(0.7));
        world
            .entity_mut(player_entity)
            .insert(TeamIdComponent(TeamId(0)));
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
        world
            .entity_mut(player_entity)
            .insert(sim_components::PerceptionSnapshot {
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

        // Insert a Match entity so player_decision_system can compute tick.
        let match_entity = world.spawn(()).id();
        world.insert_resource(sim_components::Match {
            id: 1,
            home_team: Entity::PLACEHOLDER,
            away_team: Entity::PLACEHOLDER,
            score: (0, 0),
            state: sim_components::MatchState::InPlay,
            seed: 0,
        });
        // Phase D: player_decision_system requires this resource.
        world.insert_resource(DecisionEvaluationCount::default());
        world
            .entity_mut(match_entity)
            .insert(sim_components::MatchClock {
                elapsed_ticks: 0,
                half: 1,
                added_time_ticks: 0,
                is_running: true,
            });

        let mut schedule = Schedule::default();
        schedule.add_systems(player_decision_system);

        // Drive 60 ticks total. The first 6 are the warm-up phase
        // (pre-Phase D the test looped 6 ticks then 100 more); with
        // cadence = 6, exactly 60 / 6 = 10 of those ticks should be
        // a player's evaluation tick.
        for _ in 0..60 {
            if let Some(mut clock) = world
                .entity_mut(match_entity)
                .get_mut::<sim_components::MatchClock>()
            {
                clock.elapsed_ticks += 1;
            }
            schedule.run(&mut world);
        }

        // Phase D assertion: exactly 10 evaluations over 60 ticks at
        // cadence 6. Before Phase D this count was 60 (every tick).
        let eval_count = world.resource::<DecisionEvaluationCount>().0;
        assert_eq!(
            eval_count, 10,
            "expected exactly 10 evaluations over 60 ticks at cadence 6, got {eval_count}"
        );

        let player = world.entity(player_entity).get::<Player>().unwrap();
        assert!(player.intent.is_some());
    }

    /// Phase D regression: the cadence counter must not fire on ticks that
    /// do not match a player's slot. Spawning 22 players (a full roster)
    /// and driving 60 ticks should produce exactly 60 evaluations total
    /// (60 ticks × 1 evaluation per tick, with each tick evaluating
    /// roughly 22/6 ≈ 3-4 players) — and crucially NOT 22 × 60 = 1320.
    #[test]
    fn test_cadence_full_roster_load_distribution() {
        use sim_ai_core::ResponseCurve;
        let mut world = World::new();

        // Insert Match as Resource (Phase C §4.3).
        world.insert_resource(sim_components::Match {
            id: 1,
            home_team: Entity::PLACEHOLDER,
            away_team: Entity::PLACEHOLDER,
            score: (0, 0),
            state: sim_components::MatchState::InPlay,
            seed: 0,
        });

        // MatchClock on a single entity, as the production code does.
        let match_entity = world.spawn(()).id();
        world
            .entity_mut(match_entity)
            .insert(sim_components::MatchClock {
                elapsed_ticks: 0,
                half: 1,
                added_time_ticks: 0,
                is_running: true,
            });

        // 22-player roster (Phase 3 default), 11 per team.
        let mut player_entities = Vec::new();
        for team in 0..2u8 {
            for _ in 0..11 {
                let e = world.spawn(()).id();
                world.entity_mut(e).insert(Player {
                    team_id: TeamId(team),
                    intent: None,
                });
                world.entity_mut(e).insert(Position(Vec2::new(50.0, 34.0)));
                world.entity_mut(e).insert(Velocity(Vec2::zero()));
                world.entity_mut(e).insert(Stamina(1.0));
                world.entity_mut(e).insert(RoleComponent(Role::Striker));
                world.entity_mut(e).insert(Skill(0.7));
                world.entity_mut(e).insert(TeamIdComponent(TeamId(team)));
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
                world.entity_mut(e).insert(brain);
                world
                    .entity_mut(e)
                    .insert(sim_components::PerceptionSnapshot {
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
                player_entities.push(e);
            }
        }

        world.insert_resource(DecisionEvaluationCount::default());

        let mut schedule = Schedule::default();
        schedule.add_systems(player_decision_system);

        // Drive 60 ticks.
        for _ in 0..60 {
            if let Some(mut clock) = world
                .entity_mut(match_entity)
                .get_mut::<sim_components::MatchClock>()
            {
                clock.elapsed_ticks += 1;
            }
            schedule.run(&mut world);
        }

        let eval_count = world.resource::<DecisionEvaluationCount>().0;
        // Each tick of the 60 must evaluate at least one player (since
        // slots cycle through 0..6 every 6 ticks), but never all 22.
        // Pre-Phase D count was 60 × 22 = 1320.
        assert!(
            (60..=300).contains(&eval_count),
            "expected evaluation count in [60, 300] (per-tick ≥1, far below 1320); got {eval_count}"
        );
        assert!(
            eval_count < 22 * 60,
            "expected far fewer evaluations than full every-tick roster (got {eval_count})"
        );
    }

    /// Regression: `PerceptionSnapshot.nearby_*[].entity` must carry the real
    /// entity id, not `Entity::PLACEHOLDER`. Downstream `Tackle` /
    /// `MarkOpponent` / `Press` intents reference this id.
    #[test]
    fn test_perception_records_real_entity_ids() {
        let mut world = World::new();

        // Phase C §4.3: Match and Ball are both Resources now.
        world.insert_resource(sim_components::Match {
            id: 1,
            home_team: Entity::PLACEHOLDER,
            away_team: Entity::PLACEHOLDER,
            score: (0, 0),
            state: sim_components::MatchState::InPlay,
            seed: 0,
        });
        world.insert_resource(sim_components::Ball {
            spin: 0.0,
            state: sim_components::BallState::Free,
            possessor: None,
            last_touched_by: None,
            kick_velocity: None,
        });

        // Two opposing players within perception range.
        let home_player = world.spawn(()).id();
        world.entity_mut(home_player).insert(Player {
            team_id: TeamId(0),
            intent: None,
        });
        world
            .entity_mut(home_player)
            .insert(Position(Vec2::new(50.0, 34.0)));
        world
            .entity_mut(home_player)
            .insert(TeamIdComponent(TeamId(0)));

        let away_player = world.spawn(()).id();
        world.entity_mut(away_player).insert(Player {
            team_id: TeamId(1),
            intent: None,
        });
        world
            .entity_mut(away_player)
            .insert(Position(Vec2::new(55.0, 34.0)));
        world
            .entity_mut(away_player)
            .insert(TeamIdComponent(TeamId(1)));

        let mut schedule = Schedule::default();
        schedule.add_systems(perception_system);

        schedule.run(&mut world);

        let snapshot = world
            .entity(home_player)
            .get::<sim_components::PerceptionSnapshot>()
            .expect("perception_system must insert a snapshot");

        assert_eq!(
            snapshot.nearby_opponents.len(),
            1,
            "home player should see the away player"
        );
        assert_eq!(
            snapshot.nearby_opponents[0].entity, away_player,
            "nearby_opponents[0].entity must be the real entity, not PLACEHOLDER"
        );
        assert_ne!(
            snapshot.nearby_opponents[0].entity,
            Entity::PLACEHOLDER,
            "Entity::PLACEHOLDER must not leak into production perception snapshots"
        );
    }

    /// PR 3: the new `Consideration` enum must replace the string-dispatch
    /// `compute_consideration_input`. `DistanceToTarget` falls back to
    /// distance-to-ball when the intent has no target (e.g. `HoldPosition`),
    /// so we feed a snapshot where the player is 2.5m from the ball. The
    /// raw input is `30 - 2.5 = 27.5`, which the brief asserts is in
    /// `(25, 30)`.
    #[test]
    fn test_consideration_distance_to_target() {
        let perception = sim_components::PerceptionSnapshot {
            self_position: Vec2::new(50.0, 34.0),
            nearby_teammates: Default::default(),
            nearby_opponents: Default::default(),
            ball_position: Vec2::new(52.5, 34.0),
            ball_state: sim_components::BallState::Free,
            goal_position: Vec2::new(105.0, 34.0),
            pitch_bounds: sim_components::PitchBounds {
                distance_to_left: 50.0,
                distance_to_right: 55.0,
                distance_to_top: 34.0,
                distance_to_bottom: 34.0,
            },
        };
        let intent = sim_components::Intent::HoldPosition;
        let c = Consideration::DistanceToTarget {
            weight: 1.0,
            curve: ResponseCurve::Linear { min: 0.0, max: 1.0 },
        };
        let ctx = ConsiderationContext {
            perception: &perception,
            intent: &intent,
            stamina: 1.0,
            skill: 0.7,
            grid: None,
        };
        let raw = c.raw_input(&ctx);
        assert!(raw > 25.0 && raw < 30.0, "raw input was {raw}");
    }
}
