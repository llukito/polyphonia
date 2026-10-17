//! Small deterministic generator for cover art and whitening.

/// Channels in an RGB colour.
const RGB_CHANNELS: usize = 3;

/// SplitMix64 (Steele, Lea & Flood, "Fast splittable pseudorandom number
/// generators", OOPSLA 2014): tiny, seedable, and plenty for cover art.
/// Not used for anything secret.
pub(crate) struct SplitMix64(pub(crate) u64);

impl SplitMix64 {
    /// Increment from the paper: 2^64 / golden ratio, rounded to odd.
    const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
    /// Output mixing multipliers from the paper (MurmurHash3's fmix64 variant 13).
    const MIX1: u64 = 0xBF58_476D_1CE4_E5B9;
    const MIX2: u64 = 0x94D0_49BB_1331_11EB;

    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(Self::GAMMA);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(Self::MIX1);
        z = (z ^ (z >> 27)).wrapping_mul(Self::MIX2);
        z ^ (z >> 31)
    }

    pub(crate) fn color(&mut self) -> [u8; RGB_CHANNELS] {
        let [r, g, b, ..] = self.next().to_le_bytes();
        [r, g, b]
    }
}
