//! The operator: a log-sin table, an exponential table, the attenuation
//! domain, and the feedback and modulation arithmetic.
//!
//! **The tables are the crate's definition** (`YM2151-OPEN-02`), generated
//! from two closed forms — a quarter sine of 256 entries in a log unit of
//! 1/256 octave, and 256 entries of `2^(x/256)`. The attenuation domain the
//! manual states (0.75 dB steps of `TL`, 3 dB of `D1L`, a 96 dB envelope,
//! `AMS` maxima of 23.90625 dB) is 1/64 of a 6 dB octave per unit.
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
pub(crate) const LN2_Q48: i128 = 195_103_586_505_167;

/// Entries in each of the two tables.
pub(crate) const TABLE_ENTRIES: usize = 256;

/// The sine table's length: one cycle (`YM2151-OPEN-02`).
pub(crate) const SINE_ENTRIES: u32 = 1024;

/// Attenuation units per factor of two in amplitude: the envelope's 96 dB
/// over 1024 units is 0.09375 dB a unit, and 64 of them are 6 dB
/// (`YM2151-027`, `YM2151-028`, `YM2151-030`).
#[allow(dead_code, reason = "states the unit the tables are built in")]
pub(crate) const UNITS_PER_OCTAVE: u32 = 64;

/// The largest attenuation the envelope reaches: silence (`YM2151-024`).
pub(crate) const ENVELOPE_MAX: u16 = 1023;

/// The operator's output is this many bits including the sign, so its
/// magnitude reaches 8192 (`YM2151-OPEN-02`).
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
pub(crate) const fn exp_q48(y: i128) -> i128 {
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
/// 8180.
pub(crate) const fn amplitude(envelope_units: u32, log_sin_value: u16) -> u16 {
    let total = (envelope_units << 2) + log_sin_value as u32;
    let shift = total >> 8;
    if shift >= 16 {
        return 0;
    }
    let mantissa = (HIDDEN_BIT | EXP[(!total & 0xFF) as usize]) as u32;
    ((mantissa << 3) >> (shift + 1)) as u16
}

/// A slot's output for its phase index (with modulation already added),
/// its envelope attenuation in units, and whether it is the noise slot
/// carrying the noise bit. A 14-bit signed value.
pub(crate) fn operator_output(phase_index: u32, envelope_units: u32, noise: Option<bool>) -> i16 {
    match noise {
        Some(bit) => {
            let att = if envelope_units > ENVELOPE_MAX as u32 {
                ENVELOPE_MAX as u32
            } else {
                envelope_units
            };
            let magnitude = ((ENVELOPE_MAX as u32 - att) << (OUTPUT_BITS - 11)) as i16;
            if bit { magnitude } else { -magnitude }
        }
        None => {
            let (ls, negative) = log_sin(phase_index);
            let magnitude = amplitude(envelope_units, ls) as i16;
            if negative { -magnitude } else { magnitude }
        }
    }
}

/// How deep a connection's modulation runs: the phase a full-scale
/// modulator output adds (`YM2151-OPEN-11`). The manual prints a depth for
/// the feedback path and none for a connection, so this is a choice of
/// reading and not a pin.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ModulationDepth {
    /// The 14-bit output halved: 8π either way, four cycles at full scale,
    /// twice the deepest feedback the manual prints.
    #[default]
    EightPi,
    /// The output quartered: 4π either way, two cycles, the same depth
    /// `FL = 7` reaches (`YM2151-021`, Fig. 2.10), which is the deepest
    /// phase deviation stated anywhere in the manual.
    FourPi,
}

/// The phase offset, in sine-table indices, that a modulating output
/// contributes at a chosen depth (`YM2151-OPEN-11`).
pub(crate) const fn modulation_index_at(modulator: i32, depth: ModulationDepth) -> i32 {
    match depth {
        ModulationDepth::EightPi => modulator >> 1,
        ModulationDepth::FourPi => modulator >> 2,
    }
}

/// The phase offset the self-feedback path contributes from the average of
/// the last two outputs at feedback level `fl` (`YM2151-021`,
/// `YM2151-OPEN-11`): `FL = 7` is 4π, two cycles, and each step below
/// halves it.
pub(crate) const fn feedback_index(previous: i16, before: i16, fl: u8) -> i32 {
    if fl == 0 {
        return 0;
    }
    (previous as i32 + before as i32) >> (10 - fl as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_levels() {
        assert_eq!(feedback_index(8191, 8191, 0), 0);
        assert_eq!(feedback_index(8191, 8191, 7), 2047);
        for fl in 1..7u8 {
            assert_eq!(
                feedback_index(8191, 8191, fl + 1),
                2 * feedback_index(8191, 8191, fl) + 1
            );
        }
        assert_eq!(feedback_index(8191, 8191, 1), 31);
    }

    #[test]
    fn total_level_in_amplitude() {
        let full = u64::from(amplitude(0, 0));
        let step = u64::from(amplitude(8, 0));
        assert!(
            (step * 10_000 / full).abs_diff(9170) <= 10,
            "{step} of {full}"
        );
        assert_eq!(u64::from(amplitude(64, 0)), full / 2);
    }

    #[test]
    fn modulation_depths() {
        assert_eq!(modulation_index_at(8191, ModulationDepth::EightPi), 4095);
        assert_eq!(modulation_index_at(8191, ModulationDepth::FourPi), 2047);
        assert_eq!(
            modulation_index_at(8191, ModulationDepth::FourPi),
            feedback_index(8191, 8191, 7),
            "FourPi is FL = 7's depth"
        );
        assert_eq!(
            modulation_index_at(8191, ModulationDepth::EightPi) / 1024,
            3
        );
        assert_eq!(
            modulation_index_at(8192, ModulationDepth::EightPi) / 1024,
            4
        );
        assert_eq!(modulation_index_at(8192, ModulationDepth::FourPi) / 1024, 2);
    }
}
