//! A seeded random stream with no dependency: SplitMix64 (Steele, Lea and
//! Flood, "Fast splittable pseudorandom number generators", OOPSLA 2014;
//! the constants are the ones in `java.util.SplittableRandom` and in
//! Vigna's reference `splitmix64.c`, prng.di.unimi.it). Same seed, same
//! stream, on every platform: placement is reproducible from `seed`.

#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1), 53 bits.
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
    pub fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.f64()
    }
    /// Uniform index in 0..n (n > 0).
    pub fn below(&mut self, n: usize) -> usize {
        debug_assert!(n > 0);
        ((self.next_u64() >> 11) % n as u64) as usize
    }
    /// Index drawn with probability proportional to `w`.
    pub fn weighted(&mut self, w: &[f64]) -> usize {
        let total: f64 = w.iter().sum();
        let mut r = self.f64() * total;
        for (i, x) in w.iter().enumerate() {
            if r < *x {
                return i;
            }
            r -= x;
        }
        w.len() - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_values() {
        // splitmix64.c with x = 0: the first outputs.
        let mut r = Rng::new(0);
        assert_eq!(r.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(r.next_u64(), 0x6E78_9E6A_A1B9_65F4);
    }
}
