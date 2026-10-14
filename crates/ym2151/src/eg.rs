//! The envelope generator: four stages, key-scaled rates, and the step
//! machinery that reproduces the manual's attack and decay times.
//!
//! **The unit is 0.09375 dB** — 96 dB over 1024 steps (`YM2151-024`,
//! `YM2151-030`) — and the machinery is the crate's reading of the manual's
//! Fig. 2.11 (`YM2151-029`, `YM2151-OPEN-07`): the generator is clocked once
//! every [`SAMPLES_PER_STEP`] samples; at a `RATE` of 48 it adds one unit a
//! clock in decay, doubling the unit every four `RATE` steps above and
//! halving the clock every four below; the two low bits of `RATE` spread
//! 8, 10, 12 or 14 units over eight clocks. In attack the unit is applied
//! as a fraction of the distance left, which is what makes the attack
//! exponential and the decay a straight line in the manual's words. `RATE`
//! 0 to 3 never move (the manual's INFINITY) and 63 attacks at once.

use crate::op::ENVELOPE_MAX;

/// Samples between two clocks of the envelope generator.
pub(crate) const SAMPLES_PER_STEP: usize = 3;

/// The 6-bit `RATE` below which the envelope never moves (`YM2151-029`).
pub(crate) const RATE_STILL: u8 = 4;
/// The `RATE` at which an attack completes on its first clock
/// (`YM2151-029`).
pub(crate) const RATE_INSTANT_ATTACK: u8 = 63;
/// The `RATE` at which decay steps one unit a clock; above it the unit
/// doubles every four (`YM2151-029`).
pub(crate) const RATE_UNIT: u8 = 48;
/// The largest step per clock, reached at `RATE` 60 (`YM2151-029`).
pub(crate) const STEP_MAX: u16 = 8;

/// The eight-clock spread of the two low `RATE` bits: 8, 10, 12 and 14
/// units in eight clocks (`YM2151-OPEN-07`).
pub(crate) const SPREAD: [[u8; 8]; 4] = [
    [1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 2, 1, 1, 1, 2],
    [1, 2, 1, 2, 1, 2, 1, 2],
    [1, 2, 2, 2, 1, 2, 2, 2],
];

/// The key-scaling offset `Rks` for a key code and `KS` (`YM2151-025`):
/// the key code's top five bits shifted right by `3 − KS`.
pub(crate) const fn key_scale_offset(kc: u8, ks: u8) -> u8 {
    ((kc >> 2) & 0x1F) >> (3 - (ks & 3))
}

/// `RATE = 2 × R + Rks`, saturating at 63 (`YM2151-025`).
///
/// `R = 0` is key-scaled like any other, so `rate(0, rks)` is `rks` and a
/// held note with `D2R` 0 and `KS` above 0 still decays at a high key code.
/// A choice of reading, not a pin (`YM2151-OPEN-09`).
pub(crate) const fn rate(r: u8, rks: u8) -> u8 {
    let v = 2 * (r as u16) + rks as u16;
    if v > 63 { 63 } else { v as u8 }
}

/// The release rate's `R`: twice the 4-bit `RR` plus one (`YM2151-025`).
pub(crate) const fn release_r(rr: u8) -> u8 {
    2 * (rr & 0xF) + 1
}

/// `D1L`'s attenuation in units: 3 dB a step, and 15 is 93 dB
/// (`YM2151-026`).
pub(crate) const fn first_decay_level(d1l: u8) -> u16 {
    let d1l = (d1l & 0xF) as u16;
    if d1l == 15 { 992 } else { d1l * 32 }
}

/// `TL`'s attenuation in units: 0.75 dB a step (`YM2151-027`).
pub(crate) const fn total_level(tl: u8) -> u16 {
    (tl & 0x7F) as u16 * 8
}

/// The envelope's stage (`YM2151-023`). The manual names these four.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnvelopeStage {
    /// Rising from the key-on at the attack rate.
    Attack,
    /// Falling at the first decay rate to the first decay level.
    Decay1,
    /// Falling at the second decay rate while keyed.
    Decay2,
    /// Falling at the release rate after key-off.
    Release,
}

/// One slot's envelope, read through [`crate::Ym2151::envelope`]. Its
/// [`Default`] is the envelope after initial clear: silent, in release.
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

/// The rates a slot's envelope runs at, already key-scaled: attack, first
/// decay, second decay, release, each a 6-bit `RATE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Rates {
    /// The attack's `RATE`, from `AR`.
    pub attack: u8,
    /// The first decay's `RATE`, from `D1R`.
    pub decay1: u8,
    /// The second decay's `RATE`, from `D2R`.
    pub decay2: u8,
    /// The release's `RATE`, from `RR`.
    pub release: u8,
    /// `D1L` in steps of 0.09375 dB.
    pub decay1_level_steps: u32,
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

    /// Key on: attack from the present attenuation (`YM2151-023`,
    /// `YM2151-OPEN-08`). At `RATE` 63 the attack is complete at once.
    pub(crate) fn key_on(&mut self, attack_rate: u8) {
        self.stage = EnvelopeStage::Attack;
        if attack_rate >= RATE_INSTANT_ATTACK {
            self.attenuation = 0;
            self.stage = EnvelopeStage::Decay1;
        }
    }

    /// Key off: release from the present attenuation (`YM2151-023`).
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
                    self.stage = EnvelopeStage::Decay1;
                }
            }
            EnvelopeStage::Decay1 => {
                let s = Self::step(rates.decay1, clock);
                self.attenuation = (self.attenuation + s).min(ENVELOPE_MAX);
                if u32::from(self.attenuation) >= rates.decay1_level_steps {
                    self.stage = EnvelopeStage::Decay2;
                }
            }
            EnvelopeStage::Decay2 => {
                let s = Self::step(rates.decay2, clock);
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
    use std::vec::Vec;

    const DECAY_90_10_MICROSECONDS: [(u8, u32); 15] = [
        (63, 1_360),
        (62, 1_360),
        (61, 1_360),
        (60, 1_360),
        (59, 1_550),
        (58, 1_810),
        (57, 2_180),
        (56, 2_720),
        (52, 5_440),
        (51, 6_220),
        (50, 7_250),
        (49, 8_700),
        (48, 10_880),
        (44, 21_760),
        (32, 174_080),
    ];

    const DECAY_0_96_MICROSECONDS: [(u8, u32); 12] = [
        (63, 6_730),
        (60, 6_730),
        (56, 13_450),
        (55, 15_380),
        (52, 26_910),
        (48, 53_810),
        (44, 107_630),
        (40, 215_250),
        (28, 1_722_030),
        (16, 13_776_210),
        (7, 62_976_980),
        (4, 110_209_710),
    ];

    const ATTACK_96_0_MICROSECONDS: [(u8, u32); 9] = [
        (63, 0),
        (62, 530),
        (60, 530),
        (56, 1_120),
        (52, 2_130),
        (48, 3_890),
        (44, 7_790),
        (40, 15_570),
        (32, 62_290),
    ];

    const FIG_2_11_CLOCK_HZ: u32 = 3_600_000;

    fn within_percent(measured: u64, expected: u64, percent: u64) -> bool {
        measured.abs_diff(expected) * 100 <= expected * percent
    }

    fn decay_samples(rate: u8, from: u32, to: u32) -> u64 {
        let rates = Rates {
            attack: 0,
            decay1: rate,
            decay2: 0,
            release: 0,
            decay1_level_steps: 1023,
        };
        let mut e = Envelope::new();
        e.key_on(63);
        let mut clock = 0u32;
        let mut samples = 0u64;
        let mut started = None;
        loop {
            for _ in 0..SAMPLES_PER_STEP {
                samples += 1;
            }
            clock += 1;
            e.tick(&rates, clock);
            if started.is_none() && e.attenuation_steps() > from {
                started = Some(samples);
            }
            if e.attenuation_steps() >= to {
                return samples - started.unwrap_or(samples);
            }
            assert!(samples < 100_000_000, "rate {rate} never reaches {to}");
        }
    }

    fn attack_samples(rate: u8) -> u64 {
        let rates = Rates {
            attack: rate,
            decay1: 0,
            decay2: 0,
            release: 0,
            decay1_level_steps: 1023,
        };
        let mut e = Envelope::new();
        e.key_on(rate);
        if e.attenuation_steps() == 0 {
            return 0;
        }
        let mut clock = 0u32;
        let mut samples = 0u64;
        loop {
            samples += SAMPLES_PER_STEP as u64;
            clock += 1;
            e.tick(&rates, clock);
            if e.attenuation_steps() == 0 {
                return samples;
            }
            assert!(samples < 100_000_000, "rate {rate} never attacks");
        }
    }

    #[test]
    fn first_decay_levels() {
        assert_eq!(first_decay_level(0), 0);
        assert_eq!(first_decay_level(1), 32);
        assert_eq!(first_decay_level(8), 256);
        assert_eq!(first_decay_level(15), 992);
    }

    #[test]
    fn key_scaling() {
        let table_rows: [(u8, [u8; 4]); 8] = [
            (0, [0, 0, 0, 0]),
            (1, [0, 0, 0, 1]),
            (7, [0, 1, 3, 7]),
            (8, [1, 2, 4, 8]),
            (15, [1, 3, 7, 15]),
            (16, [2, 4, 8, 16]),
            (23, [2, 5, 11, 23]),
            (31, [3, 7, 15, 31]),
        ];
        for (kc_prime, rks) in table_rows {
            for ks in 0..4u8 {
                assert_eq!(
                    key_scale_offset(kc_prime << 2, ks),
                    rks[usize::from(ks)],
                    "KC' {kc_prime} KS {ks}"
                );
            }
        }
        assert_eq!(rate(31, 0), 62);
        assert_eq!(rate(31, 3), 63, "saturates");
        assert_eq!(rate(10, 5), 25);
        assert_eq!(release_r(0), 1);
        assert_eq!(release_r(15), 31);
        assert_eq!(rate(11, 31), 53);
    }

    #[test]
    fn total_levels() {
        assert_eq!(total_level(1), 8);
        assert_eq!(total_level(0x7F), 1016);
    }

    #[test]
    fn attack_and_decay_times() {
        let per_second = u64::from(FIG_2_11_CLOCK_HZ / crate::CYCLES_PER_SAMPLE as u32);
        for (rate, us) in DECAY_90_10_MICROSECONDS {
            let samples = decay_samples(rate, 9, 213);
            let expect = u64::from(us) * per_second / 1_000_000;
            assert!(
                within_percent(samples, expect, 4),
                "RATE {rate}: {samples} samples, printed {expect}"
            );
        }
        for (rate, us) in DECAY_0_96_MICROSECONDS {
            let samples = decay_samples(rate, 0, 1023);
            let expect = u64::from(us) * per_second / 1_000_000;
            assert!(
                within_percent(samples, expect, 4),
                "RATE {rate}: {samples} samples, printed {expect}"
            );
        }
        for (rate, us) in ATTACK_96_0_MICROSECONDS {
            let samples = attack_samples(rate);
            let expect = u64::from(us) * per_second / 1_000_000;
            if expect == 0 {
                assert_eq!(samples, 0, "RATE 63 attacks at once");
            } else {
                assert!(
                    within_percent(samples, expect, 12),
                    "RATE {rate}: {samples} samples, printed {expect}"
                );
            }
        }
        for rate in 0..4u8 {
            for clock in 0..5000u32 {
                assert_eq!(Envelope::step(rate, clock), 0);
            }
        }
        let rates = Rates {
            attack: 48,
            decay1: 48,
            decay2: 0,
            release: 0,
            decay1_level_steps: 1023,
        };
        let mut e = Envelope::new();
        e.key_on(48);
        let mut steps = Vec::new();
        let mut clock = 0;
        while e.attenuation_steps() > 0 {
            let before = e.attenuation_steps();
            clock += 1;
            e.tick(&rates, clock);
            steps.push(before - e.attenuation_steps());
        }
        assert!(steps[0] > *steps.last().unwrap());
    }
}
