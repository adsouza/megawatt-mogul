//! Deterministic, versioned-by-construction random number generation.
//!
//! All simulation randomness flows through [`SimRng`]. Distribution algorithms are implemented here
//! (not taken from the `rand` crate) so a dependency upgrade can never silently change saved games.

use std::fmt;

use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{Rng, SeedableRng};

/// 2^-53, the spacing of f64 values in `[0, 1)` built from 53 random bits.
const UNIT_SCALE: f64 = 1.0 / (1u64 << 53) as f64;

/// Seedable simulation RNG wrapping ChaCha8.
///
/// `Debug` and `PartialEq` deliberately expose the full generator state (seed and stream position),
/// because the session state hash is computed from the `Debug` output.
#[derive(Clone)]
pub struct SimRng {
    inner: ChaCha8Rng,
}

impl SimRng {
    #[must_use]
    pub fn from_seed(seed: u64) -> Self {
        Self {
            inner: ChaCha8Rng::seed_from_u64(seed),
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.inner.next_u64()
    }

    /// Uniform in `[0, 1)` with 53 bits of resolution.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * UNIT_SCALE
    }

    /// One standard normal draw by Box–Muller, consuming two uniforms and discarding the second
    /// variate so the generator carries no cached state beyond its stream position.
    pub fn standard_normal(&mut self) -> f64 {
        // Map u1 to (0, 1] so that ln(u1) is finite.
        let u1 = 1.0 - self.next_f64();
        let u2 = self.next_f64();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }

    pub fn normal(&mut self, mean: f64, sd: f64) -> f64 {
        mean + sd * self.standard_normal()
    }
}

impl fmt::Debug for SimRng {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SimRng")
            .field("seed", &self.inner.get_seed())
            .field("word_pos", &self.inner.get_word_pos())
            .finish()
    }
}

impl PartialEq for SimRng {
    fn eq(&self, other: &Self) -> bool {
        self.inner.get_seed() == other.inner.get_seed()
            && self.inner.get_word_pos() == other.inner.get_word_pos()
    }
}

impl Eq for SimRng {}

impl crate::state::State for SimRng {
    fn write(&self, out: &mut Vec<u8>) {
        self.inner.get_seed().write(out);
        self.inner.get_word_pos().write(out);
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        let seed = <[u8; 32]>::read(input)?;
        let word = u128::read(input)?;
        let mut inner = ChaCha8Rng::from_seed(seed);
        inner.set_word_pos(word);
        Ok(Self { inner })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let (mut a, mut b) = (SimRng::from_seed(7), SimRng::from_seed(7));
        assert!((0..5).all(|_| a.next_u64() == b.next_u64()));
    }

    #[test]
    fn different_seeds_differ() {
        assert_ne!(
            SimRng::from_seed(1).next_u64(),
            SimRng::from_seed(2).next_u64()
        );
    }

    #[test]
    fn uniform_in_unit_interval() {
        let mut r = SimRng::from_seed(3);
        assert!((0..10_000).all(|_| {
            let x = r.next_f64();
            (0.0..1.0).contains(&x)
        }));
    }

    #[test]
    fn standard_normal_moments() {
        let mut r = SimRng::from_seed(4);
        let xs: Vec<f64> = (0..20_000).map(|_| r.standard_normal()).collect();
        let mean = xs.iter().sum::<f64>() / xs.len() as f64;
        let sd = (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64).sqrt();
        assert!(mean.abs() < 0.05 && (sd - 1.0).abs() < 0.05);
    }

    #[test]
    fn stream_is_stable_across_dependency_upgrades() {
        let mut r = SimRng::from_seed(42);
        insta::assert_debug_snapshot!([r.next_u64(), r.next_u64(), r.next_u64()]);
    }

    #[test]
    fn normal_scales_and_shifts_standard_normal() {
        let (mut a, mut b) = (SimRng::from_seed(9), SimRng::from_seed(9));
        let z = a.standard_normal();
        assert_eq!(b.normal(10.0, 2.0), 10.0 + 2.0 * z);
    }

    #[test]
    fn debug_and_eq_reflect_draw_count() {
        let (a, mut b) = (SimRng::from_seed(5), SimRng::from_seed(5));
        assert_eq!(a, b);
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
        b.next_u64();
        assert_ne!(a, b);
        assert_ne!(format!("{a:?}"), format!("{b:?}"));
    }

    #[test]
    fn clone_continues_same_stream() {
        let mut a = SimRng::from_seed(11);
        a.next_u64();
        let mut b = a.clone();
        assert_eq!(a, b);
        assert_eq!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn different_seeds_are_unequal() {
        assert_ne!(SimRng::from_seed(1), SimRng::from_seed(2));
    }
}
