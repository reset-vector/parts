//! The Texas Instruments SN74175 quadruple D-type flip-flop with clear, at its
//! pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part is edge-triggered** (`SN74175-009`), so unlike `i8216` this is a
//! machine and not a function: [`Sn74175`] holds the four bits the package holds
//! (`SN74175-014`) and the clock level it last saw, because an edge is a change
//! between two levels and a part at its pins is only ever shown one at a time.
//!
//! **There are no nanoseconds here.** Setup, hold and pulse-width figures are
//! conditions a board meets so that an edge means what `SN74175-009` says it
//! means, and this crate assumes they are met.
//!
//! **What the crate chooses**, two open items: an input pin is High or Low
//! and nothing between (`SN74175-OPEN-04`), and a clear released on the call
//! an edge arrives lets the edge through (`SN74175-OPEN-02`).
//!
//! # Examples
//!
//! A rising clock edge stores `1D`–`4D`, and each `Q̄` is its `Q`'s complement.
//!
//! ```
//! use sn74175::{Inputs, Level, Sn74175};
//!
//! let mut part = Sn74175::default();
//! let d = Inputs { d: 0b0110, ..Inputs::default() };
//! part.step(d);
//! let out = part.step(Inputs { clk: Level::High, ..d });
//! assert_eq!((out.q, out.q_bar), (0b0110, 0b1001));
//! ```

#![no_std]

/// How many pins the package presents (`SN74175-002`).
pub const PIN_COUNT: usize = 16;

/// The four flip-flops of the package, in the low four bits of a byte.
///
/// Bit 0 is the flip-flop the data sheet numbers 1. `SN74175-001` gives the part
/// four; nothing above bit 3 leaves a pin, and every value this crate produces
/// is masked with this.
pub const FLIP_FLOP_MASK: u8 = 0x0F;

/// A pin's level, in a pin word's field, an argument and a getter's result.
pub use signal::Level;

/// The levels the outside world holds the part's inputs at.
///
/// **A field is a level, not a condition.** `CLR` is drawn with a bar over it on
/// the package (`SN74175-002`); here the field is the pin's level, so that
/// reading this needs no negation carried in the head. No level lies between
/// High and Low (`SN74175-OPEN-04`).
///
/// There is one clock and one clear for the whole package rather than one per
/// flip-flop, which is `SN74175-013`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `1D`–`4D`, in the low four bits.
    pub d: u8,
    /// The `CLK` pin.
    pub clk: Level,
    /// The `CLR` pin; High is the state in which the part is *not* being
    /// cleared (`SN74175-012`).
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

/// What the part drives, on all eight output pins at once.
///
/// Both fields are always a level: the outputs are totem poles with no
/// output-control pin and no high-impedance state (`SN74175-007`), which is why
/// neither of these is an `Option`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `1Q`–`4Q`, in the low four bits.
    pub q: u8,
    /// `1Q̄`–`4Q̄`, in the low four bits.
    pub q_bar: u8,
}

/// The part.
///
/// `Default` stands at power-up. The data sheet fixes no level there, so this
/// crate starts the part in its reset state: cleared, with the clock remembered
/// Low. A board that needs a known level asserts `CLR` (`SN74175-012`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Sn74175 {
    /// The four bits the package holds, in the low four bits. `SN74175-014`
    /// says this is the whole of its state, and `clk` below is the other half
    /// of that sentence: the level, not a fifth bit of storage.
    q: u8,
    /// The clock level this part was last shown.
    ///
    /// A part at its pins cannot see an edge, only two levels in succession;
    /// the edge is the change between them. `SN74175-010` is why holding one
    /// level is enough — the trigger is a level and is not related to the
    /// transition time of the pulse, so there is no duration for this to be
    /// missing.
    clk: Level,
}

impl Default for Sn74175 {
    /// Power-up: cleared, with the clock remembered Low.
    fn default() -> Self {
        Self::with_state(0, Level::Low)
    }
}

impl Sn74175 {
    /// The part with a given four bits held and a given clock level remembered.
    ///
    /// For a caller that has to reach a state rather than clock its way to
    /// one. `q` is masked to the pins that exist; `with_state(p.q(), p.clk())`
    /// rebuilds `p`.
    #[must_use]
    pub const fn with_state(q: u8, clk: Level) -> Self {
        Self {
            q: q & FLIP_FLOP_MASK,
            clk,
        }
    }

    /// The four bits the package holds.
    #[must_use]
    pub const fn q(&self) -> u8 {
        self.q
    }

    /// The clock level the part was last shown.
    #[must_use]
    pub const fn clk(&self) -> Level {
        self.clk
    }

    /// What the part drives, given what it holds.
    ///
    /// `SN74175-001` brings out both outputs of every flip-flop and
    /// `SN74175-014` takes each `Q̄` from its own flip-flop, so in every state
    /// this crate can be in the two are complements. That they are complements
    /// *while a flip-flop is changing* is not claimed and is
    /// `SN74175-OPEN-01`.
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        Outputs {
            q: self.q,
            q_bar: !self.q & FLIP_FLOP_MASK,
        }
    }

    /// Show the part one set of input levels, and read back what it drives.
    ///
    /// The three branches are the three documented behaviours, in the order the
    /// function table gives them:
    ///
    /// - `CLR` low clears all four whatever the clock and the data are doing
    ///   (`SN74175-012`), and the clock level is still taken up, because the pin
    ///   is what it is regardless of what the part is doing about it;
    /// - a clock that has gone from low to high since the last call transfers
    ///   `D` to `Q` (`SN74175-009`);
    /// - anything else holds (`SN74175-011`) — a clock standing at either level,
    ///   and a falling edge.
    pub fn step(&mut self, inputs: Inputs) -> Outputs {
        let rising = inputs.clk.is_high() && self.clk.is_low();
        self.clk = inputs.clk;
        if inputs.clr_n.is_low() {
            self.q = 0;
        } else if rising {
            self.q = inputs.d & FLIP_FLOP_MASK;
        }
        self.outputs()
    }
}
