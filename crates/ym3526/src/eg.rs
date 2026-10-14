//! The envelope generator: four stages, key-scaled rates, the level offsets
//! (`TL`, `KSL`, AM) and the step machinery that reproduces the attack,
//! decay and release times `YM3526-020` states.
//!
//! **The unit is 0.09375 dB**, 96 dB over 1024 steps, and the generator is
//! clocked once a sample, so a `RATE` gives the slope `YM3526-020` states of
//! this part's rates in its own samples. The step pattern is the crate's
//! (`YM3526-OPEN-04`). At a `RATE` of 48 decay adds one unit a clock,
//! doubling the unit every four `RATE` steps above and halving the clock
//! every four below; the two low bits of `RATE` spread 8, 10, 12 or 14 units
//! over eight clocks. In attack the unit is applied as a fraction of the
//! distance left, in steps of two units (0.1875 dB), which makes the attack
//! exponential and the decays straight lines in decibels. `RATE` 0 to 3 never
//! move and 63 attacks at once.

use crate::op::ENVELOPE_MAX;

/// The 6-bit `RATE` below which the envelope never moves (`YM3526-OPEN-04`).
pub(crate) const RATE_STILL: u8 = 4;
/// The `RATE` at which an attack completes on its first clock
/// (`YM3526-OPEN-04`).
pub(crate) const RATE_INSTANT_ATTACK: u8 = 63;
/// The `RATE` at which decay steps one unit a clock; above it the unit
/// doubles every four (`YM3526-020`).
pub(crate) const RATE_UNIT: u8 = 48;
/// The largest step per clock, reached at `RATE` 60 (`YM3526-OPEN-04`).
pub(crate) const STEP_MAX: u16 = 8;

/// The eight-clock spread of the two low `RATE` bits: 8, 10, 12 and 14
/// units in eight clocks (`YM3526-OPEN-04`).
pub(crate) const SPREAD: [[u8; 8]; 4] = [
    [1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 2, 1, 1, 1, 2],
    [1, 2, 1, 2, 1, 2, 1, 2],
    [1, 2, 2, 2, 1, 2, 2, 2],
];

/// The key-scaling offset for a 4-bit key code: the key code itself with
/// `KSR` set, its top two bits with it clear (`YM3526-019`).
pub(crate) const fn key_scale_offset(kc: u8, ksr: bool) -> u8 {
    if ksr { kc & 0xF } else { (kc & 0xF) >> 2 }
}

/// `RATE = 4 × R + Rks`, saturating at 63; a field of 0 is not key-scaled
/// and stays still (`YM3526-019`).
pub(crate) const fn rate(r: u8, rks: u8) -> u8 {
    if r & 0xF == 0 {
        return 0;
    }
    let v = 4 * (r & 0xF) as u16 + rks as u16;
    if v > 63 { 63 } else { v as u8 }
}

/// `SL`'s attenuation in units: 3 dB a step, and 15 is 93 dB
/// (`YM3526-018`, `YM3526-OPEN-10`).
pub(crate) const fn sustain_level(sl: u8) -> u16 {
    let sl = (sl & 0xF) as u16;
    if sl == 15 { 992 } else { sl * 32 }
}

/// `TL`'s attenuation in units: 0.75 dB a step (`YM3526-017`).
pub(crate) const fn total_level(tl: u8) -> u16 {
    (tl & 0x3F) as u16 * 8
}

/// `KSL`'s table: the attenuation at block 7 by the F-number's top four
/// bits, in steps of 0.75 dB at 6 dB an octave (`YM3526-021`).
pub(crate) const KSL_ROW: [u8; 16] = [
    0, 24, 32, 37, 40, 43, 45, 47, 48, 50, 51, 52, 53, 54, 55, 56,
];

/// `KSL`'s attenuation in units for a block, F-number and the 2-bit field:
/// the table less 8 steps a block below 7, floored at 0, at 6 dB an octave
/// for 3, 3 for 1 and 1.5 for 2 (`YM3526-021`).
pub(crate) const fn key_scale_level(block: u8, fnum: u16, ksl: u8) -> u16 {
    let row = KSL_ROW[((fnum >> 6) & 0xF) as usize] as i32 - 8 * (7 - (block & 7) as i32);
    let steps = if row < 0 { 0 } else { row as u16 };
    match ksl & 3 {
        0 => 0,
        1 => steps * 4,
        2 => steps * 2,
        _ => steps * 8,
    }
}

/// The envelope's stage (`YM3526-016`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnvelopeStage {
    /// Rising from the key-on at the attack rate.
    Attack,
    /// The sheet's decay, falling to `SL`.
    Decay,
    /// The sheet's sustain, from `SL` while keyed.
    Sustain,
    /// Falling at the release rate after key-off.
    Release,
}

/// One slot's envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Envelope {
    stage: EnvelopeStage,
    attenuation: u16,
}

/// The rates a slot's envelope runs at, already key-scaled: attack, decay,
/// the rate it runs at once it reaches `SL` while keyed, and release.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Rates {
    /// The attack's `RATE`.
    pub attack: u8,
    /// The decay's `RATE`, to `SL`.
    pub decay: u8,
    /// The `RATE` from `SL` while keyed: 0 for a sustained tone, the
    /// release rate for a percussive one (`YM3526-018`).
    pub sustain: u8,
    /// The release's `RATE`.
    pub release: u8,
    /// `SL` in steps of 0.09375 dB.
    pub sustain_level_steps: u32,
}

impl Envelope {
    /// Silent, in release.
    pub(crate) const fn new() -> Self {
        Envelope {
            stage: EnvelopeStage::Release,
            attenuation: ENVELOPE_MAX,
        }
    }

    /// The stage the envelope is in.
    #[must_use]
    pub const fn stage(&self) -> EnvelopeStage {
        self.stage
    }

    /// The attenuation in steps of 0.09375 dB, 0 (full) to 1023 (silent).
    #[must_use]
    pub const fn attenuation_steps(&self) -> u32 {
        self.attenuation as u32
    }

    /// Key on: attack from the present attenuation (`YM3526-016`,
    /// `YM3526-OPEN-11`). At `RATE` 63 the attack is complete at once.
    pub(crate) fn key_on(&mut self, attack_rate: u8) {
        self.stage = EnvelopeStage::Attack;
        if attack_rate >= RATE_INSTANT_ATTACK {
            self.attenuation = 0;
            self.stage = EnvelopeStage::Decay;
        }
    }

    /// Key off: release from the present attenuation (`YM3526-016`).
    pub(crate) fn key_off(&mut self) {
        self.stage = EnvelopeStage::Release;
    }

    /// The step this clock contributes at `rate`, given the generator's
    /// clock count: zero on clocks a rate below 48 skips.
    pub(crate) const fn step(rate: u8, clock: u32) -> u16 {
        if rate < RATE_STILL {
            return 0;
        }
        let group = (rate >> 2) as u32;
        let fine = (rate & 3) as usize;
        if rate < RATE_UNIT {
            let shift = 12 - group;
            if clock & ((1 << shift) - 1) != 0 {
                return 0;
            }
            SPREAD[fine][((clock >> shift) & 7) as usize] as u16
        } else {
            let scale = group - 12;
            let s = (SPREAD[fine][(clock & 7) as usize] as u16) << scale;
            if s > STEP_MAX { STEP_MAX } else { s }
        }
    }

    /// One clock of the generator, once a sample.
    pub(crate) fn tick(&mut self, rates: &Rates, clock: u32) {
        match self.stage {
            EnvelopeStage::Attack => {
                let s = Self::step(rates.attack, clock);
                if s != 0 {
                    let a = self.attenuation as u32 / 2;
                    let dec = ((a + 1) * s as u32).div_ceil(16);
                    self.attenuation = (2 * (a - dec.min(a))) as u16;
                }
                if self.attenuation == 0 {
                    self.stage = EnvelopeStage::Decay;
                }
            }
            EnvelopeStage::Decay => {
                let s = Self::step(rates.decay, clock);
                self.attenuation = (self.attenuation + s).min(ENVELOPE_MAX);
                if u32::from(self.attenuation) >= rates.sustain_level_steps {
                    self.stage = EnvelopeStage::Sustain;
                }
            }
            EnvelopeStage::Sustain => {
                let s = Self::step(rates.sustain, clock);
                self.attenuation = (self.attenuation + s).min(ENVELOPE_MAX);
            }
            EnvelopeStage::Release => {
                let s = Self::step(rates.release, clock);
                self.attenuation = (self.attenuation + s).min(ENVELOPE_MAX);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_is_four_r_plus_rks() {
        assert_eq!(rate(0, 15), 0);
        assert_eq!(rate(5, 3), 23);
        assert_eq!(rate(15, 15), 63);
    }
}
