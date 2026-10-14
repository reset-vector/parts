//! The Texas Instruments SN74174 hex D-type flip-flop with clear, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part is edge-triggered** (`SN74174-011`), so this is a machine and not
//! a function: [`Sn74174`] holds the six bits the package holds
//! (`SN74174-016`) and the clock level it last saw, because an edge is a change
//! between two levels and a part at its pins is only ever shown one at a time.
//!
//! **There are no nanoseconds here**, and no rate either. Setup, hold and
//! pulse-width figures are conditions a board meets so that an edge means what
//! `SN74174-011` says it means, and this crate assumes they are met.
//! `SN74174-017` gives the part a specified lower bound of DC, so counting
//! edges and not time is a reading of the sheet rather than a simplification
//! of it.
//!
//! **What the crate chooses**, two open items: an input pin is High or Low
//! and nothing between (`SN74174-OPEN-03`), and a clear released on the call
//! an edge arrives lets the edge through (`SN74174-OPEN-01`).
//!
//! # Examples
//!
//! A rising clock edge stores `1D`–`6D`.
//!
//! ```
//! use sn74174::{Inputs, Level, Sn74174};
//!
//! let mut part = Sn74174::default();
//! let d = Inputs { d: 0b10_1010, ..Inputs::default() };
//! part.step(d);
//! assert_eq!(part.step(Inputs { clk: Level::High, ..d }).q, 0b10_1010);
//! ```

#![no_std]

/// How many pins the package presents (`SN74174-002`).
pub const PIN_COUNT: usize = 16;

/// The six flip-flops of the package, in the low six bits of a byte.
///
/// Bit 0 is the flip-flop the data sheet numbers 1. `SN74174-001` gives the part
/// six; nothing above bit 5 leaves a pin, and every value this crate produces is
/// masked with this.
pub const FLIP_FLOP_MASK: u8 = 0x3F;

/// A pin's level, in a pin word, in [`Sn74174::with_state`] and from a getter.
pub use signal::Level;

/// The levels the outside world holds the part's inputs at.
///
/// **A field is a level, not a condition.** `CLR` is drawn with a bar over it on
/// the package (`SN74174-002`), so `High` on it is the state in which the
/// part is **not** being cleared. No level lies between High and Low
/// (`SN74174-OPEN-03`).
///
/// There is one clock and one clear for the whole package rather than one per
/// flip-flop, which is `SN74174-015`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `1D`–`6D`, in the low six bits.
    pub d: u8,
    /// The `CLK` pin.
    pub clk: Level,
    /// The `CLR` pin, High while the part is *not* being cleared
    /// (`SN74174-014`).
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

/// What the part drives, on all six output pins at once.
///
/// One field, because `SN74174-001` gives this part a single rail: the `'175`'s
/// complemented outputs are the thing the sheet separates the two functions by,
/// and there is no pin here to carry them. The field is always a level — the
/// outputs are totem poles with no output-control pin and no high-impedance
/// state (`SN74174-009`), which is why it is not an `Option`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `1Q`–`6Q`, in the low six bits.
    pub q: u8,
}

/// The part.
///
/// `Default` stands at power-up. The data sheet fixes no level there, so this
/// crate starts the part in its reset state: cleared, with the clock remembered
/// Low. A board that needs a known level asserts `CLR` (`SN74174-014`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Sn74174 {
    /// The six bits the package holds, in the low six bits. `SN74174-016` says
    /// this is the whole of its state, and `clk` below is the other half of that
    /// sentence: the level, not a seventh bit of storage.
    q: u8,
    /// The clock level this part was last shown.
    ///
    /// A part at its pins cannot see an edge, only two levels in succession; the
    /// edge is the change between them. `SN74174-012` is why holding one level
    /// is enough — the trigger is a level and is not related to the transition
    /// time of the pulse, so there is no duration for this to be missing.
    clk: bool,
}

impl Sn74174 {
    /// The part with a given six bits held and a given clock level remembered.
    ///
    /// For a caller that has to reach a state rather than clock its way to one.
    /// `q` is masked to the pins that exist.
    #[must_use]
    pub const fn with_state(q: u8, clk: Level) -> Self {
        Self {
            q: q & FLIP_FLOP_MASK,
            clk: clk.is_high(),
        }
    }

    /// The six bits the package holds.
    #[must_use]
    pub const fn q(&self) -> u8 {
        self.q
    }

    /// The clock level the part was last shown.
    #[must_use]
    pub const fn clk(&self) -> Level {
        Level::from_bool(self.clk)
    }

    /// What the part drives, given what it holds.
    ///
    /// `SN74174-016` makes the six bits and the six pins the same six things, so
    /// this is the held value and no computation.
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        Outputs { q: self.q }
    }

    /// Show the part one set of input levels, and read back what it drives.
    ///
    /// The three branches are the three documented behaviours, in the order the
    /// function table gives them:
    ///
    /// - `CLR` low clears all six whatever the clock and the data are doing
    ///   (`SN74174-014`), and the clock level is still taken up, because the pin
    ///   is what it is regardless of what the part is doing about it;
    /// - a clock that has gone from low to high since the last call transfers
    ///   `D` to `Q` (`SN74174-011`);
    /// - anything else holds (`SN74174-013`) — a clock standing at either level,
    ///   and a falling edge.
    pub fn step(&mut self, inputs: Inputs) -> Outputs {
        let rising = inputs.clk.is_high() && !self.clk;
        self.clk = inputs.clk.is_high();
        if inputs.clr_n.is_low() {
            self.q = 0;
        } else if rising {
            self.q = inputs.d & FLIP_FLOP_MASK;
        }
        self.outputs()
    }
}
