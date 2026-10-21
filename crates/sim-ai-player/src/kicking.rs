//! Kick execution: turns a possessing player's intent into actual ball
//! velocity.
//!
//! Before this system existed, nothing in the simulation ever wrote to the
//! ball's `Velocity` component: players could gain possession via
//! `possession_resolution_system`'s proximity check, but the ball itself
//! never moved as a result of an action, so "kick/pass" was purely
//! notional. This system closes that gap for the current demo scope only —
//! it is intentionally not real ball control:
//! - `ShootAtGoal`: kicks toward the already-resolved shot target.
//! - `PassTo`: kicks toward the nearest teammate (if any); the
//!   teammate lookup is encapsulated in `Intent::kick`. No
//!   lead/interception modeling, no pass-completion mechanic beyond
//!   that — out of scope for "just movement and kick/pass".
//! - `ChaseBall` / `Tackle` / `Press`: continues the ball in the player's
//!   current heading.

use std::sync::atomic::{AtomicU64, Ordering};

use bevy_ecs::prelude::*;
use sim_components::{
    ActionIntent, Attribute, Ball, BallMarker, Intent, LastTouch, MatchClock, PendingBallTouches,
    PerceptionSnapshot, Player, PlayerId, PlayerProfile, Position, Team, TouchKind, Velocity,
    pass_error_spread,
};
use sim_math::{Tuning, Vec2};
use sim_physics::{KICK_SPEED_DRIBBLE, KICK_SPEED_PASS, KICK_SPEED_SHOT};
use sim_rules::{CurrentTick, PendingRuleOutcome, PendingPassState, PendingShotState};
use sim_telemetry::TelemetryConfig;

/// Monotonic counter for `pass()` flow ids. Two passes share the same
/// process and could otherwise collide on `(tick, from_pid)`; the atomic
static NEXT_PASS_FLOW_ID: AtomicU64 = AtomicU64::new(1);
///
/// Runs after `player_action_execution_system`, before `Physics`.
///
/// (so it sees this tick's resolved intent and velocity)
///
/// Clears `ball.possessor` on every kick so the ball goes properly loose
/// (`BallState::InFlight`) rather than getting stuck reporting `Possessed`
/// by a player who has since moved away: `ball_physics_system` only
/// downgrades state to `Possessed`/`Free` from velocity + possessor, and
/// `possession_resolution_system` only re-resolves possession while
/// `ball.state == BallState::Free`. Without this, a kicked ball that
/// decelerates to rest before another player reaches it would silently
/// re-freeze as "possessed" by the original kicker.
#[allow(
    clippy::too_many_arguments,
    reason = "Bevy system param set: kick execution needs player/ball state, touch + pass/shot signals, tick, clock and telemetry; splitting would scatter the kick write across systems"
)]
pub fn kick_execution_system(
    player_query: Query<(
        Entity,
        &Player,
        &PerceptionSnapshot,
        &Velocity,
        Option<&PlayerProfile>,
    )>,
    ball_query: Query<&Position, With<BallMarker>>,
    team_query: Query<&Team>,
    mut ball: ResMut<Ball>,
    mut pending: ResMut<PendingBallTouches>,
    mut pending_rule_outcome: ResMut<PendingRuleOutcome>,
    current_tick: Res<CurrentTick>,
    match_clock: Res<MatchClock>,
    telemetry: Option<Res<TelemetryConfig>>,
    // Ticket 21: pass-accuracy RNG and tuning.
    mut rng: ResMut<sim_physics::SimRng>,
    tuning: Res<Tuning>,
) {
    let Some(possessor) = ball.possessor else {
        return;
    };
    // Capture the possessor's `Player` (carries `team_id`) in the same
    // pass that resolves perception/velocity — the team is denormalised
    // into `LastTouch` at the write site below. The `else` early-return
    // makes the team lookup at the write site provably infallible.
    let Some((_, possessor_player, perception, player_vel, profile)) =
        player_query.iter().find(|(e, _, _, _, _)| *e == possessor)
    else {
        return;
    };
    let Some(intent) = possessor_player.intent else {
        return;
    };

    // Ticket 25: `Possession` shortens passes and `CounterAttack`
    // lengthens them (PRD §7.2) via `pass_length_factor`, a pure numeric
    // parameter of the team's tactic. Look up the possessor's team to
    // read its tactic parameters.
    let pass_length_factor = team_query
        .iter()
        .find(|team| team.id == possessor_player.team_id)
        .map_or(1.0, |team| {
            sim_components::TacticParameters::from_setup(team.mentality, team.tactic).pass_length_factor
        });

    // ChaseBall was used to approach the ball; once possessed, the player retains
    // possession until a deliberate action (PassTo, ShootAtGoal, etc.) is chosen.
    if matches!(intent, Intent::Movement(sim_components::MovementIntent::ChaseBall)) {
        return;
    }

    // Phase F (F8): the per-variant match moved to `Intent::kick` in
    // `sim_components::intent_dispatch`. The speed multiplier depends on
    // whether the kick is a shot (uses `KICK_SPEED_SHOT`) or a pass /
    // velocity-continuation (uses `KICK_SPEED_PASS`).
    let Some(mut kick_dir) = intent.kick(perception, player_vel.0) else {
        return;
    };
    let kick_speed = match intent {
        Intent::Action(ActionIntent::ShootAtGoal(_)) => KICK_SPEED_SHOT,
        Intent::Action(ActionIntent::DribbleForward) => KICK_SPEED_DRIBBLE,
        // `pass_length_factor` scales pass length: Possession < 1, CounterAttack > 1.
        _ => KICK_SPEED_PASS * pass_length_factor,
    };

    // Ticket 21: Passing narrows the pass error spread. An inaccurate
    // pass is kicked slightly off-target, so it is less likely to reach
    // the intended receiver and the `pass` event's `to` field stays
    // `None`. Skipped for a kicker without a `PlayerProfile`.
    if matches!(intent, Intent::Action(ActionIntent::PassTo))
        && kick_dir.length() > 0.01
        && let Some(passing_profile) = profile
        && let Some(passing) = passing_profile.get(Attribute::Passing)
    {
        let pass_error = pass_error_spread(passing, 1.0, &tuning);
        if rng.gen_f32() < pass_error {
            let dev = pass_error * (3.0 / 60.0);
            kick_dir = (kick_dir
                + Vec2::new(rng.gen_range_f32(-dev, dev), rng.gen_range_f32(-dev, dev)))
            .normalized();
        }
    }

    // Preserve the original `dir.length() > 0.01` guard: if the
    // computed direction is effectively zero (e.g. a `ShootAtGoal`
    // target equal to the player, or a stationary player kicking on
    // `ChaseBall`/`Tackle`/`Press`), skip the kick entirely so we
    // don't touch `ball.kick_velocity` or clear `ball.possessor`.
    if kick_dir.length() > 0.01 {
        ball.kick_velocity = Some(kick_dir * kick_speed);
        // Register the kick as an explicit `Play` touch. Push to
        // `PendingBallTouches`; the consumer `apply_ball_touched_system`
        // applies it to `Ball.last_touch` at the start of the Rules set.
        // The same entity is recorded as both the kicker (during this
        // tick) and the last-toucher (after this tick).
        let tick = current_tick.0;
        pending.0.push(
            LastTouch::by_player(possessor_player.team_id, possessor, TouchKind::Play)
                .with_tick(tick),
        );
        ball.possessor = None;
        // PRD §9.3: a `pass` event is emitted at resolution. The
        // resolver (acquisition, OOB, goal) reads `PendingRuleOutcome.pass`
        // and emits the event with `kick_tick` and `start` (kick
        // position). Record the kick site now so the resolver
        // doesn't have to reconstruct the kick from history.
        //
        // Only `PassTo` actions register a pending pass. Shots,
        // tackles and movement-driven velocity continuations do not
        // emit `pass` events.
        //
        // A `ShootAtGoal` kick registers a pending shot instead (ticket
        // 10): the resolver (acquisition, OOB, goal) reads it and emits
        // the `shot` event — plus a `save` event when a goalkeeper makes
        // the stop — with the kick tick and position recorded here.
        if matches!(intent, Intent::Action(ActionIntent::ShootAtGoal(_))) {
            // Ball position at the kick. Same defensive `Vec2::zero()`
            // fallback as the pass arm below for test fixtures without
            // a ball entity.
            let kick_pos = ball_query
                .iter()
                .next()
                .map_or(Vec2::zero(), |p| p.0);
            pending_rule_outcome.0.shot = Some(PendingShotState {
                shooter: possessor,
                shooter_team: possessor_player.team_id,
                kick_tick: tick,
                kick_pos,
            });
            pending_rule_outcome.0.pass = None;
        } else if matches!(intent, Intent::Action(ActionIntent::PassTo)) {
            // Ball position at the kick (`start`). The position is
            // on the ball entity as a `Position` component; look it
            // up directly. If the entity is missing (test fixture
            // without a ball entity), leave `start` as zero — the
            // event still emits with valid `from` / `to` fields.
            let kick_pos = ball_query
                .iter()
                .next()
                .map_or(Vec2::zero(), |p| p.0);
            let kick_tick = match_clock.elapsed_ticks;
            pending_rule_outcome.0.pass = Some(PendingPassState {
                from: PlayerId::from_entity(possessor),
                from_team: possessor_player.team_id,
                intended_to: perception
                    .nearby_teammates
                    .first()
                    .map(|t| PlayerId::from_entity(t.entity)),
                kick_tick,
                kick_position: kick_pos,
            });
            // A pass kick supersedes any stale pending shot (defensive;
            // the shot resolver normally drains first — see above).
            pending_rule_outcome.0.shot = None;
        } else {
            // Any non-pass kick cancels a stale pending pass. This
            // shouldn't happen in normal play (a player with the
            // ball has only one intent at a time) but the defensive
            // cancel keeps the resolver from emitting a `pass` for
            // a shot.
            pending_rule_outcome.0.pass = None;
            // Same for a stale pending shot (defensive; the shot
            // resolver normally drains first — see above).
            pending_rule_outcome.0.shot = None;
        }
        // Emit a `pass` flow event when telemetry is on and this was a
        // pass. Shots / tackles don't carry identity of a "receiver", so
        // we restrict to `PassTo`.
        if let (true, Some(receiver_entity)) = (
            telemetry.is_some() && matches!(intent, Intent::Action(ActionIntent::PassTo)),
            perception.nearby_teammates.first().map(|t| t.entity),
        ) {
            let tick = match_clock.elapsed_ticks;
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
