//! The Texas Instruments SN7417 hex buffer and driver with open-collector
//! outputs, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`SN7417-010`), so the whole of it is
//! [`drive`]: a function from the levels the outside world holds the six
//! input pins at to what the part does to the six output pins. There is no
//! clock here and no `tick`, because the data sheet specifies no edge for one
//! to correspond to.
//!
//! **An output is the level it leaves a pulled-up line at.** `SDLS032H` p. 8
//! draws each output pin as the collector of a transistor with nothing between
//! it and the supply (`SN7417-006`), and its function table's High row reads
//! `High-Z` (`SN7417-004`): the part sinks the pin or lets go of it, and what a
//! pin that has been let go of sits at is whatever the board pulls it to — up
//! to 15 V by the sheet's own rating (`SN7417-007`). So a clear bit is a pin
//! sunk Low and a set bit a pin released, whose High is the pull-up's at
//! whatever voltage, 15 V or above it (`SN7417-OPEN-02`); no voltage is
//! anywhere in this crate. The pull-up belongs to the consumer.
//!
//! **This crate is a reading of the function table**, because the sheet's
//! equation `Y = A` (`SN7417-005`) is a function of levels and says nothing
//! about the High. An input pin is High or Low and nothing between
//! (`SN7417-OPEN-04`).
//!
//! # Examples
//!
//! Six open-collector buffers: a set bit is an output released to the pull-up.
//!
//! ```
//! use sn7417::{Inputs, drive};
//!
//! assert_eq!(drive(Inputs { a: 0b10_1010 }), 0b10_1010);
//! ```

#![no_std]

/// How many buffers the package holds (`SN7417-001`).
pub const BUFFER_COUNT: usize = 6;

/// The six buffers of the package, in the low six bits of a byte.
///
/// Bit 0 is the buffer the data sheet numbers 1. Nothing above bit 5 leaves a
/// pin, and every value this crate produces is masked with this.
pub const BUFFER_MASK: u8 = 0x3F;

/// How many pins the package presents (`SN7417-002`).
pub const PIN_COUNT: usize = 14;

/// A pin's level, as [`gate`] takes its input and returns its output. At an
/// output, `Low` is the transistor on and the pin sunk; `High` the transistor
/// off and the pin released, so the High is the pull-up's, with no voltage or
/// current in it (`SN7417-004`, `SN7417-OPEN-02`).
pub use signal::Level;

/// The level one buffer leaves its pulled-up output pin at, given the level
/// held on its input (`SN7417-004`, `SN7417-006`).
///
/// Low in is [`Level::Low`] out: the output
/// transistor is on and the pin is sunk to ground, within the `VOL` the sheet
/// guarantees at the load's current (`SN7417-009`). High in is [`Level::High`]
/// out: the transistor is off and
/// the pin is left to the load, the sheet's `High-Z`, so the High is the
/// pull-up's.
#[must_use]
pub const fn gate(a: Level) -> Level {
    a
}

/// The levels the outside world holds the part's six input pins at.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `1A`–`6A`, bit 0 for `1A` through bit 5 for `6A`, per [`BUFFER_MASK`]; a set
    /// bit is a High pin, and no level lies between High and Low
    /// (`SN7417-OPEN-04`).
    pub a: u8,
}

impl Inputs {
    /// Every input pin Low.
    #[must_use]
    pub const fn idle() -> Self {
        Self { a: 0 }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// The levels the part leaves `1Y`–`6Y` at, given the levels held on
/// `1A`–`6A`, one bit per buffer per [`BUFFER_MASK`]: a clear bit is a pin
/// sunk Low, a set bit a pin released to the pull-up.
///
/// Each output is [`gate`] of its own input and of nothing else
/// (`SN7417-011`); bits above [`BUFFER_MASK`] on the way in reach nothing.
#[must_use]
pub fn drive(inputs: Inputs) -> u8 {
    let mut y = 0;
    for n in 0..BUFFER_COUNT {
        if gate(bit(inputs.a, n)).is_high() {
            y |= 1 << n;
        }
    }
    y
}

/// The level of one bit of a byte, bit 0 at index 0, a set bit High.
#[must_use]
pub(crate) const fn bit(byte: u8, index: usize) -> Level {
    Level::from_bool((byte >> index) & 1 == 1)
}
