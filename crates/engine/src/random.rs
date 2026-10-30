//! Random numbers that are the same every time: for painting scripts.
//!
//! Scripts can scatter dabs or wobble lines "randomly", but playing a
//! script must always give exactly the same picture. So the numbers come
//! from a *seed*: the same seed always gives the same numbers, in the same
//! order, on every computer.
//!
//! The method is "SplitMix64", a tiny, well-known generator. It's written
//! out here (instead of using a library) so it can never change under us:
//! a library update could otherwise change every scripted picture.

/// A source of repeatable random numbers.
#[derive(Clone, Debug)]
pub struct Random {
    state: u64,
}

impl Random {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Combines two numbers into one seed, e.g. a script's seed and a
    /// command's number, so every command gets its own numbers.
    pub fn mix(a: u64, b: u64) -> u64 {
        let mut random = Random::new(a ^ b.rotate_left(32) ^ 0x5851_F42D_4C95_7F2D);
        random.next_u64()
    }

    /// The next number, anywhere in the u64 range.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number from 0 up to (but not including) 1.
    pub fn unit(&mut self) -> f32 {
        // The top 24 bits: exactly as many as an f32 can hold.
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// A number from `low` up to (but not including) `high`.
    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }

    /// A whole number from 0 up to (but not including) `len`. `len` must
    /// not be 0.
    pub fn index(&mut self, len: usize) -> usize {
        (self.next_u64() % len as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_numbers() {
        let mut a = Random::new(42);
        let mut b = Random::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        assert_ne!(Random::new(1).next_u64(), Random::new(2).next_u64());
    }

    #[test]
    fn known_values_never_change() {
        // If this fails, the generator changed, and every script that
        // uses randomness would paint a different picture.
        let mut random = Random::new(0);
        assert_eq!(random.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(random.next_u64(), 0x6E78_9E6A_A1B9_65F4);
    }

    #[test]
    fn numbers_stay_in_their_range() {
        let mut random = Random::new(7);
        for _ in 0..10_000 {
            let u = random.unit();
            assert!((0.0..1.0).contains(&u));
            let r = random.range(-3.0, 5.0);
            assert!((-3.0..5.0).contains(&r));
            assert!(random.index(6) < 6);
        }
    }

    #[test]
    fn mixing_gives_different_seeds() {
        assert_ne!(Random::mix(1, 2), Random::mix(1, 3));
        assert_ne!(Random::mix(1, 2), Random::mix(2, 1));
        assert_eq!(Random::mix(5, 9), Random::mix(5, 9));
    }
}
