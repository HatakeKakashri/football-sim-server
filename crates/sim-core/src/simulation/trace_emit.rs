//! Telemetry wiring for `Simulation`: enabling, the per-tick gate, and the
//! position/ball/referee snapshot system.
//!
//! Everything here is a read-only side channel: no system in this module
//! mutates simulation components, and with no `TelemetryConfig` resource the
//! gate stays all-false so nothing is collected or emitted.

use bevy_ecs::prelude::*;
use sim_ai_player::DECISION_CADENCE_TICKS;
use sim_components::{
    Ball, BallMarker, BallState, MatchClock, Player, Position, Referee, RoleComponent, Stamina,
    TeamIdComponent, Velocity,
};
use sim_telemetry::emit::{
    self, PID_BALL, PID_REFEREE, TID_POSITION, Track, player_pid, player_process_name,
};
use sim_telemetry::{TelemetryConfig, TraceGate};

use super::{Simulation, SimulationSet};

impl Simulation {
    /// Turn on post-match tracing with `config`. Without this call the gate
    /// stays all-false and no trace code runs.
    pub fn enable_telemetry(&mut self, config: TelemetryConfig) {
        self.world.insert_resource(config);
        self.world.insert_resource(TraceGate::default());
        // Referee stoppage events are queued by the Rules set and drained by
        // `added_time_calculation_system`; emit after Rules, before the drain.
        self.schedule.add_systems(
            trace_emission_system
                .in_set(SimulationSet::MatchAdmin)
                .before(sim_rules::added_time_calculation_system),
        );
    }
}

/// Recompute the gate for the clock tick about to run. No-op when telemetry
/// was never enabled.
pub(super) fn advance_trace_gate(world: &mut World) {
    let Some(config) = world.get_resource::<TelemetryConfig>().cloned() else {
        return;
    };
    let tick = world
        .get_resource::<MatchClock>()
        .map_or(0, |clock| clock.elapsed_ticks);
    if let Some(mut gate) = world.get_resource_mut::<TraceGate>() {
        gate.advance(tick, &config, DECISION_CADENCE_TICKS);
    }
}

/// What was last emitted, so change events fire once per change.
#[derive(Default)]
pub struct ChangeTracker {
    /// Last emitted `(state, possessor)`; `None` until the first snapshot
    /// establishes the baseline.
    ball: Option<(BallState, Option<Entity>)>,
    /// `Referee.cards` persists, so only cards past this count are new.
    /// (`stoppage_events` needs no such counter: it is drained every tick.)
    cards_seen: usize,
}

const BALL_TRACK: Track<'static> = Track {
    pid: PID_BALL,
    tid: 1,
    process: "Ball",
    thread: "State",
};

const REFEREE_TRACK: Track<'static> = Track {
    pid: PID_REFEREE,
    tid: 1,
    process: "Referee",
    thread: "Events",
};

fn possessor_label(possessor: Option<Entity>) -> String {
    possessor.map_or_else(|| "none".to_string(), |e| e.index().to_string())
}

/// Emit player/ball/referee snapshots on recorded ticks.
#[allow(clippy::type_complexity, reason = "Bevy Query signature")]
pub fn trace_emission_system(
    gate: Option<Res<TraceGate>>,
    players: Query<(
        Entity,
        &Player,
        &RoleComponent,
        &TeamIdComponent,
        &Position,
        &Velocity,
        &Stamina,
    )>,
    ball: Option<Res<Ball>>,
    ball_body: Query<(&Position, &Velocity), With<BallMarker>>,
    referees: Query<&Referee>,
    mut tracker: Local<ChangeTracker>,
) {
    let Some(gate) = gate.filter(|g| g.snapshot()) else {
        return;
    };
    let tick = gate.tick();

    for (entity, _player, role, team, position, velocity, stamina) in &players {
        let index = entity.index();
        let process = player_process_name(team.0.0, index, &role.0);
        let track = Track {
            pid: player_pid(index),
            tid: TID_POSITION,
            process: &process,
            thread: "Position",
        };
        emit::counter(track, tick, "x", position.0.x);
        emit::counter(track, tick, "y", position.0.y);
        emit::counter(track, tick, "vx", velocity.0.x);
        emit::counter(track, tick, "vy", velocity.0.y);
        emit::counter(track, tick, "stamina", stamina.0);
    }

    if let (Some(ball), Some((position, velocity))) = (ball, ball_body.iter().next()) {
        emit::counter(BALL_TRACK, tick, "x", position.0.x);
        emit::counter(BALL_TRACK, tick, "y", position.0.y);
        emit::counter(BALL_TRACK, tick, "vx", velocity.0.x);
        emit::counter(BALL_TRACK, tick, "vy", velocity.0.y);
        emit::counter(BALL_TRACK, tick, "spin", ball.spin);

        // The first snapshot only establishes the baseline.
        if let Some((previous_state, previous_possessor)) = tracker.ball {
            if previous_state != ball.state {
                emit::state_change(
                    BALL_TRACK,
                    tick,
                    &format!("{previous_state:?}"),
                    &format!("{:?}", ball.state),
                );
            }
            if previous_possessor != ball.possessor {
                emit::possession_change(
                    BALL_TRACK,
                    tick,
                    &possessor_label(previous_possessor),
                    &possessor_label(ball.possessor),
                );
            }
        }
        tracker.ball = Some((ball.state, ball.possessor));
    }

    // At most one `Referee` is expected (spec); absence is not an error.
    if let Some(referee) = referees.iter().next() {
        for card in referee.cards.iter().skip(tracker.cards_seen) {
            emit::card(
                REFEREE_TRACK,
                tick,
                u64::from(card.player.index()),
                &format!("{:?}", card.color),
                card.tick,
            );
        }
        tracker.cards_seen = referee.cards.len();
        // `added_time_calculation_system` drains this queue every tick and runs
        // after this system, so everything in it was queued since the last
        // drain and is reported exactly once. A running "already seen" count
        // would wrongly skip new events once the queue has been emptied.
        for event in &referee.stoppage_events {
            emit::stoppage(REFEREE_TRACK, tick, &event.reason, event.tick);
        }
    }
}
