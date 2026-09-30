//! `Simulation::get_state_hash` and the small discriminant helpers it
//! uses to fold enum variants into the FNV-1a mix without depending on
//! the variants' `Debug` representation.

use bevy_ecs::prelude::Entity;
use sim_components::{BallState, MatchClock, MatchState, Role, RoleComponent};

use super::Simulation;

pub const fn ball_state_discriminant(s: BallState) -> u64 {
    use sim_components::BallState as B;
    match s {
        B::Free => 0,
        B::Possessed => 1,
        B::InFlight => 2,
        B::Dead => 3,
    }
}

pub const fn match_state_discriminant(s: MatchState) -> u64 {
    use sim_components::MatchState as M;
    match s {
        M::PreMatch => 0,
        M::Kickoff => 1,
        M::InPlay => 2,
        M::Stoppage => 3,
        M::HalfTime => 4,
        M::FullTime => 5,
    }
}

pub const fn role_discriminant(r: Role) -> u64 {
    use sim_components::Role as R;
    match r {
        R::Goalkeeper => 0,
        R::CenterBack => 1,
        R::FullBack => 2,
        R::DefensiveMidfielder => 3,
        R::CentralMidfielder => 4,
        R::AttackingMidfielder => 5,
        R::Winger => 6,
        R::Striker => 7,
    }
}

impl Simulation {
    /// Compute a deterministic FNV-1a (64-bit) hash over the world state.
    ///
    /// Covers (in this fixed order, per-entity, entities sorted by
    /// `Entity::to_bits()`): `Position`, `Velocity`, `Stamina`, `Skill`, `Ball`,
    /// `Player`, `Team`, `Match`. After all entity bytes are mixed in, the
    /// original seed is appended so two simulations seeded identically
    /// hash identically.
    ///
    /// **Excluded** by design: the live `SimRng` state, the tick counter,
    /// and any wall-clock data. The hash identifies *persistent state*,
    /// not *time* — two identical persistent states reached at different
    /// ticks must hash equal, which is what makes replay divergence
    /// detection meaningful. RNG-state divergence still surfaces on the
    /// next tick through the downstream state it influences (positions,
    /// velocities, stamina deltas, etc.), just with one tick of latency.
    pub fn get_state_hash(&self) -> u64 {
        use sim_components::{Match, Player, Position, Skill, Stamina, Team, Velocity};

        fn mix(h: &mut u64, v: u64) {
            // FNV-1a: XOR one byte at a time (little-endian) then multiply.
            let bytes = v.to_le_bytes();
            for b in bytes {
                *h ^= u64::from(b);
                *h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }

        let mut h: u64 = 0xcbf2_9ce4_8422_2325;

        // Collect entities in a deterministic order: by `to_bits()`.
        let mut entities: Vec<Entity> = self.world.iter_entities().map(|e| e.id()).collect();
        entities.sort_by_key(|e| e.to_bits());

        for entity in entities {
            let er = self.world.entity(entity);
            mix(&mut h, entity.to_bits());

            if let Some(p) = er.get::<Position>() {
                mix(&mut h, u64::from(p.0.x.to_bits()));
                mix(&mut h, u64::from(p.0.y.to_bits()));
            }
            if let Some(v) = er.get::<Velocity>() {
                mix(&mut h, u64::from(v.0.x.to_bits()));
                mix(&mut h, u64::from(v.0.y.to_bits()));
            }
            if let Some(s) = er.get::<Stamina>() {
                mix(&mut h, u64::from(s.0.to_bits()));
            }
            if let Some(s) = er.get::<Skill>() {
                mix(&mut h, u64::from(s.0.to_bits()));
            }
            if er.get::<sim_components::BallMarker>().is_some() {
                // Phase C §4.3: ball is tagged with BallMarker. Mix
                // position/velocity from components (still per-entity) and
                // ball state from the Ball Resource below.
                if let Some(pos) = er.get::<Position>() {
                    mix(&mut h, u64::from(pos.0.x.to_bits()));
                    mix(&mut h, u64::from(pos.0.y.to_bits()));
                }
                if let Some(vel) = er.get::<Velocity>() {
                    mix(&mut h, u64::from(vel.0.x.to_bits()));
                    mix(&mut h, u64::from(vel.0.y.to_bits()));
                }
            }
            if let Some(p) = er.get::<Player>() {
                mix(&mut h, u64::from(p.team_id.0));
            }
            if let Some(rc) = er.get::<RoleComponent>() {
                mix(&mut h, role_discriminant(rc.0));
            }
            if let Some(st) = er.get::<Stamina>() {
                mix(&mut h, u64::from(st.0.to_bits()));
            }
            if let Some(sk) = er.get::<Skill>() {
                mix(&mut h, u64::from(sk.0.to_bits()));
            }
            if let Some(t) = er.get::<Team>() {
                mix(&mut h, u64::from(t.id.0));
            }
            if let Some(clock) = er.get::<MatchClock>() {
                mix(&mut h, clock.elapsed_ticks);
                mix(&mut h, u64::from(clock.half));
                mix(&mut h, clock.added_time_ticks);
            }
        }

        // Phase C §4.3: `Match` is now a Resource, not a Component. Read
        // its state from the resource so the hash still reflects match
        // score and state (the per-entity branch above is a no-op now
        // since no entity inserts `Match`).
        if let Some(m) = self.world.get_resource::<Match>() {
            mix(&mut h, match_state_discriminant(m.state));
            mix(&mut h, u64::from(m.score.0));
            mix(&mut h, u64::from(m.score.1));
        }

        // Phase C §4.3: `Ball` is also a Resource — mix state/possessor/
        // last_touched_by/spin from the resource so the hash reflects the
        // dynamic ball state in addition to position/velocity (mixed per-
        // entity above).
        if let Some(b) = self.world.get_resource::<sim_components::Ball>() {
            mix(&mut h, u64::from(b.spin.to_bits()));
            mix(&mut h, ball_state_discriminant(b.state));
            mix(
                &mut h,
                b.possessor
                    .map_or(u64::MAX, bevy_ecs::entity::Entity::to_bits),
            );
            mix(
                &mut h,
                b.last_touched_by
                    .map_or(u64::MAX, bevy_ecs::entity::Entity::to_bits),
            );
        }

        // Mix the original seed so same-seed simulations hash identically.
        mix(&mut h, self.original_seed);

        h
    }
}