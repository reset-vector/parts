//! The phase generator: block and F-number to a phase increment, the key
//! code, detune and the multiplier.
//!
//! **The increment's unit is one sample's `2^−20` of a cycle.** An
//! operator's phase is a 20-bit accumulator advanced once a sample, one
//! sample every `12 × n` cycles of `φM` for the FM divider `n`, so an
//! increment of `k` is `k × φM / (12 × n × 2^20)` in frequency, and
//! `F × 2^block / 2` is the increment of block and F-number: the pitch
//! `YM2203-014` states.

/// Bits in an operator's phase accumulator: ten of sine-table index over
/// ten of fraction.
pub(crate) const PHASE_BITS: u32 = 20;
/// The phase accumulator's fraction bits.
pub(crate) const PHASE_FRACTION_BITS: u32 = 10;
/// The phase accumulator wraps here.
pub(crate) const PHASE_MODULUS: u32 = 1 << PHASE_BITS;

/// The 5-bit key code of a block and F-number (`YM2203-017`): the block over
/// two note bits, `N4` the F-number's bit 10 and `N3` decoded from bits 10
/// to 7 — any of 9, 8, 7 set when bit 10 is, all three when it is not.
pub(crate) const fn key_code(block: u8, fnum: u16) -> u8 {
    let f11 = (fnum >> 10) & 1;
    let f10 = (fnum >> 9) & 1;
    let f9 = (fnum >> 8) & 1;
    let f8 = (fnum >> 7) & 1;
    let n3 = if f11 != 0 {
        f10 | f9 | f8
    } else {
        f10 & f9 & f8
    };
    ((block & 7) << 2) | ((f11 as u8) << 1) | n3 as u8
}

/// The increment of a block and F-number before detune and multiplier:
/// `F × 2^block / 2` (`YM2203-014`).
pub(crate) const fn base_increment(block: u8, fnum: u16) -> u32 {
    ((fnum as u32 & 0x7FF) << (block & 7)) >> 1
}

/// Detune's magnitude in increment units for `DT = 1, 2, 3`, by key code
/// (`YM2203-016`).
pub(crate) const DT_UNITS: [[u8; 3]; 32] = [
    [0, 1, 2],
    [0, 1, 2],
    [0, 1, 2],
    [0, 1, 2],
    [1, 2, 2],
    [1, 2, 3],
    [1, 2, 3],
    [1, 2, 3],
    [1, 2, 4],
    [1, 3, 4],
    [1, 3, 4],
    [1, 3, 5],
    [2, 4, 5],
    [2, 4, 6],
    [2, 4, 6],
    [2, 5, 7],
    [2, 5, 8],
    [3, 6, 8],
    [3, 6, 9],
    [3, 7, 10],
    [4, 8, 11],
    [4, 8, 12],
    [4, 9, 13],
    [5, 10, 14],
    [5, 11, 16],
    [6, 12, 17],
    [6, 13, 19],
    [7, 14, 20],
    [8, 16, 22],
    [8, 16, 22],
    [8, 16, 22],
    [8, 16, 22],
];

/// Detune's signed offset for a key code and the 3-bit field: bit 2 the
/// sign, 0 and 4 no change (`YM2203-016`).
pub(crate) const fn dt_offset(kc: u8, dt: u8) -> i32 {
    let magnitude = match dt & 3 {
        0 => 0,
        n => DT_UNITS[(kc & 0x1F) as usize][n as usize - 1] as i32,
    };
    if dt & 4 != 0 { -magnitude } else { magnitude }
}

/// The multiplier's numerator over two: `MUL = 0` halves, `MUL = n`
/// multiplies by `n` (`YM2203-015`).
pub(crate) const fn mul_times_two(mul: u8) -> u32 {
    match mul & 0xF {
        0 => 1,
        n => 2 * n as u32,
    }
}

/// An operator's increment: detune added to the base, then the multiplier
/// (`YM2203-016`). A negative detune past zero stops at zero
/// (`YM2203-OPEN-09`).
pub(crate) const fn increment(block: u8, fnum: u16, dt: u8, mul: u8) -> u32 {
    let kc = key_code(block, fnum);
    let detuned = base_increment(block, fnum) as i32 + dt_offset(kc, dt);
    let detuned = if detuned < 0 { 0 } else { detuned as u32 };
    (detuned * mul_times_two(mul)) >> 1
}
