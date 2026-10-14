//! The three kinds of generator, each advanced on a divided edge — one per
//! eight master clocks. The rates are `YM2149-012`, `YM2149-014`,
//! `YM2149-017` and `YM2149-022`; the eight-clock grain, each counter's
//! direction and every divider out of reset are this crate's choice under
//! `YM2149-OPEN-05`.

use crate::Level;

/// One tone generator: a 12-bit counter and its square-wave level. Reaching
/// `max(1, period)` toggles the level, so one cycle is `16 × max(1, TP)`
/// master clocks (`YM2149-012`, `YM2149-013`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ToneState {
    /// The 12-bit counter, counting divided edges since the last toggle.
    pub counter: u16,
    /// The square-wave level.
    pub level: Level,
}

impl Default for ToneState {
    /// The counter at 0 and the level Low.
    fn default() -> Self {
        Self {
            counter: 0,
            level: Level::Low,
        }
    }
}

impl ToneState {
    pub(crate) fn tick(&mut self, period: u16) {
        self.counter += 1;
        if self.counter >= period.max(1) {
            self.counter = 0;
            self.level = !self.level;
        }
    }
}

/// The noise generator: a 5-bit counter, a divide-by-two, and a 17-stage
/// register shifting once per `16 × max(1, NP)` master clocks (`YM2149-014`,
/// `YM2149-015`). The register shifts towards bit 16 and takes
/// `bit 16 XOR bit 13` into bit 0, and the level is bit 16, so the level's
/// sequence obeys `out[n] = out[n−14] XOR out[n−17]` (`YM2149-016`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct NoiseState {
    /// The 5-bit counter, counting divided edges since it last cleared.
    pub counter: u8,
    /// The divide-by-two flip-flop the counter toggles; the register shifts
    /// as it returns Low.
    pub divider: Level,
    /// The seventeen stages, bit 16 highest; 1 out of reset.
    pub shift_register: u32,
}

impl Default for NoiseState {
    fn default() -> Self {
        Self {
            counter: 0,
            divider: Level::Low,
            shift_register: 1,
        }
    }
}

impl NoiseState {
    /// The noise level the mixer sees.
    #[must_use]
    pub fn level(self) -> Level {
        Level::from_bool(self.shift_register & 1 << 16 != 0)
    }

    pub(crate) fn tick(&mut self, period: u8) {
        self.counter += 1;
        if self.counter >= period.max(1) {
            self.counter = 0;
            self.divider = !self.divider;
            if self.divider.is_low() {
                let feedback = ((self.shift_register >> 16) ^ (self.shift_register >> 13)) & 1;
                self.shift_register = (self.shift_register << 1 | feedback) & 0x1_FFFF;
            }
        }
    }
}

/// The envelope generator: a 16-bit counter that steps a five-bit shape
/// counter once per `8 × max(1, EP)` master clocks, thirty-two steps to a
/// cycle of `256 × EP` (`YM2149-017`, `YM2149-018`, `YM2149-021`,
/// `YM2149-022`), and the shape's polarity and hold flags (`YM2149-019`,
/// `YM2149-020`).
///
/// A write to R13 restarts it at step 31 with Attack as its polarity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct EnvelopeState {
    /// The 16-bit counter, counting divided edges since it last cleared.
    pub counter: u16,
    /// R13's low four bits as latched by the last trigger.
    pub shape: u8,
    /// The shape counter, counting 31 down to 0.
    pub shape_counter: u8,
    /// Output polarity: the code is `shape_counter` inverted while set.
    pub attack: bool,
    /// Whether the shape has stopped and holds its code until the next trigger.
    pub holding: bool,
}

impl Default for EnvelopeState {
    fn default() -> Self {
        Self::triggered(0)
    }
}

impl EnvelopeState {
    const HOLD: u8 = 1;
    const ALTERNATE: u8 = 2;
    const ATTACK: u8 = 4;
    const CONTINUE: u8 = 8;

    /// The five-bit converter code the envelope selects, `E4`..`E0`, 0..=31
    /// ([`crate::dac::CODE_BIT_COUNT`]).
    #[must_use]
    pub fn code(self) -> u8 {
        if self.attack {
            self.shape_counter ^ 0x1F
        } else {
            self.shape_counter
        }
    }

    pub(crate) fn triggered(shape: u8) -> Self {
        let shape = shape & 0x0F;
        Self {
            counter: 0,
            shape,
            shape_counter: 31,
            attack: shape & Self::ATTACK != 0,
            holding: false,
        }
    }

    pub(crate) fn tick(&mut self, period: u16) {
        self.counter += 1;
        if self.counter >= period.max(1) {
            self.counter = 0;
            self.advance();
        }
    }

    fn advance(&mut self) {
        if self.holding {
            return;
        }
        if self.shape_counter > 0 {
            self.shape_counter -= 1;
            return;
        }
        let alternate = self.shape & Self::ALTERNATE != 0;
        if self.shape & Self::CONTINUE == 0 {
            self.attack = false;
            self.holding = true;
        } else if self.shape & Self::HOLD != 0 {
            self.holding = true;
            self.attack ^= alternate;
        } else {
            self.shape_counter = 31;
            self.attack ^= alternate;
        }
    }
}
