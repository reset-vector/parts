//! The Texas Instruments SN74H55 expandable 2-wide 4-input AND-OR-INVERT gate,
//! at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`SN74H55-009`), so the whole of it is [`drive`]:
//! a function from the levels the outside world holds the eight input pins at,
//! and from what is wired to the expander pair, to the [`Outputs`] the part
//! drives back, its one pin `Y`.
//!
//! **This crate is a reading of the equation, and the expander is a term.**
//! Printed 3-237 gives the function as `Y = ¬(ABCD + EFGH + X)` with `X` the
//! output of an expander package (`SN74H55-005`). The two expander pins are
//! nodes of the phase splitter and carry currents, not levels (`SN74H55-010`,
//! `SN74H55-016`); a crate at the pins has no current to put on them, so
//! [`Expander`] is what the caller declares the third product to be — open,
//! or a term that is true or false. What an expander presents there, and at
//! what level of it the term is true, is `SN74H55-OPEN-03`.
//!
//! # Examples
//!
//! `A`–`D` all High drives `Y` Low; an expander term does too.
//!
//! ```
//! use sn74h55::{Expander, Inputs, Level, drive};
//!
//! let y = |abcdefgh, expander| drive(Inputs { abcdefgh, expander }).y;
//! assert_eq!(y(0x0F, Expander::Open), Level::Low);
//! assert_eq!(y(0x77, Expander::Open), Level::High);
//! assert_eq!(y(0, Expander::Term(true)), Level::Low);
//! ```

#![no_std]

/// How many input pins the gate has, in two groups of four (`SN74H55-001`).
pub const INPUT_COUNT: usize = 8;

/// How many pins the package presents (`SN74H55-002`).
pub const PIN_COUNT: usize = 14;

/// The one pin of the 14-pin assignment that is connected to nothing
/// (`SN74H55-002`).
pub const NO_CONNECT_PINS: [u8; 1] = [6];

/// The two expander pins, `X` and `X̄` (`SN74H55-002`).
pub const EXPANDER_PINS: [u8; 2] = [5, 9];

/// The eight inputs as bits of a byte: `A` is bit 0 through `H` at bit 7.
pub const ABCDEFGH_MASK: u8 = 0xFF;

/// The first AND group, `A`, `B`, `C`, `D`.
pub const ABCD_MASK: u8 = 0x0F;

/// The second AND group, `E`, `F`, `G`, `H`.
pub const EFGH_MASK: u8 = 0xF0;

/// What is wired to the expander pair.
///
/// `Open` is the condition the sheet tests the part under (`SN74H55-006`) and
/// contributes no term. `Term` is a third product a caller declares — what an
/// `SN74H60` wired to the pair would be asserting — and is a reading of the
/// equation's `X`, not of a level at either pin.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Expander {
    /// Pins 5 and 9 left open.
    #[default]
    Open,
    /// An expander wired to the pair, and whether it asserts the term: a
    /// condition, not a level at either pin.
    Term(bool),
}

impl Expander {
    /// The third product of the equation: false when the pair is open.
    #[must_use]
    const fn term(self) -> bool {
        match self {
            Self::Open => false,
            Self::Term(x) => x,
        }
    }
}

/// Whether every pin of a group is High.
#[must_use]
pub(crate) const fn group_high(inputs: u8, group: u8) -> bool {
    inputs & group == group
}

/// The levels the outside world holds the part's eight inputs at, and what is
/// wired to its expander pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `A`–`H`, a level per bit per [`ABCDEFGH_MASK`].
    pub abcdefgh: u8,
    /// What is wired to pins 5 and 9.
    pub expander: Expander,
}

impl Inputs {
    /// Every input Low and the expander pair open.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            abcdefgh: 0,
            expander: Expander::Open,
        }
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
    /// The `Y` pin (`SN74H55-002`).
    pub y: Level,
}

/// What the part's one gate drives on `Y`, given the levels held on the eight
/// inputs and what is wired to the expander pair.
///
/// `Y = ¬(ABCD + EFGH + X)` (`SN74H55-005`): Low when either group is all High
/// or the expander term is true, High otherwise. Bits above [`ABCDEFGH_MASK`] do not
/// exist and there are none.
#[must_use]
pub const fn drive(inputs: Inputs) -> Outputs {
    let a = inputs.abcdefgh;
    Outputs {
        y: Level::from_bool(
            !(group_high(a, ABCD_MASK) || group_high(a, EFGH_MASK) || inputs.expander.term()),
        ),
    }
}
