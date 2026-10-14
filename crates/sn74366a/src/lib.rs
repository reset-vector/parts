//! The Texas Instruments SN74366A hex bus driver with inverting three-state
//! outputs, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`SN74366A-008`), so the whole of it is
//! [`drive`]: a function from the levels the outside world holds the eight
//! input pins at to what the six output pins then do. There is no clock here
//! and no `tick`, and an output follows a moving control pin at once, with
//! nothing between (`SN74366A-OPEN-03`).
//!
//! **This crate is a reading of a logic diagram, because there is no table to
//! read.** The opening carries no function table for any of its sixteen types
//! (`SN74366A-011`), so what stands between an input pin and an output pin is
//! the drawing on printed 3-1012: a bubble on each buffer's output, and both
//! control pins entering an AND gate through a negation each.
//!
//! **An output that is off is `None` and not a level.** With either control
//! High the six outputs are in the high-impedance state
//! (`SN74366A-006`) and the part drives nothing, so there is no level to
//! report. What a board's net settles to is the board's, and this crate has
//! none.
//!
//! # Examples
//!
//! Six inverting buffers that drive while both enables are Low.
//!
//! ```
//! use sn74366a::{Inputs, Level, drive};
//!
//! let on = Inputs { a: 0b00_0001, ..Inputs::default() };
//! assert_eq!(drive(on).y, Some(0b11_1110));
//! assert_eq!(drive(Inputs { g1_n: Level::High, ..on }).y, None);
//! ```

#![no_std]

/// How many buffers the package carries (`SN74366A-001`).
pub const BUFFER_COUNT: usize = 6;

/// How many pins the package presents (`SN74366A-002`).
pub const PIN_COUNT: usize = 16;

/// The six data pins, or the six output pins, in the low six bits of a byte.
pub const BUFFER_MASK: u8 = 0b0011_1111;

/// The levels the outside world holds the part's eight input pins at.
///
/// **A field is a level, not a condition.** The two control pins are drawn with
/// bars on the package (`SN74366A-002`), so `High` on either is the state in
/// which the part is **not** enabled.
///
/// [`Inputs::default`] holds both control pins Low, so the part drives.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// The six data pins `A1` to `A6`, `A1` in bit 0 — the same order as
    /// [`Outputs::y`], because a buffer drives its own output and no other
    /// (`SN74366A-010`).
    pub a: u8,
    /// The `Ḡ1` pin, pin 1.
    pub g1_n: Level,
    /// The `Ḡ2` pin, pin 15. It is not a qualifier on `g1_n`:
    /// `SN74366A-009` makes the two pins interchangeable and
    /// `SN74366A-007` is the one enable they produce between them.
    pub g2_n: Level,
}

impl Inputs {
    /// Every data pin Low and both control pins Low: the part driving.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            a: 0,
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

/// What the part does at its six output pins.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// The six output pins, `Y1` in bit 0, when the part is driving them; and
    /// `None` when it is not.
    ///
    /// It is one `Option` for all six rather than six of them because the six
    /// share one enable line (`SN74366A-006`): they go off together or not at
    /// all, and a per-buffer option would be six copies of one bit.
    pub y: Option<u8>,
}

/// What the part drives, given the levels held on its eight input pins.
///
/// Two branches, in the order the drawing settles them: the enable first
/// (`SN74366A-007`), and the six inversions under it (`SN74366A-005`). Nothing
/// here is timed — what an output does *while* a control pin is moving is
/// `SN74366A-OPEN-03` and is on no page of the opening.
#[must_use]
pub fn drive(inputs: Inputs) -> Outputs {
    if !enabled(inputs.g1_n, inputs.g2_n) {
        return Outputs { y: None };
    }
    let mut y = 0;
    for index in 0..BUFFER_COUNT {
        if gate(bit(inputs.a, index)).is_high() {
            y |= 1 << index;
        }
    }
    Outputs { y: Some(y) }
}

/// Whether the six outputs are driving: both control pins Low, and neither one
/// alone (`SN74366A-007`).
///
/// This is the AND gate of printed 3-1012 with its two input negations, written
/// out.
const fn enabled(g1_n: Level, g2_n: Level) -> bool {
    g1_n.is_low() && g2_n.is_low()
}

/// A pin's level: a control pin's, and a buffer's input and driven output, as
/// [`gate`] takes and returns them.
pub use signal::Level;

/// One buffer's driven level, the per-gate function of [`drive`]: the complement of its data pin (`SN74366A-005`).
#[must_use]
pub const fn gate(a: Level) -> Level {
    Level::from_bool(a.is_low())
}

/// The level of one bit of a byte, bit 0 at index 0, a set bit High.
#[must_use]
pub(crate) const fn bit(byte: u8, index: usize) -> Level {
    Level::from_bool((byte >> index) & 1 == 1)
}
