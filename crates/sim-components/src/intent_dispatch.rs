//! Centralised dispatch tables for `Intent`.
//!
//! `Intent::steer_target` and `Intent::kick` are the single source of truth
//! for "what does this intent want the player / ball to do?" Mirrors the
//! inline dispatch that previously lived in
//! `sim-ai-player::execution::{steer, kick_execution_system}`. Speeds,
//! formulas, and return shapes are preserved 1:1 — this is a pure refactor
//! (Phase F brief, Option A ruling).
//!
//! Per-system state-machine logic (tackle roll / pass roll / shot roll in
//! `player_action_execution_system`) stays at the call site — F8 centralises
//! the **steer / kick** dispatch, not the per-system effects.

use crate::{ActionIntent, Intent, MovementIntent, NearbyEntity, PerceptionSnapshot};
use sim_math::Vec2;

/// Closest-by-distance helper, used both for `steer_target` (resolve nearest
/// teammate / opponent as a steer target) and for the velocity-normalised
/// `kick` arms. Mirrors the `closest_by_distance` helper already extracted
/// in `sim-ai-player::execution` — re-stated here so `Intent::kick` /
/// `Intent::steer_target` don't depend on a downstream crate's helpers.
///
/// NaN distances compare as equal (consistent with the previous inline
/// `unwrap_or(Ordering::Equal)` pattern).
fn closest_by_distance(items: &[NearbyEntity]) -> Option<&NearbyEntity> {
    items.iter().min_by(|a, b| {
        a.distance
            .partial_cmp(&b.distance)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

impl Intent {
    /// Steering target world position + speed for this intent, or `None`
    /// when the player should not steer (currently only `HoldPosition`).
    ///
    /// Mirrors every arm of the existing `steer()` function in
    /// `sim-ai-player::execution`. The caller (`steer()`) is responsible for
    /// converting `(target, speed)` into a unit-direction × speed vector.
    ///
    /// Speeds preserved per intent:
    /// `MoveToPosition`=5.0, `ChaseBall`=8.0, `Intercept`=9.0,
    /// `SupportRun`=5.0, `TrackBack`=6.0, `PassTo`=8.0,
    /// `ShootAtGoal`=10.0, `Tackle`/`Press`=10.0, `MarkOpponent`=4.0.
    #[must_use]
    pub fn steer_target(&self, perception: &PerceptionSnapshot) -> Option<(Vec2, f32)> {
        let player_pos = perception.self_position;
        let ball_pos = perception.ball_position;
        let nearest_opponent_pos = || -> Option<Vec2> {
            closest_by_distance(&perception.nearby_opponents)
                .map(|o| o.relative_position + player_pos)
        };
        let nearest_teammate_pos = || -> Option<Vec2> {
            closest_by_distance(&perception.nearby_teammates)
                .map(|t| t.relative_position + player_pos)
        };

        match self {
            Self::Movement(MovementIntent::MoveToPosition(target)) => Some((*target, 5.0)),
            Self::Movement(MovementIntent::ChaseBall) => Some((ball_pos, 8.0)),
            Self::Movement(MovementIntent::Intercept) => {
                // Existing behaviour: ball_vel is not on the perception
                // snapshot, so the half-tick lead collapses to `ball_pos`.
                Some((ball_pos, 9.0))
            }
            Self::Movement(MovementIntent::HoldPosition) => None,
            Self::Movement(MovementIntent::SupportRun) => {
                // If no opponent is visible, no steering target. If an
                // opponent is visible, steer to the reflection of the player
                // across the opponent (mirrors `player_pos +
                // (player_pos - opp.relative_position)`).
                closest_by_distance(&perception.nearby_opponents).map(|worst| {
                    (
                        player_pos + (player_pos - worst.relative_position),
                        5.0,
                    )
                })
            }
            Self::Movement(MovementIntent::TrackBack) => Some((Vec2::new(0.0, 34.0), 6.0)),
            Self::Action(ActionIntent::PassTo) => {
                Some((nearest_teammate_pos().unwrap_or(ball_pos), 8.0))
            }
            Self::Action(ActionIntent::ShootAtGoal(target)) => Some((*target, 10.0)),
            Self::Action(ActionIntent::Tackle(_) | ActionIntent::Press(_)) => {
                Some((nearest_opponent_pos().unwrap_or(player_pos), 10.0))
            }
            Self::Action(ActionIntent::MarkOpponent(_)) => {
                Some((nearest_opponent_pos().unwrap_or(player_pos), 4.0))
            }
        }
    }

    /// Kick direction for this intent (unit vector), or `None` when this
    /// intent does not kick.
    ///
    /// The caller multiplies the returned `Some(direction)` by the
    /// relevant kick speed (`KICK_SPEED_SHOT` for `ShootAtGoal`,
    /// `KICK_SPEED_PASS` for everything else). Mirrors every arm of the
    /// existing `kick_execution_system` match in
    /// `sim-ai-player::execution`.
    ///
    /// `player_vel` is consumed by the `ChaseBall` / `Tackle` / `Press`
    /// arms, which continue the ball in the player's current heading —
    /// this preserves the velocity-continuation behaviour the existing
    /// caller relied on.
    #[must_use]
    pub fn kick(&self, perception: &PerceptionSnapshot, player_vel: Vec2) -> Option<Vec2> {
        let player_pos = perception.self_position;
        let nearest_teammate_pos = || -> Option<Vec2> {
            closest_by_distance(&perception.nearby_teammates)
                .map(|t| t.relative_position + player_pos)
        };

        match self {
            Self::Movement(MovementIntent::ChaseBall) => Some(player_vel.normalized()),
            Self::Movement(_) | Self::Action(ActionIntent::MarkOpponent(_)) => None,
            Self::Action(ActionIntent::ShootAtGoal(target)) => {
                let dir = *target - player_pos;
                Some(dir.normalized())
            }
            Self::Action(ActionIntent::PassTo) => {
                let target = nearest_teammate_pos().unwrap_or(perception.ball_position);
                let dir = target - player_pos;
                Some(dir.normalized())
            }
            Self::Action(ActionIntent::Tackle(_) | ActionIntent::Press(_)) => {
                Some(player_vel.normalized())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::float_cmp,
        reason = "tests compare freshly-constructed values against the literals they were built from; equality is exact"
    )]

    use super::*;
    use crate::{IntentKind, PitchBounds};
    use bevy_ecs::prelude::Entity;
    use smallvec::smallvec;

    fn entity(raw: u32) -> Entity {
        Entity::from_raw(raw)
    }

    fn empty_perception(self_pos: Vec2, ball_pos: Vec2) -> PerceptionSnapshot {
        PerceptionSnapshot {
            self_position: self_pos,
            nearby_teammates: smallvec![],
            nearby_opponents: smallvec![],
            ball_position: ball_pos,
            ball_state: crate::BallState::Free,
            goal_position: Vec2::new(60.0, 34.0),
            pitch_bounds: PitchBounds {
                distance_to_left: self_pos.x,
                distance_to_right: 105.0 - self_pos.x,
                distance_to_top: self_pos.y,
                distance_to_bottom: 68.0 - self_pos.y,
            },
        }
    }

    fn perception_with_opponents(
        self_pos: Vec2,
        ball_pos: Vec2,
        opp_relative: Vec2,
    ) -> PerceptionSnapshot {
        let mut p = empty_perception(self_pos, ball_pos);
        p.nearby_opponents.push(NearbyEntity {
            entity: entity(1),
            distance: opp_relative.length(),
            relative_position: opp_relative,
        });
        p
    }

    fn perception_with_teammate(
        self_pos: Vec2,
        ball_pos: Vec2,
        tm_relative: Vec2,
    ) -> PerceptionSnapshot {
        let mut p = empty_perception(self_pos, ball_pos);
        p.nearby_teammates.push(NearbyEntity {
            entity: entity(2),
            distance: tm_relative.length(),
            relative_position: tm_relative,
        });
        p
    }

    /// Every `Movement` variant except `HoldPosition` should return
    /// `Some((target, speed))`; `HoldPosition` returns `None`. Every
    /// `Action` variant returns `Some((target, speed))`.
    #[test]
    fn steer_target_returns_some_for_every_variant_except_hold_position() {
        let player_pos = Vec2::new(10.0, 20.0);
        let ball_pos = Vec2::new(15.0, 25.0);

        // MoveToPosition: target = the embedded point, speed = 5.0
        let p = empty_perception(player_pos, ball_pos);
        let intent = Intent::Movement(MovementIntent::MoveToPosition(Vec2::new(40.0, 50.0)));
        assert_eq!(intent.steer_target(&p), Some((Vec2::new(40.0, 50.0), 5.0)));

        // ChaseBall: target = ball_pos, speed = 8.0
        let p = empty_perception(player_pos, ball_pos);
        let intent = Intent::Movement(MovementIntent::ChaseBall);
        assert_eq!(intent.steer_target(&p), Some((ball_pos, 8.0)));

        // Intercept: target = ball_pos (no ball_vel on perception), speed = 9.0
        let p = empty_perception(player_pos, ball_pos);
        let intent = Intent::Movement(MovementIntent::Intercept);
        assert_eq!(intent.steer_target(&p), Some((ball_pos, 9.0)));

        // HoldPosition: returns None
        let p = empty_perception(player_pos, ball_pos);
        let intent = Intent::Movement(MovementIntent::HoldPosition);
        assert!(intent.steer_target(&p).is_none());

        // SupportRun with no opponent: returns None
        let p = empty_perception(player_pos, ball_pos);
        let intent = Intent::Movement(MovementIntent::SupportRun);
        assert!(intent.steer_target(&p).is_none());

        // SupportRun with opponent: target = reflection across opponent, speed = 5.0
        let opp_rel = Vec2::new(2.0, 0.0);
        let p = perception_with_opponents(player_pos, ball_pos, opp_rel);
        let intent = Intent::Movement(MovementIntent::SupportRun);
        // player_pos + (player_pos - opp_rel) = (10, 20) + (8, 20) = (18, 40)
        assert_eq!(
            intent.steer_target(&p),
            Some((Vec2::new(18.0, 40.0), 5.0))
        );

        // TrackBack: hardcoded midline (0.0, 34.0), speed = 6.0
        let p = empty_perception(player_pos, ball_pos);
        let intent = Intent::Movement(MovementIntent::TrackBack);
        assert_eq!(intent.steer_target(&p), Some((Vec2::new(0.0, 34.0), 6.0)));

        // PassTo with no teammate: target = ball_pos, speed = 8.0
        let p = empty_perception(player_pos, ball_pos);
        let intent = Intent::Action(ActionIntent::PassTo);
        assert_eq!(intent.steer_target(&p), Some((ball_pos, 8.0)));

        // PassTo with teammate: target = teammate world pos, speed = 8.0
        let tm_rel = Vec2::new(5.0, 0.0);
        let p = perception_with_teammate(player_pos, ball_pos, tm_rel);
        let intent = Intent::Action(ActionIntent::PassTo);
        assert_eq!(
            intent.steer_target(&p),
            Some((player_pos + tm_rel, 8.0))
        );

        // ShootAtGoal: target = embedded point, speed = 10.0
        let p = empty_perception(player_pos, ball_pos);
        let intent = Intent::Action(ActionIntent::ShootAtGoal(Vec2::new(60.0, 34.0)));
        assert_eq!(
            intent.steer_target(&p),
            Some((Vec2::new(60.0, 34.0), 10.0))
        );

        // Tackle / Press with no opponent: target = player_pos, speed = 10.0
        let p = empty_perception(player_pos, ball_pos);
        for variant in [
            ActionIntent::Tackle(entity(99)),
            ActionIntent::Press(entity(99)),
        ] {
            let intent = Intent::Action(variant);
            assert_eq!(intent.steer_target(&p), Some((player_pos, 10.0)));
        }

        // Tackle / Press with opponent: target = opponent world pos, speed = 10.0
        let opp_rel = Vec2::new(3.0, -1.0);
        let p = perception_with_opponents(player_pos, ball_pos, opp_rel);
        for variant in [
            ActionIntent::Tackle(entity(99)),
            ActionIntent::Press(entity(99)),
        ] {
            let intent = Intent::Action(variant);
            assert_eq!(
                intent.steer_target(&p),
                Some((player_pos + opp_rel, 10.0))
            );
        }

        // MarkOpponent with no opponent: target = player_pos, speed = 4.0
        let p = empty_perception(player_pos, ball_pos);
        let intent = Intent::Action(ActionIntent::MarkOpponent(entity(99)));
        assert_eq!(intent.steer_target(&p), Some((player_pos, 4.0)));

        // MarkOpponent with opponent: target = opponent world pos, speed = 4.0
        let opp_rel = Vec2::new(4.0, 0.0);
        let p = perception_with_opponents(player_pos, ball_pos, opp_rel);
        let intent = Intent::Action(ActionIntent::MarkOpponent(entity(99)));
        assert_eq!(
            intent.steer_target(&p),
            Some((player_pos + opp_rel, 4.0))
        );
    }

    /// `kick` should return `Some(direction)` for the kick-bearing variants
    /// (`ChaseBall`, `ShootAtGoal`, `PassTo`, `Tackle`, `Press`) and `None`
    /// for everything else.
    #[test]
    fn kick_returns_some_only_for_kick_bearing_variants() {
        let player_pos = Vec2::new(10.0, 20.0);
        let ball_pos = Vec2::new(15.0, 25.0);
        let player_vel = Vec2::new(3.0, 4.0);

        // Movement: no-kick variants
        for variant in [
            MovementIntent::MoveToPosition(Vec2::new(40.0, 50.0)),
            MovementIntent::HoldPosition,
            MovementIntent::Intercept,
            MovementIntent::SupportRun,
            MovementIntent::TrackBack,
        ] {
            let intent = Intent::Movement(variant);
            let p = empty_perception(player_pos, ball_pos);
            assert!(intent.kick(&p, player_vel).is_none());
        }

        // ChaseBall: direction = player_vel.normalized()
        let p = empty_perception(player_pos, ball_pos);
        let intent = Intent::Movement(MovementIntent::ChaseBall);
        assert_eq!(
            intent.kick(&p, player_vel),
            Some(player_vel.normalized())
        );

        // Action: no-kick variant
        let p = empty_perception(player_pos, ball_pos);
        let intent = Intent::Action(ActionIntent::MarkOpponent(entity(99)));
        assert!(intent.kick(&p, player_vel).is_none());

        // ShootAtGoal: direction = (target - self).normalized()
        let p = empty_perception(player_pos, ball_pos);
        let target = Vec2::new(60.0, 34.0);
        let intent = Intent::Action(ActionIntent::ShootAtGoal(target));
        assert_eq!(intent.kick(&p, player_vel), Some((target - player_pos).normalized()));

        // PassTo with no teammate: direction = (ball - self).normalized()
        let p = empty_perception(player_pos, ball_pos);
        let intent = Intent::Action(ActionIntent::PassTo);
        assert_eq!(intent.kick(&p, player_vel), Some((ball_pos - player_pos).normalized()));

        // PassTo with teammate: direction = (teammate - self).normalized()
        let tm_rel = Vec2::new(5.0, 0.0);
        let p = perception_with_teammate(player_pos, ball_pos, tm_rel);
        let intent = Intent::Action(ActionIntent::PassTo);
        assert_eq!(
            intent.kick(&p, player_vel),
            Some((player_pos + tm_rel - player_pos).normalized())
        );

        // Tackle / Press: direction = player_vel.normalized() (velocity continuation)
        let p = empty_perception(player_pos, ball_pos);
        for variant in [
            ActionIntent::Tackle(entity(99)),
            ActionIntent::Press(entity(99)),
        ] {
            let intent = Intent::Action(variant);
            assert_eq!(intent.kick(&p, player_vel), Some(player_vel.normalized()));
        }
    }

    /// `is_movement` / `is_action` discriminate the coarse kind. They
    /// must agree with `kind()`.
    #[test]
    fn is_movement_and_is_action_are_consistent_with_kind() {
        let movement_cases = [
            Intent::Movement(MovementIntent::MoveToPosition(Vec2::new(0.0, 0.0))),
            Intent::Movement(MovementIntent::HoldPosition),
            Intent::Movement(MovementIntent::ChaseBall),
            Intent::Movement(MovementIntent::Intercept),
            Intent::Movement(MovementIntent::SupportRun),
            Intent::Movement(MovementIntent::TrackBack),
        ];
        for intent in movement_cases {
            assert!(intent.is_movement());
            assert!(!intent.is_action());
            assert!(matches!(intent.kind(), IntentKind::Movement));
        }

        let action_cases = [
            Intent::Action(ActionIntent::PassTo),
            Intent::Action(ActionIntent::ShootAtGoal(Vec2::new(0.0, 0.0))),
            Intent::Action(ActionIntent::Tackle(entity(1))),
            Intent::Action(ActionIntent::MarkOpponent(entity(1))),
            Intent::Action(ActionIntent::Press(entity(1))),
        ];
        for intent in action_cases {
            assert!(intent.is_action());
            assert!(!intent.is_movement());
            assert!(matches!(intent.kind(), IntentKind::Action));
        }
    }
}