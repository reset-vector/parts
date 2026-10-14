//! The Intel 8216 four-bit parallel bi-directional bus driver, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`I8216-015`), so the whole of it is [`drive`]:
//! a function from the levels the outside world holds the input pins at to what
//! the part drives back. There is no clock here and no `tick`, because the data
//! sheet specifies no edge for one to correspond to.
//!
//! # Examples
//!
//! ```
//! use i8216::{Inputs, Level, drive};
//!
//! // Selected with `DIEN` Low: `DI` drives `DB`.
//! let inputs = Inputs {
//!     di: 0x5,
//!     cs_n: Level::Low,
//!     dien_n: Level::Low,
//!     ..Inputs::default()
//! };
//! let outputs = drive(inputs);
//! assert_eq!((outputs.db, outputs.dout), (Some(0x5), None));
//! // Deselected, both groups float.
//! assert_eq!(drive(Inputs::default()).db, None);
//! ```

#![no_std]

pub use signal::Level;

/// The four buffers of the package, one bit each, in the low four bits of a
/// byte: the four lines of a `DI`, `DO` or `DB` group.
///
/// `I8216-002` gives the part four of each. Nothing above bit 3 leaves a pin,
/// and every value this crate produces is masked with this.
pub const BUFFER_MASK: u8 = 0x0F;

/// The levels the outside world holds the part's inputs at.
///
/// **A field is a level, not a condition.** `DIEN` and `CS` are drawn with a bar
/// over them on the package (`I8216-002`), so their fields end `_n`.
/// [`Inputs::default`] holds both High, the part deselected, and every data
/// line Low.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `DI0`–`DI3`, in the low four bits.
    pub di: u8,
    /// `DB0`–`DB3` as the outside world drives them, in the low four bits.
    ///
    /// What a board puts here while the part is itself driving `DB` is the
    /// board's business: `I8216-014` says the part's two buffers never contend,
    /// and says nothing about contention with anything else.
    pub db: u8,
    /// The `DIEN` pin.
    pub dien_n: Level,
    /// The `CS` pin.
    pub cs_n: Level,
}

impl Inputs {
    /// The part idle; see [`Inputs`].
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            di: 0,
            db: 0,
            dien_n: Level::High,
            cs_n: Level::High,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// What the part drives back, one entry per three-state group.
///
/// `None` is the high-impedance state — `I8216-011` for both groups at once,
/// `I8216-014` for whichever of the two the direction leaves idle.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Outputs {
    /// `DB0`–`DB3`, in the low four bits, or nothing.
    pub db: Option<u8>,
    /// `DO0`–`DO3`, in the low four bits, or nothing.
    pub dout: Option<u8>,
}

/// The whole part.
///
/// `I8216-011` takes the first branch, `I8216-013` chooses between the other
/// two, and `I8216-014` is why exactly one group is ever `Some`. The values pass
/// through unchanged because this is the 8216 and not the 8226 (`I8216-007`).
#[must_use]
pub const fn drive(inputs: Inputs) -> Outputs {
    if inputs.cs_n.is_high() {
        return Outputs {
            db: None,
            dout: None,
        };
    }
    if inputs.dien_n.is_high() {
        Outputs {
            db: None,
            dout: Some(inputs.db & BUFFER_MASK),
        }
    } else {
        Outputs {
            db: Some(inputs.di & BUFFER_MASK),
            dout: None,
        }
    }
}
