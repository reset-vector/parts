//! The Texas Instruments SN7402 quadruple 2-input positive-NOR gate, at its
//! pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`SN7402-009`), so the whole of it is [`drive`]:
//! a function from the levels the outside world holds the eight input pins at
//! to the four levels the part drives back. There is no clock here and no
//! `tick`, because the data sheet specifies no edge for one to correspond to.
//!
//! **This crate is a reading of the first equation.** `SDLS027` p. 1 gives the
//! function as `Y = ¬(A + B)` and again as `Y = Ā · B̄` (`SN7402-005`), and
//! [`gate`] is the first of those, which [`drive`] writes four times at once.
//!
//! **Which pin is which is not this crate's to say.** Bit `n` of a field is
//! gate `n + 1`; where that gate's pins are is `SN7402-002`, and on this part
//! the output pin comes *before* its two inputs, which is not the `'00`'s
//! order.
//!
//! **What the crate chooses**, an open item: an input pin is High or Low and
//! nothing between (`SN7402-OPEN-04`).
//!
//! # Examples
//!
//! Four NOR gates, one bit per gate: only gate 3 has both inputs Low.
//!
//! ```
//! use sn7402::{Inputs, drive};
//!
//! assert_eq!(drive(Inputs { a: 0b0011, b: 0b0101 }), 0b1000);
//! ```

#![no_std]

/// How many gates the package holds (`SN7402-001`).
pub const GATE_COUNT: usize = 4;

/// The four gates of the package, in the low four bits of a byte.
///
/// Bit 0 is the gate the data sheet numbers 1. Nothing above bit 3 leaves a
/// pin, and every value this crate produces is masked with this.
pub const GATE_MASK: u8 = 0x0F;

/// How many pins the package presents (`SN7402-002`).
pub const PIN_COUNT: usize = 14;

/// The levels the outside world holds the part's eight input pins at.
///
/// A field is a level per bit, bit 0 for gate 1 through bit 3 for gate 4;
/// a set bit is a High pin, and no level lies between High and Low
/// (`SN7402-OPEN-04`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `1A`–`4A`, bit `n` for gate `n + 1`, per [`GATE_MASK`].
    pub a: u8,
    /// `1B`–`4B`, bit `n` for gate `n + 1`.
    pub b: u8,
}

impl Inputs {
    /// Every input pin Low.
    #[must_use]
    pub const fn idle() -> Self {
        Self { a: 0, b: 0 }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// The levels the part drives on `1Y`–`4Y`, one bit per gate per [`GATE_MASK`].
///
/// Each output is High exactly when both of its own inputs are Low and
/// depends on no other pin (`SN7402-004`, `SN7402-007`), and the result is
/// masked to the four gates that exist, so bits set above [`GATE_MASK`] on the way
/// in reach nothing on the way out.
#[must_use]
pub const fn drive(inputs: Inputs) -> u8 {
    !(inputs.a | inputs.b) & GATE_MASK
}

/// A pin's level, as [`gate`] takes its inputs and returns its output.
pub use signal::Level;

/// One 2-input NOR gate: the level its output pin is driven to
/// (`SN7402-005`).
#[must_use]
pub const fn gate(a: Level, b: Level) -> Level {
    Level::from_bool(!(a.is_high() || b.is_high()))
}
