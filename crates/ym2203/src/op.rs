//! The operator: a log-sin table, an exponential table, the attenuation
//! domain, and the modulation and feedback arithmetic.
//!
//! **The tables are the crate's definition** (`YM2203-OPEN-03`), two closed
//! forms — a quarter sine of 256 entries in a log unit of 1/256 octave, and
//! 256 entries of `2^(x/256)`. The attenuation domain `YM2203-020` fixes
//! (0.75 dB steps of `TL`) is 1/64 of a 6 dB octave per unit.
//!
//! **The depths are `YM2203-023` and `YM2203-024`**: a full-scale
//! modulator adds 8π of phase to its carrier, and the self-feedback path
//! adds `π/16 × 2^(FB − 1)` at the average of the last two outputs.
//!
//! Everything is integer. The generation works in Q48.

/// Fractional bits the table generation works in.
const SHIFT: u32 = 48;
/// One unit in Q48.
const ONE: i128 = 1 << SHIFT;
/// Half a unit in Q48, for rounding to nearest.
const HALF: i128 = ONE / 2;
/// π in Q48.
const PI_Q48: i128 = 884_279_719_003_555;
/// ln 2 in Q48.
const LN2_Q48: i128 = 195_103_586_505_167;

/// Entries in each of the two tables.
pub(crate) const TABLE_ENTRIES: usize = 256;

/// The sine table's length: one cycle (`YM2203-OPEN-03`).
pub(crate) const SINE_ENTRIES: u32 = 1024;

/// Attenuation units per factor of two in amplitude: 0.09375 dB a unit, 64
/// of them 6 dB (`YM2203-020`).
#[allow(dead_code, reason = "states the unit the tables are built in")]
pub(crate) const UNITS_PER_OCTAVE: u32 = 64;

/// The largest attenuation the envelope reaches: silence.
pub(crate) const ENVELOPE_MAX: u16 = 1023;

/// The operator's output is this many bits including the sign, so its
/// magnitude reaches 8192 (`YM2203-OPEN-03`).
#[allow(dead_code, reason = "states the operator output's width")]
pub(crate) const OUTPUT_BITS: u32 = 14;

/// `sin(θ)` in Q48 for `0 < θ < π/2`, by the odd Taylor series.
const fn sin_q48(theta: i128) -> i128 {
    let square = (theta * theta) >> SHIFT;
    let mut term = theta;
    let mut total = theta;
    let mut k: i128 = 1;
    while term != 0 && k < 20 {
        term = ((term * square) >> SHIFT) / ((2 * k) * (2 * k + 1));
        total += if k % 2 == 1 { -term } else { term };
        k += 1;
    }
    total
}

/// `log2(u)` in Q48 for `u` in `[1, 2)`, by repeated squaring.
const fn log2_fraction_q48(u: i128) -> i128 {
    let mut value = u;
    let mut fraction = 0;
    let mut bit = 1;
    while bit <= SHIFT {
        value = (value * value) >> SHIFT;
        if value >= 2 * ONE {
            value >>= 1;
            fraction += ONE >> bit;
        }
        bit += 1;
    }
    fraction
}

/// `e^y` in Q48 for `0 <= y < 3`, by the Taylor series.
const fn exp_q48(y: i128) -> i128 {
    let mut term = ONE;
    let mut total = ONE;
    let mut n: i128 = 1;
    while term != 0 && n < 60 {
        term = ((term * y) >> SHIFT) / n;
        total += term;
        n += 1;
    }
    total
}

/// `round(-log2(sin((x + 0.5) × π/512)) × 256)`.
const fn log_sin_entry(x: usize) -> u16 {
    let theta = ((2 * x as i128 + 1) * PI_Q48) >> 10;
    let sine = sin_q48(theta);
    let mut significand = sine;
    let mut octaves: i128 = 0;
    while significand < ONE {
        significand <<= 1;
        octaves += 1;
    }
    let fraction = log2_fraction_q48(significand);
    ((((octaves << SHIFT) - fraction) * 256 + HALF) >> SHIFT) as u16
}

/// `round((2^(x/256) - 1) × 1024)`.
const fn exp_entry(x: usize) -> u16 {
    let y = (((x as i128 * ONE) / 256) * LN2_Q48) >> SHIFT;
    let total = exp_q48(y);
    (((total - ONE) * 1024 + HALF) >> SHIFT) as u16
}

/// The log-sin table: the sine's first quarter, 256 entries, in units of
/// 1/256 octave.
pub(crate) const LOG_SIN: [u16; TABLE_ENTRIES] = {
    let mut table = [0u16; TABLE_ENTRIES];
    let mut x = 0;
    while x < TABLE_ENTRIES {
        table[x] = log_sin_entry(x);
        x += 1;
    }
    table
};

/// The exponential table: 256 entries of 10 bits, the hidden bit dropped.
pub(crate) const EXP: [u16; TABLE_ENTRIES] = {
    let mut table = [0u16; TABLE_ENTRIES];
    let mut x = 0;
    while x < TABLE_ENTRIES {
        table[x] = exp_entry(x);
        x += 1;
    }
    table
};

/// The bit the exponential table does not store, because it is always set.
pub(crate) const HIDDEN_BIT: u16 = 1024;

/// The log-sin value at a phase index of a full cycle, and the sign of
/// that half: the second half is the first negated, the second quarter of
/// each half the first mirrored.
pub(crate) const fn log_sin(index: u32) -> (u16, bool) {
    let i = index & (SINE_ENTRIES - 1);
    let quarter = (i & 0xFF) as usize;
    let mirrored = i & 0x100 != 0;
    let negative = i & 0x200 != 0;
    let value = if mirrored {
        LOG_SIN[0xFF - quarter]
    } else {
        LOG_SIN[quarter]
    };
    (value, negative)
}

/// The amplitude for a total attenuation in the envelope's unit (64 per
/// octave) plus a log-sin value (256 per octave): a magnitude of at most
/// 8168.
pub(crate) const fn amplitude(envelope_units: u32, log_sin_value: u16) -> u16 {
    let total = (envelope_units << 2) + log_sin_value as u32;
    let shift = total >> 8;
    if shift >= 16 {
        return 0;
    }
    let mantissa = (HIDDEN_BIT | EXP[(!total & 0xFF) as usize]) as u32;
    ((mantissa << 3) >> (shift + 1)) as u16
}

/// An operator's output for its phase index (with modulation already
/// added) and its attenuation in envelope units: a 14-bit signed value, the
/// negative half the positive negated (`YM2203-025`).
pub(crate) const fn operator_output(phase_index: u32, envelope_units: u32) -> i16 {
    let (ls, negative) = log_sin(phase_index);
    let magnitude = amplitude(envelope_units, ls) as i16;
    if negative { -magnitude } else { magnitude }
}

/// The phase offset, in sine-table indices, that a modulating output
/// contributes: the 14-bit output halved, so full scale is 4096 indices,
/// four cycles, 8π (`YM2203-023`).
pub(crate) const fn modulation_index(modulator: i32) -> i32 {
    modulator >> 1
}

/// The phase offset the self-feedback path contributes from the last two
/// outputs at feedback level `fb` (`YM2203-024`): their sum shifted so that
/// `FB = 1` is π/16 at a full-scale average and each step doubles it.
pub(crate) const fn feedback_index(previous: i16, before: i16, fb: u8) -> i32 {
    if fb == 0 {
        return 0;
    }
    (previous as i32 + before as i32) >> (10 - fb as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eg;

    #[test]
    fn modulation_and_feedback_depths() {
        const INDEX_MILLIPI: [(u8, u32); 4] = [(8, 3988), (16, 1980), (24, 971), (32, 506)];
        const FEEDBACK_MILLIPI: [(u8, u32); 4] = [(1, 70), (2, 140), (3, 274), (5, 1038)];
        let peak = |tl: u8| i32::from(operator_output(256, u32::from(eg::total_level(tl))));
        for (tl, millipi) in INDEX_MILLIPI {
            let got = modulation_index(peak(tl)) as u32 * 2000 / 1024;
            assert!(
                got.abs_diff(millipi) <= millipi / 20,
                "TL {tl}: {got} against {millipi}"
            );
        }
        let full = operator_output(256, 0);
        assert_eq!(full, 8168, "full scale, amplitude's figure");
        for (fb, millipi) in FEEDBACK_MILLIPI {
            let got = feedback_index(full, full, fb) as u32 * 2000 / 1024;
            assert!(
                got.abs_diff(millipi) <= millipi * 3 / 20,
                "FB {fb}: {got} against {millipi}"
            );
        }
    }
}
