use crate::Level;

/// The noise generator: a 5-bit period counter, its divide-by-two flip-flop, and
/// the 17-bit shift register, in the die's own encoding.
///
/// The counter advances on every divided edge and clears at the live
/// `max(1, R6)`, toggling the divider; the register shifts only when the divider
/// returns low, so one shift per `16 × max(1, period)` external clocks.
///
/// **The divider is High out of reset** (`AY8910-049`), so the first carry after
/// a reset shifts rather than the second.
///
/// The register is cleared by reset, shifts **left**, and takes
/// `bit 16 XOR bit 13` into bit 0, with an all-zero register forcing a one in
/// so the sequence starts at all. The level the mixers see is bit 16
/// **inverted** (`AY8910-031`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct NoiseState {
    /// The 5-bit period counter, counting divided edges since it last cleared.
    pub counter: u8,
    /// The divide-by-two flip-flop the counter toggles; the register shifts
    /// as it returns Low.
    pub divider: Level,
    /// The seventeen latches, bit 16 highest. Zero after reset.
    pub shift_register: u32,
}

impl Default for NoiseState {
    fn default() -> Self {
        Self {
            counter: 0,
            divider: Level::High,
            shift_register: 0,
        }
    }
}

impl NoiseState {
    /// The raw noise level: bit 16 of the shift register, inverted.
    #[must_use]
    pub fn level(self) -> Level {
        Level::from_bool(self.shift_register & 1 << 16 == 0)
    }

    pub(crate) fn tick(&mut self, period: u8) {
        self.counter += 1;
        if self.counter >= period.max(1) {
            self.counter = 0;
            self.divider = !self.divider;
            if self.divider.is_low() {
                let feedback = ((self.shift_register >> 16) ^ (self.shift_register >> 13)) & 1;
                let escape = u32::from(self.shift_register == 0);
                self.shift_register = (self.shift_register << 1 | feedback | escape) & 0x1FFFF;
            }
        }
    }
}
