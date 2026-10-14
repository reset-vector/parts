//! Phase generation: the increment, the accumulator, and the vibrato that
//! perturbs it.

/// The multiplying factor `MUL` selects, **doubled**, so that the table is
/// integers (`YM2413-040`).
///
/// The manual's Table III-1 runs `½ 1 2 3 4 5 6 7 8 9 10 10 12 12 15 15`: index
/// 0 is a half, and four pairs of adjacent indices repeat. Doubling every entry
/// makes the half an integer and costs one shift at the point of use.
pub(crate) const MUL2: [u32; 16] = [1, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 20, 24, 24, 30, 30];

/// Fractional bits below the sine-table index in the phase accumulator
/// (`YM2413-037`).
pub(crate) const PHASE_FRACTION_BITS: u32 = 9;
/// Entries in the sine table, and so integer bits in the phase accumulator
/// (`YM2413-036`).
pub(crate) const SINE_ENTRIES: u32 = 1024;
/// The phase accumulator wraps here: one full cycle (`YM2413-037`).
pub(crate) const PHASE_MODULUS: u32 = SINE_ENTRIES << PHASE_FRACTION_BITS;

/// The phase increment one sample advances, from F-Number, block and `MUL`
/// (`YM2413-037`).
///
/// The manual gives `ΔP = 2^B × F' × MUL`, and separately `F = (f_mus × 2^18 /
/// f_sam) / 2^(b-1)` at `MUL = 1`. Those two fix the accumulator: with `MUL`
/// doubled into [`MUL2`], the increment is `(fnum << block) × MUL2 / 2`, and a
/// cycle is [`PHASE_MODULUS`].
///
/// # Panics
///
/// Panics if `mul` is not below 16.
#[must_use]
pub(crate) fn increment(fnum: u16, block: u8, mul: u8) -> u32 {
    assert!(mul < 16, "MUL {mul} is not a four-bit field");
    (u32::from(fnum) << block) * MUL2[mul as usize] / 2
}

/// Samples the vibrato oscillator takes to advance one step of its pattern
/// (`YM2413-042`).
pub(crate) const VIBRATO_SAMPLES_PER_STEP: u32 = 1024;
/// Steps in one vibrato cycle (`YM2413-042`).
pub(crate) const VIBRATO_STEPS: u32 = 8;

/// The signed increment the vibrato oscillator adds at sample count `lfo`
/// (`YM2413-042`).
///
/// The rate is the part's — one step of an eight-step pattern every 1024
/// samples. **The shape and depth are this crate's choice** under
/// `YM2413-OPEN-14`: a signed triangle `0 1 2 1 0 −1 −2 −1` whose peak is
/// 1/128 of the increment, 13.5 cents, where `YM2413-042` gives an
/// "approximate depth of 14 cent".
#[must_use]
pub(crate) fn vibrato_offset(increment: u32, lfo: u32) -> i32 {
    const TRIANGLE: [i64; VIBRATO_STEPS as usize] = [0, 1, 2, 1, 0, -1, -2, -1];
    let step = (lfo / VIBRATO_SAMPLES_PER_STEP) % VIBRATO_STEPS;
    let offset = i64::from(increment) * TRIANGLE[step as usize] / 256;
    i32::try_from(offset).expect("a 128th of a u32")
}

/// The phase accumulator of one slot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Phase {
    accumulator: u32,
}

impl Phase {
    /// The accumulator at zero.
    pub(crate) const ZERO: Self = Self { accumulator: 0 };

    /// Advance one sample by `increment`, wrapping at one cycle.
    pub(crate) fn advance(&mut self, increment: u32) {
        self.accumulator = (self.accumulator + increment) % PHASE_MODULUS;
    }

    /// Reset to the start of the cycle, which is what a key-on does.
    pub(crate) fn reset(&mut self) {
        self.accumulator = 0;
    }

    /// The whole accumulator, integer and fractional parts together.
    #[must_use]
    pub(crate) fn raw(self) -> u32 {
        self.accumulator
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    #[test]
    fn the_increment_is_block_fnumber_and_mul() {
        assert_eq!(
            MUL2,
            [1, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 20, 24, 24, 30, 30]
        );
        assert_eq!(MUL2[0xA], MUL2[0xB]);
        assert_eq!(MUL2[0xC], MUL2[0xD]);
        assert_eq!(MUL2[0xE], MUL2[0xF]);
        for absent in [11u32, 13, 26, 28] {
            assert!(!MUL2.contains(&absent), "{absent} is not a factor");
        }

        assert_eq!(increment(288, 4, 1), 288 << 4);
        assert_eq!(increment(288, 5, 1), 2 * increment(288, 4, 1));
        assert_eq!(increment(288, 4, 0) * 2, increment(288, 4, 1));
        for code in 1..=9u8 {
            assert_eq!(increment(100, 0, code), 100 * MUL2[usize::from(code)] / 2);
        }
        assert_eq!(increment(511, 7, 1), 511 << 7);
    }

    #[test]
    fn vibrato_steps_once_every_1024_samples() {
        assert_eq!(VIBRATO_SAMPLES_PER_STEP, 1024);
        assert_eq!(VIBRATO_STEPS, 8);
        let f_sam_millihertz = 3_579_545_000u64 / 72;
        let cycle = u64::from(VIBRATO_STEPS * VIBRATO_SAMPLES_PER_STEP);
        assert_eq!((f_sam_millihertz + cycle / 2) / cycle, 6069);

        let offsets: Vec<i32> = (0..VIBRATO_STEPS)
            .map(|step| vibrato_offset(1 << 16, step * VIBRATO_SAMPLES_PER_STEP))
            .collect();
        assert_eq!(offsets, [0, 256, 512, 256, 0, -256, -512, -256]);
        assert_eq!(vibrato_offset(1 << 16, VIBRATO_SAMPLES_PER_STEP - 1), 0);
    }
}
