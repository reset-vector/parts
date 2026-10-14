//! The Fairchild 7411 triple 3-input AND gate, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`F7411-009`), so the whole of it is [`drive`]:
//! a function from the levels the outside world holds the nine input pins at
//! to the three levels the part drives back. There is no clock here and no
//! `tick`.
//!
//! **This crate is a reading of a word and a shape.** Fairchild's page states
//! the function once, as the title *AND* and three AND symbols drawn inside the
//! package outline (`F7411-010`); [`gate`] is that word written as a
//! conjunction.
//!
//! # Examples
//!
//! Three 3-input AND gates, one bit per gate: gate 0 has every input High.
//!
//! ```
//! use f7411::{Inputs, drive};
//!
//! assert_eq!(drive(Inputs { a: 0b001, b: 0b011, c: 0b101 }), 0b001);
//! ```

#![no_std]

/// How many gates the package carries (`F7411-001`).
pub const GATE_COUNT: usize = 3;

/// How many inputs one gate has (`F7411-001`).
pub const INPUTS_PER_GATE: usize = 3;

/// How many pins the package presents, none of them a no-connect
/// (`F7411-001`).
pub const PIN_COUNT: usize = 14;

/// The three pins of one field — the first input of each gate, or the second,
/// or the third — in the low three bits of a byte.
pub const GATE_MASK: u8 = 0b111;

/// The levels the outside world holds the part's nine input pins at.
///
/// Fairchild's page names no signal pin (`F7411-002`), so the field names here
/// are TI's, carried over by `F7411-SIB-01`. Bit `n` of each field is gate
/// `n + 1`'s pin, a numbering this crate imposes for the convenience of
/// driving three gates at once; where those pins are on the package, and that
/// gate 1's are not contiguous, is `F7411-002` and `F7411-003`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// The three first inputs, bit `n` for gate `n + 1`, per [`GATE_MASK`].
    pub a: u8,
    /// The three second inputs.
    pub b: u8,
    /// The three third inputs.
    pub c: u8,
}

impl Inputs {
    /// Every input pin Low.
    #[must_use]
    pub const fn idle() -> Self {
        Self { a: 0, b: 0, c: 0 }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// What the part drives on its three outputs, one bit per gate per [`GATE_MASK`].
///
/// A set bit is a High pin. Each gate reads its own three pins and nothing
/// else (`F7411-008`).
#[must_use]
pub fn drive(inputs: Inputs) -> u8 {
    let mut y = 0;
    for index in 0..GATE_COUNT {
        if gate(
            bit(inputs.a, index),
            bit(inputs.b, index),
            bit(inputs.c, index),
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

/// One 3-input AND gate: the level its output pin is driven to (`F7411-005`,
/// `F7411-006`).
#[must_use]
pub const fn gate(a: Level, b: Level, c: Level) -> Level {
    Level::from_bool(a.is_high() && b.is_high() && c.is_high())
}

/// The level of one bit of a triple, bit 0 at index 0, a set bit High.
#[must_use]
pub(crate) const fn bit(triple: u8, index: usize) -> Level {
    Level::from_bool((triple >> index) & 1 == 1)
}
