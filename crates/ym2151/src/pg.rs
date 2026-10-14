//! The phase generator: key code and fraction to a phase increment, the
//! detunes, the multiplier, and the low-frequency oscillator's phase
//! modulation.
//!
//! **The increment's unit is `φM / 2^26`.** A slot's phase is a 20-bit
//! accumulator advanced once a sample (`φM / 64`), so one unit of increment
//! is one 1024th of a sine-table step a sample, `φM / (64 × 2^20)` in
//! frequency: 0.0533 Hz at 3.579545 MHz. DT1's offsets, Fig. 2.6, are whole
//! numbers of it (`YM2151-017`), and `KC = $4A` sounds 440 Hz in it
//! (`YM2151-014`).

use crate::op::{LN2_Q48, exp_q48};

/// Bits in a slot's phase accumulator: ten of sine-table index over ten of
/// fraction.
pub(crate) const PHASE_BITS: u32 = 20;
/// The phase accumulator's fraction bits.
pub(crate) const PHASE_FRACTION_BITS: u32 = 10;
/// The phase accumulator wraps here.
pub(crate) const PHASE_MODULUS: u32 = 1 << PHASE_BITS;

/// The increment unit's denominator: an increment of `n` is `n × φM / 2^26`
/// in frequency (`YM2151-017`).
pub(crate) const INCREMENT_UNIT_LOG2: u32 = 26;

/// Key-fraction steps in a semitone (`YM2151-015`).
pub(crate) const STEPS_PER_SEMITONE: u32 = 64;
/// Key-fraction steps in an octave.
pub(crate) const STEPS_PER_OCTAVE: u32 = 12 * STEPS_PER_SEMITONE;
/// Octaves the key code spans (`YM2151-014`).
pub(crate) const OCTAVES: u32 = 8;
/// The last key position: octave 7, `C`, key fraction 63.
pub(crate) const KEY_POSITION_MAX: u32 = OCTAVES * STEPS_PER_OCTAVE - 1;

/// The reference: the note the manual anchors the scale to, `KC = $4A`
/// (octave 4, note 10, `A`), sounds 440.0 Hz at 3.579545 MHz with `KF = 0`,
/// `MUL = 1` and no detune (`YM2151-014`).
#[allow(dead_code, reason = "states the note the table is anchored to")]
pub(crate) const REFERENCE_KC: u8 = 0x4A;
/// The reference frequency in hertz.
pub(crate) const REFERENCE_HZ: u32 = 440;
/// The clock the reference is stated at, in hertz (`YM2151-002`).
pub(crate) const REFERENCE_CLOCK_HZ: u32 = 3_579_545;

/// The reference note's key position: octave 4, `A` (semitone 8 above
/// `C#`), fraction 0.
const REFERENCE_POSITION: u32 = (4 * 12 + 8) * STEPS_PER_SEMITONE;

const SHIFT: u32 = 48;
const ONE: i128 = 1 << SHIFT;
const HALF: i128 = ONE / 2;

/// The increment for key position `p` in octave 7's range: `round(2^26 ×
/// 440 / 3 579 545 × 2^((p − reference) / 768))`, in Q48 arithmetic.
const fn top_octave_entry(step: usize) -> u32 {
    let position = 7 * STEPS_PER_OCTAVE as i128 + step as i128;
    let reference_q48 =
        ((1i128 << INCREMENT_UNIT_LOG2) * REFERENCE_HZ as i128 * ONE) / REFERENCE_CLOCK_HZ as i128;
    let exponent_steps = position - REFERENCE_POSITION as i128;
    let whole = exponent_steps / STEPS_PER_OCTAVE as i128;
    let part = exponent_steps % STEPS_PER_OCTAVE as i128;
    let y = (part * LN2_Q48) / STEPS_PER_OCTAVE as i128;
    let scale = exp_q48(y) << whole;
    ((((reference_q48 * scale) >> SHIFT) + HALF) >> SHIFT) as u32
}

/// The increment for each of the 768 key positions of the top octave; lower
/// octaves are this shifted right (`YM2151-014`, `YM2151-015`,
/// `YM2151-OPEN-05`).
pub(crate) const TOP_OCTAVE: [u32; STEPS_PER_OCTAVE as usize] = {
    let mut table = [0u32; STEPS_PER_OCTAVE as usize];
    let mut i = 0;
    while i < STEPS_PER_OCTAVE as usize {
        table[i] = top_octave_entry(i);
        i += 1;
    }
    table
};

/// The semitone above `C#` that a 4-bit note code names (Fig. 2.4): codes
/// 0, 1, 2 are `C#`, `D`, `D#`; 4, 5, 6 are `E`, `F`, `F#`; 8, 9, 10 are
/// `G`, `G#`, `A`; 12, 13, 14 are `A#`, `B`, `C`. The manual leaves 3, 7,
/// 11 and 15 blank; the crate reads each as the note after it
/// (`YM2151-OPEN-04`), so 15 is the next octave's `C#`.
pub(crate) const fn semitone_of_note_code(code: u8) -> u32 {
    let code = (code & 0xF) as u32;
    code - (code >> 2)
}

/// The key position — 64 steps a semitone from octave 0 `C#` — of a key
/// code and key fraction (`YM2151-014`, `YM2151-015`).
pub(crate) const fn key_position(kc: u8, kf: u8) -> u32 {
    let octave = ((kc >> 4) & 7) as u32;
    (octave * 12 + semitone_of_note_code(kc)) * STEPS_PER_SEMITONE + (kf & 0x3F) as u32
}

/// DT2's offset in key-position steps: 0, +600, +781 and +950 cents at 64
/// steps a semitone (`YM2151-018`).
pub(crate) const DT2_STEPS: [u32; 4] = [0, 384, 500, 608];

/// The maximum phase-modulation deviation for each `PMS`, in 1/1024
/// semitone: 0, ±5, ±10, ±20, ±50, ±100, ±400 and ±700 cents
/// (`YM2151-019`).
pub(crate) const PMS_DEPTH_1024THS: [u32; 8] = [0, 51, 102, 205, 512, 1024, 4096, 7168];

/// The base increment for a key position (with every detune and modulation
/// already folded into the position), before DT1 and MUL. Positions past
/// the last key clamp to it (`YM2151-OPEN-05`).
pub(crate) fn base_increment(position: i32) -> u32 {
    let position = if position < 0 {
        0
    } else if position as u32 > KEY_POSITION_MAX {
        KEY_POSITION_MAX
    } else {
        position as u32
    };
    let octave = position / STEPS_PER_OCTAVE;
    let step = position % STEPS_PER_OCTAVE;
    TOP_OCTAVE[step as usize] >> (OCTAVES - 1 - octave)
}

/// DT1's offset in increment units for `DT1 = 1, 2, 3`, indexed by `KC'` —
/// the key code's top five bits, octave over the two note MSBs — as the
/// manual's Fig. 2.6 prints it in hertz (`YM2151-017`). `DT1 = 0` is zero,
/// and `DT1 = 4..7` is the crate's reading as `−(DT1 − 4)`
/// (`YM2151-OPEN-06`).
pub(crate) const DT1_UNITS: [[u8; 3]; 32] = [
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
    [8, 17, 22],
    [9, 19, 22],
    [10, 20, 22],
];

/// DT1's signed offset in increment units for a key code and a 3-bit DT1
/// field.
pub(crate) const fn dt1_offset(kc: u8, dt1: u8) -> i32 {
    let kc_prime = ((kc >> 2) & 0x1F) as usize;
    let magnitude = match dt1 & 3 {
        0 => 0,
        n => DT1_UNITS[kc_prime][n as usize - 1] as i32,
    };
    if dt1 & 4 != 0 { -magnitude } else { magnitude }
}

/// The multiplier's numerator over two: `MUL = 0` halves, `MUL = n` for
/// 1..15 multiplies by `n` (`YM2151-016`).
pub(crate) const fn mul_times_two(mul: u8) -> u32 {
    match mul & 0xF {
        0 => 1,
        n => 2 * n as u32,
    }
}

/// A slot's phase increment per sample from its key position (with DT2 and
/// phase modulation folded in), key code (for DT1's scaling), DT1 and MUL.
#[cfg(test)]
pub fn increment(position: i32, kc: u8, dt1: u8, mul: u8) -> u32 {
    increment_at(position, kc, dt1, mul, DetunePosition::Inside)
}

/// Where DT1 joins the phase increment against MUL (`YM2151-OPEN-19`). The
/// register descriptions leave the order open, so this is a choice of
/// reading and not a pin.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DetunePosition {
    /// `(base + DT1) × MUL`, truncating the half at `MUL = 0`: the crate's
    /// own reading, under which the detune scales with the multiplier.
    #[default]
    Inside,
    /// `base × MUL + DT1`: chapter 1's `(N + D)·ωt` read literally
    /// (`YM2151-044`), under which the detune is Fig. 2.6's at every MUL.
    Outside,
}

/// A slot's phase increment under a reading of where DT1 sits
/// (`YM2151-OPEN-19`).
pub(crate) fn increment_at(position: i32, kc: u8, dt1: u8, mul: u8, at: DetunePosition) -> u32 {
    let offset = dt1_offset(kc, dt1);
    let sum = match at {
        DetunePosition::Inside => {
            let base = base_increment(position) as i32 + offset;
            let base = if base < 0 { 0 } else { base as u32 };
            return (base * mul_times_two(mul)) >> 1;
        }
        DetunePosition::Outside => {
            ((base_increment(position) * mul_times_two(mul)) >> 1) as i32 + offset
        }
    };
    if sum < 0 { 0 } else { sum as u32 }
}

/// The low-frequency oscillator (`YM2151-032` to `YM2151-035`).
///
/// Its phase is a 32-bit accumulator advanced once a sample by
/// `(16 + m) << (e + 2)` for `LFRQ = e:m`, so its frequency is
/// `φM / 64 × (16 + m) × 2^(e − 30)`, which at `e = 15` is
/// `φM / 2^17 × (16 + m) / 16`: the manual's 27.31 Hz at `$F0` and 52.91 Hz
/// at `$FF`. Read through [`crate::Ym2151::lfo`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Lfo {
    accumulator: u32,
    /// The noise waveform's sample, captured when the phase steps.
    noise_sample: u8,
    last_phase: u8,
}

/// The LFO waveforms of `W` (`YM2151-033`); the field's two bits name
/// these four.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Waveform {
    Sawtooth,
    Square,
    Triangle,
    Noise,
}

impl Waveform {
    /// The waveform a 2-bit `W` field selects.
    pub(crate) const fn from_field(w: u8) -> Self {
        match w & 3 {
            0 => Waveform::Sawtooth,
            1 => Waveform::Square,
            2 => Waveform::Triangle,
            _ => Waveform::Noise,
        }
    }
}

impl Lfo {
    /// At the LFO reset: phase zero.
    pub(crate) const fn new() -> Self {
        Lfo {
            accumulator: 0,
            noise_sample: 0,
            last_phase: 0,
        }
    }

    /// The increment per sample for an `LFRQ` byte.
    pub(crate) const fn increment(lfrq: u8) -> u32 {
        let e = (lfrq >> 4) as u32;
        let m = (lfrq & 0xF) as u32;
        (16 + m) << (e + 2)
    }

    /// Advance one sample; `noise_bits` is the noise generator's current
    /// low byte, sampled for the noise waveform whenever the 8-bit phase
    /// steps.
    pub(crate) fn advance(&mut self, lfrq: u8, noise_bits: u8) {
        self.accumulator = self.accumulator.wrapping_add(Self::increment(lfrq));
        let phase = self.phase();
        if phase != self.last_phase {
            self.noise_sample = noise_bits;
            self.last_phase = phase;
        }
    }

    /// Hold the phase at zero (the `TEST` register's LFO reset,
    /// `YM2151-035`).
    pub(crate) fn reset(&mut self) {
        self.accumulator = 0;
        self.last_phase = 0;
    }

    /// The 8-bit phase within the cycle.
    #[must_use]
    pub const fn phase(&self) -> u8 {
        (self.accumulator >> 24) as u8
    }

    /// The phase at its full 32 bits, of which [`Lfo::phase`] is the top
    /// eight.
    #[must_use]
    pub const fn fine_phase(&self) -> u32 {
        self.accumulator
    }

    /// The phase-modulation waveform's value, −128..127: the sawtooth rises from
    /// zero, wraps to −128 halfway and rises back to zero; the square is
    /// high then low; the triangle rises from zero (`YM2151-033`).
    pub(crate) const fn pm_waveform(&self, w: Waveform) -> i8 {
        let p = self.phase();
        match w {
            Waveform::Sawtooth => p as i8,
            Waveform::Square => {
                if p < 128 {
                    127
                } else {
                    -128
                }
            }
            Waveform::Triangle => {
                let q = p as i32;
                let v = if q < 64 {
                    q * 2
                } else if q < 192 {
                    255 - q * 2
                } else {
                    q * 2 - 512
                };
                v as i8
            }
            Waveform::Noise => self.noise_sample as i8,
        }
    }

    /// The amplitude-modulation waveform's value, 0..255: the sawtooth falls
    /// through the cycle, the square is high then low, the triangle falls
    /// from the maximum (`YM2151-033`).
    pub(crate) const fn am_waveform(&self, w: Waveform) -> u8 {
        let p = self.phase();
        match w {
            Waveform::Sawtooth => 255 - p,
            Waveform::Square => {
                if p < 128 {
                    255
                } else {
                    0
                }
            }
            Waveform::Triangle => {
                let q = p as u32;
                let v = if q < 128 { 255 - q * 2 } else { q * 2 - 256 };
                v as u8
            }
            Waveform::Noise => self.noise_sample,
        }
    }
}

/// The phase-modulation output `LFP`, −128..127, scaled by `PMD`
/// (`YM2151-034`).
pub(crate) const fn lfp(pm_waveform: i8, pmd: u8) -> i32 {
    (pm_waveform as i32 * (pmd & 0x7F) as i32) >> 7
}

/// The amplitude-modulation output `LFA`, 0..254, scaled by `AMD`
/// (`YM2151-034`).
pub(crate) const fn lfa(am_waveform: u8, amd: u8) -> u32 {
    (am_waveform as u32 * (amd & 0x7F) as u32) >> 7
}

/// The key-position offset the LFO contributes at sensitivity `PMS`
/// (`YM2151-019`).
pub(crate) const fn pm_offset_steps(lfp: i32, pms: u8) -> i32 {
    let magnitude = (lfp.unsigned_abs() * PMS_DEPTH_1024THS[(pms & 7) as usize]) >> 11;
    if lfp < 0 {
        -(magnitude as i32)
    } else {
        magnitude as i32
    }
}

/// The attenuation the LFO contributes in envelope units at sensitivity
/// `AMS`: none, `LFA`, `2 × LFA` or `4 × LFA` (`YM2151-028`).
pub(crate) const fn am_attenuation(lfa: u32, ams: u8) -> u32 {
    match ams & 3 {
        0 => 0,
        n => lfa << (n - 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::println;
    use std::vec::Vec;

    const LFO_MICROHERTZ: [(u8, u64); 8] = [
        (0xFF, 52_912_700),
        (0xFE, 51_205_800),
        (0xF0, 27_309_800),
        (0xBF, 3_307_000),
        (0xB0, 1_706_900),
        (0x7F, 206_700),
        (0x70, 106_700),
        (0x00, 800),
    ];

    fn within_percent(measured: u64, expected: u64, percent: u64) -> bool {
        measured.abs_diff(expected) * 100 <= expected * percent
    }

    #[test]
    fn phase_modulation_sensitivity() {
        let cents = [0u32, 5, 10, 20, 50, 100, 400, 700];
        for pms in 0..8u8 {
            let steps = pm_offset_steps(128, pms);
            let got = steps as u32 * 100 / 64;
            assert!(
                got.abs_diff(cents[usize::from(pms)]) <= 2 + cents[usize::from(pms)] / 50,
                "PMS {pms}: {got}"
            );
            assert_eq!(pm_offset_steps(-128, pms), -steps);
        }
    }

    #[test]
    fn am_sensitivity() {
        assert_eq!(am_attenuation(255, 0), 0);
        assert_eq!(am_attenuation(255, 1), 255);
        assert_eq!(am_attenuation(255, 2), 510);
        assert_eq!(am_attenuation(255, 3), 1020);
    }

    #[test]
    fn lfo() {
        for (lfrq, printed_uhz) in LFO_MICROHERTZ {
            let inc = u128::from(Lfo::increment(lfrq));
            let uhz = (u128::from(crate::CLOCK_TYP_HZ.0) * inc * 1_000_000 / 64) >> 32;
            let uhz = uhz as u64;
            assert!(
                within_percent(uhz, printed_uhz, 1) || uhz.abs_diff(printed_uhz) <= 50,
                "LFRQ {lfrq:#04x}: {uhz} vs {printed_uhz}"
            );
        }
        let mut lfo = Lfo::new();
        let mut pm = Vec::new();
        let mut am = Vec::new();
        for w in [Waveform::Sawtooth, Waveform::Square, Waveform::Triangle] {
            lfo.reset();
            pm.clear();
            am.clear();
            for i in 0..1057 {
                pm.push(lfo.pm_waveform(w));
                am.push(lfo.am_waveform(w));
                lfo.advance(0xFF, i as u8);
            }
            match w {
                Waveform::Sawtooth => {
                    assert_eq!(pm[0], 0);
                    assert_eq!(pm.windows(2).filter(|x| x[1] < x[0]).count(), 1);
                    assert_eq!(*pm.iter().max().unwrap(), 127);
                    assert_eq!(am[0], 255);
                    assert!(am.windows(2).all(|x| x[1] <= x[0]));
                }
                Waveform::Square => {
                    let levels: BTreeSet<i8> = pm.iter().copied().collect();
                    assert_eq!(levels.len(), 2);
                    assert_eq!(am[0], 255);
                    assert_eq!(am[600], 0);
                }
                Waveform::Triangle => {
                    assert_eq!(pm[0], 0);
                    let peak = pm.iter().position(|v| *v == 127).unwrap();
                    assert!(peak > 200 && peak < 300);
                    assert_eq!(am[0], 255);
                    assert!(am[500] < 20);
                }
                Waveform::Noise => unreachable!(),
            }
        }
        lfo.reset();
        let mut noise_values = BTreeSet::new();
        for i in 0..1057u32 {
            noise_values.insert(lfo.pm_waveform(Waveform::Noise));
            lfo.advance(0xFF, (i * 37) as u8);
        }
        assert!(noise_values.len() > 50);
        assert_eq!(lfp(127, 64), 63);
        assert_eq!(lfa(255, 127), 253);
        assert_eq!(lfp(127, 0), 0);
    }

    #[test]
    fn multiplier_position() {
        const QUARTER_TONE_PPM: u64 = 29_302;
        let mut worst_ppm = 0;
        let mut worst = (0u8, 0u8, 0u8, 0u8);
        for kc in 0..128u8 {
            for kf in 0..64u8 {
                for dt2 in 0..4u8 {
                    let position = key_position(kc, kf) as i32 + DT2_STEPS[usize::from(dt2)] as i32;
                    let base = base_increment(position);
                    for dt1 in 0..8u8 {
                        let offset = dt1_offset(kc, dt1);
                        let inside = increment(position, kc, dt1, 0);
                        let outside = increment_at(position, kc, dt1, 0, DetunePosition::Outside);
                        assert_eq!(outside, ((base >> 1) as i32 + offset).max(0) as u32);
                        let rounded = (((base as i32 + offset).max(0) as u32) + 1) >> 1;
                        for other in [outside, rounded] {
                            let ppm = u64::from(inside.abs_diff(other)) * 1_000_000
                                / u64::from(inside.max(1));
                            if ppm > worst_ppm {
                                worst_ppm = ppm;
                                worst = (kc, kf, dt1, dt2);
                            }
                        }
                    }
                }
            }
        }
        let (kc, kf, dt1, dt2) = worst;
        println!("worst: {worst_ppm} ppm at KC {kc:#04x} KF {kf} DT1 {dt1} DT2 {dt2}");
        assert!(
            worst_ppm * 4 < QUARTER_TONE_PPM,
            "the readings of MUL = 0 differ by {worst_ppm} ppm at KC {kc:#04x} KF {kf} \
             DT1 {dt1} DT2 {dt2}, against a quarter tone's {QUARTER_TONE_PPM}"
        );
    }

    #[test]
    fn detune_position() {
        use DetunePosition::{Inside, Outside};
        let kc = 0x4A;
        let position = key_position(kc, 0) as i32;
        let base = base_increment(position);
        for dt1 in 0..8u8 {
            let offset = dt1_offset(kc, dt1);
            for mul in 0..16u8 {
                let times_two = mul_times_two(mul);
                assert_eq!(
                    increment_at(position, kc, dt1, mul, Inside),
                    increment(position, kc, dt1, mul),
                    "Inside is the default increment"
                );
                assert_eq!(
                    increment_at(position, kc, dt1, mul, Outside),
                    (((base * times_two) >> 1) as i32 + offset) as u32,
                    "Outside adds Fig. 2.6's detune after the multiply"
                );
            }
        }
    }
}
