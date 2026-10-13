//! The default `UtilityBrain` template used by `create_match`.

use bevy_ecs::prelude::Entity;
use sim_ai_core::ResponseCurve;
use sim_ai_player::{Consideration, PlayerAction, UtilityBrain};
use sim_components::{ActionIntent, Intent, MovementIntent};
use sim_math::Vec2;

/// Phase 2: build the default `UtilityBrain` used by every player at match
/// start. Mirrors the §4a consideration list — `MoveToPosition`, `ChaseBall`,
/// `PassTo`, `ShootAtGoal`, Tackle, `MarkOpponent`, Press, `SupportRun`,
/// `HoldPosition`. Curves are tuned for the standard 105×68 pitch (midpoints
/// are real metres, not normalised). Hysteresis bonus = 0.1.
///
/// Decision cadence (`DECISION_CADENCE_TICKS` = 6, ~10 Hz) is applied by
/// `player_decision_system` itself per spec §13 #15 — this brain is
/// cadence-agnostic.
#[expect(
    clippy::too_many_lines,
    reason = "declarative table of actions and considerations; splitting it would only scatter the data"
)]
pub fn default_utility_brain() -> UtilityBrain {
    const fn lin(min: f32, max: f32) -> ResponseCurve {
        ResponseCurve::Linear { min, max }
    }
    const fn log(mid: f32, steep: f32) -> ResponseCurve {
        ResponseCurve::Logistic {
            midpoint: mid,
            steepness: steep,
        }
    }
    const fn step(threshold: f32, below: f32, above: f32) -> ResponseCurve {
        ResponseCurve::Step {
            threshold,
            below,
            above,
        }
    }

    let actions = vec![
        // MoveToPosition — distance_to_target: closer = better.
        // The embedded target Vec2 is a sentinel only;
        // `MovementIntent::resolve_target` (sim-components) replaces
        // `Vec2::zero()` with `perception.home_slot` when this action
        // wins, so no hardcoded pitch coordinate survives into the
        // committed intent.
        PlayerAction {
            intent: Intent::Movement(MovementIntent::MoveToPosition(Vec2::zero())),
            considerations: vec![Consideration::DistanceToTarget {
                weight: 1.0,
                // Step: 0.6 when out of position (raw ≤ 28, i.e. d ≥ 2m),
                // 0.0 when at slot (raw > 28, i.e. d < 2m). This means that
                // when a player is already on their home slot, MoveToPosition
                // scores 0.0 and loses to ChaseBall, PassTo, or HoldPosition.
                // When the player is displaced (≥ 2m from slot) it scores 0.6
                // and will pull the player back unless a stronger action wins.
                curve: step(28.0, 0.6, 0.0),
            }],
        },
        // ChaseBall — distance_to_ball, stamina, pitch control at ball.
        PlayerAction {
            intent: Intent::Movement(MovementIntent::ChaseBall),
            considerations: vec![
                Consideration::DistanceToBall {
                    weight: 0.6,
                    // Logistic: high score within ~20m (>0.6), ~0.5 at 25m,
                    // falls off past 30m so distant players hold position.
                    curve: log(25.0, -0.1),
                },
                Consideration::Stamina {
                    weight: 0.4,
                    curve: lin(0.0, 1.0),
                },
                Consideration::PitchControlAtBall {
                    weight: 0.2,
                    curve: step(30.0, 0.5, 0.9),
                },
            ],
        },
        // PassTo — open passing lane, reasonable teammate distance, space.
        PlayerAction {
            intent: Intent::Action(ActionIntent::PassTo),
            considerations: vec![
                Consideration::PassAngleClear {
                    weight: 0.5,
                    curve: lin(0.0, 1.0),
                },
                Consideration::TeammateDistance {
                    weight: 0.3,
                    curve: lin(0.0, 30.0),
                },
                Consideration::TeammateSpace {
                    weight: 0.2,
                    curve: lin(0.0, 20.0),
                },
            ],
        },
        // ShootAtGoal — distance sweet-spot, angle, low pressure.
        // The embedded target Vec2 is a sentinel only;
        // `player_decision_system` replaces it with `perception.goal_position`
        // when this action wins, so no hardcoded pitch coordinate
        // survives into the committed intent.
        PlayerAction {
            intent: Intent::Action(ActionIntent::ShootAtGoal(Vec2::zero())),
            considerations: vec![
                Consideration::DistanceToGoal {
                    weight: 0.5,
                    // Logistic: inside the box (d < 18m, raw > 17) score is
                    // high (>0.7), outside 25m it falls off sharply.
                    curve: log(15.0, 0.25),
                },
                Consideration::GoalAngle {
                    weight: 0.3,
                    curve: lin(-1.0, 1.0),
                },
                Consideration::DefenderPressure {
                    weight: 0.2,
                    curve: ResponseCurve::Step {
                        threshold: 2.0,
                        below: 1.0,
                        above: 0.5,
                    },
                },
            ],
        },
        // Tackle — close + skill advantage.
        PlayerAction {
            intent: Intent::Action(ActionIntent::Tackle(Entity::PLACEHOLDER)),
            considerations: vec![
                Consideration::DistanceToOpponent {
                    weight: 0.6,
                    curve: log(2.0, 1.5),
                },
                Consideration::SkillDiff {
                    weight: 0.4,
                    curve: lin(-1.0, 1.0),
                },
            ],
        },
        // MarkOpponent — close to marked, between opponent and own goal.
        PlayerAction {
            intent: Intent::Action(ActionIntent::MarkOpponent(Entity::PLACEHOLDER)),
            considerations: vec![
                Consideration::DistanceToMarked {
                    weight: 0.5,
                    curve: lin(0.0, 10.0),
                },
                Consideration::DefensivePosition {
                    weight: 0.5,
                    curve: step(0.5, 0.0, 1.0),
                },
            ],
        },
        // Press — close-range press + stamina.
        PlayerAction {
            intent: Intent::Action(ActionIntent::Press(Entity::PLACEHOLDER)),
            considerations: vec![
                Consideration::DistanceToPress {
                    weight: 0.6,
                    curve: log(8.0, 0.4),
                },
                Consideration::Stamina {
                    weight: 0.4,
                    curve: lin(0.0, 1.0),
                },
            ],
        },
        // SupportRun — open space ahead + teammate on the ball.
        PlayerAction {
            intent: Intent::Movement(MovementIntent::SupportRun),
            considerations: vec![
                Consideration::SpaceAhead {
                    weight: 0.6,
                    curve: lin(0.0, 15.0),
                },
                Consideration::TeammateBall {
                    weight: 0.4,
                    curve: step(0.5, 0.0, 1.0),
                },
            ],
        },
        // HoldPosition — formation discipline baseline. The score is intentionally
        // below the cap so it only wins when no other action has strong
        // positive signal — i.e. when the player has nothing better to do.
        PlayerAction {
            intent: Intent::Movement(MovementIntent::HoldPosition),
            considerations: vec![Consideration::FormationDiscipline {
                weight: 1.0,
                // Step: 0.5 always (a "default" score, not a "best").
                curve: ResponseCurve::Step {
                    threshold: -1.0,
                    below: 0.5,
                    above: 0.5,
                },
            }],
        },
    ];

    UtilityBrain {
        actions,
        hysteresis: 0.05,
    }
}
