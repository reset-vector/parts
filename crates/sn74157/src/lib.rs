//! The Texas Instruments SN74157 quadruple 2-line to 1-line data
//! selector/multiplexer, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`SN74157-011`), so the whole of it is
//! [`drive`]: a function from the levels the outside world holds the input pins
//! at to the four levels the part drives back. There is no clock here and no
//! `tick`, because the data sheet specifies no edge for one to correspond to.
//!
//! # Examples
//!
//! `A/B` Low passes the `A` inputs, High the `B` inputs.
//!
//! ```
//! use sn74157::{Inputs, Level, drive};
//!
//! let inputs = Inputs { a: 0b0101, b: 0b1010, ..Inputs::default() };
//! assert_eq!(drive(inputs).y, 0b0101);
//! assert_eq!(drive(Inputs { a_b: Level::High, ..inputs }).y, 0b1010);
//! ```

#![no_std]

/// How many pins the package presents (`SN74157-002`).
pub const PIN_COUNT: usize = 16;

/// A pin's level, in a pin word's field, an argument and a getter's result.
pub use signal::Level;

/// The four selector channels, in the low four bits of a byte.
///
/// Bit 0 is channel 1, bit 3 channel 4 — the numbering of `SN74157-002`, and
/// **not** the order the numbers run round the package, which is `SN74157-003`.
/// Nothing above bit 3 leaves a pin and every value this crate produces is
/// masked with this.
pub const SELECTOR_MASK: u8 = 0x0F;

/// The levels the outside world holds the part's inputs at.
///
/// **A field is a level, not a condition.** The strobe is drawn `Ḡ` on the
/// package (`SN74157-005`); here each field is the pin's level, so that
/// reading this needs no negation carried in the head.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `1A`–`4A`, one bit per channel per [`SELECTOR_MASK`].
    pub a: u8,
    /// `1B`–`4B`, one bit per channel per [`SELECTOR_MASK`].
    pub b: u8,
    /// The `A/B` pin. One select for all four channels (`SN74157-004`), and
    /// the bar over its `A` is `SN74157-007`.
    pub a_b: Level,
    /// The `G` pin. One strobe for all four channels, and the bar on it is
    /// `SN74157-005`.
    pub g_n: Level,
}

impl Inputs {
    /// Every data pin and the select Low and the strobe Low, enabling the
    /// part: every output follows its `A` input.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            a: 0,
            b: 0,
            a_b: Level::Low,
            g_n: Level::Low,
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
    /// `1Y`–`4Y`, one bit per channel per [`SELECTOR_MASK`], a set bit High.
    pub y: u8,
}

/// What the part drives on `1Y`–`4Y`.
///
/// `SN74157-006` takes the first branch and `SN74157-007` and `SN74157-008`
/// choose between the other two. The bits pass through unchanged because this
/// is the `'157` and not the `'LS158` (`SN74157-009`), and each bit is decided
/// by its own channel's two inputs alone (`SN74157-010`).
#[must_use]
pub const fn drive(inputs: Inputs) -> Outputs {
    let y = if inputs.g_n.is_high() {
        0
    } else {
        (if inputs.a_b.is_high() {
            inputs.b
        } else {
            inputs.a
        }) & SELECTOR_MASK
    };
    Outputs { y }
}
