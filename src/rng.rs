//! A small seedable random number generator.
//!
//! The BEAM version leaned on `:rand`, which is process-global and reseedable
//! from anywhere. Here the generator is an ordinary value that gets threaded
//! through the code that needs it, so `--seed` reproducibility is a property of
//! the call graph rather than of hidden state.
//!
//! SplitMix64 is the algorithm: a single multiply-xor-shift chain with no
//! carried state beyond a counter. It is not cryptographic and does not need to
//! be — it picks words for a typing test.

use std::fs::File;
use std::io::Read;

const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

pub struct Rng {
    state: u64,
}

impl Rng {
    /// A generator that will produce the same words every time.
    pub fn seeded(seed: u64) -> Self {
        Rng { state: seed }
    }

    /// A generator seeded from the operating system, falling back to the clock
    /// and process id if `/dev/urandom` cannot be read.
    pub fn from_entropy() -> Self {
        Rng::seeded(os_entropy().unwrap_or_else(clock_entropy))
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GAMMA);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniform integer in `0..n`.
    ///
    /// Values are rejected rather than folded with a bare modulo, which would
    /// make the low end of the range very slightly more likely.
    pub fn below(&mut self, n: usize) -> usize {
        assert!(n > 0, "below(0) has no answer");
        let n = n as u64;
        let limit = u64::MAX - (u64::MAX % n);

        loop {
            let value = self.next_u64();
            if value < limit {
                return (value % n) as usize;
            }
        }
    }

    /// A uniform float in `0.0..1.0`.
    pub fn fraction(&mut self) -> f64 {
        // 53 bits is exactly the mantissa of an f64, so every value in the
        // range is equally reachable.
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A uniform integer in `low..=high`.
    pub fn between(&mut self, low: u32, high: u32) -> u32 {
        low + self.below((high - low + 1) as usize) as u32
    }

    /// One of the given items.
    pub fn choose<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

fn os_entropy() -> Option<u64> {
    let mut bytes = [0u8; 8];
    File::open("/dev/urandom")
        .ok()?
        .read_exact(&mut bytes)
        .ok()?;
    Some(u64::from_ne_bytes(bytes))
}

fn clock_entropy() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(0);

    nanos ^ (std::process::id() as u64).wrapping_mul(GAMMA)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_sequence() {
        let mut one = Rng::seeded(42);
        let mut two = Rng::seeded(42);

        let first: Vec<usize> = (0..20).map(|_| one.below(1000)).collect();
        let second: Vec<usize> = (0..20).map(|_| two.below(1000)).collect();

        assert_eq!(first, second);
    }

    #[test]
    fn different_seeds_diverge() {
        let mut one = Rng::seeded(1);
        let mut two = Rng::seeded(2);

        let first: Vec<usize> = (0..20).map(|_| one.below(1000)).collect();
        let second: Vec<usize> = (0..20).map(|_| two.below(1000)).collect();

        assert_ne!(first, second);
    }

    #[test]
    fn below_stays_in_range_and_reaches_both_ends() {
        let mut rng = Rng::seeded(7);
        let mut seen = [false; 4];

        for _ in 0..500 {
            let value = rng.below(4);
            assert!(value < 4);
            seen[value] = true;
        }

        assert!(seen.iter().all(|hit| *hit), "some values were never drawn");
    }

    #[test]
    fn fractions_stay_within_the_unit_interval() {
        let mut rng = Rng::seeded(9);

        for _ in 0..500 {
            let value = rng.fraction();
            assert!((0.0..1.0).contains(&value), "{value} is out of range");
        }
    }

    #[test]
    fn between_covers_an_inclusive_range() {
        let mut rng = Rng::seeded(11);
        let mut low = false;
        let mut high = false;

        for _ in 0..500 {
            let value = rng.between(1, 4);
            assert!((1..=4).contains(&value));
            low |= value == 1;
            high |= value == 4;
        }

        assert!(low && high, "the range ends were never drawn");
    }
}
