//! Perception: snapshot of what each player knows about the world each tick.

use bevy_ecs::prelude::*;
use sim_components::{
    Ball, BallMarker, Match, MatchClock, NearbyEntity, PerceptionSnapshot, PitchBounds, Position,
    Team, TeamIdComponent,
};
use sim_math::Vec2;

#[expect(
    clippy::type_complexity,
    reason = "Bevy ParamSet/Query signatures are inherently verbose; this system is one linear snapshot-then-build pass"
)]
pub fn perception_system(
    mut commands: Commands,
    mut queries: ParamSet<(
        Query<(Entity, &Position, &TeamIdComponent)>,
        Query<(Entity, &Position, &TeamIdComponent)>,
        Query<&Position, With<BallMarker>>,
        Query<&Team>,
    )>,
    match_res: Res<Match>,
    ball_res: Res<Ball>,
    // Phase F follow-up: MatchClock is a Resource (spec §3). Read it
    // directly via `Res<MatchClock>` instead of a `Query<&MatchClock>`
    // (which used a silent `unwrap_or(MatchClock::default())` fallback
    // that masked missing-clock bugs).
    clock_res: Res<MatchClock>,
) {
    // Phase C §4.3: Ball state lives on the `Ball` Resource. Ball position
    // is queried from the entity carrying `BallMarker`.
    let ball_state = ball_res.state;
    let ball_position = {
        let q = queries.p2();
        q.get_single().map_or_else(|_| Vec2::zero(), |pos| pos.0)
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

    // Phase F follow-up: MatchClock is a Resource. Snapshot once at the
    // top of the system for use filling in match-context fields on each
    // player's PerceptionSnapshot (time remaining, etc.). Reading from
    // `Res<MatchClock>` is a single pointer indirection — faster than
    // the previous `Query<&MatchClock>::iter().next()` lookup.
    let clock = clock_res.clone();
    // Derive match-context fields from the resource.
    let (_score_diff, _time_remaining_secs, _home_id, _away_id) = {
        let diff = i16::from(match_res.score.0) - i16::from(match_res.score.1);
        let time_remaining_secs = sim_components::time::match_time_remaining_secs(&clock);
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
        let q = queries.p3();
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
                let nearby_entity = NearbyEntity {
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

        nearby_teammates.sort_by(|a: &NearbyEntity, b: &NearbyEntity| {
            a.distance
                .partial_cmp(&b.distance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        nearby_opponents.sort_by(|a: &NearbyEntity, b: &NearbyEntity| {
            a.distance
                .partial_cmp(&b.distance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let pitch_bounds = PitchBounds {
            distance_to_left: player_pos.x,
            distance_to_right: 105.0 - player_pos.x,
            distance_to_top: player_pos.y,
            distance_to_bottom: 68.0 - player_pos.y,
        };
        let goal_position = Vec2::new(105.0, 34.0);

        let perception = PerceptionSnapshot {
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
