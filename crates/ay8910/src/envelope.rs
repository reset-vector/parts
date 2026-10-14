use crate::Level;

/// The envelope generator: a 16-bit period counter, its divide-by-two flip-flop,
/// and the four-bit shape counter with its polarity and hold flags.
///
/// The counter advances on every divided edge and clears at the live
/// `max(1, period)`, toggling the divider. The shape advances when the divider
/// returns low — every `16 × period` external clocks, which is the manual's
/// own `fCLK / (256 × EP)` cycle divided into its sixteen states. A period of
/// zero runs at the rate of one, because the die's comparator is already
/// satisfied at a count of zero (`AY8910-024`).
///
/// The divider is the TEST1 pin.
///
/// **A reset and a trigger leave the divider in opposite halves** — High out of a
/// reset, so the first carry steps the shape, and Low out of a trigger, so the
/// second does (`AY8910-050`).
///
/// A write to R13 (`trigger`) clears the counter and divider, starts the shape
/// counter at 15 with Attack as the output polarity, and releases any hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct EnvelopeState {
    /// The 16-bit period counter, counting divided edges since it last cleared.
    pub counter: u16,
    /// The divide-by-two flip-flop the counter toggles, which is the TEST1
    /// pin; the shape steps as it returns Low.
    pub test1: Level,
    /// R13's low four bits as latched by the last trigger.
    pub shape: u8,
    /// The shape counter, counting 15 down to 0.
    pub shape_counter: u8,
    /// Output polarity: the code is `shape_counter` inverted while set.
    pub attack: bool,
    /// Whether the shape has stopped and holds its code until the next trigger.
    pub holding: bool,
    /// One divided edge the period counter owes before it resumes, a delay
    /// an R13 write that landed on an odd count started.
    pub counter_deferred: bool,
}

impl Default for EnvelopeState {
    fn default() -> Self {
        Self {
            test1: Level::High,
            ..Self::triggered(0)
        }
    }
}

impl EnvelopeState {
    const HOLD: u8 = 1;
    const ALTERNATE: u8 = 2;
    const ATTACK: u8 = 4;
    const CONTINUE: u8 = 8;

    /// The four-bit converter code the envelope selects, 0..=15
    /// ([`crate::dac::CODE_BIT_COUNT`]).
    #[must_use]
    pub fn code(self) -> u8 {
        if self.attack {
            self.shape_counter ^ 0x0F
        } else {
            self.shape_counter
        }
    }

    pub(crate) fn triggered(shape: u8) -> Self {
        let shape = shape & 0x0F;
        Self {
            counter: 0,
            test1: Level::Low,
            shape,
            shape_counter: 15,
            attack: shape & Self::ATTACK != 0,
            holding: false,
            counter_deferred: false,
        }
    }

    /// An R13 write. It retriggers, and it costs the period counter one extra
    /// divided edge before its first carry when it lands on an odd count.
    pub(crate) fn trigger(&mut self, shape: u8) {
        let odd = self.counter & 1 == 1;
        *self = Self::triggered(shape);
        self.counter_deferred = odd;
    }

    pub(crate) fn tick(&mut self, period: u16) {
        if self.counter_deferred {
            self.counter_deferred = false;
            return;
        }
        self.counter += 1;
        if self.counter >= period.max(1) {
            self.counter = 0;
            self.test1 = !self.test1;
            if self.test1.is_low() {
                self.advance();
            }
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
            self.shape_counter = 15;
            self.attack ^= alternate;
        }
    }
}
