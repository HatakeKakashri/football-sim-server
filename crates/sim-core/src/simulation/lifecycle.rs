//! `lifecycle_system`: the `MatchState` transition driver, ball placement
//! at restart, and `apply_player_slot` (formation reset helper).
//!
//! Decomposed (Phase F follow-up): the per-tick driver loops `decide_transition`
//! and `apply_transition` until the state stabilises. The split isolates the
//! pure decision from the side effects (commit the new state, place the
//! ball, update the clock, reset formation).

use bevy_ecs::prelude::*;
use sim_components::time::{HALF_LENGTH_TICKS, HALFTIME_BREAK_TICKS};
use sim_components::{
    LastTouch, Match, MatchClock, MatchState, PendingBallTouches, Position, Referee, Team, TouchKind,
    Velocity,
};
use sim_math::Vec2;

use super::formation::reset_formation_to_4_4_2;

/// Tracks the simulation tick at which the match entered the `HalfTime`
/// state. `None` means we are not currently in `HalfTime`. Used to time out
/// the half-time break using the sim-tick counter rather than the paused
/// match clock (which doesn't accumulate `elapsed` while frozen).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct HalfTimeEntryTick {
    pub tick: Option<u64>,
}

/// First-half added time in ticks, stashed at the half-time whistle.
///
/// The `MatchClock` is per-half and its `added_time_ticks` is
/// overwritten by the second half, so the `full_time` event reads the
/// first half's value back from here to report per-half added time
/// (PRD §9.2). Ticket 11 owns the added-time semantics; this resource
/// is only the carrier.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct FirstHalfAddedTime(pub u64);

/// Place a single player at `target`, zero velocity, reset intent to
/// `HoldPosition`. Mirrors the Position into Player.position so any system
/// that reads the player component sees the same value.
///
/// Phase 2: leave intent as `None` so the player's first decision cycle
/// isn't dominated by a hysteresis bonus on `HoldPosition`. The
/// `player_decision_system` will set the first real intent on its cadence
/// tick.
pub fn apply_player_slot(world: &mut World, player_entity: Entity, target: Vec2) {
    if let Some(mut pos) = world.entity_mut(player_entity).get_mut::<Position>() {
        pos.0 = target;
    }
    if let Some(mut vel) = world.entity_mut(player_entity).get_mut::<Velocity>() {
        vel.0 = Vec2::zero();
    }
    if let Some(mut p) = world
        .entity_mut(player_entity)
        .get_mut::<sim_components::Player>()
    {
        p.intent = None;
    }
    if let Some(mut hs) = world
        .entity_mut(player_entity)
        .get_mut::<sim_components::HomeSlot>()
    {
        hs.set_position(target);
    }
}

/// Pick the kickoff-taker entity for `team_id`:
/// - prefer a player with `RoleComponent(Role::Striker)`,
/// - fall back to the last player on the team by entity bits (which after
///   `reset_formation_to_4_4_2` lands on a forward slot),
/// - fall back to `Entity::PLACEHOLDER` if the team is empty.
///
/// This helper is deterministic and tolerates the current state where
/// no player carries `Role::Striker` (every spawned player is
/// `Role::CentralMidfielder`).
fn pick_kickoff_taker(world: &mut World, team_id: sim_components::TeamId) -> bevy_ecs::prelude::Entity {
    use sim_components::{Player, Role, RoleComponent, TeamIdComponent};

    let mut striker: Option<bevy_ecs::prelude::Entity> = None;
    let mut last_player: Option<bevy_ecs::prelude::Entity> = None;

    // Bevy `Query` iterates in spawn (creation) order — the same
    // ordering `World::iter_entities` provides, so determinism is
    // preserved (§9: prefer a typed query over `iter_entities`
    // reach-in).
    let mut query = world.query::<(
        Entity,
        &Player,
        &TeamIdComponent,
        Option<&RoleComponent>,
    )>();
    for (entity, _player, team, role) in query.iter(world) {
        if team.0 != team_id {
            continue;
        }
        let role = role.copied().map_or(Role::CentralMidfielder, |rc| rc.0);
        let bits = entity.to_bits();
        if role == Role::Striker {
            // Among strikers, pick the smallest entity id for
            // determinism.
            striker = match striker {
                Some(s) if s.to_bits() <= bits => Some(s),
                _ => Some(entity),
            };
        }
        // Track the largest entity id (last on the team by creation
        // order) — this is the fallback kickoff taker.
        last_player = match last_player {
            Some(l) if l.to_bits() >= bits => Some(l),
            _ => Some(entity),
        };
    }

    striker
        .or(last_player)
        .unwrap_or(bevy_ecs::prelude::Entity::PLACEHOLDER)
}

/// Pure decision: given the current state + clock + sim-tick + half-time entry,
/// compute the next state and the side-effect descriptors (clock mutation,
/// ball placement, kickoff impulse). Does not mutate the world.
#[derive(Debug, Clone)]
pub struct TransitionDecision {
    pub next_state: MatchState,
    pub new_clock: Option<MatchClock>,
    pub place_ball_center: bool,
    pub apply_kickoff_impulse: bool,
}

impl TransitionDecision {
    const fn stable(next_state: MatchState) -> Self {
        Self {
            next_state,
            new_clock: None,
            place_ball_center: false,
            apply_kickoff_impulse: false,
        }
    }
}

/// Decide whether the match state machine should advance from `current_state`
/// given the current clock + sim-tick. Pure function — no world mutation.
#[allow(
    clippy::too_long_first_doc_paragraph,
    reason = "single doc paragraph documents the entire state-machine decision table"
)]
pub fn decide_transition(
    current_state: MatchState,
    clock: Option<MatchClock>,
    sim_tick: u64,
    half_time_entry_tick: Option<u64>,
) -> TransitionDecision {
    match current_state {
        MatchState::PreMatch => TransitionDecision {
            next_state: MatchState::Kickoff,
            new_clock: None,
            place_ball_center: true,
            apply_kickoff_impulse: false,
        },
        MatchState::Kickoff => TransitionDecision {
            next_state: MatchState::InPlay,
            new_clock: None,
            place_ball_center: true,
            apply_kickoff_impulse: true,
        },
        MatchState::InPlay => {
            let Some(c) = clock else {
                return TransitionDecision::stable(current_state);
            };
            // Both halves use the same per-half length: `elapsed_ticks` is
            // reset to 0 at the second-half kickoff, so there is no
            // "cumulative 90 minutes" quantity to compare against here.
            let in_play_threshold = HALF_LENGTH_TICKS + c.added_time_ticks;
            let is_final_half = c.half >= 2;
            if c.elapsed_ticks < in_play_threshold {
                return TransitionDecision::stable(current_state);
            }
            let next_state = if is_final_half {
                MatchState::FullTime
            } else {
                MatchState::HalfTime
            };
            let mut updated = c;
            updated.is_running = false;
            TransitionDecision {
                next_state,
                new_clock: Some(updated),
                place_ball_center: false,
                apply_kickoff_impulse: false,
            }
        }
        MatchState::HalfTime => {
            // If we just entered HalfTime, this is the first tick of the break.
            // Caller is responsible for persisting the entry tick on the resource
            // (see the driver); here we only read it.
            let entry_tick = half_time_entry_tick.unwrap_or(sim_tick);
            let in_halftime_for = sim_tick.saturating_sub(entry_tick);
            if in_halftime_for < HALFTIME_BREAK_TICKS {
                return TransitionDecision::stable(current_state);
            }
            let new_clock = clock.map(|_| MatchClock {
                elapsed_ticks: 0,
                half: 2,
                // Added time is per-half (PRD §8.4): the first half's
                // value is stashed in `FirstHalfAddedTime` at the
                // whistle (see `emit_transition_event`); the second
                // half starts from zero and accrues its own.
                added_time_ticks: 0,
                is_running: true,
            });
            TransitionDecision {
                next_state: MatchState::Kickoff,
                new_clock,
                place_ball_center: true,
                apply_kickoff_impulse: false,
            }
        }
        MatchState::FullTime | MatchState::Stoppage => TransitionDecision::stable(current_state),
    }
}

/// Team that takes the kickoff for the current half: first half → home
/// (team 0), second half → away (team 1), per Law 8.
fn kickoff_taker_team(world: &World) -> sim_components::TeamId {
    match world.get_resource::<MatchClock>() {
        Some(clock) if clock.half >= 2 => sim_components::TeamId(1),
        _ => sim_components::TeamId(0),
    }
}

/// Apply a `TransitionDecision` to the world: write the new clock, place the
/// ball at the center spot if requested, apply the kickoff impulse if
/// requested, reset the formation when entering `Kickoff`, and commit the
/// new match state.
///
/// `sim_tick` is the global tick the driver is settling (used for touch
/// timestamps and lifecycle event envelopes).
pub fn apply_transition(
    decision: &TransitionDecision,
    current_state: MatchState,
    match_entity: Entity,
    ball_entity: Entity,
    sim_tick: u64,
    world: &mut World,
) {
    // Apply clock mutation. Phase F follow-up: MatchClock is a Resource
    // (spec §3). Write to the Resource form (source of truth); the
    // Component form on the match entity is kept in sync for backwards
    // compat with tests + external consumers.
    if let Some(clock) = decision.new_clock.clone() {
        if let Some(mut existing) = world.get_resource_mut::<MatchClock>() {
            *existing = clock.clone();
        } else {
            world.insert_resource(clock.clone());
        }
        world.entity_mut(match_entity).insert(clock);
    }

    // Apply ball placement / kickoff impulse.
    if decision.place_ball_center {
        if let Some(mut pos) = world.entity_mut(ball_entity).get_mut::<Position>() {
            pos.0 = Vec2::new(52.5, 34.0);
        }
        if let Some(mut vel) = world.entity_mut(ball_entity).get_mut::<Velocity>() {
            vel.0 = Vec2::zero();
        }
        // Phase C §4.3: Ball state lives on the Resource, not on the entity.
        world.resource_mut::<sim_components::Ball>().state = sim_components::BallState::Free;
        // Drop any pending kick velocity — placement at the center
        // spot must not be combined with an inbound kick impulse.
        world.resource_mut::<sim_components::Ball>().kick_velocity = None;
        // Register the kickoff placement as a `Restart` touch on the
        // team's striker (preferring a player with
        // `RoleComponent(Role::Striker)`, falling back to the last
        // player on the team). The kickoff taker alternates by half
        // (see `kickoff_taker_team`); the end swap itself happens via
        // `attack_direction` below.
        let kickoff_taker_team = kickoff_taker_team(world);
        let taker_entity = pick_kickoff_taker(world, kickoff_taker_team);
        // `pick_kickoff_taker` returns `Entity::PLACEHOLDER` when the
        // team has no players; `from_taker` collapses the placeholder
        // case to a team-only touch.
        let touch =
            LastTouch::from_taker(kickoff_taker_team, taker_entity, TouchKind::Restart)
                .with_tick(sim_tick);
        world.resource_mut::<PendingBallTouches>().0.push(touch);
    }
    if decision.apply_kickoff_impulse {
        let kickoff_taker_team = kickoff_taker_team(world);
        let taker_dir = world
            .query::<&Team>()
            .iter(world)
            .find(|team| team.id == kickoff_taker_team)
            .map_or(1, Team::attack_direction);
        if let Some(mut vel) = world.entity_mut(ball_entity).get_mut::<Velocity>() {
            vel.0 = Vec2::new(2.0 * f32::from(taker_dir), 0.0);
        }
    }

    // Clear the HalfTime entry-tick resource once we've left HalfTime.
    // Also flip every team's attack_direction so they attack the opposite
    // end in the second half (Law 8: teams switch ends at half-time).
    // This MUST run before `reset_formation_to_4_4_2` so the formation reset
    // uses the second half's slot tables.
    if current_state == MatchState::HalfTime && decision.next_state != MatchState::HalfTime {
        world.insert_resource(HalfTimeEntryTick { tick: None });
        // Bevy `Query` iterates in spawn (creation) order — the same
        // ordering `World::iter_entities` provides, so determinism is
        // preserved (§9: prefer a typed query over `iter_entities`
        // reach-in).
        let mut team_query = world.query::<&mut Team>();
        for mut team in team_query.iter_mut(world) {
            let flipped = -team.attack_direction();
            team.set_attack_direction(flipped);
        }
        // Added time is per-half (PRD §8.4): the first half's stoppage
        // queue was stashed into `FirstHalfAddedTime` at the whistle, so
        // clear it here for the second half's own accrual. Worlds without
        // a referee (sparse unit fixtures) simply skip.
        let mut referee_query = world.query::<&mut Referee>();
        for mut referee in referee_query.iter_mut(world) {
            referee.stoppage_events.clear();
        }
    }

    // Reset 4-4-2 formation whenever we enter Kickoff.
    if decision.next_state == MatchState::Kickoff && current_state != MatchState::Kickoff {
        reset_formation_to_4_4_2(world);
        if world
            .get_resource::<sim_telemetry::TelemetryConfig>()
            .is_some()
        {
            let tick = world
                .get_resource::<sim_rules::CurrentTick>()
                .map_or(0, |t| t.0);
            sim_telemetry::emit::kickoff(tick);
        }
    }

    // Commit state mutation.
    world.resource_mut::<Match>().state = decision.next_state;

    // Structured lifecycle events (ticket 10): `kickoff` on half
    // starts, `half_time` at the whistle, `full_time` at the end.
    // Emitted after the commit so the envelopes read the
    // post-transition clock (half 2 for the second-half kickoff).
    emit_transition_event(world, current_state, decision.next_state, sim_tick);
}

/// Per-team attack directions as `(home, away)`.
///
/// Falls back to the construction-time convention (home `+1`, away
/// `-1`) in worlds without both teams (sparse unit fixtures). A live
/// `Simulation` always has both teams, so the fallback never fires
/// there.
fn attack_directions(world: &mut World) -> (i8, i8) {
    let mut home_dir = 1;
    let mut away_dir = -1;
    let mut query = world.query::<&Team>();
    for team in query.iter(world) {
        if team.id == sim_components::TeamId(0) {
            home_dir = team.attack_direction();
        } else if team.id == sim_components::TeamId(1) {
            away_dir = team.attack_direction();
        }
    }
    (home_dir, away_dir)
}

/// Append the structured event for one lifecycle transition, if the
/// transition earns one (ticket 10).
///
/// - entering `Kickoff`: `kickoff` with `reason = match_start`
///   (from `PreMatch`) or `second_half` (from `HalfTime`), the taker
///   team per Law 8, and both attack directions (ends are set for the
///   half by now — the half-time flip runs before this call).
/// - entering `HalfTime`: `half_time` with the running score; stashes
///   the first half's added time for the `full_time` report.
/// - entering `FullTime`: `full_time` with the final score, the total
///   event count including itself, the state hash, and per-half added
///   time.
///
/// Worlds without an `EventLog` resource (unit-test fixtures built by
/// hand) skip emission: the state transition itself still commits.
fn emit_transition_event(world: &mut World, from: MatchState, to: MatchState, sim_tick: u64) {
    use super::events::{
        EventLog, build_full_time_event, build_half_time_event, build_kickoff_event,
    };
    use sim_components::{KickoffReason, PlayerId};

    // No log (hand-built test world) → transition only, no event.
    let seq = match world.get_resource::<EventLog>() {
        Some(log) => log.next_seq,
        None => return,
    };
    let clock = world.get_resource::<MatchClock>().cloned().unwrap_or(MatchClock {
        elapsed_ticks: 0,
        half: 1,
        added_time_ticks: 0,
        is_running: false,
    });

    // Build the event first (immutable borrows only), then append.
    // The `full_time` hash reads the whole world, so it must run
    // before the log's mutable borrow starts.
    let event = match to {
        MatchState::Kickoff => {
            let reason = match from {
                MatchState::HalfTime => KickoffReason::SecondHalf,
                _ => KickoffReason::MatchStart,
            };
            let taker_team = kickoff_taker_team(world);
            let taker_entity = pick_kickoff_taker(world, taker_team);
            let taker = if taker_entity == Entity::PLACEHOLDER {
                None
            } else {
                Some(PlayerId::from_entity(taker_entity))
            };
            let (home_dir, away_dir) = attack_directions(world);
            Some(build_kickoff_event(
                seq,
                sim_tick,
                &clock,
                taker_team,
                taker,
                reason,
                Some(home_dir),
                Some(away_dir),
            ))
        }
        MatchState::HalfTime if from == MatchState::InPlay => {
            let score = world
                .get_resource::<Match>()
                .map_or((0, 0), |m| m.score);
            world.insert_resource(FirstHalfAddedTime(clock.added_time_ticks));
            Some(build_half_time_event(seq, sim_tick, &clock, score))
        }
        MatchState::FullTime if from == MatchState::InPlay => {
            let (score, seed) = world
                .get_resource::<Match>()
                .map_or(((0, 0), 0), |m| (m.score, m.seed));
            let state_hash = super::state_hash::hash_world(world, seed);
            let first_half_added = world
                .get_resource::<FirstHalfAddedTime>()
                .map_or(0, |r| r.0);
            // `seq` is the seq this event is about to take, so the
            // total including itself is `seq + 1`.
            Some(build_full_time_event(
                seq,
                sim_tick,
                &clock,
                score,
                seq + 1,
                state_hash,
                (first_half_added, clock.added_time_ticks),
            ))
        }
        _ => None,
    };

    if let Some(event) = event
        && let Some(mut log) = world.get_resource_mut::<EventLog>()
    {
        log.append(event);
    }
}

/// Match state machine driver. Runs once per tick after `schedule.run()` and
/// before `tick_clock`. Loops `decide_transition` + `apply_transition` until
/// the state stabilises.
///
/// Note: the `PreMatch` -> `Kickoff` -> `InPlay` chain settles on tick 1,
/// so the loop is bounded.
pub fn lifecycle_system(
    match_entity: Entity,
    ball_entity: Entity,
    sim_tick: u64,
    world: &mut World,
) {
    const MAX_TRANSITIONS_PER_TICK: usize = 8;

    for _iteration in 0..MAX_TRANSITIONS_PER_TICK {
        let current_state = world.resource::<Match>().state;
        // Phase F follow-up: MatchClock is a Resource. Read from the
        // Resource form (source of truth).
        let clock = world.get_resource::<MatchClock>().cloned();

        // HalfTime entry-tick persistence: on the first HalfTime tick the
        // entry resource is None; the decision reads it to compute
        // `in_halftime_for`. We persist here in the driver (not in the pure
        // decision) so `decide_transition` stays a pure function.
        if current_state == MatchState::HalfTime {
            let mut entry = world
                .get_resource::<HalfTimeEntryTick>()
                .copied()
                .unwrap_or_default();
            if entry.tick.is_none() {
                entry.tick = Some(sim_tick);
                world.insert_resource(entry);
                continue;
            }
        }

        let entry_tick = world
            .get_resource::<HalfTimeEntryTick>()
            .and_then(|e| e.tick);

        let decision = decide_transition(current_state, clock, sim_tick, entry_tick);

        if decision.next_state == current_state {
            return;
        }

        apply_transition(
            &decision,
            current_state,
            match_entity,
            ball_entity,
            sim_tick,
            world,
        );
    }
}
