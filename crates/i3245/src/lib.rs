//! The Intel 3245 quad TTL-to-MOS driver, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`I3245-011`), so the whole of it is [`drive`].
//! An output here is a logic level; that its High is a swing to within half a
//! volt of the +12 V rail (`I3245-006`) is what the part is for and is not
//! modelled.
//!
//! # Examples
//!
//! ```
//! use i3245::{Inputs, Level, drive};
//!
//! // Enabled, `C` Low, `I1` Low: driver 1 drives High.
//! let inputs = Inputs { i_n: 0b1110, c_n: Level::Low, ..Inputs::default() };
//! assert_eq!(drive(inputs).o, 0b0001);
//! // `C` High holds every output Low.
//! assert_eq!(drive(Inputs::default()).o, 0);
//! ```

#![no_std]

pub use signal::Level;

/// How many pins the package presents (`I3245-002`).
pub const PIN_COUNT: usize = 16;

/// The four drivers, in the low four bits of a byte: bit 0 is driver 1.
///
/// Nothing above bit 3 leaves a pin, and every value this crate produces is
/// masked with this.
pub const DRIVER_MASK: u8 = 0x0F;

/// The levels the outside world holds the part's inputs at.
///
/// **A field is a level, not a condition.** Every input is drawn with a bar
/// over it (`I3245-002`). [`Inputs::default`] holds the enables `E1` and `E2` Low and every other
/// input High, so the part is enabled, every select and `C` are deasserted and
/// refresh is idle.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `I1`–`I4`, the select inputs, in the low four bits: bit 0 is `I1`.
    pub i_n: u8,
    /// The `R` pin, refresh select.
    pub r_n: Level,
    /// The `C` pin, clock control.
    pub c_n: Level,
    /// The `E1` pin.
    pub e1_n: Level,
    /// The `E2` pin.
    pub e2_n: Level,
}

impl Inputs {
    /// `E1` and `E2` Low and every other input High.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            i_n: DRIVER_MASK,
            r_n: Level::High,
            c_n: Level::High,
            e1_n: Level::Low,
            e2_n: Level::Low,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// The levels the part drives.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `O1`–`O4` in the low four bits, a set bit High: bit 0 is `O1`.
    pub o: u8,
}

/// The whole part.
///
/// `I3245-009` is the first branch; `I3245-010` is the rest, and the output is
/// not inverted, so a Low select gives a set bit.
#[must_use]
pub const fn drive(inputs: Inputs) -> Outputs {
    let o = if inputs.e1_n.is_high() || inputs.e2_n.is_high() || inputs.c_n.is_high() {
        0
    } else if inputs.r_n.is_high() {
        !inputs.i_n & DRIVER_MASK
    } else {
        DRIVER_MASK
    };
    Outputs { o }
}
