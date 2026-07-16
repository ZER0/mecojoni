/// Compatibility identifier for the deterministic random stream.
pub const PRNG_VERSION: &str = "splitmix64/1";

const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;
const MIX_1: u64 = 0xbf58_476d_1ce4_e5b9;
const MIX_2: u64 = 0x94d0_49bb_1331_11eb;

/// Small deterministic random stream with a fully specified seed mapping.
///
/// The initial state is exactly the caller's `u64` seed. Each word increments
/// the state by the `SplitMix64` gamma and applies the version-1 mixing function.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplitMix64 {
    state: u64,
    words: u64,
}

impl SplitMix64 {
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self {
            state: seed,
            words: 0,
        }
    }

    /// Restores an exact version-1 stream cursor from a validated snapshot.
    #[must_use]
    pub const fn from_state(state: u64, words: u64) -> Self {
        Self { state, words }
    }

    #[must_use]
    pub const fn state(self) -> u64 {
        self.state
    }

    #[must_use]
    pub const fn words(self) -> u64 {
        self.words
    }

    #[must_use]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GAMMA);
        self.words = self.words.wrapping_add(1);

        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(MIX_1);
        value = (value ^ (value >> 27)).wrapping_mul(MIX_2);
        value ^ (value >> 31)
    }

    /// Samples uniformly from `0..upper`, using rejection rather than modulo
    /// bias. Returns `None` when `upper` is zero or the word budget is exhausted.
    #[must_use]
    pub fn uniform_below(&mut self, upper: u64, word_budget: u64) -> Option<u64> {
        if upper == 0 {
            return None;
        }

        let threshold = upper.wrapping_neg() % upper;
        for _ in 0..word_budget {
            let word = self.next_u64();
            if word >= threshold {
                return Some(word % upper);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{PRNG_VERSION, SplitMix64};

    #[test]
    fn seed_zero_matches_the_versioned_vector() {
        let mut random = SplitMix64::new(0);
        let words = [
            random.next_u64(),
            random.next_u64(),
            random.next_u64(),
            random.next_u64(),
        ];

        assert_eq!(PRNG_VERSION, "splitmix64/1");
        assert_eq!(
            words,
            [
                0xe220_a839_7b1d_cdaf,
                0x6e78_9e6a_a1b9_65f4,
                0x06c4_5d18_8009_454f,
                0xf88b_b8a8_724c_81ec,
            ]
        );
        assert_eq!(random.words(), 4);
        assert_eq!(random.state(), 4_u64.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    }

    #[test]
    fn bounded_sampling_rejects_zero_and_obeys_its_budget() {
        let mut random = SplitMix64::new(7);

        assert_eq!(random.uniform_below(0, 10), None);
        assert_eq!(random.words(), 0);
        assert_eq!(random.uniform_below(10, 0), None);
        assert_eq!(random.words(), 0);
        assert!(random.uniform_below(10, 1).is_some());
        assert_eq!(random.words(), 1);
    }

    #[test]
    fn cloned_streams_are_replayable() {
        let mut original = SplitMix64::new(42);
        let _ = original.next_u64();
        let mut replay = original;

        assert_eq!(original.next_u64(), replay.next_u64());
        assert_eq!(original, replay);
    }
}
