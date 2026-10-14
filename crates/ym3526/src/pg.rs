//! The phase generator: block and F-number to a phase increment, the key
//! code, the multiplier and vibrato's offset.
//!
//! **The increment's unit is one sample's `2^−20` of a cycle.** An
//! operator's phase is a 20-bit accumulator advanced once a sample, one
//! sample every 72 cycles of `φM` (`YM3526-008`), so an increment of `k` is
//! `k × φM / (72 × 2^20)` in frequency, and `F × 2^block` is the increment of
//! block and F-number: the pitch `YM3526-013` states.

/// Bits in an operator's phase accumulator: ten of sine-table index over
/// ten of fraction.
pub(crate) const PHASE_BITS: u32 = 20;
/// The phase accumulator's fraction bits.
pub(crate) const PHASE_FRACTION_BITS: u32 = 10;
/// The phase accumulator wraps here.
pub(crate) const PHASE_MODULUS: u32 = 1 << PHASE_BITS;

/// The 4-bit key code of a block and F-number (`YM3526-015`): the block over
/// one note bit, the F-number's bit 9 with `NOTE SEL` clear and its bit 8
/// with it set.
pub(crate) const fn key_code(block: u8, fnum: u16, note_sel: bool) -> u8 {
    let bit = if note_sel { 8 } else { 9 };
    ((block & 7) << 1) | ((fnum >> bit) & 1) as u8
}

/// The increment of a block and F-number before the multiplier:
/// `F × 2^block` (`YM3526-013`). The F-number is taken to ten bits after
/// vibrato's offset is added, so an offset past 3FFH wraps (`YM3526-026`);
/// no offset can take it below 0, being at most its top three bits.
pub(crate) const fn base_increment(block: u8, fnum: i32) -> u32 {
    ((fnum & 0x3FF) as u32) << (block & 7)
}

/// `MULTI` as twice the factor it multiplies by: ½, 1–10, 10, 12, 12, 15,
/// 15 (`YM3526-014`).
pub(crate) const MUL_TIMES_TWO: [u32; 16] =
    [1, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 20, 24, 24, 30, 30];

/// An operator's increment (`YM3526-013`, `YM3526-014`).
pub(crate) const fn increment(block: u8, fnum: i32, mul: u8) -> u32 {
    (base_increment(block, fnum) * MUL_TIMES_TWO[(mul & 0xF) as usize]) >> 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_sel_takes_bit_8() {
        assert_eq!(key_code(4, 0x100, true), 9);
        assert_eq!(key_code(4, 0x200, true), 8);
    }

    #[test]
    fn raised_f_number_wraps() {
        assert_eq!(increment(7, 0x3FF + 3, 1), 2 << 7);
    }
}
