//! The Texas Instruments SN7410 triple 3-input positive-NAND gate, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`SN7410-009`), so the whole of it is [`drive`]:
//! a function from the levels the outside world holds the nine input pins at to
//! the three levels the part drives back. There is no clock here and no `tick`,
//! because the data book specifies no edge for one to correspond to.
//!
//! **This crate is a reading of the first of the two printed forms.** TI's *The
//! TTL Data Book, Volume 2* (1985), p. 3-57, gives the function as the
//! complement of a product and then, after an `or`, as a sum of complements
//! (`SN7410-007`), and [`gate`] is the first of those written as it is printed.
//!
//! # Examples
//!
//! Three 3-input NAND gates, one bit per gate: gate 0 has every input High.
//!
//! ```
//! use sn7410::{Inputs, drive};
//!
//! assert_eq!(drive(Inputs { a: 0b001, b: 0b011, c: 0b101 }), 0b110);
//! ```

#![no_std]

/// How many gates the package carries (`SN7410-001`).
pub const GATE_COUNT: usize = 3;

/// How many inputs one gate has (`SN7410-001`).
pub const INPUTS_PER_GATE: usize = 3;

/// How many pins the package presents, none of them a no-connect
/// (`SN7410-003`).
pub const PIN_COUNT: usize = 14;

/// The three pins of one nibble — an `A` pin per gate, or a `B`, or a `C` — in
/// the low three bits of a byte.
pub const GATE_MASK: u8 = 0b111;

/// The levels the outside world holds the part's nine input pins at.
///
/// **A field is a level, not a condition.** No input of this part is drawn with
/// a bar (`SN7410-002`), so a set bit is simply High on the pin. Bit `n` of each
/// field is gate `n + 1`'s pin, which is a numbering this crate imposes for the
/// convenience of driving three gates at once; it is **not** the order the pins
/// run round the package, where no gate's four pins are even contiguous
/// (`SN7410-003`).
///
/// There is no enable and no strobe here: the package has none, and the two
/// supplies are the only pins the three gates share (`SN7410-001`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// The three `A` pins, bit `n` for gate `n + 1`, per [`GATE_MASK`].
    pub a: u8,
    /// The three `B` pins, bit `n` for gate `n + 1`.
    pub b: u8,
    /// The three `C` pins, bit `n` for gate `n + 1`.
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
/// A set bit is a High pin. Each gate reads its own three pins and nothing else,
/// so this is [`gate`] three times and no gate's inputs reach another's output
/// (`SN7410-010`).
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

/// One 3-input NAND gate: the level its output pin is driven to.
///
/// This is the complement of `A·B·C`, the first of the two forms printed under
/// *positive logic* on p. 3-57 of the data book, written out (`SN7410-007`).
#[must_use]
pub const fn gate(a: Level, b: Level, c: Level) -> Level {
    Level::from_bool(!(a.is_high() && b.is_high() && c.is_high()))
}

/// The level of one bit of a triple, bit 0 at index 0, a set bit High.
#[must_use]
pub(crate) const fn bit(triple: u8, index: usize) -> Level {
    Level::from_bool((triple >> index) & 1 == 1)
}
