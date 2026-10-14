//! The envelope select logic, and the envelope generator and modulator.
//!
//! Two logic inputs choose which of four signals gates the mixer output
//! (`SN76477-032`), and the attack and decay ramps then shape the amplitude that
//! gate calls for (`SN76477-034` to `SN76477-036`). The ramps are the only
//! analogue part of the sound: everything before them is a square wave.

/// Full amplitude, in steps. The envelope runs `0..=FULL`.
///
/// A scale and not a voltage: what full amplitude is worth at the pin is
/// `Timing::output_swing`, which Equation 8 sets from two resistors.
pub(crate) const FULL: u32 = 1 << 16;

/// A row of the envelope select table — `SN76477-032`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Envelope {
    /// `1 L, 2 L` — the mixer output is gated by the VCO output.
    Vco,
    /// `1 L, 2 H` — no envelope; the mixer output goes on continuously.
    MixerOnly,
    /// `1 H, 2 L` — the mixer output is enabled for the one-shot pulse.
    OneShot,
    /// `1 H, 2 H` — as `Vco`, but only every other VCO pulse enables it.
    VcoAlternating,
}

impl Envelope {
    /// The row two logic levels select. `one` is pin 1 and `two` is pin 28.
    #[must_use]
    pub(crate) const fn select(one: bool, two: bool) -> Self {
        match (one, two) {
            (false, false) => Self::Vco,
            (false, true) => Self::MixerOnly,
            (true, false) => Self::OneShot,
            (true, true) => Self::VcoAlternating,
        }
    }

    /// The gate this envelope presents to the generator.
    #[must_use]
    pub(crate) const fn gate(self, vco: bool, vco_alternate: bool, one_shot: bool) -> bool {
        match self {
            Self::Vco => vco,
            Self::MixerOnly => true,
            Self::OneShot => one_shot,
            Self::VcoAlternating => vco && vco_alternate,
        }
    }

    /// Whether this envelope's fall is a decay ramp.
    ///
    /// The data sheet says in as many words that the decay has no effect on the
    /// mixer-only function — `SN76477-036`.
    #[must_use]
    pub(crate) const fn decays(self) -> bool {
        !matches!(self, Self::MixerOnly)
    }
}

/// The attack/decay capacitor, as an amplitude.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct EnvelopeGenerator {
    level: u32,
    /// The fraction of a step carried between ticks, out of the ramp length, so
    /// a ramp of any length takes exactly its own number of ticks.
    carry: u64,
}

impl EnvelopeGenerator {
    /// Full amplitude, in steps of [`EnvelopeGenerator::amplitude_steps`];
    /// what it is worth at the pin is [`crate::Timing::output_swing`].
    pub(crate) const FULL_STEPS: u32 = FULL;

    /// A reset generator, capacitor empty.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self { level: 0, carry: 0 }
    }

    /// One tick.
    ///
    /// `inhibit` is pin 9: it discharges the capacitor outright rather than
    /// starting a decay (`SN76477-037`). `gate` is the envelope's, `decays`
    /// says whether its fall ramps or is simply the absence of a gate, and
    /// `attack_ticks` and `decay_ticks` are [`crate::Timing`]'s.
    pub(crate) const fn tick(
        &mut self,
        inhibit: bool,
        gate: bool,
        decays: bool,
        attack_ticks: usize,
        decay_ticks: usize,
    ) {
        if inhibit {
            self.level = 0;
            self.carry = 0;
            return;
        }
        if gate {
            self.ramp(attack_ticks, true);
        } else if decays {
            self.ramp(decay_ticks, false);
        } else {
            self.level = 0;
            self.carry = 0;
        }
    }

    /// Move `FULL / length` of the way, carrying the remainder.
    const fn ramp(&mut self, length: usize, up: bool) {
        if length == 0 {
            self.level = if up { FULL } else { 0 };
            self.carry = 0;
            return;
        }
        let carry = self.carry as u128 + FULL as u128;
        let step = (carry / length as u128) as u32;
        self.carry = (carry % length as u128) as u64;
        if up {
            self.level = if self.level + step > FULL {
                FULL
            } else {
                self.level + step
            };
        } else {
            self.level = self.level.saturating_sub(step);
        }
    }

    /// The amplitude, `0..=`[`EnvelopeGenerator::FULL_STEPS`]; one step is
    /// 1/65 536 of full amplitude.
    #[must_use]
    pub(crate) const fn amplitude_steps(self) -> u32 {
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::{Envelope, EnvelopeGenerator};

    /// `SN76477-018`, `SN76477-032`, `SN76477-033`: the select table and the
    /// gate each row presents.
    #[test]
    fn select_table_and_gates() {
        assert_eq!(Envelope::select(false, false), Envelope::Vco);
        assert_eq!(Envelope::select(false, true), Envelope::MixerOnly);
        assert_eq!(Envelope::select(true, false), Envelope::OneShot);
        assert_eq!(Envelope::select(true, true), Envelope::VcoAlternating);
        for vco in [false, true] {
            for alternate in [false, true] {
                for one_shot in [false, true] {
                    assert_eq!(Envelope::Vco.gate(vco, alternate, one_shot), vco);
                    assert!(Envelope::MixerOnly.gate(vco, alternate, one_shot));
                    assert_eq!(Envelope::OneShot.gate(vco, alternate, one_shot), one_shot);
                    assert_eq!(
                        Envelope::VcoAlternating.gate(vco, alternate, one_shot),
                        vco && alternate
                    );
                }
            }
        }
    }

    /// `SN76477-036`: mixer only has no decay, every other row does.
    #[test]
    fn only_mixer_only_has_no_decay() {
        assert!(!Envelope::MixerOnly.decays());
        for selection in [Envelope::Vco, Envelope::OneShot, Envelope::VcoAlternating] {
            assert!(selection.decays());
        }
    }

    /// `SN76477-035`: the longest ramp a saturated count gives carries its
    /// fraction without overflowing, and takes its first step once a whole
    /// one has accrued.
    #[test]
    fn the_longest_ramp_carries_its_fraction() {
        let longest = u32::MAX as usize;
        let mut envelope = EnvelopeGenerator::default();
        for _ in 1..EnvelopeGenerator::FULL_STEPS {
            envelope.tick(false, true, true, longest, 1);
        }
        assert_eq!(envelope.amplitude_steps(), 0);
        for _ in 0..EnvelopeGenerator::FULL_STEPS {
            envelope.tick(false, true, true, longest, 1);
        }
        assert_eq!(envelope.amplitude_steps(), 1);
    }
}
