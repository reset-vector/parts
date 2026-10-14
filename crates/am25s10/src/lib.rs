//! The Advanced Micro Devices Am25S10 four-bit shifter, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`AM25S10-008`), so the whole of it is [`drive`]:
//! a function from the levels the outside world holds the input pins at to what
//! the part drives back. There is no clock here and no `tick`, because the data
//! sheet specifies no edge for one to correspond to.
//!
//! # Examples
//!
//! ```
//! use am25s10::{Inputs, Level, drive};
//!
//! // Shift by one place: Y1 takes I0, which is bit 3 of `i`.
//! let out = drive(Inputs { i: 0b000_1000, s: 1, oe_n: Level::Low });
//! assert_eq!(out.y, Some(0b0010));
//!
//! // OE High floats all four outputs.
//! assert_eq!(drive(Inputs { oe_n: Level::High, ..Inputs::default() }).y, None);
//! ```

#![no_std]

pub use signal::Level;

/// How many pins the package presents (`AM25S10-002`).
pub const PIN_COUNT: usize = 16;

/// The seven data inputs, in the low seven bits of a byte.
///
/// `AM25S10-002` names them `I-3` to `I3`. **Bit `n` of a data value is input
/// `I(n - 3)`**: bit 0 is `I-3`, bit 3 is `I0` and bit 6 is `I3`. The part's own
/// numbering runs through zero and a bit position does not, so one offset has to
/// live somewhere; it lives here, in [`I_INDEX_OFFSET`], and nothing else in this crate
/// carries a signed index.
pub const I_MASK: u8 = 0x7F;

/// The four outputs `Y0` to `Y3`, in the low four bits of a byte, bit `k` being
/// `Yk`.
pub const Y_MASK: u8 = 0x0F;

/// The two select inputs as a two-bit number, `S1` in bit 1 and `S0` in bit 0.
///
/// `AM25S10-005` reads the select field as the number of places shifted, which
/// is what this masks.
pub const S_MASK: u8 = 0x03;

/// What is added to an `I` input's own index to reach its bit position in
/// [`Inputs::i`]: input `I(n - I_INDEX_OFFSET)` sits in bit `n`.
pub const I_INDEX_OFFSET: usize = 3;

/// The levels the outside world holds the part's inputs at.
///
/// **A field is a level, not a condition.** The enable is drawn with a bar over
/// it on the package (`AM25S10-002`). [`Inputs::default`] holds it Low, so the
/// part drives.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `I-3` to `I3`, in the low seven bits — see [`I_MASK`] for which bit is
    /// which input.
    pub i: u8,
    /// `S1` and `S0`, in the low two bits — see [`S_MASK`].
    pub s: u8,
    /// The `OE` pin.
    pub oe_n: Level,
}

impl Inputs {
    /// Every data and select pin Low and the enable Low: the part driving.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            i: 0,
            s: 0,
            oe_n: Level::Low,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// What the part does at its four output pins.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `Y0` to `Y3`, in the low four bits per [`Y_MASK`], or `None`, the
    /// high-impedance state.
    ///
    /// One `Option` covers all four outputs because `AM25S10-012` says the
    /// part has no per-output control, so there is no state of the pins this
    /// type cannot express and none it can express that the part cannot reach.
    pub y: Option<u8>,
}

/// The whole part: the four outputs together, or nothing.
///
/// `AM25S10-010` takes the first branch and `AM25S10-005` is the second. The
/// values pass through uninverted (`AM25S10-007`).
#[must_use]
pub fn drive(inputs: Inputs) -> Outputs {
    if inputs.oe_n.is_high() {
        return Outputs { y: None };
    }
    let places = usize::from(inputs.s & S_MASK);
    let data = inputs.i & I_MASK;
    let mut y = 0u8;
    for k in 0..4usize {
        let bit = k + I_INDEX_OFFSET - places;
        y |= ((data >> bit) & 1) << k;
    }
    Outputs {
        y: Some(y & Y_MASK),
    }
}
