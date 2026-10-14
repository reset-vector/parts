//! The Texas Instruments SN74153 dual 4-line to 1-line data
//! selector/multiplexer, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`SN74153-010`), so the whole of it is
//! [`drive`]: a function from the levels the outside world holds the input pins
//! at to the two levels the part drives back. There is no clock here and no
//! `tick`, because the data sheet specifies no edge for one to correspond to.
//!
//! # Examples
//!
//! `A`/`B` select input 2 of each section; only section 1's is High.
//!
//! ```
//! use sn74153::{Inputs, drive};
//!
//! let inputs = Inputs { c1: 0b0100, c2: 0b1011, ab: 2, ..Inputs::default() };
//! assert_eq!(drive(inputs).y, 0b01);
//! ```

#![no_std]

/// How many pins the package presents (`SN74153-002`).
pub const PIN_COUNT: usize = 16;

/// A pin's level, in a pin word's field, an argument and a getter's result.
pub use signal::Level;

/// The two selectors, in the low two bits of a byte.
///
/// Bit 0 is section 1 and bit 1 section 2 — the numbering of `SN74153-002`, and
/// **not** where those sections sit round the package, which is `SN74153-003`.
/// Nothing above bit 1 leaves a pin and every value this crate produces is
/// masked with this.
pub const SELECTOR_MASK: u8 = 0x03;

/// One section's four data inputs, in the low four bits of a byte.
///
/// Bit `n` is `Cn`, so bit 0 is `C0` and bit 3 is `C3`. That is the order the
/// select value indexes them in (`SN74153-007`) and **not** the order they run
/// round the package, which is `SN74153-002`.
pub const C_MASK: u8 = 0x0F;

/// The levels the outside world holds the part's inputs at.
///
/// **A field is a level, not a condition.** The strobes are drawn `Ḡ` on the
/// package (`SN74153-005`); here each field is the pin's level, so that
/// reading this needs no negation carried in the head.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `1C0`–`1C3`, one bit per input per [`C_MASK`].
    pub c1: u8,
    /// `2C0`–`2C3`, one bit per input per [`C_MASK`].
    pub c2: u8,
    /// The `A` and `B` select pins, common to both sections, `A` in bit 0 and
    /// `B` in bit 1, a set bit High: the select value as a two-bit number
    /// (`SN74153-004`). Bits above 1 are no pin and are ignored.
    pub ab: u8,
    /// The `1G` pin. Section 1's own strobe (`SN74153-005`).
    pub g1_n: Level,
    /// The `2G` pin. Section 2's own strobe.
    pub g2_n: Level,
}

impl Inputs {
    /// Every data and select pin Low and both strobes Low, enabling both
    /// sections: each output follows its `C0`.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            c1: 0,
            c2: 0,
            ab: 0,
            g1_n: Level::Low,
            g2_n: Level::Low,
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
    /// `1Y` and `2Y`, one bit per selector per [`SELECTOR_MASK`], a set bit
    /// High.
    pub y: u8,
}

/// What the part drives on `1Y` and `2Y`.
///
/// Each section is decided by its own four data inputs and its own strobe
/// (`SN74153-009`) together with the shared select pair. `SN74153-006` is a
/// strobe High, which drives the section Low, and `SN74153-007` a strobe Low,
/// which selects one data input; the bit passes through unchanged because
/// the output presents the selected input true (`SN74153-008`).
#[must_use]
pub fn drive(inputs: Inputs) -> Outputs {
    let mut y = 0;
    if section(inputs.c1, inputs.ab, inputs.g1_n) {
        y |= 1;
    }
    if section(inputs.c2, inputs.ab, inputs.g2_n) {
        y |= 2;
    }
    Outputs { y }
}

/// One section: the level its `Y` pin is driven to.
///
/// The select value is `B` over `A` as a two-bit number, which is the weighting
/// the logic symbol prints and the function table's column order (`SN74153-004`).
fn section(c: u8, ab: u8, g_n: Level) -> bool {
    if g_n.is_high() {
        return false;
    }
    let selected = ab & 0b11;
    (c >> selected) & 1 == 1
}
