use crate::{Level, Parameters};

/// The noise generator: its own counter, the divide-by-two that counter's
/// output bit is, and the shift register (`SN76489-022`, `SN76489-027`).
///
/// **The register's length and taps are the [`Parameters`]'s**, and are passed in
/// on every advance rather than held here, so that this structure carries only
/// state and a saved copy of it means nothing without the part it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct NoiseState {
    /// The counter, reloaded from one of three fixed values or from the third
    /// tone channel's period register (`SN76489-023`).
    pub counter: u16,
    /// The counter's output bit, which toggles on each reload. The register
    /// advances on its low-to-high transitions only (`SN76489-027`).
    pub divider: Level,
    /// The shift register, stage 0 in bit 0. Stage `noise_stage_count - 1` is the
    /// highest and is where feedback arrives.
    pub shift_register: u16,
}

impl NoiseState {
    /// The state a reset leaves: every stage clear but the highest, the counter
    /// at zero and the divider Low (`SN76489-030`).
    #[must_use]
    pub(crate) const fn reset(parameters: Parameters) -> Self {
        Self {
            counter: 0,
            divider: Level::Low,
            shift_register: parameters.noise_reset(),
        }
    }

    /// The channel's level: the bit about to be shifted off the end
    /// (`SN76489-028`).
    #[must_use]
    pub const fn level(self) -> Level {
        Level::from_bool(self.shift_register & 1 != 0)
    }

    pub(crate) fn reset_register(&mut self, parameters: Parameters) {
        *self = Self::reset(parameters);
    }

    /// One divided clock edge, with the counter's live reload value and the
    /// live noise control register.
    ///
    /// A reload under two holds the counter's output bit where it is, and the
    /// register therefore does not advance. That bit is toggled as a tone
    /// channel's is (`SN76489-027`), which carries `SN76489-018` onto it,
    /// and the divide-by-two then never sees the low-to-high transition
    /// `SN76489-027` advances on. Only the tone-driven rate of `SN76489-023`
    /// reaches this — the three fixed reloads are `0x10`, `0x20` and `0x40` —
    /// and it holds whichever way `SN76489-OPEN-07` falls.
    pub(crate) fn tick(&mut self, reload: u16, control: u8, parameters: Parameters) {
        if reload < 2 {
            self.counter = reload;
            return;
        }
        if self.counter > 0 {
            self.counter -= 1;
        }
        if self.counter > 0 {
            return;
        }
        self.counter = reload;
        let rising = self.divider.is_low();
        self.divider = !self.divider;
        if rising {
            self.advance(control & 0x04 != 0, parameters);
        }
    }

    /// One stage. White noise feeds back the exclusive-OR of the tapped stages
    /// and the periodic configuration feeds the output bit straight back
    /// (`SN76489-029`, `SN76489-031`).
    pub(crate) fn advance(&mut self, white: bool, parameters: Parameters) {
        let feedback = if white {
            (self.shift_register & parameters.noise_tap_mask()).count_ones() & 1 == 1
        } else {
            self.shift_register & 1 == 1
        };
        let top = u16::from(feedback) << (parameters.noise_stage_count().get() - 1);
        self.shift_register = (self.shift_register >> 1) | top;
    }
}
