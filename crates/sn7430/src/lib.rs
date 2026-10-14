//! The Texas Instruments SN7430 8-input positive-NAND gate, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`SN7430-009`), so the whole of it is [`drive`]:
//! a function from the levels the outside world holds the eight input pins at
//! to the [`Outputs`] the part drives back, its one pin `Y`. There is no clock here and no `tick`,
//! because the data book specifies no edge for one to correspond to.
//!
//! **This crate is a reading of the first of the two printed equations.**
//! Printed 3-139 gives the function as the complement of a product and then as
//! a sum of complements (`SN7430-007`), and [`drive`] is the first of those
//! written as it is printed: the conjunction of eight levels, complemented.
//!
//! # Examples
//!
//! The one gate: Low only with all eight inputs High.
//!
//! ```
//! use sn7430::{Inputs, Level, drive};
//!
//! assert_eq!(drive(Inputs { abcdefgh: 0xFF }).y, Level::Low);
//! assert_eq!(drive(Inputs { abcdefgh: 0x7F }).y, Level::High);
//! ```

#![no_std]

/// How many inputs the one gate has (`SN7430-001`).
pub const INPUT_COUNT: usize = 8;

/// How many pins the package presents (`SN7430-002`).
pub const PIN_COUNT: usize = 14;

/// The pins of the 14-pin package that reach nothing, in pin numbers
/// (`SN7430-003`).
pub const NO_CONNECT_PINS: [u8; 3] = [9, 10, 13];

/// The eight input pins, in [`Inputs::abcdefgh`], bit 0 for `A` through bit 7 for
/// `H`. Every one High is the one combination that drives the output Low
/// (`SN7430-005`).
pub const ABCDEFGH_MASK: u8 = 0xFF;

/// The levels the outside world holds the part's eight input pins at.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `A`–`H`, a level per bit, bit 0 for `A` through bit 7 for `H` — a
    /// numbering this crate imposes to have one value to sweep, and **not** the
    /// order the pins run round the package, which is `SN7430-002` and puts `G`
    /// and `H` at pins 11 and 12.
    pub abcdefgh: u8,
}

impl Inputs {
    /// Every input pin Low.
    #[must_use]
    pub const fn idle() -> Self {
        Self { abcdefgh: 0 }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// A pin's level, as [`Outputs::y`] holds it.
pub use signal::Level;

/// The level the part drives on its one output pin.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// The `Y` pin, pin 8 of the 14-pin package (`SN7430-002`).
    pub y: Level,
}

/// What the part's one gate drives on `Y`, given the levels held on its eight
/// input pins `A` to `H`.
///
/// The one gate takes every input and there is nothing else in the package to
/// take them (`SN7430-001`), so this is the whole part: the complement of
/// `A·B·C·D·E·F·G·H`, written as it is printed.
#[must_use]
pub fn drive(inputs: Inputs) -> Outputs {
    let pin = |index: usize| (inputs.abcdefgh >> index) & 1 == 1;
    let (a, b, c, d) = (pin(0), pin(1), pin(2), pin(3));
    let (e, f, g, h) = (pin(4), pin(5), pin(6), pin(7));
    Outputs {
        y: Level::from_bool(!(a && b && c && d && e && f && g && h)),
    }
}
