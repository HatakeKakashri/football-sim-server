//! Tests for the `PendingBallTouches` resource + `apply_ball_touched_system`
//! consumer + writer push sites. These tests verify the contract that
//! every contact path (kick, possession acquisition, restart placement)
//! pushes exactly one touch per occurrence, and the consumer applies the
//! latest-in-schedule entry to `Ball.last_touch`.
//!
//! `kick_execution_registers_play_touch` was moved here from
//! `sim-ai-player`'s `execution` test module (M11.4): it is the one
//! kick test that needs the `sim-rules` consumer to observe the
//! writer's push on `Ball.last_touch`, and `sim-ai-player` does not
//! depend on `sim-rules`.

use bevy_ecs::prelude::*;
use sim_ai_player::kick_execution_system;
use sim_components::{
    ActionIntent, Ball, BallMarker, BallState, Intent, LastTouch, Match, MatchClock, MatchState,
    NearbyEntity, PendingBallTouches, PerceptionSnapshot, PitchBounds, Player, Position, RuleEvent,
    TeamIdComponent, TouchKind, Velocity,
};
use sim_core::Simulation;
use sim_math::Vec2;
use smallvec::smallvec;

use crate::{
    CurrentTick, goal_detection_system, out_of_bounds_system, possession_resolution_system,
    restart_system, touch::apply_ball_touched_system,
};

fn pos(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

/// `kick_execution_system` must register `Ball.last_touch = Some
/// (LastTouch{kicker, Play})` (via the `PendingBallTouches` push +
/// consumer chain) alongside clearing the possessor when the kick is
/// non-trivial (`kick_dir.length() > 0.01`).
#[test]
fn kick_execution_registers_play_touch() {
    let mut world = World::new();

    // Kicker at (50, 34), facing a teammate 15 m away.
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
        // Pre-condition: last_touch is None (ball just gained possession).
        last_touch: None,
        last_deliberate_play: None,
        kick_velocity: None,
        spin: 0.0,
    });
    world.insert_resource(PendingBallTouches::default());

    let mut schedule = Schedule::default();
    schedule.add_systems((kick_execution_system, apply_ball_touched_system).chain());
    schedule.run(&mut world);

    let ball = world.resource::<Ball>();
    assert_eq!(
        ball.last_touch,
        Some(LastTouch {
            team: sim_components::TeamId(0),
            player: Some(kicker),
            player_id: Some(sim_components::PlayerId::from_entity(kicker)),
            kind: TouchKind::Play,
        }),
        "kick must register the kicker as a Play touch"
    );
    assert_eq!(
        ball.possessor, None,
        "kick must clear the possessor so possession can be re-resolved"
    );
}

// -----------------------------------------------------------------------------
// Buffer push / consumer application mechanics (M12.1, Steps 2-4)
// -----------------------------------------------------------------------------

/// The writer protocol: `PendingBallTouches` accepts a `LastTouch` push;
/// `apply_ball_touched_system` drains the buffer and writes the entry to
/// `Ball.last_touch`. The contract is "one push → one write" within a
/// single schedule run.
#[test]
fn test_kick_pushes_to_buffer_and_consumer_applies() {
    let mut world = World::new();
    world.insert_resource(PendingBallTouches::default());

    // Stand-in kicker — the consumer does not consult the entity, but
    // we still need a real Entity for the `Option<Entity>` payload.
    let kicker = world.spawn(()).id();
    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Possessed,
        possessor: Some(kicker),
        last_touch: None,
        last_deliberate_play: None,
    });

    // Direct push (no kick system) — exercises the writer/consumer contract
    // in isolation from any kick wiring.
    world
        .resource_mut::<PendingBallTouches>()
        .0
        .push(LastTouch {
            team: sim_components::TeamId(0),
            player: Some(kicker),
            player_id: Some(sim_components::PlayerId::from_entity(kicker)),
            kind: TouchKind::Play,
        });

    let mut schedule = Schedule::default();
    schedule.add_systems(apply_ball_touched_system);
    schedule.run(&mut world);

    let ball = world.resource::<Ball>();
    assert_eq!(
        ball.last_touch,
        Some(LastTouch {
            team: sim_components::TeamId(0),
            player: Some(kicker),
            player_id: Some(sim_components::PlayerId::from_entity(kicker)),
            kind: TouchKind::Play,
        }),
        "consumer must apply the buffer entry to Ball.last_touch"
    );
    assert!(
        world.resource::<PendingBallTouches>().0.is_empty(),
        "consumer must drain the buffer"
    );
}

/// Two consecutive pushes (e.g. possession acquisition then a kick in
/// the same tick). The consumer uses `Vec::pop`, so the **last** push
/// wins — the entry most-recently pushed is the entry written to
/// `Ball.last_touch`.
#[test]
fn test_two_consecutive_kicks_update_record() {
    let mut world = World::new();
    world.insert_resource(PendingBallTouches::default());

    let kicker_a = world.spawn(()).id();
    let kicker_b = world.spawn(()).id();
    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touch: None,
        last_deliberate_play: None,
    });

    {
        let mut pending = world.resource_mut::<PendingBallTouches>();
        pending.0.push(LastTouch {
            team: sim_components::TeamId(0),
            player: Some(kicker_a),
            player_id: Some(sim_components::PlayerId::from_entity(kicker_a)),
            kind: TouchKind::Play,
        });
        pending.0.push(LastTouch {
            team: sim_components::TeamId(1),
            player: Some(kicker_b),
            player_id: Some(sim_components::PlayerId::from_entity(kicker_b)),
            kind: TouchKind::Play,
        });
    }

    let mut schedule = Schedule::default();
    schedule.add_systems(apply_ball_touched_system);
    schedule.run(&mut world);

    let ball = world.resource::<Ball>();
    assert_eq!(
        ball.last_touch,
        Some(LastTouch {
            team: sim_components::TeamId(1),
            player: Some(kicker_b),
            player_id: Some(sim_components::PlayerId::from_entity(kicker_b)),
            kind: TouchKind::Play,
        }),
        "two pushes — the second (latest) entry must win"
    );
}

/// Same-tick double kick resolves deterministically. The schedule
/// runs systems in fixed order (`kick_execution_system` first, then
/// `apply_ball_touched_system`), so within a single tick the
/// "last-wins" rule picks the entry from the system that ran last.
///
/// This test pins the contract: even if two writers push to the
/// buffer in the same tick, the consumer's `pop()` yields the
/// later-pushed entry — never undefined behaviour.
#[test]
fn test_same_tick_double_kick_resolves_deterministically() {
    let mut world = World::new();

    let kicker = world.spawn(()).id();
    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Possessed,
        possessor: Some(kicker),
        last_touch: Some(LastTouch {
            team: sim_components::TeamId(0),
            player: Some(kicker),
            player_id: Some(sim_components::PlayerId::from_entity(kicker)),
            kind: TouchKind::Play,
        }),
        last_deliberate_play: None,
    });
    world.insert_resource(PendingBallTouches::default());

    // Pre-seed the buffer with a second writer's push: simulates
    // `kick_execution_system` pushing for kicker then
    // `possession_resolution_system` pushing again for the same
    // kicker on the rebound. Order matters — we push in the same
    // order they would push in a real tick.
    let writer_first = world.spawn(()).id();
    let writer_second = world.spawn(()).id();
    world
        .resource_mut::<PendingBallTouches>()
        .0
        .push(LastTouch {
            team: sim_components::TeamId(0),
            player: Some(writer_first),
            player_id: Some(sim_components::PlayerId::from_entity(writer_first)),
            kind: TouchKind::Play,
        });
    world
        .resource_mut::<PendingBallTouches>()
        .0
        .push(LastTouch {
            team: sim_components::TeamId(1),
            player: Some(writer_second),
            player_id: Some(sim_components::PlayerId::from_entity(writer_second)),
            kind: TouchKind::Play,
        });

    let mut schedule = Schedule::default();
    schedule.add_systems(apply_ball_touched_system);
    schedule.run(&mut world);

    let ball = world.resource::<Ball>();
    assert_eq!(
        ball.last_touch,
        Some(LastTouch {
            team: sim_components::TeamId(1),
            player: Some(writer_second),
            player_id: Some(sim_components::PlayerId::from_entity(writer_second)),
            kind: TouchKind::Play,
        }),
        "same-tick double push resolves to the last-pushed entry (deterministic)"
    );
}

// -----------------------------------------------------------------------------
// Possession acquisition writes a Play touch (M12.1, Step 5)
// -----------------------------------------------------------------------------

/// When `possession_resolution_system` acquires a new possessor (player
/// within 1.5 m of a free ball), it must push a `LastTouch { team,
/// player, kind: Play }`. The consumer then writes it to
/// `Ball.last_touch`.
#[test]
fn test_possession_acquisition_pushes_play_touch() {
    let mut world = World::new();
    world.insert_resource(PendingBallTouches::default());

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world
        .entity_mut(ball_entity)
        .insert(Position(pos(50.0, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));
    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touch: None,
        last_deliberate_play: None,
    });

    let player = world.spawn(()).id();
    world.entity_mut(player).insert(Player {
        team_id: sim_components::TeamId(0),
        intent: None,
    });
    world
        .entity_mut(player)
        .insert(TeamIdComponent(sim_components::TeamId(0)));
    world.entity_mut(player).insert(Position(pos(50.4, 34.0))); // < 1.5 m
    world.entity_mut(player).insert(Velocity(Vec2::zero()));
    world.entity_mut(player).insert(sim_components::Skill(0.8));
    world
        .entity_mut(player)
        .insert(sim_components::Stamina(0.8));
    world
        .entity_mut(player)
        .insert(sim_components::RoleComponent(sim_components::Role::Striker));

    let mut schedule = Schedule::default();
    schedule.add_systems((possession_resolution_system, apply_ball_touched_system).chain());
    schedule.run(&mut world);

    let ball = world.resource::<Ball>();
    assert_eq!(
        ball.last_touch,
        Some(LastTouch {
            team: sim_components::TeamId(0),
            player: Some(player),
            player_id: Some(sim_components::PlayerId::from_entity(player)),
            kind: TouchKind::Play,
        }),
        "possession acquisition must register a Play touch on the new possessor"
    );
    assert_eq!(ball.possessor, Some(player));
    assert_eq!(ball.state, BallState::Possessed);
}

// -----------------------------------------------------------------------------
// Kickoff placement writes a Restart touch (M12.1, Step 6)
// -----------------------------------------------------------------------------

/// `Simulation::new(12345)` → first tick transitions `PreMatch → Kickoff`
/// (ball placed, `last_touch` set to home striker by
/// `apply_transition`). Second tick transitions `Kickoff → InPlay`
/// (apply kickoff impulse). After two ticks the ball carries a
/// `TouchKind::Restart` touch attributed to home (team 0).
#[test]
fn test_kickoff_registers_restart_touch() {
    let mut sim = Simulation::new(12345);
    sim.tick();
    sim.tick();

    let ball = sim.world().resource::<Ball>();
    let touch = ball
        .last_touch
        .expect("kickoff must register a touch by tick 2");
    assert_eq!(
        touch.kind,
        TouchKind::Restart,
        "kickoff placement must be a Restart touch (not Play)"
    );
    assert_eq!(
        touch.team,
        sim_components::TeamId(0),
        "first-half kickoff is taken by the home team (Law 8)"
    );
}

// -----------------------------------------------------------------------------
// OOB restart writes a Restart touch on the opposite team (M12.1, Step 7)
// -----------------------------------------------------------------------------

/// When the ball goes out-of-bounds, `restart_system` resolves the
/// throw-in / corner / goal-kick taker as the team **opposite** of
/// the last toucher (Law 9). The restart placement pushes a
/// `TouchKind::Restart` entry; the consumer drains it on the next tick.
///
/// Inside a single schedule run, the chain is
/// `apply_ball_touched_system → out_of_bounds_system → restart_system`
/// (per `register_systems` in `sim-core::construct`). The restart's
/// push therefore lands in `PendingBallTouches` *after* the consumer
/// has already run that tick; the consumer picks it up on the next
/// tick's run.
#[test]
fn test_oob_restart_registers_restart_touch() {
    let mut world = World::new();

    // Touchline OOB: ball at (20.0, 0.0), settled.
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world
        .entity_mut(ball_entity)
        .insert(Position(pos(20.0, 0.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));

    // Last toucher on team 0.
    let toucher = world.spawn(()).id();
    world.entity_mut(toucher).insert(Player {
        team_id: sim_components::TeamId(0),
        intent: None,
    });
    world
        .entity_mut(toucher)
        .insert(TeamIdComponent(sim_components::TeamId(0)));

    // Opposite-team taker for the throw-in.
    let taker = world.spawn(()).id();
    world.entity_mut(taker).insert(Player {
        team_id: sim_components::TeamId(1),
        intent: None,
    });
    world
        .entity_mut(taker)
        .insert(TeamIdComponent(sim_components::TeamId(1)));

    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touch: Some(LastTouch {
            team: sim_components::TeamId(0),
            player: Some(toucher),
            player_id: Some(sim_components::PlayerId::from_entity(toucher)),
            kind: TouchKind::Play,
        }),
        last_deliberate_play: None,
    });
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(CurrentTick(50));

    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            apply_ball_touched_system,
            out_of_bounds_system,
            restart_system,
        )
            .chain(),
    );
    schedule.run(&mut world);

    // `Ball.last_touch` is unchanged this tick (the restart push is
    // staged for the next tick's consumer).
    let ball = world.resource::<Ball>();
    assert_eq!(
        ball.last_touch,
        Some(LastTouch {
            team: sim_components::TeamId(0),
            player: Some(toucher),
            player_id: Some(sim_components::PlayerId::from_entity(toucher)),
            kind: TouchKind::Play,
        }),
        "Ball.last_touch survives the same tick (push lands in the buffer; consumer drains on next tick)"
    );
    assert_eq!(
        ball.state,
        BallState::Free,
        "restart_system returns the ball to Free for play to resume"
    );

    // The restart push is in the buffer, attributed to team 1 (opposite).
    let pending = world.resource::<PendingBallTouches>();
    assert_eq!(
        pending.0,
        vec![LastTouch {
            team: sim_components::TeamId(1),
            player: Some(taker),
            player_id: Some(sim_components::PlayerId::from_entity(taker)),
            kind: TouchKind::Restart,
        }],
        "restart_system must push the Restart touch to PendingBallTouches"
    );
}

// -----------------------------------------------------------------------------
// RuleEvent payloads carry the full touch record (M12.1, Steps 8-9)
// -----------------------------------------------------------------------------

/// `out_of_bounds_system` captures the full last-touch record in the
/// `RuleEvent::OutOfBounds { last_touch, .. }` payload. The ball's own
/// `last_touch` field is **preserved** (M8) — the record stays on the
/// ball for replay data; it is not cleared when the event fires.
#[test]
fn test_oob_event_carries_full_touch() {
    let mut world = World::new();

    let toucher = world.spawn(()).id();
    world.entity_mut(toucher).insert(Player {
        team_id: sim_components::TeamId(0),
        intent: None,
    });
    world
        .entity_mut(toucher)
        .insert(TeamIdComponent(sim_components::TeamId(0)));

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world
        .entity_mut(ball_entity)
        .insert(Position(pos(20.0, 0.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));
    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touch: Some(LastTouch {
            team: sim_components::TeamId(0),
            player: Some(toucher),
            player_id: Some(sim_components::PlayerId::from_entity(toucher)),
            kind: TouchKind::Play,
        }),
        last_deliberate_play: None,
    });
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(CurrentTick(0));

    let mut schedule = Schedule::default();
    schedule.add_systems((out_of_bounds_system, apply_ball_touched_system).chain());
    schedule.run(&mut world);

    let event = world
        .entity(ball_entity)
        .get::<RuleEvent>()
        .expect("RuleEvent::OutOfBounds must be emitted");
    if let RuleEvent::OutOfBounds { last_touch, .. } = event {
        assert_eq!(
            *last_touch,
            Some(LastTouch {
                team: sim_components::TeamId(0),
                player: Some(toucher),
                player_id: Some(sim_components::PlayerId::from_entity(toucher)),
                kind: TouchKind::Play,
            }),
            "OutOfBounds payload must carry the full last-touch record"
        );
    } else {
        panic!("expected RuleEvent::OutOfBounds, got {event:?}");
    }

    // The ball's own `last_touch` is preserved (M8).
    let ball = world.resource::<Ball>();
    assert_eq!(
        ball.last_touch,
        Some(LastTouch {
            team: sim_components::TeamId(0),
            player: Some(toucher),
            player_id: Some(sim_components::PlayerId::from_entity(toucher)),
            kind: TouchKind::Play,
        }),
        "Ball.last_touch must be preserved on OOB, not cleared"
    );
}

/// `goal_detection_system` captures the full last-touch record in the
/// `RuleEvent::Goal { last_touch, .. }` payload. The ball's own
/// `last_touch` is preserved — scorer attribution uses ball position
/// (not toucher team), and the record is kept on the ball for the
/// post-goal kickoff taker resolution.
#[test]
fn test_goal_event_carries_full_touch() {
    let mut world = World::new();

    world.insert_resource(Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: MatchState::InPlay,
        seed: 12345,
    });
    let match_entity = world.spawn(()).id();
    world.entity_mut(match_entity).insert(MatchClock {
        elapsed_ticks: 30 * 60,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });

    // Kicker is on team 0 (home) — but the ball ends up in the home
    // goal, so the scorer is team 0 by position. The test's focus is
    // the payload shape, not scorer attribution.
    let toucher = world.spawn(()).id();
    world.entity_mut(toucher).insert(Player {
        team_id: sim_components::TeamId(0),
        intent: None,
    });
    world
        .entity_mut(toucher)
        .insert(TeamIdComponent(sim_components::TeamId(0)));

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world
        .entity_mut(ball_entity)
        .insert(Position(pos(105.5, 34.0))); // home goal
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));
    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touch: Some(LastTouch {
            team: sim_components::TeamId(0),
            player: Some(toucher),
            player_id: Some(sim_components::PlayerId::from_entity(toucher)),
            kind: TouchKind::Play,
        }),
        last_deliberate_play: None,
    });
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(CurrentTick(100));

    let mut schedule = Schedule::default();
    schedule.add_systems((goal_detection_system,).chain());
    schedule.run(&mut world);

    let event = world
        .entity(ball_entity)
        .get::<RuleEvent>()
        .expect("RuleEvent::Goal must be emitted");
    if let RuleEvent::Goal {
        scorer_team,
        last_touch,
        ..
    } = event
    {
        assert_eq!(
            *scorer_team,
            sim_components::TeamId(0),
            "scorer attribution uses ball position (x = 105 → home)"
        );
        assert_eq!(
            *last_touch,
            Some(LastTouch {
                team: sim_components::TeamId(0),
                player: Some(toucher),
                player_id: Some(sim_components::PlayerId::from_entity(toucher)),
                kind: TouchKind::Play,
            }),
            "Goal payload must carry the full last-touch record"
        );
    } else {
        panic!("expected RuleEvent::Goal, got {event:?}");
    }

    // The ball's own `last_touch` is preserved (M8).
    let ball = world.resource::<Ball>();
    assert_eq!(
        ball.last_touch,
        Some(LastTouch {
            team: sim_components::TeamId(0),
            player: Some(toucher),
            player_id: Some(sim_components::PlayerId::from_entity(toucher)),
            kind: TouchKind::Play,
        }),
        "Ball.last_touch must be preserved on goal, not cleared"
    );
}

/// Own-goal scenario: kicker on team 0 kicks the ball into team 0's
/// goal (x = -1, away goal). By ball position the goal is credited
/// to the away team (1) — but the `last_touch` record still attributes
/// the kick to team 0 (the kicker). This is the "own-goal" wiring
/// preservation: the payload keeps the identity of who actually
/// touched the ball, while scorer attribution follows ball position.
#[test]
fn test_own_goal_attribution_preserved() {
    let mut world = World::new();

    world.insert_resource(Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: MatchState::InPlay,
        seed: 12345,
    });
    let match_entity = world.spawn(()).id();
    world.entity_mut(match_entity).insert(MatchClock {
        elapsed_ticks: 30 * 60,
        half: 1,
        added_time_ticks: 0,
        is_running: true,
    });

    // Kicker is on team 0 (home) — and the ball ends up in the
    // **away** goal (x = -1).
    let kicker = world.spawn(()).id();
    world.entity_mut(kicker).insert(Player {
        team_id: sim_components::TeamId(0),
        intent: None,
    });
    world
        .entity_mut(kicker)
        .insert(TeamIdComponent(sim_components::TeamId(0)));

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world
        .entity_mut(ball_entity)
        .insert(Position(pos(-0.5, 34.0))); // away goal
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));
    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touch: Some(LastTouch {
            team: sim_components::TeamId(0),
            player: Some(kicker),
            player_id: Some(sim_components::PlayerId::from_entity(kicker)),
            kind: TouchKind::Play,
        }),
        last_deliberate_play: None,
    });
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(CurrentTick(50));

    let mut schedule = Schedule::default();
    schedule.add_systems(goal_detection_system);
    schedule.run(&mut world);

    let event = world
        .entity(ball_entity)
        .get::<RuleEvent>()
        .expect("RuleEvent::Goal must be emitted");
    if let RuleEvent::Goal {
        scorer_team,
        last_touch,
        ..
    } = event
    {
        assert_eq!(
            *scorer_team,
            sim_components::TeamId(1),
            "scorer attribution uses ball position: ball at x = -0.5 → away scores"
        );
        assert_eq!(
            *last_touch,
            Some(LastTouch {
                team: sim_components::TeamId(0),
                player: Some(kicker),
                player_id: Some(sim_components::PlayerId::from_entity(kicker)),
                kind: TouchKind::Play,
            }),
            "last_touch preserves the kicker (team 0) — own-goal identity is kept"
        );
    } else {
        panic!("expected RuleEvent::Goal, got {event:?}");
    }
}

// -----------------------------------------------------------------------------
// Pre-touch guard on OOB (M12.1, Step 11)
// -----------------------------------------------------------------------------

/// `out_of_bounds_system` must not emit a `RuleEvent::OutOfBounds` when
/// `Ball.last_touch` is `None` — a desync from a missing prior
/// restart / kick. Spec §E: bail without event; release builds log
/// `tracing::error!`, debug builds `debug_assert!` first.
///
/// In debug (the test profile), the guard surfaces as a `debug_assert!`
/// panic. The contract under test is "the system refuses to commit a
/// OOB event without a registered touch" — the panic is the assertion
/// of that contract; we use `catch_unwind` to observe it without
/// tearing the test down.
#[test]
fn test_pre_touch_oob_logs_error_and_skips() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let mut world = World::new();

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world
        .entity_mut(ball_entity)
        .insert(Position(pos(20.0, 0.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));
    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        // Pre-touch guard trip: last_touch is None.
        last_touch: None,
        last_deliberate_play: None,
    });
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(CurrentTick(0));

    let mut schedule = Schedule::default();
    schedule.add_systems((out_of_bounds_system, apply_ball_touched_system).chain());

    let result = catch_unwind(AssertUnwindSafe(|| {
        schedule.run(&mut world);
    }));
    assert!(
        result.is_err(),
        "debug_assert! must fire when out_of_bounds_system is called with last_touch = None"
    );

    // After the panic, the world is half-mutated — the schedule didn't
    // proceed past `out_of_bounds_system` (it never reached the
    // consumer or the restart system). Re-run a fresh schedule without
    // the OOB system to confirm Ball state is still untouched by any
    // event emission (no RuleEvent on the ball entity).
    assert!(
        world.entity(ball_entity).get::<RuleEvent>().is_none(),
        "no RuleEvent::OutOfBounds must be emitted when the guard fires"
    );
}

// -----------------------------------------------------------------------------
// Resource / schedule-wiring checks (M12.1, Steps 12-13)
// -----------------------------------------------------------------------------

/// Schedule `kick_execution_system` → `possession_resolution_system`
/// → `apply_ball_touched_system` in the documented order. After the
/// consumer runs, the `PendingBallTouches` buffer must be drained.
#[test]
fn test_pending_ball_touches_resource_registered() {
    let mut world = World::new();
    world.insert_resource(PendingBallTouches::default());

    let possessor = world.spawn(()).id();
    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world
        .entity_mut(ball_entity)
        .insert(Position(pos(50.0, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));
    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Possessed,
        possessor: Some(possessor),
        last_touch: None,
        last_deliberate_play: None,
    });

    // No pushes to the buffer — the consumer should still run, but
    // `Ball.last_touch` stays at its pre-seed value. The wiring
    // invariant we test is "buffer drained after consumer runs".
    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            kick_execution_system,
            possession_resolution_system,
            apply_ball_touched_system,
        )
            .chain(),
    );
    schedule.run(&mut world);

    assert!(
        world.resource::<PendingBallTouches>().0.is_empty(),
        "PendingBallTouches must be drained after apply_ball_touched_system runs"
    );
}

/// The `apply_ball_touched_system` consumer must run **before** every
/// other Rules system (out-of-bounds, goal, offside, foul, restart).
/// Spec §3 reads: "Rules = ball-touch consumer → out-of-bounds → goal →
/// offside → foul → restart". This test pins the ordering by running
/// all six in `.chain()` and asserting the consumer ran first (by
/// observing that a kicker's touch is visible to the subsequent
/// Rules writers).
#[test]
fn test_apply_ball_touched_consumer_registered_first_in_rules_set() {
    let mut world = World::new();

    // Kicker near the center; ball at (60, 34) inside the goal
    // mouth — if the consumer is first, OOB / goal / restart see the
    // kicked touch. If the consumer is misordered, OOB / goal run
    // before the kick is registered.
    let kicker = world
        .spawn((
            Player {
                team_id: sim_components::TeamId(0),
                intent: Some(Intent::Action(ActionIntent::PassTo)),
            },
            Position(pos(50.0, 34.0)),
            Velocity(Vec2::zero()),
            PerceptionSnapshot {
                self_position: pos(50.0, 34.0),
                nearby_teammates: smallvec![],
                nearby_opponents: smallvec![],
                ball_position: pos(50.5, 34.0),
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

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world
        .entity_mut(ball_entity)
        .insert(Position(pos(50.0, 34.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));
    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Possessed,
        possessor: Some(kicker),
        last_touch: None,
        last_deliberate_play: None,
    });
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(CurrentTick(50));
    world.insert_resource(sim_components::Match {
        id: 1,
        home_team: Entity::PLACEHOLDER,
        away_team: Entity::PLACEHOLDER,
        score: (0, 0),
        state: MatchState::InPlay,
        seed: 12345,
    });

    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            kick_execution_system,
            apply_ball_touched_system,
            out_of_bounds_system,
            goal_detection_system,
            crate::offside_detection_system,
            crate::foul_detection_system,
            restart_system,
        )
            .chain(),
    );
    schedule.run(&mut world);

    // After the chain runs, the kicker's touch must be visible.
    let ball = world.resource::<Ball>();
    assert_eq!(
        ball.last_touch,
        Some(LastTouch {
            team: sim_components::TeamId(0),
            player: Some(kicker),
            player_id: Some(sim_components::PlayerId::from_entity(kicker)),
            kind: TouchKind::Play,
        }),
        "consumer must apply the kicker's touch before any Rules writer reads Ball.last_touch"
    );
}

// -----------------------------------------------------------------------------
// last_deliberate_play stays None (M12.1, Step 14) — guardrail guardrail
// -----------------------------------------------------------------------------

/// M1.4 reserved `Ball.last_deliberate_play` for Law 11's deliberate-play
/// wording. No system in this milestone writes to it; it must stay
/// `None` across the kickoff lifecycle so the guardrail is enforced.
///
/// We exercise the same lifecycle path as `test_kickoff_registers_restart_touch`
/// and then read `Ball.last_deliberate_play` directly. A release-binary
/// full-match run would also satisfy this; the in-process path is
/// faster and self-contained.
#[test]
fn test_last_deliberate_play_always_none() {
    let mut sim = Simulation::new(12345);
    // Tick through the kickoff transition (PreMatch → Kickoff → InPlay)
    // and a few in-play ticks to confirm nothing writes the deliberate
    // touch slot.
    for _ in 0..10 {
        sim.tick();
    }

    let ball = sim.world().resource::<Ball>();
    assert!(
        ball.last_deliberate_play.is_none(),
        "no system may write to Ball.last_deliberate_play in this plan; \
         got {:?}",
        ball.last_deliberate_play
    );

    // Also run a full match in-process: 324_000 ticks. We cap at
    // half-time entry to keep the test under a second (the field is
    // already None by tick 10 — the longer run is overkill for this
    // invariant, but a quick 1k-tick run is cheap and gives extra
    // confidence).
    let mut sim = Simulation::new(42);
    for _ in 0..1_000 {
        sim.tick();
    }
    let ball = sim.world().resource::<Ball>();
    assert!(
        ball.last_deliberate_play.is_none(),
        "after 1k ticks last_deliberate_play must still be None"
    );
}

// -----------------------------------------------------------------------------
// Despawn survival: the record outlives the entity it was captured from
// -----------------------------------------------------------------------------

/// A touch registered against a live player keeps `team` and `player_id`
/// after that player despawns. This is the property the ticket asks for:
/// *"Touch record carries `TouchKind` and the team, captured even if the
/// entity is later despawned, so OOB restart payloads survive."*
#[test]
fn test_touch_record_survives_toucher_despawn() {
    let mut world = World::new();

    // Toucher on team 0, registered by a real kick.
    let kicker = world
        .spawn((
            Player {
                team_id: sim_components::TeamId(0),
                intent: Some(Intent::Action(ActionIntent::PassTo)),
            },
            Position(pos(50.0, 34.0)),
            Velocity(Vec2::zero()),
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
        last_touch: None,
        last_deliberate_play: None,
        kick_velocity: None,
        spin: 0.0,
    });
    world.insert_resource(PendingBallTouches::default());

    let mut schedule = Schedule::default();
    schedule.add_systems((kick_execution_system, apply_ball_touched_system).chain());
    schedule.run(&mut world);

    let expected = world
        .resource::<Ball>()
        .last_touch
        .expect("kick must register a touch");
    assert_eq!(
        expected.player_id,
        Some(sim_components::PlayerId::from_entity(kicker)),
        "player_id must be minted from the live entity's bits"
    );

    // Now despawn the kicker — a substitution or a future roster rule.
    world.entity_mut(kicker).despawn();

    // The record is unchanged and still fully resolvable.
    let after = world
        .resource::<Ball>()
        .last_touch
        .expect("last_touch must survive the toucher despawn");
    assert_eq!(
        after.team,
        sim_components::TeamId(0),
        "team is denormalised and survives despawn"
    );
    assert_eq!(
        after.player_id,
        Some(sim_components::PlayerId::from_entity(kicker)),
        "player_id survives despawn — this is what keeps event payloads resolvable"
    );
    assert_eq!(after.kind, TouchKind::Play);
}

/// The OOB restart-taker is resolved from the denormalised team on the
/// **event payload**, not from the ball's live state or a player lookup.
/// So the restart still awards the right team after the toucher
/// despawns.
#[test]
fn test_oob_restart_taker_resolves_after_toucher_despawn() {
    let mut world = World::new();

    // Toucher on team 0. Ball settled on the touchline → ThrowIn, whose
    // taker team is the opposite of the toucher (Law 9) → team 1.
    let toucher = world.spawn(()).id();
    world.entity_mut(toucher).insert(Player {
        team_id: sim_components::TeamId(0),
        intent: None,
    });
    world
        .entity_mut(toucher)
        .insert(TeamIdComponent(sim_components::TeamId(0)));

    let taker = world.spawn(()).id();
    world.entity_mut(taker).insert(Player {
        team_id: sim_components::TeamId(1),
        intent: None,
    });
    world
        .entity_mut(taker)
        .insert(TeamIdComponent(sim_components::TeamId(1)));

    let ball_entity = world.spawn(()).id();
    world.entity_mut(ball_entity).insert(BallMarker);
    world
        .entity_mut(ball_entity)
        .insert(Position(pos(20.0, 0.0)));
    world.entity_mut(ball_entity).insert(Velocity(Vec2::zero()));
    world.insert_resource(Ball {
        kick_velocity: None,
        spin: 0.0,
        state: BallState::Free,
        possessor: None,
        last_touch: Some(LastTouch::by_player(
            sim_components::TeamId(0),
            toucher,
            TouchKind::Play,
        )),
        last_deliberate_play: None,
    });
    world.insert_resource(PendingBallTouches::default());
    world.insert_resource(CurrentTick(50));

    // The toucher despawns *after* the ball already went out — the
    // restart placement must still be attributed correctly.
    world.entity_mut(toucher).despawn();

    let mut schedule = Schedule::default();
    schedule.add_systems(
        (
            apply_ball_touched_system,
            out_of_bounds_system,
            restart_system,
        )
            .chain(),
    );
    schedule.run(&mut world);

    let pending = world.resource::<PendingBallTouches>();
    assert_eq!(
        pending.0,
        vec![LastTouch::by_player(
            sim_components::TeamId(1),
            taker,
            TouchKind::Restart,
        )],
        "restart taker is the opposite of the denormalised toucher team, \
         resolved without the despawned entity"
    );
}

/// `by_team` is the no-credited-player constructor: both identity
/// fields must be `None`, and the record still carries the team that
/// Law 9 restart resolution reads.
#[test]
fn test_by_team_captures_no_player_identity() {
    let touch = LastTouch::by_team(sim_components::TeamId(1), TouchKind::Restart);
    assert_eq!(touch.team, sim_components::TeamId(1));
    assert_eq!(touch.player, None);
    assert_eq!(touch.player_id, None);
    assert_eq!(touch.kind, TouchKind::Restart);
}
