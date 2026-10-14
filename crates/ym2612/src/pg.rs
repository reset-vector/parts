//! The phase generator: block and F-number to a phase increment, the key
//! code, detune, the multiplier, and the low-frequency oscillator.
//!
//! **The increment's unit is `φM / (144 × 2^20)`.** An operator's phase is a
//! 20-bit accumulator advanced once a sample (`φM / 144`), so an increment of
//! `n` is `n × φM / (144 × 2^20)` in frequency, and `F × 2^block / 2` is the
//! increment of block and F-number: the pitch `YM2612-012` states.

use crate::op::{LN2_Q48, exp_q48};

/// Bits in an operator's phase accumulator: ten of sine-table index over
/// ten of fraction.
pub(crate) const PHASE_BITS: u32 = 20;
/// The phase accumulator's fraction bits.
pub(crate) const PHASE_FRACTION_BITS: u32 = 10;
/// The phase accumulator wraps here.
pub(crate) const PHASE_MODULUS: u32 = 1 << PHASE_BITS;

/// The 5-bit key code of a block and F-number (`YM2612-017`): the block over
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
/// `F × 2^block / 2` (`YM2612-012`). The F-number is 11 bits as written and
/// up to 12 under phase modulation.
pub(crate) const fn base_increment(block: u8, fnum: u16) -> u32 {
    ((fnum as u32 & 0xFFF) << (block & 7)) >> 1
}

/// Detune's magnitude in increment units for `DT = 1, 2, 3`, by key code
/// (`YM2612-015`).
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
/// sign, 0 and 4 no change (`YM2612-014`, `YM2612-015`).
pub(crate) const fn dt_offset(kc: u8, dt: u8) -> i32 {
    let magnitude = match dt & 3 {
        0 => 0,
        n => DT_UNITS[(kc & 0x1F) as usize][n as usize - 1] as i32,
    };
    if dt & 4 != 0 { -magnitude } else { magnitude }
}

/// The multiplier's numerator over two: `MUL = 0` halves, `MUL = n`
/// multiplies by `n` (`YM2612-013`).
pub(crate) const fn mul_times_two(mul: u8) -> u32 {
    match mul & 0xF {
        0 => 1,
        n => 2 * n as u32,
    }
}

/// An operator's increment: detune added to the base, then the multiplier
/// (`YM2612-016`). A negative detune past zero stops at zero
/// (`YM2612-OPEN-12`).
/// `kc` is the key code of the F-number as written, not as modulated.
pub(crate) const fn increment(block: u8, fnum: u16, kc: u8, dt: u8, mul: u8) -> u32 {
    let detuned = base_increment(block, fnum) as i32 + dt_offset(kc, dt);
    let detuned = if detuned < 0 { 0 } else { detuned as u32 };
    (detuned * mul_times_two(mul)) >> 1
}

/// The LFO's divider for each 3-bit frequency: it steps once every this many
/// samples through 128 steps a cycle (`YM2612-027`).
pub(crate) const LFO_DIVIDERS: [u32; 8] = [108, 77, 71, 67, 62, 44, 8, 5];

/// Steps in one LFO cycle (`YM2612-027`).
pub(crate) const LFO_STEPS: u8 = 128;

/// The low-frequency oscillator (`YM2612-026` to `YM2612-029`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Lfo {
    step: u8,
    count: u32,
}

impl Lfo {
    /// At step zero.
    pub(crate) const fn new() -> Self {
        Lfo { step: 0, count: 0 }
    }

    /// One sample, under the `0x22` byte: disabled, the LFO is held at step
    /// zero (`YM2612-029`).
    pub(crate) fn advance(&mut self, reg22: u8) {
        if reg22 & 0x08 == 0 {
            *self = Self::new();
            return;
        }
        self.count += 1;
        if self.count >= LFO_DIVIDERS[(reg22 & 7) as usize] {
            self.count = 0;
            self.step = (self.step + 1) % LFO_STEPS;
        }
    }

    /// The phase within the cycle: its step, 0 to 127.
    #[must_use]
    pub const fn phase(&self) -> u8 {
        self.step
    }

    /// The amplitude wave in the envelope's steps of 0.09375 dB: a triangle
    /// from 126 at the LFO's step 0 down to 0 and back (`YM2612-029`).
    #[must_use]
    pub const fn am_waveform_steps(&self) -> u32 {
        let s = self.step;
        (if s < 64 { (63 - s) * 2 } else { (s - 64) * 2 }) as u32
    }

    /// The phase wave in steps of a seventh of `FMS`'s deviation, −7 to 7: a
    /// triangle rising from 0 at the LFO's step 0, in 32 positions a cycle
    /// (`YM2612-029`).
    #[must_use]
    pub const fn pm_waveform_steps(&self) -> i32 {
        let i = self.step >> 2;
        let q = (i & 7) as i32;
        let v = if i & 8 != 0 { 7 - q } else { q };
        if i & 16 != 0 { -v } else { v }
    }
}

/// The attenuation `AM` adds at `AMS`, in envelope units: the wave's 126 at
/// most shifted to 11.8, 5.9 and 1.4 dB (`YM2612-028`).
pub(crate) const fn am_attenuation(am_waveform: u32, ams: u8) -> u32 {
    match ams & 3 {
        0 => 0,
        1 => am_waveform >> 3,
        2 => am_waveform >> 1,
        _ => am_waveform,
    }
}

/// `FMS`'s maximum deviation in tenths of a cent: the manual's percent of a
/// halftone (`YM2612-028`).
pub(crate) const FMS_DECICENTS: [u32; 8] = [0, 34, 67, 100, 140, 200, 400, 800];

const SHIFT: u32 = 48;
const ONE: i128 = 1 << SHIFT;

/// `2^(decicents / 12000) − 1` in Q20, and its negative counterpart
/// `2^(−decicents / 12000) − 1`, rounded toward zero.
const fn pm_factor_q20(decicents: u32, negative: bool) -> i32 {
    let y = (decicents as i128 * LN2_Q48) / 12000;
    let up = exp_q48(y);
    let ratio = if negative { (ONE * ONE) / up } else { up };
    (((ratio - ONE) << 20) >> SHIFT) as i32
}

/// The F-number factor for every `FMS` and wave magnitude 0 to 7, in Q20:
/// `[fms][magnitude][0 up, 1 down]`.
pub(crate) const PM_FACTOR_Q20: [[[i32; 2]; 8]; 8] = {
    let mut t = [[[0i32; 2]; 8]; 8];
    let mut fms = 0;
    while fms < 8 {
        let mut m = 0;
        while m < 8 {
            let dc = FMS_DECICENTS[fms] * m as u32 / 7;
            t[fms][m][0] = pm_factor_q20(dc, false);
            t[fms][m][1] = pm_factor_q20(dc, true);
            m += 1;
        }
        fms += 1;
    }
    t
};

/// The F-number under phase modulation: moved by `FMS`'s deviation scaled
/// by the wave (`YM2612-028`, `YM2612-029`, `YM2612-OPEN-13`).
pub(crate) const fn modulated_fnum(fnum: u16, pm_waveform: i32, fms: u8) -> u16 {
    if pm_waveform == 0 || fms & 7 == 0 {
        return fnum;
    }
    let m = pm_waveform.unsigned_abs() as usize;
    let f = PM_FACTOR_Q20[(fms & 7) as usize][m][(pm_waveform < 0) as usize];
    let delta = ((fnum as i64 * f as i64) >> 20) as i32;
    let v = fnum as i32 + delta;
    if v < 0 {
        0
    } else if v > 0xFFF {
        0xFFF
    } else {
        v as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn am_depths() {
        const MANUAL_AMS_MILLIDB: [u32; 4] = [0, 1400, 5900, 11800];
        for ams in 0..4u8 {
            let got = u64::from(am_attenuation(126, ams)) * 9375 / 100;
            let want = u64::from(MANUAL_AMS_MILLIDB[ams as usize]);
            assert!(got.abs_diff(want) <= 60, "AMS {ams}: {got} mdB");
        }
    }

    #[test]
    fn fms_deviations() {
        const MANUAL_FMS_TENTHS: [u32; 8] = [0, 34, 67, 100, 140, 200, 400, 800];
        assert_eq!(MANUAL_FMS_TENTHS, FMS_DECICENTS);
    }
}
