//! The Texas Instruments SN7404 hex inverter, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`SN7404-008`), so the whole of it is
//! [`drive`]: a function from the levels the outside world holds the six input
//! pins at to the six levels the part drives back. There is no clock here and
//! no `tick`, because the data sheet specifies no edge for one to correspond
//! to.
//!
//! **This crate is a reading of the equation.** `SDLS029C` p. 3 gives the
//! function as `Y = Ā` under six inverter symbols (`SN7404-005`), and
//! [`gate`] is that equation, which [`drive`] writes six times at once.
//!
//! **What the crate chooses**, an open item: an input pin is High or Low and
//! nothing between (`SN7404-OPEN-03`).
//!
//! # Examples
//!
//! Six inverters, one bit per inverter.
//!
//! ```
//! use sn7404::{Inputs, drive};
//!
//! assert_eq!(drive(Inputs { a: 0b10_1010 }), 0b01_0101);
//! ```

#![no_std]

/// How many inverters the package holds (`SN7404-001`).
pub const INVERTER_COUNT: usize = 6;

/// The six inverters of the package, in the low six bits of a byte.
///
/// Bit 0 is the inverter the data sheet numbers 1. Nothing above bit 5 leaves
/// a pin, and every value this crate produces is masked with this.
pub const INVERTER_MASK: u8 = 0x3F;

/// How many pins the package presents (`SN7404-002`).
pub const PIN_COUNT: usize = 14;

/// The levels the outside world holds the part's six input pins at.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `1A`–`6A`, bit 0 for `1A` through bit 5 for `6A`, per [`INVERTER_MASK`];
    /// a set bit is a High pin, and no level lies between High and Low
    /// (`SN7404-OPEN-03`).
    pub a: u8,
}

impl Inputs {
    /// Every input pin Low.
    #[must_use]
    pub const fn idle() -> Self {
        Self { a: 0 }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// The levels the part drives on `1Y`–`6Y`, one bit per inverter per
/// [`INVERTER_MASK`].
///
/// Each output is the complement of its own input and of nothing else
/// (`SN7404-004`, `SN7404-006`), and the result is masked to the six pins
/// that exist, so bits set above [`INVERTER_MASK`] on the way in reach nothing on
/// the way out.
#[must_use]
pub const fn drive(inputs: Inputs) -> u8 {
    !inputs.a & INVERTER_MASK
}

/// A pin's level, as [`gate`] takes its input and returns its output.
pub use signal::Level;

/// One inverter: the level its output pin is driven to (`SN7404-005`).
#[must_use]
pub const fn gate(a: Level) -> Level {
    Level::from_bool(a.is_low())
}
