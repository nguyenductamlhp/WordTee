//! A small deterministic PRNG.
//!
//! The app needs randomness for distractor choices, Quick Scan sampling and the
//! placement test's pseudo-words — none of it security-sensitive. Rolling our
//! own avoids `getrandom`, which needs extra wiring on `wasm32-unknown-unknown`
//! and would be one more thing to keep working on three platforms.

/// xorshift64*, seeded from the clock.
pub struct Rng(u64);

impl Rng {
    /// Seeds from the current time, falling back to a fixed seed if the clock
    /// is unavailable.
    pub fn new() -> Self {
        let nanos = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map_or(0x2545_F491_4F6C_DD1D, |d| d.as_nanos() as u64);
        Self::seeded(nanos)
    }

    pub fn seeded(seed: u64) -> Self {
        // Zero is a fixed point of xorshift, so only that one value is
        // replaced — forcing a bit on instead would make neighbouring seeds
        // (42 and 43, say) produce identical streams.
        Self(if seed == 0 {
            0x2545_F491_4F6C_DD1D
        } else {
            seed
        })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `0..n`. Returns 0 when `n` is 0.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % n as u64) as usize
    }

    /// Picks one element, or `None` when the slice is empty.
    pub fn choice<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        items.get(self.below(items.len()))
    }

    /// Fisher-Yates.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i + 1));
        }
    }
}

impl Default for Rng {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_in_range() {
        let mut rng = Rng::seeded(7);
        for n in [1usize, 2, 5, 37, 1000] {
            for _ in 0..500 {
                assert!(rng.below(n) < n);
            }
        }
        assert_eq!(rng.below(0), 0);
    }

    #[test]
    fn shuffle_keeps_every_element() {
        let mut rng = Rng::seeded(99);
        let mut items: Vec<u32> = (0..64).collect();
        rng.shuffle(&mut items);
        items.sort_unstable();
        assert_eq!(items, (0..64).collect::<Vec<_>>());
    }

    #[test]
    fn a_seed_reproduces_its_sequence() {
        let draw = |seed| {
            let mut rng = Rng::seeded(seed);
            (0..20).map(|_| rng.below(1000)).collect::<Vec<_>>()
        };
        assert_eq!(draw(42), draw(42));
        assert_ne!(draw(42), draw(43));
    }

    #[test]
    fn does_not_get_stuck() {
        // Every seed must keep producing varied output, zero included.
        for seed in [0, 1, u64::MAX, 0xdead_beef] {
            let mut rng = Rng::seeded(seed);
            let draws: std::collections::BTreeSet<_> = (0..64).map(|_| rng.next_u64()).collect();
            assert!(
                draws.len() > 60,
                "seed {seed} produced {} values",
                draws.len()
            );
        }
    }

    #[test]
    fn choice_covers_the_slice() {
        let mut rng = Rng::seeded(5);
        let items = [1, 2, 3, 4];
        let seen: std::collections::BTreeSet<_> = (0..200)
            .filter_map(|_| rng.choice(&items))
            .copied()
            .collect();
        assert_eq!(seen.len(), 4);
        assert!(rng.choice::<u8>(&[]).is_none());
    }
}
