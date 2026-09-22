use bevy_ecs::prelude::Resource;
use std::hash::{Hash, Hasher};
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn zero() -> Self {
        Self { x: 0.0, y: 0.0 }
    }

    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    pub fn normalized(self) -> Self {
        let len = self.length();
        if len < f32::EPSILON {
            Self::zero()
        } else {
            self / len
        }
    }

    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    pub fn distance_squared(self, other: Self) -> f32 {
        (self - other).length_squared()
    }

    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }

    pub fn clamp_length(self, max_length: f32) -> Self {
        let len = self.length();
        if len > max_length {
            self * (max_length / len)
        } else {
            self
        }
    }
}

impl Add for Vec2 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub for Vec2 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl MulAssign<f32> for Vec2 {
    fn mul_assign(&mut self, rhs: f32) {
        self.x *= rhs;
        self.y *= rhs;
    }
}

impl Mul<Vec2> for f32 {
    type Output = Vec2;

    fn mul(self, rhs: Vec2) -> Self::Output {
        Vec2 {
            x: self * rhs.x,
            y: self * rhs.y,
        }
    }
}

impl Div<f32> for Vec2 {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x / rhs,
            y: self.y / rhs,
        }
    }
}

impl DivAssign<f32> for Vec2 {
    fn div_assign(&mut self, rhs: f32) {
        self.x /= rhs;
        self.y /= rhs;
    }
}

impl Eq for Vec2 {}

impl Hash for Vec2 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.x.to_bits().hash(state);
        self.y.to_bits().hash(state);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeterministicRng {
    state: u32,
    seed: u64,
}

impl DeterministicRng {
    /// Construct a deterministic RNG from a 64-bit seed.
    ///
    /// The internal 32-bit state is initialised by folding both halves of the
    /// seed (`(seed as u32) ^ ((seed >> 32) as u32)`) so distinct u64 seeds
    /// yield distinct starting states. The original `seed` is retained for
    /// `seed()` access and for the per-entity fork in [`Self::clone_for_entity`].
    pub fn new(seed: u64) -> Self {
        Self {
            state: (seed as u32) ^ ((seed >> 32) as u32),
            seed,
        }
    }

    /// Canonical Mulberry32 (Tommy Ettinger, public domain). Verbatim port —
    /// see <https://gist.github.com/tommyettinger/46a874533244883189143505d203312c>.
    ///
    /// ```text
    /// z = (state += 0x6D2B79F5)
    /// z = (z ^ (z >> 15)) * (z | 1)
    /// z ^= z + ((z ^ (z >> 7)) * (z | 61))
    /// return z ^ (z >> 14)
    /// ```
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_add(0x6D2B79F5);
        let mut t = self.state;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        t ^ (t >> 14)
    }

    /// Uniform f32 in `[0.0, 1.0)` derived from the top 24 mantissa bits.
    /// Uses the canonical `(u >> 8) * 2^-24` conversion so the granularity is
    /// `1 / 16_777_216` per draw.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    /// Return an *independent* deterministic stream derived from `entity_id`.
    ///
    /// The parent RNG is **not** advanced and is left untouched; the returned
    /// RNG is a fresh stream seeded with `self.seed ^ entity_id * GOLDEN`
    /// (the SplitMix64 golden-ratio constant) so that each entity gets a
    /// reproducible but distinct stream, and the same `(seed, entity_id)` pair
    /// always yields the same stream.
    pub fn clone_for_entity(&self, entity_id: u64) -> DeterministicRng {
        DeterministicRng::new(self.seed ^ entity_id.wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn state(&self) -> u32 {
        self.state
    }
}

#[derive(Debug, Clone, Copy, Resource)]
pub struct PitchDimensions {
    pub width: f32,
    pub length: f32,
    pub penalty_area: Rect,
    pub goal_area: Rect,
    pub center_circle: Circle,
}

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub min: Vec2,
    pub max: Vec2,
}

#[derive(Debug, Clone, Copy)]
pub struct Circle {
    pub center: Vec2,
    pub radius: f32,
}

impl PitchDimensions {
    pub fn standard() -> Self {
        Self {
            width: 105.0,
            length: 68.0,
            penalty_area: Rect {
                min: Vec2::new(0.0, 0.0),
                max: Vec2::new(0.0, 0.0),
            },
            goal_area: Rect {
                min: Vec2::new(0.0, 0.0),
                max: Vec2::new(0.0, 0.0),
            },
            center_circle: Circle {
                center: Vec2::new(52.5, 34.0),
                radius: 9.15,
            },
        }
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn length(&self) -> f32 {
        self.length
    }

    pub fn contains(&self, point: Vec2) -> bool {
        point.x >= 0.0 && point.x <= self.width && point.y >= 0.0 && point.y <= self.length
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vec2_operations() {
        let v1 = Vec2::new(1.0, 2.0);
        let v2 = Vec2::new(3.0, 4.0);

        assert_eq!(v1 + v2, Vec2::new(4.0, 6.0));
        assert_eq!(v2 - v1, Vec2::new(2.0, 2.0));
        assert_eq!(v1 * 2.0, Vec2::new(2.0, 4.0));
        assert_eq!(2.0 * v1, Vec2::new(2.0, 4.0));
        assert_eq!(v1.dot(v2), 11.0);
        assert_eq!(v1.length_squared(), 5.0);
    }

    // Golden vectors for the canonical Mulberry32. Generated independently
    // with both a Python reference and a compiled C reference (see
    // `/tmp/opencode/mulberry_golden.py` and `/tmp/opencode/mulberry_verify.c`).
    // Seed folding matches the Rust port: `state = (seed as u32) ^ ((seed >> 32) as u32)`.
    //   seed=0  -> first 5 u32 outputs:
    //     1144304738, 1416247, 958946056, 627933444, 2007157716
    //   seed=42 -> first 5 u32 outputs:
    //     2581720956, 1925393290, 3661312704, 2876485805, 750819978

    #[test]
    fn test_mulberry32_golden_seed_zero() {
        let mut rng = DeterministicRng::new(0);
        let expected = [1144304738u32, 1416247, 958946056, 627933444, 2007157716];
        let mut actual = [0u32; 5];
        for (slot, exp) in actual.iter_mut().zip(expected.iter()) {
            let v = rng.next_u32();
            *slot = v;
            assert_eq!(v, *exp, "Mulberry32 mismatch for seed=0");
        }
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_mulberry32_golden_seed_42() {
        let mut rng = DeterministicRng::new(42);
        let expected = [2581720956u32, 1925393290, 3661312704, 2876485805, 750819978];
        let mut actual = [0u32; 5];
        for (slot, exp) in actual.iter_mut().zip(expected.iter()) {
            let v = rng.next_u32();
            *slot = v;
            assert_eq!(v, *exp, "Mulberry32 mismatch for seed=42");
        }
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_same_seed_same_stream() {
        let mut a = DeterministicRng::new(0xDEAD_BEEF_CAFE_F00D);
        let mut b = DeterministicRng::new(0xDEAD_BEEF_CAFE_F00D);
        for _ in 0..10 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn test_different_seeds_diverge() {
        // Adjacent seeds whose folded u32 states are different (here the
        // upper/lower halves happen to differ for 1 and 2).
        let mut a = DeterministicRng::new(1);
        let mut b = DeterministicRng::new(2);
        assert_ne!(a.next_u32(), b.next_u32());
    }

    #[test]
    fn test_next_f32_in_unit_interval() {
        let mut rng = DeterministicRng::new(0x1234_5678_9ABC_DEF0);
        for _ in 0..1000 {
            let v = rng.next_f32();
            assert!(v >= 0.0 && v < 1.0, "next_f32 out of [0,1): {v}");
        }
    }

    #[test]
    fn test_clone_for_entity_is_independent_and_deterministic() {
        let parent = DeterministicRng::new(0x4242_4242_4242_4242);
        let mut c1 = parent.clone_for_entity(7);
        let mut c2 = parent.clone_for_entity(7);
        // Same (seed, entity_id) → same stream.
        for _ in 0..10 {
            assert_eq!(c1.next_u32(), c2.next_u32());
        }
        // Different entity_id → different stream.
        let mut other = parent.clone_for_entity(8);
        let first_c1 = c1.next_u32();
        let first_other = other.next_u32();
        assert_ne!(
            first_c1, first_other,
            "clone_for_entity produced identical first draws"
        );
        // Parent is untouched: its state is still the freshly-constructed one.
        let fresh = DeterministicRng::new(0x4242_4242_4242_4242);
        assert_eq!(parent.state(), fresh.state());
        assert_eq!(parent.seed(), fresh.seed());
    }
}