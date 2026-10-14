//! The Texas Instruments SN74393 dual 4-bit binary counter, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part is two counters that share nothing** (`SN74393-014`), so the
//! package is [`Sn74393`] and one half of it is [`Counter`], and a consumer
//! that wires one half and leaves the other alone can hold a [`Counter`] and
//! nothing else.
//!
//! **A counter is edge-triggered on the falling edge** (`SN74393-009`), so a
//! [`Counter`] holds the four bits it holds (`SN74393-013`) and the clock level
//! it last saw, because an edge is a change between two levels and a part at
//! its pins is only ever shown one at a time.
//!
//! **The ripple is not here.** On a real part the four outputs settle one after
//! another (`SN74393-011`), and between the edge and the settled count a fast
//! enough decoder sees values that are not the next count. This crate shows the
//! settled count on the call the edge arrives and nothing in between
//! (`SN74393-OPEN-04`).
//!
//! **There are no nanoseconds here.** Pulse-width and setup figures are
//! conditions a board meets, and this crate assumes they are met: a clear High
//! on the call a falling edge arrives holds the count at 0
//! (`SN74393-OPEN-02`). `Default` is both counters cleared
//! (`SN74393-OPEN-03`).
//!
//! # Examples
//!
//! Each counter advances on a falling edge of its `A` pin.
//!
//! ```
//! use sn74393::{Inputs, Level, Sn74393};
//!
//! let mut part = Sn74393::default();
//! let idle = Inputs::default();
//! part.step([Inputs { a: Level::High, clr: Level::Low }, idle]);
//! let out = part.step([idle, idle]);
//! assert_eq!(out[0].q, 1);
//! assert_eq!(part.counter(1).q(), 0);
//! ```

#![no_std]

/// How many pins the package presents (`SN74393-002`).
pub const PIN_COUNT: usize = 14;

/// The four outputs of one counter, in the low four bits: `QA` at bit 0
/// through `QD` at bit 3 (`SN74393-008`). Nothing above bit 3 leaves a pin.
pub const Q_MASK: u8 = 0x0F;

/// How many counters the package holds (`SN74393-001`).
pub const COUNTER_COUNT: usize = 2;

/// A pin's level, in a pin word's field, an argument and a getter's result.
pub use signal::Level;

/// The levels the outside world holds one counter's two input pins at.
///
/// **A field is a level, not a condition.** `CLR` is drawn **without** a bar
/// on this package (`SN74393-002`): `clr` is the pin's level, and High is
/// the state in which the counter *is* being cleared
/// (`SN74393-012`). That is the opposite sense from the `'161`'s and the
/// `'273`'s clears, and it is the pin's, not this crate's.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// The `A` pin. The counter advances when this goes from High to Low
    /// (`SN74393-009`).
    pub a: Level,
    /// The `CLR` pin; High is the state in which the counter is held at 0
    /// (`SN74393-012`).
    pub clr: Level,
}

impl Inputs {
    /// Both pins Low: the clock standing and the counter not cleared.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            a: Level::Low,
            clr: Level::Low,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// What one counter drives on its four output pins.
///
/// Always a level on every pin: every output is its flip-flop's state
/// (`SN74393-013`) and a totem pole with no output-control pin
/// (`SN74393-006`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `QA`–`QD`, `QA` at bit 0.
    pub q: u8,
}

/// One of the package's two counters.
///
/// `Default` stands at power-up. The data sheet fixes no level there
/// (`SN74393-OPEN-03`), so this crate starts the counter in its reset state:
/// cleared, with the clock remembered Low. A board that needs a known level
/// raises `CLR`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Counter {
    /// The four bits the counter holds, `QA` at bit 0 (`SN74393-013`).
    q: u8,
    /// The level the `A` pin was last shown. The level, not a fifth bit of
    /// storage: a part at its pins cannot see an edge, only two levels in
    /// succession, and the edge is the change between them.
    a: Level,
}

impl Default for Counter {
    /// Power-up, which the sheet does not fix: cleared, with `A` remembered
    /// Low.
    fn default() -> Self {
        Self::with_state(0, Level::Low)
    }
}

/// The next count of a modulo-16 binary counter (`SN74393-008`).
///
/// Every one of the sixteen values is on the one cycle, so this is total and
/// needs no table. That it is what four stages each toggling on the fall of the
/// one before produce is `SN74393-010`.
fn next(q: u8) -> u8 {
    q.wrapping_add(1) & Q_MASK
}

impl Counter {
    /// The counter with a given four bits held and a given clock level
    /// remembered, for a caller that has to reach a state. `q` is masked to
    /// the pins that exist; `with_state(c.q(), c.a())` rebuilds `c`.
    #[must_use]
    pub const fn with_state(q: u8, a: Level) -> Self {
        Self { q: q & Q_MASK, a }
    }

    /// The four bits the counter holds, which are the levels on `QA`–`QD`:
    /// every output is its flip-flop's state (`SN74393-013`) and every output
    /// is a totem pole with no output-control pin (`SN74393-006`), so this is a
    /// level on every output pin at all times.
    #[must_use]
    pub const fn q(&self) -> u8 {
        self.q
    }

    /// The level the `A` pin was last shown.
    #[must_use]
    pub const fn a(&self) -> Level {
        self.a
    }

    /// What the counter drives, which is what it holds (`SN74393-013`).
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        Outputs { q: self.q }
    }

    /// Show the counter one pair of input levels, and read back what it
    /// drives.
    ///
    /// - `CLR` High holds the count at 0 whatever the clock is doing
    ///   (`SN74393-012`), and the clock level is still taken up, because the
    ///   pin is what it is regardless of what the part is doing about it;
    /// - a clock that has gone from High to Low since the last call advances
    ///   the count (`SN74393-009`, `SN74393-008`);
    /// - anything else holds — a clock standing at either level, and a rising
    ///   edge.
    pub fn step(&mut self, inputs: Inputs) -> Outputs {
        let falling = inputs.a.is_low() && self.a.is_high();
        self.a = inputs.a;
        if inputs.clr.is_high() {
            self.q = 0;
        } else if falling {
            self.q = next(self.q);
        }
        self.outputs()
    }
}

/// The package: two counters, the first the one whose pins are numbered `1A`,
/// `1CLR`, `1QA`–`1QD` (`SN74393-002`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Sn74393 {
    counters: [Counter; COUNTER_COUNT],
}

impl Sn74393 {
    /// The package with both counters in given states.
    #[must_use]
    pub const fn with_counters(counters: [Counter; COUNTER_COUNT]) -> Self {
        Self { counters }
    }

    /// One of the two counters as it stands, index 0 being the one whose pins
    /// are numbered `1A`, `1CLR`, `1QA`–`1QD`.
    ///
    /// # Panics
    ///
    /// If `index` is not 0 or 1.
    #[must_use]
    pub const fn counter(&self, index: usize) -> Counter {
        self.counters[index]
    }

    /// What each counter drives.
    #[must_use]
    pub const fn outputs(&self) -> [Outputs; COUNTER_COUNT] {
        [self.counters[0].outputs(), self.counters[1].outputs()]
    }

    /// Show both counters their input levels at once, and read back what each
    /// drives. Neither reads the other's pins (`SN74393-014`): this is two
    /// calls to [`Counter::step`] and nothing between them.
    pub fn step(&mut self, inputs: [Inputs; COUNTER_COUNT]) -> [Outputs; COUNTER_COUNT] {
        [
            self.counters[0].step(inputs[0]),
            self.counters[1].step(inputs[1]),
        ]
    }
}
