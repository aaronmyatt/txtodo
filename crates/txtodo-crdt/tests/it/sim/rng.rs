//! A tiny seeded PRNG so the simulator never touches `getrandom`/wall-clock time: every random
//! choice (clock advance, ULID entropy, op kind, edit position, partition switch) is a pure
//! function of the run's seed, so a failure prints one number a human can hand back for an exact
//! replay (`TXTODO_SIM_SEED`). SplitMix64: <https://prng.di.unimi.it/splitmix64.c> — not
//! cryptographic, just well-distributed and dependency-free.

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..bound`. `bound` must be nonzero — every call site here picks among a
    /// non-empty, statically known set.
    pub fn below(&mut self, bound: usize) -> usize {
        debug_assert!(bound > 0, "below(0) has no value to return");
        (self.next_u64() % bound as u64) as usize
    }

    /// `true` with probability `num/den`.
    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        debug_assert!(num <= den && den > 0);
        self.next_u64() % den < num
    }

    /// `N` fresh random bytes, for ULID entropy.
    pub fn bytes<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0u8; N];
        let mut i = 0;
        while i < N {
            let word = self.next_u64().to_le_bytes();
            let take = (N - i).min(8);
            out[i..i + take].copy_from_slice(&word[..take]);
            i += take;
        }
        out
    }
}
