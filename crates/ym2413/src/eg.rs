//! Envelope generation: four stages over a logarithmic attenuation scale, and
//! the two key scales that bend the rates and the levels.
//!
//! **Attenuation is carried in sixteenths of a decibel throughout.** Every
//! figure the manual gives — 0.375, 0.75, 1.125, 1.5, 3, 6 dB — is an exact
//! multiple of 1/16 dB, so the unit costs nothing in rounding and lets the
//! `KSL` halving and doubling stay exact integers.

/// Sixteenths of a decibel in one decibel.
pub(crate) const PER_DB: u16 = 16;

/// The envelope's four stages (`YM2413-044`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum EnvelopeStage {
    /// Silent, and waiting for a key-on. Not one of the manual's four; it is
    /// where a slot sits before it has ever been keyed.
    #[default]
    Idle,
    /// Rising, exponentially, until 0 dB of attenuation (`YM2413-046`).
    Attack,
    /// Falling linearly until the sustain level (`YM2413-048`).
    Decay,
    /// Held, while the key is on (`YM2413-048`).
    Sustain,
    /// Falling linearly after key-off (`YM2413-049`).
    Release,
}

/// The dynamic range of the envelope generator, in sixteenths of a decibel
/// (`YM2413-045`).
pub(crate) const RANGE: u16 = 48 * PER_DB;

/// Attenuation steps the scale is divided into.
///
/// The manual states a 48 dB range and prints a resolution of "0.325 dB", and
/// those two do not divide: 48 / 0.325 is 147.7. 0.375 dB gives exactly 128
/// steps, a seven-bit field; the step is this crate's choice under
/// `YM2413-OPEN-09`.
pub(crate) const STEPS: usize = 128;
/// One envelope step, in sixteenths of a decibel: 0.375 dB.
pub(crate) const STEP: u16 = RANGE / STEPS as u16;
/// Maximum attenuation, in steps: silence.
pub(crate) const MAX_ATTENUATION: u16 = STEPS as u16 - 1;

/// The key scale of RATE, from block and the F-Number's top bit
/// (`YM2413-051`).
///
/// The manual's Table III-2 gives, against the eight blocks and the F-Number
/// MSB, `0 0 0 0 1 1 1 1 2 2 2 2 3 3 3 3` when `KSR` is 0 and `0 1 2 3 … 15`
/// when it is 1. Both are the same four-bit quantity `block:fnum_msb`, shifted
/// right by two when `KSR` is clear.
#[must_use]
pub(crate) fn rate_key_scale(block: u8, fnum_msb: bool, ksr: bool) -> u8 {
    let full = ((block & 7) << 1) | u8::from(fnum_msb);
    if ksr { full } else { full >> 2 }
}

/// The effective RATE an ADSR field reaches after key scaling (`YM2413-050`).
///
/// The manual: "RATE = 4 × R + Rks", and "Note that when R=0, RATE=0" — the
/// zero is a special case and not the formula's own result.
#[must_use]
pub(crate) fn rate(field: u8, key_scale: u8) -> u8 {
    if field == 0 {
        0
    } else {
        ((field & 0x0F) * 4).saturating_add(key_scale)
    }
}

/// The sustain level a four-bit `SL` field names, in envelope steps
/// (`YM2413-055`).
///
/// The manual weights the field's bits 24, 12, 6 and 3 dB from `D7` down, so it
/// is a plain binary attenuator in units of 3 dB. One step is 0.375 dB, so a
/// unit of 3 dB is eight steps.
#[must_use]
pub(crate) fn sustain_level(field: u8) -> u16 {
    u16::from(field & 0x0F) * 8
}

/// The release rate a key-off uses, which the sustain bit overrides
/// (`YM2413-052`).
///
/// "When this bit is '1', the RR with the key off will be 5."
#[must_use]
pub(crate) fn release_field(sustain_bit: bool, rr: u8) -> u8 {
    if sustain_bit { 5 } else { rr & 0x0F }
}

/// Level key scale at 3 dB/oct, in sixteenths of a decibel, indexed by block and
/// by the F-Number's top four bits (`YM2413-066`).
///
/// This is the manual's Table III-5, corrected. Each row is an exact arithmetic
/// sequence in steps of 0.375 dB once it leaves zero, and three printed cells
/// break it: `2.525` where the row requires 2.625, and `14.825` twice where the
/// rows require 14.625.
const KSL_3DB_PER_OCT: [[u16; 16]; 8] = [
    [0; 16],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 12, 18, 24, 30, 36, 42, 48],
    [0, 0, 0, 0, 0, 18, 30, 42, 48, 60, 66, 72, 78, 84, 90, 96],
    [
        0, 0, 0, 30, 48, 66, 78, 90, 96, 108, 114, 120, 126, 132, 138, 144,
    ],
    [
        0, 0, 48, 78, 96, 114, 126, 138, 144, 156, 162, 168, 174, 180, 186, 192,
    ],
    [
        0, 48, 96, 126, 144, 162, 174, 186, 192, 204, 210, 216, 222, 228, 234, 240,
    ],
    [
        0, 96, 144, 174, 192, 210, 222, 234, 240, 252, 258, 264, 270, 276, 282, 288,
    ],
    [
        0, 144, 192, 222, 240, 258, 270, 282, 288, 300, 306, 312, 318, 324, 330, 336,
    ],
];

/// The `KSL` field's attenuation, in sixteenths of a decibel (`YM2413-065`).
///
/// The manual's Table III-4 maps `D7 D6` to `0 dB/oct`, `1.5`, `3` and `6` in
/// the row order `00`, `10`, `01`, `11`. Read as the two-bit number `D7:D6`
/// that is **not monotonic**: 1 gives 3 dB/oct and 2 gives 1.5. The row order
/// is the printed table's, and is easy to correct by accident.
#[must_use]
pub(crate) fn level_key_scale(ksl: u8, block: u8, fnum_top4: u8) -> u16 {
    let base = KSL_3DB_PER_OCT[(block & 7) as usize][(fnum_top4 & 0x0F) as usize];
    match ksl & 3 {
        0 => 0,
        1 => base,
        2 => base / 2,
        _ => base * 2,
    }
}

/// The total level field's attenuation, in sixteenths of a decibel
/// (`YM2413-063`).
///
/// Six bits in steps of 0.75 dB reaching 47.25 dB, which the manual gives both
/// as a sentence and as the bit weights 24, 12, 6, 3, 1.5 and 0.75 dB.
#[must_use]
pub(crate) fn total_level(field: u8) -> u16 {
    u16::from(field & 0x3F) * 12
}

/// The channel volume field's attenuation, in sixteenths of a decibel
/// (`YM2413-027`).
///
/// Four bits in steps of 3 dB reaching 45 dB.
#[must_use]
pub(crate) fn volume(field: u8) -> u16 {
    u16::from(field & 0x0F) * 3 * PER_DB
}

/// One slot's envelope generator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Envelope {
    stage: EnvelopeStage,
    /// Attenuation in steps, 0 loudest and [`MAX_ATTENUATION`] silent.
    attenuation: u16,
    /// Sub-step accumulator, so a rate slower than one step a sample needs no
    /// division per sample.
    counter: u32,
}

/// The counter units one attenuation step costs.
///
/// This is the crate's own scaling (`YM2413-OPEN-10`), beside the millisecond
/// table of `YM2413-056`: the fastest RATE crosses the whole scale in one
/// sample.
const RATE_UNIT: u32 = 63 * 63 / 128;

/// The share of the distance left that one attack step removes, as a divisor:
/// a sixteenth, and never less than one step. This is the crate's own, like
/// [`RATE_UNIT`] (`YM2413-OPEN-10`); what it buys is the shape `YM2413-046`
/// states, a step proportional to the distance left and so an exponential
/// approach to 0 dB.
const ATTACK_DIVISOR: u32 = 16;

impl Default for Envelope {
    fn default() -> Self {
        Self::IDLE
    }
}

impl Envelope {
    /// Idle and silent: a slot that has never been keyed sounds nothing.
    pub(crate) const IDLE: Self = Self {
        stage: EnvelopeStage::Idle,
        attenuation: MAX_ATTENUATION,
        counter: 0,
    };
}

impl Envelope {
    /// The stage the envelope is in.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn stage(self) -> EnvelopeStage {
        self.stage
    }

    /// Attenuation now, in steps.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn attenuation(self) -> u16 {
        self.attenuation
    }

    /// Attenuation now, in sixteenths of a decibel.
    #[must_use]
    pub(crate) fn attenuation_db16(self) -> u16 {
        self.attenuation * STEP
    }

    /// Key-on: begin the attack from silence (`YM2413-047`, `YM2413-053`).
    pub(crate) fn key_on(&mut self) {
        self.stage = EnvelopeStage::Attack;
        self.attenuation = MAX_ATTENUATION;
        self.counter = 0;
    }

    /// Key-off: fall to silence at the release rate (`YM2413-049`).
    pub(crate) fn key_off(&mut self) {
        if self.stage != EnvelopeStage::Idle {
            self.stage = EnvelopeStage::Release;
            self.counter = 0;
        }
    }

    /// Advance one sample.
    ///
    /// `rates` are the effective RATEs after key scaling, in the order attack,
    /// decay, release; `sustain` is the level decay ends at, in steps.
    pub(crate) fn advance(&mut self, rates: [u8; 3], sustain: u16) {
        let (rate, target) = match self.stage {
            EnvelopeStage::Idle | EnvelopeStage::Sustain => return,
            EnvelopeStage::Attack => (rates[0], 0),
            EnvelopeStage::Decay => (rates[1], sustain.min(MAX_ATTENUATION)),
            EnvelopeStage::Release => (rates[2], MAX_ATTENUATION),
        };
        if rate == 0 {
            return;
        }

        self.counter += u32::from(rate) * u32::from(rate);
        let steps = u16::try_from(self.counter / RATE_UNIT).unwrap_or(u16::MAX);
        self.counter %= RATE_UNIT;
        if steps == 0 {
            return;
        }

        self.attenuation = if self.stage == EnvelopeStage::Attack {
            let left = u32::from(self.attenuation);
            let fall = ((left + 1) * u32::from(steps)).div_ceil(ATTACK_DIVISOR);
            u16::try_from(left - fall.min(left)).expect("below the start")
        } else if self.attenuation < target {
            self.attenuation.saturating_add(steps).min(target)
        } else {
            self.attenuation.saturating_sub(steps).max(target)
        };

        if self.attenuation == target {
            self.stage = match self.stage {
                EnvelopeStage::Attack => EnvelopeStage::Decay,
                EnvelopeStage::Decay => EnvelopeStage::Sustain,
                EnvelopeStage::Release => EnvelopeStage::Idle,
                other => other,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    /// The falls one stage of `envelope` makes at `rates`, until it leaves it.
    fn falls(mut envelope: Envelope, rates: [u8; 3]) -> Vec<u16> {
        let mut falls = Vec::new();
        let stage = envelope.stage();
        while envelope.stage() == stage {
            let before = envelope.attenuation();
            envelope.advance(rates, MAX_ATTENUATION);
            if envelope.attenuation() != before {
                falls.push(before.abs_diff(envelope.attenuation()));
            }
        }
        falls
    }

    #[test]
    fn the_envelope_runs_its_four_stages() {
        let mut envelope = Envelope::default();
        assert_eq!(envelope.stage(), EnvelopeStage::Idle);
        assert_eq!(envelope.attenuation(), MAX_ATTENUATION);

        envelope.key_on();
        assert_eq!(envelope.stage(), EnvelopeStage::Attack);
        assert_eq!(envelope.attenuation(), MAX_ATTENUATION);

        let attack = falls(envelope, [4, 4, 4]);
        assert_eq!(attack.iter().sum::<u16>(), MAX_ATTENUATION);
        assert!(attack[0] > 1 && attack.last() == Some(&1), "{attack:?}");
        assert!(attack.windows(2).all(|w| w[0] >= w[1]), "{attack:?}");
        let mut decaying = envelope;
        decaying.advance([63, 4, 4], MAX_ATTENUATION);
        assert_eq!(decaying.stage(), EnvelopeStage::Decay);
        let decay = falls(decaying, [63, 4, 4]);
        assert!(decay.iter().all(|&fall| fall == decay[0]), "{decay:?}");

        let sustain = sustain_level(4);
        for _ in 0..4096 {
            envelope.advance([63, 20, 20], sustain);
            if envelope.stage() != EnvelopeStage::Attack {
                break;
            }
        }
        assert_eq!(envelope.stage(), EnvelopeStage::Decay);
        assert_eq!(envelope.attenuation(), 0);
        for _ in 0..4096 {
            envelope.advance([63, 20, 20], sustain);
            if envelope.stage() != EnvelopeStage::Decay {
                break;
            }
        }
        assert_eq!(envelope.stage(), EnvelopeStage::Sustain);
        assert_eq!(envelope.attenuation(), sustain);
        for _ in 0..100 {
            envelope.advance([63, 20, 20], sustain);
        }
        assert_eq!(envelope.attenuation(), sustain, "sustain holds");

        envelope.key_off();
        assert_eq!(envelope.stage(), EnvelopeStage::Release);
        for _ in 0..8192 {
            envelope.advance([63, 20, 40], sustain);
            if envelope.stage() == EnvelopeStage::Idle {
                break;
            }
        }
        assert_eq!(envelope.stage(), EnvelopeStage::Idle);
        assert_eq!(envelope.attenuation(), MAX_ATTENUATION);
    }

    #[test]
    fn rate_is_four_r_plus_the_key_scale() {
        assert_eq!(rate(0, 15), 0);
        assert_eq!(rate(1, 0), 4);
        assert_eq!(rate(15, 3), 63);
        assert_eq!(rate(15, 15), 75);

        let mut with_ksr = Vec::new();
        let mut without = Vec::new();
        for block in 0..8u8 {
            for msb in [false, true] {
                with_ksr.push(rate_key_scale(block, msb, true));
                without.push(rate_key_scale(block, msb, false));
            }
        }
        assert_eq!(with_ksr, (0..16u8).collect::<Vec<_>>());
        assert_eq!(without, [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3]);
    }

    #[test]
    fn sustain_and_release_fields_scale_as_stated() {
        assert_eq!(release_field(false, 12), 12);
        assert_eq!(release_field(true, 12), 5);
        assert_eq!(rate(release_field(true, 12), 3), 23);

        assert_eq!(STEP, 6, "0.375 dB in sixteenths");
        assert_eq!(RANGE, 48 * PER_DB);
        for field in 0..16u8 {
            assert_eq!(sustain_level(field) * STEP, u16::from(field) * 3 * PER_DB);
        }
    }

    #[test]
    fn total_level_and_volume_are_their_bit_weights() {
        assert_eq!(total_level(0), 0);
        assert_eq!(total_level(1), 12);
        assert_eq!(total_level(0x3F), 47 * PER_DB + 4);
        let weights = [384u16, 192, 96, 48, 24, 12];
        assert_eq!(weights.iter().sum::<u16>(), total_level(0x3F));
        for (bit, weight) in weights.iter().enumerate() {
            assert_eq!(total_level(1 << (5 - bit)), *weight, "bit {bit}");
        }
        assert_eq!(volume(0), 0);
        assert_eq!(volume(0xF), 45 * PER_DB);
    }

    #[test]
    fn level_key_scale_follows_the_printed_row_order() {
        let (block, top4) = (7u8, 0xF);
        let three = level_key_scale(1, block, top4);
        assert_eq!(three, 21 * PER_DB);
        assert_eq!(level_key_scale(0, block, top4), 0);
        assert_eq!(level_key_scale(2, block, top4), three / 2);
        assert_eq!(level_key_scale(3, block, top4), three * 2);

        for block in 1..8u8 {
            let mut previous = None;
            for value in (0..16u8)
                .map(|f| level_key_scale(1, block, f))
                .filter(|v| *v > 0)
            {
                if let Some(previous) = previous {
                    let step: u16 = value - previous;
                    assert!(step.is_multiple_of(6) && step > 0, "block {block}");
                }
                previous = Some(value);
            }
            assert_eq!(
                level_key_scale(1, block, 0xF),
                u16::from(block) * 3 * PER_DB
            );
        }
        assert!((0..16u8).all(|f| level_key_scale(1, 0, f) == 0));
    }
}
