//! `Simulation::get_state`: snapshot the current state of the match into the
//! public view types (`MatchSnapshot`, `BallView`, `PlayerView`,
//! `ClockView`, `MatchStateView`).

use bevy_ecs::prelude::*;
use sim_components::{Match, MatchClock, Position, RoleComponent, Skill, Stamina, Velocity};

use super::Simulation;

impl Simulation {
    /// Snapshot the current state of the match.
    ///
    /// # Errors
    ///
    /// Returns an error string if no `Match` resource is registered.
    pub fn get_state(&self) -> Result<crate::MatchSnapshot, String> {
        // Phase C §4.3: Match is now a Resource.
        let match_component = self.world.resource::<Match>().clone();

        // Get ball state from the Resource, position/velocity from the entity.
        let ball_res = self.world.resource::<sim_components::Ball>();
        let mut ball_view = crate::BallView {
            position: [0.0, 0.0],
            velocity: [0.0, 0.0],
            spin: ball_res.spin,
            state: ball_res.state,
            possessor: ball_res.possessor.map(bevy_ecs::entity::Entity::to_bits),
        };
        // Phase C §4.3: find the ball entity by `BallMarker` (the only
        // entity carrying Position + Velocity but no Player).
        let mut ball_entity_candidates: Vec<Entity> = self
            .world
            .iter_entities()
            .map(|e| e.id())
            .filter(|e| {
                self.world
                    .entity(*e)
                    .get::<sim_components::BallMarker>()
                    .is_some()
            })
            .collect();
        ball_entity_candidates.sort_by_key(|e| e.to_bits());
        if let Some(ball_entity) = ball_entity_candidates.first() {
            let er = self.world.entity(*ball_entity);
            if let Some(pos) = er.get::<Position>() {
                ball_view.position = [pos.0.x, pos.0.y];
            }
            if let Some(vel) = er.get::<Velocity>() {
                ball_view.velocity = [vel.0.x, vel.0.y];
            }
        }

        // Get player views
        let mut player_views = Vec::new();
        for entity in self.world.iter_entities() {
            if let Some(player) = entity.get::<sim_components::Player>() {
                let pos = entity.get::<Position>().map(|p| p.0);
                let vel = entity.get::<Velocity>().map(|v| v.0);
                let stamina = entity.get::<Stamina>().map_or(0.0, |s| s.0);
                let role = entity.get::<RoleComponent>().map(|r| r.0);
                let skill = entity.get::<Skill>().map_or(0.0, |s| s.0);

                let bp = entity
                    .get::<sim_components::PerceptionSnapshot>()
                    .map_or([0.0, 0.0], |p| [p.ball_position.x, p.ball_position.y]);

                player_views.push(crate::PlayerView {
                    entity_id: entity.id().to_bits(),
                    team_id: player.team_id,
                    position: [pos.map_or(0.0, |p| p.x), pos.map_or(0.0, |p| p.y)],
                    velocity: [vel.map_or(0.0, |v| v.x), vel.map_or(0.0, |v| v.y)],
                    stamina,
                    role: role.unwrap_or(sim_components::Role::CentralMidfielder),
                    skill,
                    perception_ball_position: bp,
                });
            }
        }

        // Get clock view from MatchClock resource (source of truth per spec §3)
        let clock_view = self
            .world
            .get_resource::<MatchClock>()
            .map_or(
                crate::ClockView {
                    elapsed_ticks: 0,
                    half: 1,
                    added_time_ticks: 0,
                    is_running: false,
                },
                |clock| crate::ClockView {
                    elapsed_ticks: clock.elapsed_ticks,
                    half: clock.half,
                    added_time_ticks: clock.added_time_ticks,
                    is_running: clock.is_running,
                },
            );

        Ok(crate::MatchSnapshot {
            tick: self.tick,
            match_state: crate::MatchStateView {
                state: match_component.state,
            },
            ball: ball_view,
            players: player_views,
            score: match_component.score,
            clock: clock_view,
            state_hash: self.get_state_hash(),
        })
    }
}