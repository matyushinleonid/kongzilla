//! A tiny deterministic random number generator.
//!
//! Monte Carlo estimates need to be reproducible - the same inputs must give the
//! same number twice - so the engine carries its own xorshift rather than pulling
//! in a dependency or reaching for system entropy.

/// A xorshift64\* generator.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// Seeds the generator. A zero seed is replaced, since xorshift cannot leave it.
    pub const fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    /// The next raw value.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value in `0..bound`.
    ///
    /// # Panics
    /// Panics if `bound` is zero.
    pub fn below(&mut self, bound: u32) -> u32 {
        assert!(bound > 0, "bound must be positive");
        (self.next_u64() % u64::from(bound)) as u32
    }

    /// A value in `0.0..1.0`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn values_stay_in_range() {
        let mut rng = Rng::new(7);
        for _ in 0..10_000 {
            assert!(rng.below(52) < 52);
            let unit = rng.unit();
            assert!((0.0..1.0).contains(&unit));
        }
    }

    #[test]
    fn a_zero_seed_still_works() {
        let mut rng = Rng::new(0);
        assert_ne!(rng.next_u64(), 0);
    }
}
