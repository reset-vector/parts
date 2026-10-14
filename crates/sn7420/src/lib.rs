//! The Texas Instruments SN7420 dual 4-input positive-NAND gate, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`SN7420-009`), so the whole of it is [`drive`]:
//! a function from the levels the outside world holds the eight input pins at
//! to the two levels the part drives back. There is no clock here and no
//! `tick`, because the data sheet specifies no edge for one to correspond to.
//!
//! **This crate is a reading of the first equation.** `SDLS079` p. 1 gives the
//! function as `Y = ¬(A · B · C · D)` and again as `Y = Ā + B̄ + C̄ + D̄`
//! (`SN7420-005`), and [`gate`] is the first of those written as it is printed.
//!
//! # Examples
//!
//! Two 4-input NAND gates, one bit per gate: gate 0 has every input High.
//!
//! ```
//! use sn7420::{Inputs, drive};
//!
//! assert_eq!(drive(Inputs { a: 0b01, b: 0b11, c: 0b01, d: 0b01 }), 0b10);
//! ```

#![no_std]

/// How many gates the package carries (`SN7420-001`).
pub const GATE_COUNT: usize = 2;

/// How many inputs one gate has (`SN7420-001`).
pub const INPUTS_PER_GATE: usize = 4;

/// How many pins the package presents (`SN7420-002`).
pub const PIN_COUNT: usize = 14;

/// The two pins of the 14-pin assignment that are connected to nothing
/// (`SN7420-002`).
pub const NO_CONNECT_PINS: [u8; 2] = [3, 11];

/// The two pins of one field — an `A` pin per gate, or a `B`, a `C` or a `D`
/// — in the low two bits of a byte.
pub const GATE_MASK: u8 = 0b11;

/// The levels the outside world holds the part's eight input pins at.
///
/// **A field is a level, not a condition.** No input of this part is drawn
/// with a bar (`SN7420-002`), so a set bit is simply High on the pin. Bit `n`
/// of each field is gate `n + 1`'s pin, a numbering this crate imposes for the
/// convenience of driving both gates at once.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// The two `A` pins, bit `n` for gate `n + 1`, per [`GATE_MASK`].
    pub a: u8,
    /// The two `B` pins.
    pub b: u8,
    /// The two `C` pins.
    pub c: u8,
    /// The two `D` pins.
    pub d: u8,
}

impl Inputs {
    /// Every input pin Low.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            a: 0,
            b: 0,
            c: 0,
            d: 0,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// What the part drives on its two outputs, one bit per gate per [`GATE_MASK`].
///
/// A set bit is a High pin. Each gate reads its own four pins and nothing
/// else, so this is [`gate`] twice and neither gate's inputs reach the other's
/// output (`SN7420-007`).
#[must_use]
pub fn drive(inputs: Inputs) -> u8 {
    let mut y = 0;
    for index in 0..GATE_COUNT {
        if gate(
            bit(inputs.a, index),
            bit(inputs.b, index),
            bit(inputs.c, index),
            bit(inputs.d, index),
        )
        .is_high()
        {
            y |= 1 << index;
        }
    }
    y
}

/// A pin's level, as [`gate`] takes its inputs and returns its output.
pub use signal::Level;

/// One 4-input NAND gate: the level its output pin is driven to.
///
/// This is the complement of `A · B · C · D`, the first of the two forms
/// printed under *positive logic* on `SDLS079` p. 1 (`SN7420-005`).
#[must_use]
pub const fn gate(a: Level, b: Level, c: Level, d: Level) -> Level {
    Level::from_bool(!(a.is_high() && b.is_high() && c.is_high() && d.is_high()))
}

/// The level of one bit of a pair, bit 0 at index 0, a set bit High.
const fn bit(pair: u8, index: usize) -> Level {
    Level::from_bool((pair >> index) & 1 == 1)
}
