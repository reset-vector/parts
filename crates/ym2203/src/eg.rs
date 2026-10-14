//! The envelope generator: four stages, key-scaled rates, and the step
//! machinery that reproduces the attack, decay and release times of
//! `YM2203-022`.
//!
//! **The unit is 0.09375 dB**, 96 dB over 1024 steps, and the machinery
//! produces the times `YM2203-022` states; its step pattern is the crate's
//! (`YM2203-OPEN-04`). The generator is clocked once every
//! [`SAMPLES_PER_STEP`] samples; at a `RATE` of 48 it adds one unit a clock
//! in decay, doubling the unit every four `RATE` steps above and halving the
//! clock every four below; the two low bits of `RATE` spread 8, 10, 12 or 14
//! units over eight clocks. In attack
//! the unit is applied as a fraction of the distance left, which makes the
//! attack exponential and the decays straight lines in decibels. `RATE` 0 to
//! 3 never move and 63 attacks at once.

use crate::op::ENVELOPE_MAX;

/// Samples between two clocks of the envelope generator.
pub(crate) const SAMPLES_PER_STEP: usize = 3;

/// The 6-bit `RATE` below which the envelope never moves (`YM2203-OPEN-04`).
pub(crate) const RATE_STILL: u8 = 4;
/// The `RATE` at which an attack completes on its first clock
/// (`YM2203-OPEN-04`).
pub(crate) const RATE_INSTANT_ATTACK: u8 = 63;
/// The `RATE` at which decay steps one unit a clock; above it the unit
/// doubles every four (`YM2203-022`).
pub(crate) const RATE_UNIT: u8 = 48;
/// The largest step per clock, reached at `RATE` 60 (`YM2203-OPEN-04`).
pub(crate) const STEP_MAX: u16 = 8;

/// The eight-clock spread of the two low `RATE` bits: 8, 10, 12 and 14
/// units in eight clocks (`YM2203-OPEN-04`).
pub(crate) const SPREAD: [[u8; 8]; 4] = [
    [1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 2, 1, 1, 1, 2],
    [1, 2, 1, 2, 1, 2, 1, 2],
    [1, 2, 2, 2, 1, 2, 2, 2],
];

/// The key-scaling offset for a 5-bit key code and `RS`: `KC/8`, `KC/4`,
/// `KC/2` or `KC`, rounded down (`YM2203-021`).
pub(crate) const fn key_scale_offset(kc: u8, rs: u8) -> u8 {
    (kc & 0x1F) >> (3 - (rs & 3))
}

/// `RATE = 2 × R + Rks`, saturating at 63 (`YM2203-021`); a field of 0 is
/// not key-scaled and stays still (`YM2203-021`).
pub(crate) const fn rate(r: u8, rks: u8) -> u8 {
    if r == 0 {
        return 0;
    }
    let v = 2 * (r as u16) + rks as u16;
    if v > 63 { 63 } else { v as u8 }
}

/// The release rate's `R`: twice the 4-bit `RR` plus one, so `RR = 0`
/// still releases (`YM2203-021`, `YM2203-OPEN-11`).
pub(crate) const fn release_r(rr: u8) -> u8 {
    2 * (rr & 0xF) + 1
}

/// `SL`'s attenuation in units: 3 dB a step, and 15 is 93 dB
/// (`YM2203-OPEN-11`).
pub(crate) const fn sustain_level(sl: u8) -> u16 {
    let sl = (sl & 0xF) as u16;
    if sl == 15 { 992 } else { sl * 32 }
}

/// `TL`'s attenuation in units: 0.75 dB a step (`YM2203-020`).
pub(crate) const fn total_level(tl: u8) -> u16 {
    (tl & 0x7F) as u16 * 8
}

/// The envelope's stage (`YM2203-019`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnvelopeStage {
    /// Rising from the key-on at the attack rate.
    Attack,
    /// Falling at the decay rate to `SL`.
    Decay,
    /// Falling at the sustain rate from `SL` while keyed.
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

impl Default for Envelope {
    fn default() -> Self {
        Self::new()
    }
}

/// The rates a slot's envelope runs at, each a 6-bit `RATE` already
/// key-scaled, and the level decay ends at (`YM2203-019`, `YM2203-021`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Rates {
    /// The attack's `RATE`, from `AR`.
    pub attack: u8,
    /// The decay's `RATE`, from `DR`.
    pub decay: u8,
    /// The sustain's `RATE`, from `SR`.
    pub sustain: u8,
    /// The release's `RATE`, from `RR`.
    pub release: u8,
    /// `SL`, the level decay ends and sustain begins at, in steps of
    /// 0.09375 dB.
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

    /// Key on: attack from the present attenuation (`YM2203-019`,
    /// `YM2203-OPEN-19`). At `RATE` 63 the attack is complete at once.
    pub(crate) fn key_on(&mut self, attack_rate: u8) {
        self.stage = EnvelopeStage::Attack;
        if attack_rate >= RATE_INSTANT_ATTACK {
            self.attenuation = 0;
            self.stage = EnvelopeStage::Decay;
        }
    }

    /// Key off: release from the present attenuation (`YM2203-019`).
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

    /// One clock of the generator.
    pub(crate) fn tick(&mut self, rates: &Rates, clock: u32) {
        match self.stage {
            EnvelopeStage::Attack => {
                let s = Self::step(rates.attack, clock);
                if s != 0 {
                    let a = self.attenuation as u32;
                    let dec = ((a + 1) * s as u32).div_ceil(16);
                    self.attenuation = (a - dec.min(a)) as u16;
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
    fn total_level_unit() {
        assert_eq!(total_level(1), 8);
    }
}
