//! The Fairchild 9322 quad 2-input multiplexer, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`F9322-013`), so the whole of it is [`drive`]:
//! a function from the levels the outside world holds the input pins at to the
//! four levels the part drives back. There is no clock here and no `tick`,
//! because the data sheet specifies no edge for one to correspond to.
//!
//! # Examples
//!
//! `S` Low passes the `I0` inputs, High the `I1` inputs.
//!
//! ```
//! use f9322::{Inputs, Level, drive};
//!
//! let inputs = Inputs { i0: 0b0101, i1: 0b1010, ..Inputs::default() };
//! assert_eq!(drive(inputs).z, 0b0101);
//! assert_eq!(drive(Inputs { s: Level::High, ..inputs }).z, 0b1010);
//! ```

#![no_std]

/// How many pins the package presents (`F9322-002`).
pub const PIN_COUNT: usize = 16;

/// A pin's level, in a pin word's field, an argument and a getter's result.
pub use signal::Level;

/// The four multiplexer channels, in the low four bits of a byte.
///
/// Bit 0 is channel `a`, bit 1 `b`, bit 2 `c`, bit 3 `d` — the lettering of
/// `F9322-002`, and **not** the order the letters run round the package, which
/// is `F9322-003`. Nothing above bit 3 leaves a pin and every value this crate
/// produces is masked with this.
pub const SELECTOR_MASK: u8 = 0x0F;

/// The levels the outside world holds the part's inputs at.
///
/// **A field is a level, not a condition.** The enable is drawn `Ē` on the
/// package (`F9322-007`); here each field is the pin's level, so that reading
/// this needs no negation carried in the head.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `I0a`–`I0d`, one bit per channel per [`SELECTOR_MASK`].
    pub i0: u8,
    /// `I1a`–`I1d`, one bit per channel per [`SELECTOR_MASK`].
    pub i1: u8,
    /// The `S` pin. One select for all four channels (`F9322-006`).
    pub s: Level,
    /// The `E` pin. One enable for all four channels, and the bar on it is
    /// `F9322-007`.
    pub e_n: Level,
}

impl Inputs {
    /// Every data pin, the select and the enable Low, enabling the part: every
    /// output follows its `I0` input.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            i0: 0,
            i1: 0,
            s: Level::Low,
            e_n: Level::Low,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// What the part drives on its outputs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `Za`–`Zd`, one bit per channel per [`SELECTOR_MASK`], a set bit High.
    pub z: u8,
}

/// What the part drives on `Za`–`Zd`.
///
/// `F9322-008` takes the first branch and `F9322-009` and `F9322-010` choose
/// between the other two. The bits pass through unchanged because the outputs
/// are true rather than complemented (`F9322-011`), and each bit is decided by
/// its own channel's two inputs alone (`F9322-012`).
#[must_use]
pub const fn drive(inputs: Inputs) -> Outputs {
    let z = if inputs.e_n.is_high() {
        0
    } else {
        (if inputs.s.is_high() {
            inputs.i1
        } else {
            inputs.i0
        }) & SELECTOR_MASK
    };
    Outputs { z }
}
