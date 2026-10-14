//! The operator: a log-sin table, an exponential table, and the two
//! rectifications.
//!
//! **The tables here are generated from two closed forms, and not
//! transcribed** (`YM2413-090`, `YM2413-091`).
//!
//! Everything is integer. The generation works in Q48, which leaves the error
//! near 10⁻¹² of a step.

use crate::pg::{PHASE_MODULUS, SINE_ENTRIES};

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

/// Entries in each of the two tables (`YM2413-090`, `YM2413-091`).
pub(crate) const TABLE_ENTRIES: usize = 256;

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
///
/// Squaring `u` doubles its logarithm, so each iteration reads off one more
/// bit: the bit is set exactly when the square leaves the interval.
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

/// `round(-log2(sin((x + 0.5) × π/512)) × 256)` (`YM2413-090`).
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

/// `round((2^(x/256) - 1) × 1024)` (`YM2413-091`).
const fn exp_entry(x: usize) -> u16 {
    let y = (((x as i128 * ONE) / 256) * LN2_Q48) >> SHIFT;
    let mut term = ONE;
    let mut total = ONE;
    let mut n: i128 = 1;
    while term != 0 && n < 30 {
        term = ((term * y) >> SHIFT) / n;
        total += term;
        n += 1;
    }
    (((total - ONE) * 1024 + HALF) >> SHIFT) as u16
}

/// The log-sin table: the sine's first quarter, 256 entries of 12 bits, in the
/// logarithmic unit (`YM2413-090`).
pub(crate) const LOG_SIN: [u16; TABLE_ENTRIES] = {
    let mut table = [0u16; TABLE_ENTRIES];
    let mut x = 0;
    while x < TABLE_ENTRIES {
        table[x] = log_sin_entry(x);
        x += 1;
    }
    table
};

/// The exponential table: 256 entries of 10 bits, the hidden bit dropped
/// (`YM2413-091`).
pub(crate) const EXP: [u16; TABLE_ENTRIES] = {
    let mut table = [0u16; TABLE_ENTRIES];
    let mut x = 0;
    while x < TABLE_ENTRIES {
        table[x] = exp_entry(x);
        x += 1;
    }
    table
};

/// The bit the exponential table does not store, because it is always set
/// (`YM2413-091`).
pub(crate) const HIDDEN_BIT: u16 = 1024;

/// Steps of the logarithmic unit in one octave (`YM2413-094`).
///
/// One channel-volume step is 128 of these and one envelope level is 16, so an
/// octave — 6.02 dB — is 256.
#[cfg(test)]
pub(crate) const UNITS_PER_OCTAVE: u32 = 256;

/// The word the converter is handed, in the form the part carries it
/// (`YM2413-095`).
///
/// Sign and magnitude, not two's complement, so zero has two representations
/// and the negative extreme is −255 rather than −256.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Word {
    /// The sign bit: set for the negative half of the wave.
    pub negative: bool,
    /// The magnitude, `0..=255`.
    pub magnitude: u8,
}

impl Word {
    /// The word as a signed number, which collapses the two zeros into one.
    #[must_use]
    pub fn value(self) -> i32 {
        let magnitude = i32::from(self.magnitude);
        if self.negative { -magnitude } else { magnitude }
    }
}

/// A log-sin lookup: the sign beside a magnitude in the logarithmic domain,
/// twelve bits (`YM2413-090`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LogWord {
    pub(crate) negative: bool,
    pub(crate) magnitude: u16,
}

/// The log-sin lookup over the full cycle (`YM2413-092`).
///
/// The table holds the first quadrant. The second and fourth come from
/// complementing the eight-bit index, and the third and fourth from the sign,
/// which is carried beside the magnitude rather than in it.
#[must_use]
pub(crate) fn log_sin(index: u16) -> LogWord {
    let index = u32::from(index) % SINE_ENTRIES;
    let mirrored = index & 0x100 != 0;
    let entry = index & 0xFF;
    let entry = if mirrored { entry ^ 0xFF } else { entry };
    LogWord {
        negative: index & 0x200 != 0,
        magnitude: LOG_SIN[entry as usize],
    }
}

/// The exponential lookup (`YM2413-091`, `YM2413-093`).
///
/// The low eight bits index the table, the hidden bit is restored, and the bits
/// above shift the significand down — the exponent of a floating point value.
/// The result's low four bits are dropped, which is what leaves a magnitude of
/// eight bits.
#[must_use]
pub(crate) fn exp(value: u32) -> u8 {
    let significand = (EXP[(value as usize & 0xFF) ^ 0xFF] | HIDDEN_BIT) << 1;
    let exponent = value >> 8;
    if exponent >= 16 {
        return 0;
    }
    u8::try_from((significand >> exponent) >> 4).expect("twelve bits shifted down four")
}

/// The logarithmic unit an attenuation in sixteenths of a decibel names.
///
/// Every attenuation term this crate uses is a whole number of sixteenths divisible
/// by three — a total level step is 12, a volume step 48, an envelope level 6,
/// and every entry of the key scale table is divisible by six — so this
/// conversion is **exact** for all of them: 0.75 dB is 32 units, 3 dB is 128,
/// an envelope level is 16.
#[must_use]
pub(crate) fn attenuation_units(db16: u16) -> u32 {
    u32::from(db16) * 8 / 3
}

/// The rectification a waveform-select bit applies (`YM2413-061`,
/// `YM2413-062`).
///
/// "The modulated wave is rectified to half wave", and the same for the
/// carrier: the negative half of the sine is replaced by silence.
#[must_use]
pub(crate) fn rectify(word: Word, half_wave: bool) -> Word {
    if half_wave && word.negative {
        Word::default()
    } else {
        word
    }
}

/// The feedback modulation index a three-bit `FB` field names, in phase
/// accumulator units (`YM2413-059`).
///
/// The manual's Table III-6 runs `0, π/16, π/8, π/4, π/2, π, 2π, 4π`. Half a
/// cycle of the accumulator is π, so `FB = 1` is a sixteenth of that and each
/// step above doubles.
#[must_use]
pub(crate) fn feedback_depth(fb: u8) -> u32 {
    match fb & 7 {
        0 => 0,
        n => ((PHASE_MODULUS / 2) >> 4) << (n - 1),
    }
}

/// Bits an operator's signed output carries, before the converter
/// (`YM2413-083`).
pub(crate) const OUTPUT_BITS: u32 = 9;
/// One more than the largest magnitude a word takes: the magnitude is eight
/// bits and the ninth is the sign (`YM2413-095`).
pub(crate) const OUTPUT_SCALE: i32 = 1 << (OUTPUT_BITS - 1);

/// One pass through the operator cell (`YM2413-089`).
///
/// `phase` is the slot's accumulator index after any modulation, `attenuation`
/// the sum of every logarithmic term in sixteenths of a decibel, and
/// `half_wave` the slot's rectification bit. The path is the part's: a log-sin
/// lookup, the attenuation added in the same unit, and an exponential lookup
/// back to a linear magnitude.
#[must_use]
pub(crate) fn operate(phase: u16, attenuation: u16, half_wave: bool) -> Word {
    let logarithmic = log_sin(phase);
    let sum = u32::from(logarithmic.magnitude) + attenuation_units(attenuation);
    let word = Word {
        negative: logarithmic.negative,
        magnitude: exp(sum),
    };
    rectify(word, half_wave)
}

/// The phase offset a modulating output imposes on the slot it drives.
///
/// **This scaling is the crate's own** (`YM2413-OPEN-12`). The manual says the
/// modulator's total level *is* the modulation index control (`YM2413-064`) and
/// gives the feedback index in radians, but it never states the deviation a
/// full-scale modulator output produces. Taking full scale as π keeps the
/// feedback path and the modulation path in one unit.
#[must_use]
pub(crate) fn modulation(output: i32, depth: u32) -> u32 {
    let scaled = i64::from(output) * i64::from(depth) / i64::from(OUTPUT_SCALE);
    let modulus = i64::from(PHASE_MODULUS);
    u32::try_from(scaled.rem_euclid(modulus)).expect("reduced modulo the accumulator")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eg;
    use crate::pg::PHASE_FRACTION_BITS;
    use std::string::String;
    use std::vec::Vec;

    /// The word at sine index `index`, full gain, no rectification.
    fn wave(index: u16) -> i32 {
        operate(index, 0, false).value()
    }

    #[test]
    fn the_sine_spans_a_cycle_of_1024() {
        assert_eq!(SINE_ENTRIES, 1024);
        assert_eq!(wave(0), 0);
        assert_eq!(wave(256), OUTPUT_SCALE - 1, "the peak is full scale");
        assert_eq!(wave(512), 0);
        assert_eq!(wave(768), -(OUTPUT_SCALE - 1));
        for i in 0..256u16 {
            if i > 0 {
                assert!(wave(i) >= wave(i - 1), "rising at {i}");
            }
            assert_eq!(wave(i), wave(511 - i), "mirror at {i}");
            assert_eq!(wave(i), -wave(512 + i), "half-cycle sign at {i}");
        }
        assert_eq!(wave(1024), wave(0));
        assert!(wave(128) > (OUTPUT_SCALE - 1) / 2);
    }

    #[test]
    fn attenuation_composes_by_addition() {
        let peak = |attenuation| operate(256, attenuation, false).magnitude;
        let six = 6 * eg::PER_DB;
        assert_eq!(peak(six + six), peak(12 * eg::PER_DB));
        assert_eq!(peak(12 * eg::PER_DB), 63, "12 dB is a quarter of 255");
        assert_eq!(peak(0), 255);
        assert_eq!(peak(eg::RANGE), 0);
        assert!(peak(eg::RANGE - eg::STEP * 8) > 0);
    }

    #[test]
    fn modulation_moves_the_phase_and_not_the_level() {
        for index in [0u16, 100, 256, 700] {
            assert_eq!(operate(index, 60 * eg::PER_DB, false).value(), 0);
        }
        let quarter = PHASE_MODULUS / 4;
        let modulated = modulation(OUTPUT_SCALE / 2, PHASE_MODULUS / 2);
        assert_eq!(modulated, quarter);
        let index = u16::try_from(modulated >> PHASE_FRACTION_BITS).expect("ten bits");
        assert_eq!(wave(index), OUTPUT_SCALE - 1);
        assert_eq!(
            modulation(-(OUTPUT_SCALE / 2), PHASE_MODULUS / 2),
            PHASE_MODULUS - quarter
        );
    }

    #[test]
    fn feedback_doubles_from_a_sixteenth_of_pi() {
        let pi = PHASE_MODULUS / 2;
        let expected = [0, pi / 16, pi / 8, pi / 4, pi / 2, pi, 2 * pi, 4 * pi];
        for (fb, depth) in (0u8..).zip(expected) {
            assert_eq!(feedback_depth(fb), depth, "FB {fb}");
        }
    }

    #[test]
    fn rectification_silences_the_negative_half() {
        let crest = operate(256, 0, false);
        let trough = operate(768, 0, false);
        assert_eq!(rectify(crest, true), crest);
        assert_eq!(rectify(trough, true), Word::default());
        assert_eq!(rectify(trough, false), trough);
        assert_eq!(operate(768, 0, true).value(), 0);
        assert_eq!(operate(256, 0, true).value(), OUTPUT_SCALE - 1);
    }

    #[test]
    fn the_word_is_nine_bits_of_sign_and_magnitude() {
        assert_eq!(OUTPUT_BITS, 9);
        assert_eq!(OUTPUT_SCALE, 256);
        assert_eq!(wave(256), 255);
        assert_eq!(wave(768), -255);

        let mut codes: Vec<Word> = (0..1024).map(|index| operate(index, 0, false)).collect();
        codes.sort_by_key(|word| (word.negative, word.magnitude));
        codes.dedup();
        assert_eq!(codes.len(), 400);
        let zero = |negative| Word {
            negative,
            magnitude: 0,
        };
        assert!(codes.contains(&zero(false)) && codes.contains(&zero(true)));
    }

    #[test]
    fn the_log_sin_and_exp_tables_hold_their_endpoints() {
        assert!(LOG_SIN.iter().all(|entry| *entry < 4096));
        assert!(EXP.iter().all(|entry| *entry < 1024));
        assert_eq!(LOG_SIN[0], 2137);
        assert_eq!(LOG_SIN[TABLE_ENTRIES - 1], 0);
        assert_eq!(EXP[0], 0);
        assert_eq!(EXP[TABLE_ENTRIES - 1], 1018);
        for x in 1..TABLE_ENTRIES {
            assert!(LOG_SIN[x] <= LOG_SIN[x - 1], "log-sin falls at {x}");
            assert!(EXP[x] > EXP[x - 1], "exponential rises at {x}");
        }

        for (index, &entry) in (0u16..).zip(LOG_SIN.iter()) {
            let first = log_sin(index);
            assert_eq!(first.magnitude, entry);
            assert!(!first.negative);
            assert_eq!(log_sin(511 - index).magnitude, entry);
            assert!(log_sin(index + 512).negative);
        }

        assert_eq!(u16::from(exp(0)), (2 * (1018 + HIDDEN_BIT)) >> 4);
        assert_eq!(exp(0), 255);
    }

    #[test]
    fn each_volume_lands_on_its_amplitude() {
        const PEAKS: [u8; 16] = [255, 180, 127, 90, 63, 45, 31, 22, 15, 11, 7, 5, 3, 2, 1, 1];
        for (field, expected) in (0u8..).zip(PEAKS) {
            let peak = operate(256, eg::volume(field), false).magnitude;
            assert_eq!(peak, expected, "volume {field}");
        }
        assert_eq!(attenuation_units(eg::volume(1)), 128);
        assert_eq!(UNITS_PER_OCTAVE, 256);
        assert_eq!(attenuation_units(eg::total_level(1)), 32);
        assert_eq!(attenuation_units(eg::STEP), 16);
    }

    #[test]
    fn the_generated_tables_agree_with_the_printed_ones() {
        let Some(path) = std::env::var_os("OPLX_TABLES") else {
            return;
        };
        let text = std::fs::read_to_string(&path).expect("the table rows");
        let rows: Vec<(usize, u16)> = text
            .lines()
            .map(|line| {
                let (x, y) = line.split_once(' ').expect("an `x y` row");
                (x.parse().expect("an index"), y.parse().expect("an entry"))
            })
            .collect();
        assert_eq!(rows.len(), 2 * TABLE_ENTRIES, "512 rows");
        let mut mismatches = String::new();
        for (index, (x, y)) in rows.iter().enumerate() {
            let (table, entry, generated) = if index < TABLE_ENTRIES {
                ("exp", index, EXP[index])
            } else {
                let entry = index - TABLE_ENTRIES;
                ("log-sin", entry, LOG_SIN[entry])
            };
            if entry != *x || generated != *y {
                mismatches +=
                    &std::format!("{table}[{entry}]: printed {x} {y}, generated {generated}\n");
            }
        }
        assert!(mismatches.is_empty(), "{mismatches}");
        std::println!("all {} entries agree with the document", rows.len());
    }
}
