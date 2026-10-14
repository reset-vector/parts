//! The rhythm noise oscillator.
//!
//! The manual establishes that one exists and says nothing about its
//! construction: "OPLL includes a noise oscillator obtained by composing a white
//! noise generator with several frequencies." The laws below are `YM2413-098`
//! for the advance this register makes and `YM2413-075` for the sequence
//! eighteen of them produce.

/// Bits in the shift register (`YM2413-075`).
pub(crate) const WIDTH: u32 = 23;

/// The taps, as bit positions from the least significant, of the law by which
/// the register advances **once, per operator slot** (`YM2413-096`,
/// `YM2413-098`): `x^23 + x^9 + 1`.
///
/// A tap position is not an exponent. This register shifts towards its output,
/// so stage `j` holds the output bit `j` steps ahead and the polynomial's `x^e`
/// is the stage `23 - e`; `YM2413-098` states that mapping once, and states why
/// the polynomial alone does not fix it. Eighteen of these advances are one
/// step of `YM2413-075`'s `x^23 + x^9 + x^8 + x + 1`, which is what a single
/// channel sees.
const TAPS: [u32; 2] = [14, 0];

/// The rhythm channels' noise source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Noise {
    state: u32,
}

impl Default for Noise {
    /// All ones. A linear feedback shift register cannot start at zero; what a
    /// reset leaves in it is this crate's choice under `YM2413-OPEN-15`.
    fn default() -> Self {
        Self {
            state: (1 << WIDTH) - 1,
        }
    }
}

impl Noise {
    /// A register loaded with a known state.
    ///
    /// No address reaches the noise register (`YM2413-015`) and what a reset
    /// leaves in it is `YM2413-OPEN-15`, so this is a
    /// verb of the model rather than of the part.
    ///
    /// # Panics
    ///
    /// If `state` does not fit [`WIDTH`] bits.
    #[must_use]
    pub(crate) fn seeded(state: u32) -> Self {
        assert!(state >> WIDTH == 0, "{state:#x} is wider than the register");
        Self { state }
    }

    /// Advance one operator slot and return the bit shifted out
    /// (`YM2413-096`, `YM2413-098`).
    pub(crate) fn step(&mut self) -> bool {
        let feedback = TAPS.iter().fold(0, |acc, tap| acc ^ (self.state >> tap)) & 1;
        let out = self.state & 1 != 0;
        self.state = (self.state >> 1) | (feedback << (WIDTH - 1));
        out
    }

    /// The register's contents.
    #[must_use]
    pub(crate) fn state(self) -> u32 {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    /// Bits a register run from `noise` shifts out, one per advance.
    fn bits(mut noise: Noise, count: usize) -> Vec<u32> {
        (0..count).map(|_| u32::from(noise.step())).collect()
    }

    /// Whether `bits` obey `a(k+23) = sum of a(k+i)` over the set bits `i` of
    /// `poly`, the recurrence form of a degree-23 law.
    fn obeys(bits: &[u32], poly: u32) -> bool {
        bits.windows(24).all(|window| {
            let sum = (0..WIDTH as usize)
                .filter(|i| poly >> i & 1 != 0)
                .fold(0, |acc, i| acc ^ window[i]);
            sum == window[23]
        })
    }

    #[test]
    fn the_register_has_a_maximal_period() {
        assert_eq!(WIDTH, 23);
        let mut generator = Noise::default();
        let start = generator.state();
        assert_ne!(start, 0, "a shift register cannot start at zero");
        let mut period: u32 = 0;
        loop {
            generator.step();
            period += 1;
            if generator.state() == start {
                break;
            }
            assert!(period <= 1 << WIDTH, "the sequence did not close");
        }
        assert_eq!(period, (1 << WIDTH) - 1);
    }

    #[test]
    fn one_advance_and_eighteen_obey_their_laws() {
        let every = bits(Noise::default(), 4096);
        assert!(obeys(&every, (1 << 14) | 1));
        let once_a_frame: Vec<u32> = bits(Noise::default(), 18 * 400)
            .into_iter()
            .step_by(18)
            .collect();
        assert!(obeys(&once_a_frame, (1 << 22) | (1 << 15) | (1 << 14) | 1));
        assert!(!obeys(&once_a_frame, (1 << 14) | 1));

        let mut generator = Noise::default();
        let ones = (0..100_000).filter(|_| generator.step()).count();
        assert!((49_000..51_000).contains(&ones), "balanced, got {ones}");
    }

    #[test]
    fn the_seed_never_reaches_the_dead_state() {
        let mut generator = Noise::default();
        assert_ne!(generator.state(), 0);
        for _ in 0..1_000_000 {
            generator.step();
            assert_ne!(generator.state(), 0, "the register reached the dead state");
        }
    }

    #[test]
    fn the_register_replays_the_published_bits() {
        const SEED: u32 = 0x03_d9ca;
        const HIGH_HAT: &str = "00011000110100100001001000110011000010110011001101";
        const SNARE_DRUM: &str = "10010001100000110111000101001100101101111111010011";
        let parse =
            |text: &str| -> Vec<u32> { text.bytes().map(|b| u32::from(b - b'0')).collect() };
        let (high_hat, snare_drum) = (parse(HIGH_HAT), parse(SNARE_DRUM));

        let mut generator = Noise::seeded(SEED);
        let mut read_high_hat = Vec::new();
        let mut read_snare_drum = Vec::new();
        for _ in 0..high_hat.len() {
            for slot in 0..18 {
                let bit = u32::from(generator.step());
                if slot == 0 {
                    read_high_hat.push(bit);
                } else if slot == 3 {
                    read_snare_drum.push(bit);
                }
            }
        }
        assert_eq!(read_high_hat, high_hat);
        assert_eq!(read_snare_drum, snare_drum);
        assert_ne!(high_hat, snare_drum);
    }
}
