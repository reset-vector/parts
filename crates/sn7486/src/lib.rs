//! The Texas Instruments SN7486 quadruple 2-input exclusive-OR gate, at its
//! pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`SN7486-007`), so the whole of it is [`drive`]:
//! a function from the levels the outside world holds the eight input pins at to
//! the four levels the part drives back. There is no clock here and no `tick`,
//! because the data sheet specifies no edge for one to correspond to.
//!
//! **This crate is a reading of the equation, not of the function table.**
//! `SDLS124` draws no logic diagram anywhere, and [`gate`] is the right-hand
//! side of the equation in the description on p. 1, written as it is printed.
//!
//! # Examples
//!
//! Four exclusive-OR gates, one bit per gate.
//!
//! ```
//! use sn7486::{Inputs, drive};
//!
//! assert_eq!(drive(Inputs { a: 0b0011, b: 0b0101 }), 0b0110);
//! ```

#![no_std]

/// How many gates the package carries (`SN7486-001`).
pub const GATE_COUNT: usize = 4;

/// How many pins the package presents (`SN7486-002`).
pub const PIN_COUNT: usize = 14;

/// The four gates of the package, in the low four bits of a byte.
///
/// Bit `n` is gate `n + 1`'s pin, so bit 0 is `1A`, `1B` or `1Y`. That is a
/// numbering this crate imposes for the convenience of driving four gates at
/// once; it is **not** the order the pins run round the package, which is
/// `SN7486-002` and runs one way on the lower half and the other way on the
/// upper (`SN7486-003`). Nothing above bit 3 leaves a pin, and every value this
/// crate produces is masked with this.
pub const GATE_MASK: u8 = 0x0F;

/// The levels the outside world holds the part's eight input pins at.
///
/// **A field is a level, not a condition.** No input of this part is drawn with
/// a bar (`SN7486-002`), so a set bit is simply High on the pin. There is no
/// enable and no strobe here: the package has none, and the two supplies are the
/// only pins the four gates share (`SN7486-001`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// The four `A` pins, bit `n` for gate `n + 1`, per [`GATE_MASK`].
    pub a: u8,
    /// The four `B` pins, bit `n` for gate `n + 1`.
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

/// What the part drives on its four outputs, one bit per gate per [`GATE_MASK`].
///
/// A set bit is a High pin. Each gate reads its own two pins and nothing else,
/// so this is [`gate`] four times and no gate's inputs reach another's output
/// (`SN7486-009`).
#[must_use]
pub fn drive(inputs: Inputs) -> u8 {
    let mut y = 0;
    for n in 0..GATE_COUNT {
        if gate(bit(inputs.a, n), bit(inputs.b, n)).is_high() {
            y |= 1 << n;
        }
    }
    y
}

/// A pin's level, as [`gate`] takes its inputs and returns its output.
pub use signal::Level;

/// One 2-input exclusive-OR gate: the level its output pin is driven to.
///
/// This is `ĀB + AB̄`, the right-hand side of the equation `SDLS124` p. 1
/// prints in its description, written out (`SN7486-005`).
#[must_use]
pub const fn gate(a: Level, b: Level) -> Level {
    let (a, b) = (a.is_high(), b.is_high());
    Level::from_bool((!a && b) || (a && !b))
}

/// The level of one bit of a nibble, bit 0 at index 0, a set bit High.
#[must_use]
pub(crate) const fn bit(nibble: u8, index: usize) -> Level {
    Level::from_bool((nibble >> index) & 1 == 1)
}
