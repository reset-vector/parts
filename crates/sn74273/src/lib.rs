//! The Texas Instruments SN74273 octal D-type flip-flop with clear, at its
//! pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part is edge-triggered** (`SN74273-008`), so this is a machine and not
//! a function: [`Sn74273`] holds the eight bits the package holds
//! (`SN74273-012`) and the clock level it last saw, because an edge is a change
//! between two levels and a part at its pins is only ever shown one at a time.
//!
//! **There are no nanoseconds here.** Setup, hold and pulse-width figures are
//! conditions a board meets so that an edge means what `SN74273-008` says it
//! means, and this crate assumes they are met.
//!
//! **What the crate chooses**, two open items: `Default`, the part at
//! power-up, is the part cleared (`SN74273-OPEN-03`), and a clear released on
//! the call an edge arrives lets the edge through (`SN74273-OPEN-02`).
//!
//! # Examples
//!
//! A rising clock edge stores `1D`–`8D`.
//!
//! ```
//! use sn74273::{Inputs, Level, Sn74273};
//!
//! let mut part = Sn74273::default();
//! let d = Inputs { d: 0xA5, ..Inputs::default() };
//! part.step(d);
//! assert_eq!(part.step(Inputs { clk: Level::High, ..d }).q, 0xA5);
//! ```

#![no_std]

/// How many pins the package presents (`SN74273-002`).
pub const PIN_COUNT: usize = 20;

/// The eight flip-flops of the package, as a byte.
///
/// Bit 0 is the flip-flop the data sheet numbers 1. `SN74273-001` gives the
/// part eight, so every bit of a byte is a pin and no mask is needed anywhere.
pub const FLIP_FLOP_MASK: u8 = 0xFF;

/// A pin's level, in a pin word's field, an argument and a getter's result.
pub use signal::Level;

/// The levels the outside world holds the part's inputs at.
///
/// **A field is a level, not a condition.** `CLR` is drawn with a bar over it
/// on the package (`SN74273-002`); here the field is the pin's level, so that
/// reading this needs no negation carried in the head.
///
/// There is one clock and one clear for the whole package rather than one per
/// flip-flop, which is `SN74273-011`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `1D`–`8D`, bit 0 for `1D`.
    pub d: u8,
    /// The `CLK` pin.
    pub clk: Level,
    /// The `CLR` pin; High is the state in which the part is *not* being
    /// cleared (`SN74273-010`).
    pub clr_n: Level,
}

impl Inputs {
    /// Every pin Low but `CLR`, which is High and so not clearing.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            d: 0,
            clk: Level::Low,
            clr_n: Level::High,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// What the part drives on its eight output pins.
///
/// Always a level on every pin: the part has no complemented output
/// (`SN74273-001`) and no output-control pin (`SN74273-006`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `1Q`–`8Q`, bit 0 for `1Q`.
    pub q: u8,
}

/// The part.
///
/// `Default` stands at power-up. The data sheet fixes no level there
/// (`SN74273-OPEN-03`), so this crate starts the part in its reset state:
/// cleared, with the clock remembered Low. A board that needs a known level
/// asserts `CLR` (`SN74273-010`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Sn74273 {
    /// The eight bits the package holds. `SN74273-012` says this is the whole
    /// of its state, and `clk` below is the other half of that sentence: the
    /// level, not a ninth bit of storage.
    q: u8,
    /// The clock level this part was last shown.
    ///
    /// A part at its pins cannot see an edge, only two levels in succession;
    /// the edge is the change between them. `SN74273-009` is why holding one
    /// level is enough — the trigger is a level and is not related to the
    /// transition time of the pulse, so there is no duration for this to be
    /// missing.
    clk: Level,
}

impl Default for Sn74273 {
    /// Power-up: cleared, with the clock remembered Low (`SN74273-OPEN-03`).
    fn default() -> Self {
        Self::with_state(0, Level::Low)
    }
}

impl Sn74273 {
    /// The part with a given eight bits held and a given clock level
    /// remembered.
    ///
    /// For a caller that has to reach a state rather than clock its way to
    /// one; `with_state(p.q(), p.clk())` rebuilds `p`.
    #[must_use]
    pub const fn with_state(q: u8, clk: Level) -> Self {
        Self { q, clk }
    }

    /// The eight bits the package holds, which are also its eight outputs
    /// `1Q`–`8Q`: the part has no complemented output (`SN74273-001`) and no
    /// output-control pin (`SN74273-006`), so this is a level on every output
    /// pin at all times.
    #[must_use]
    pub const fn q(&self) -> u8 {
        self.q
    }

    /// The clock level the part was last shown.
    #[must_use]
    pub const fn clk(&self) -> Level {
        self.clk
    }

    /// What the part drives, which is what it holds (`SN74273-012`).
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        Outputs { q: self.q }
    }

    /// Show the part one set of input levels, and read back what it drives.
    ///
    /// The three branches are the three documented behaviours, in the order the
    /// function table gives them:
    ///
    /// - `CLR` Low clears all eight whatever the clock and the data are doing
    ///   (`SN74273-010`), and the clock level is still taken up, because the
    ///   pin is what it is regardless of what the part is doing about it;
    /// - a clock that has gone from Low to High since the last call transfers
    ///   `D` to `Q` (`SN74273-008`);
    /// - anything else holds (`SN74273-009`) — a clock standing at either
    ///   level, and a falling edge.
    pub fn step(&mut self, inputs: Inputs) -> Outputs {
        let rising = inputs.clk.is_high() && self.clk.is_low();
        self.clk = inputs.clk;
        if inputs.clr_n.is_low() {
            self.q = 0;
        } else if rising {
            self.q = inputs.d;
        }
        self.outputs()
    }
}
