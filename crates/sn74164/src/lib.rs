//! The Texas Instruments SN74164 8-bit parallel-out serial shift register, at
//! its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part is edge-triggered** (`SN74164-006`), so this is a machine and not
//! a function: [`Sn74164`] holds the eight bits the package holds and the clock
//! level it last saw, because an edge is a change between two levels and a part
//! at its pins is only ever shown one at a time.
//!
//! **Every bit it holds reaches a pin** (`SN74164-015`), so [`Sn74164::step`]
//! returns the whole of the part's state and there is nothing inside it a
//! caller cannot see. That is what separates this register from the
//! parallel-load one in the `sn74166` crate, which brings out one stage of eight.
//!
//! **Two of these cascade by wiring, not by code.** The eighth stage's old
//! contents leave the part on every edge (`SN74164-007`), so a caller that
//! drives a second register's serial inputs from the first's `Q_H` has a
//! sixteen-stage register. Nothing about a board is in this crate: where the
//! clock comes from and what reads the outputs are somebody else's concern.
//!
//! **There are no nanoseconds here.** The setup, hold and pulse-width figures
//! on p. 3-624 of TI's *The TTL Data Book, Volume 2* (1985) are conditions a
//! board meets so that an edge means what `SN74164-006` says it means, and this
//! crate assumes they are met: the serial inputs are taken as they stand at
//! the edge (`SN74164-OPEN-03`), and a clear released on the call an edge
//! arrives lets the edge through (`SN74164-OPEN-06`). `SN74164-026` gives the part a specified lower
//! bound of DC, so counting edges and not time is a reading of the sheet rather
//! than a simplification of it. `Default` is the part cleared
//! (`SN74164-OPEN-02`).
//!
//! # Examples
//!
//! A rising clock edge with `A` and `B` High shifts a 1 into `QA`.
//!
//! ```
//! use sn74164::{Inputs, Level, Sn74164};
//!
//! let mut part = Sn74164::default();
//! let one = Inputs { a: Level::High, b: Level::High, ..Inputs::default() };
//! part.step(one);
//! assert_eq!(part.step(Inputs { clk: Level::High, ..one }).q, 0b0000_0001);
//! ```

#![no_std]

/// How many pins the package presents (`SN74164-002`).
pub const PIN_COUNT: usize = 14;

/// How many flip-flops the register has (`SN74164-001`).
pub const STAGE_COUNT: usize = 8;

/// A pin's level, in a pin word, in [`Sn74164::with_state`] and from a getter.
pub use signal::Level;

/// The levels the outside world holds the part's four input pins at.
///
/// **A field is a level, not a condition.** `CLR` is the one name on the
/// package drawn with a bar (`SN74164-005`), so `High` on it is the state in
/// which the part is **not** being cleared.
///
/// There is one clock and one clear for the whole register rather than one per
/// stage, and no parallel input anywhere (`SN74164-001`).
///
/// [`Inputs::default`] holds `CLR` High, so `Inputs { a, b, clk,
/// ..Default::default() }` shifts and does not clear.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// The `A` pin, pin 1 — one of the two gated serial inputs.
    pub a: Level,
    /// The `B` pin, pin 2 — the other. It is not a qualifier on `a`:
    /// `SN74164-008` makes the bit entering the first stage the conjunction of
    /// the two, so the two pins are interchangeable and either can gate the
    /// other.
    pub b: Level,
    /// The `CLK` pin, pin 8. What moves the register is this
    /// pin going from Low to High since the last call (`SN74164-006`).
    pub clk: Level,
    /// The `CLR` pin, pin 9, High while the part is *not* being cleared
    /// (`SN74164-012`).
    pub clr_n: Level,
}

impl Inputs {
    /// The serial inputs and the clock Low, and `CLR` High.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            a: Level::Low,
            b: Level::Low,
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
/// Always a byte — the outputs are a totem pole with no output-control pin
/// and no high-impedance state (`SN74164-016`), which is why this is not an
/// `Option`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `Q_A` in bit 0 through `Q_H` in bit 7.
    pub q: u8,
}

/// The part.
///
/// `Default` stands at power-up. The data sheet fixes no level there — p.
/// 3-622 draws the outputs as dashed lines until the first clear and no page
/// says what they are (`SN74164-021`) — so this crate starts the part in its
/// reset state: cleared, with the clock remembered Low (`SN74164-OPEN-02`).
/// A board that needs a known level asserts `CLR`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Sn74164 {
    /// The eight bits the package holds: `Q_A` in bit 0 through `Q_H` in bit 7.
    /// All eight are at pins (`SN74164-015`), so this is both the state and the
    /// output.
    q: u8,
    /// The level of `CLK` this part was last shown.
    ///
    /// A part at its pins cannot see an edge, only two levels in succession; the
    /// edge is the change between them. It is not a ninth bit of storage and no
    /// pin reports it.
    clk: bool,
}

impl Sn74164 {
    /// The part with a given eight bits held and a given clock level
    /// remembered.
    ///
    /// For a caller that has to reach a state rather than clock its way to
    /// one. Unlike the parallel-load register's, this constructor is a
    /// convenience and not a necessity: every state of this part is reachable
    /// from a clear in eight edges, and every bit of it is visible at a pin.
    #[must_use]
    pub const fn with_state(q: u8, clk: Level) -> Self {
        Self {
            q,
            clk: clk.is_high(),
        }
    }

    /// The eight bits the package holds, `Q_A` in bit 0 through `Q_H` in bit 7.
    #[must_use]
    pub const fn q(&self) -> u8 {
        self.q
    }

    /// The `CLK` level the part was last shown.
    #[must_use]
    pub const fn clk(&self) -> Level {
        Level::from_bool(self.clk)
    }

    /// What the part drives on its eight output pins, which is what it holds
    /// and no computation (`SN74164-015`).
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        Outputs { q: self.q }
    }

    /// Show the part one set of input levels, and read back what it drives.
    ///
    /// The branches are the function table's rows on p. 3-621, in the
    /// order it gives them:
    ///
    /// - `CLR` Low clears all eight stages whatever every other pin is doing
    ///   (`SN74164-012`, `SN74164-014`), and the clock level is still taken up,
    ///   because the pins are what they are regardless of what the part is
    ///   doing about them;
    /// - a rising edge on `CLK` shifts every stage into the next, drops what
    ///   the eighth held (`SN74164-007`) and enters the conjunction of the two
    ///   serial inputs into the first (`SN74164-008`);
    /// - anything else holds — a clock at either level, and a falling edge
    ///   (`SN74164-010`).
    ///
    /// The clear branch comes first because the function table marks the clock
    /// irrelevant on its clear row, including transitions. How long before an
    /// edge the clear has to be released is on no page of the data sheet
    /// (`SN74164-OPEN-06`), so a call that carries both a released clear and a
    /// rising edge takes the edge.
    pub fn step(&mut self, inputs: Inputs) -> Outputs {
        let rising = inputs.clk.is_high() && !self.clk;
        self.clk = inputs.clk.is_high();
        if inputs.clr_n.is_low() {
            self.q = 0;
        } else if rising {
            self.q = (self.q << 1) | u8::from(inputs.a.is_high() && inputs.b.is_high());
        }
        self.outputs()
    }
}
