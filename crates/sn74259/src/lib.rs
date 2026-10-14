//! The Texas Instruments SN74259 8-bit addressable latch, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part is level-sensitive and has no clock.** A clocked 74-series part
//! modelled at its pins remembers the level its clock pin was last shown,
//! because an edge is a change between two levels. This one has
//! no edge to see: a latch follows its data input while it is addressed and
//! enabled (`SN74259-009`), and holds what it last followed once it is not. So
//! [`Sn74259`] holds the eight bits and nothing else (`SN74259-016`), and every
//! call to [`Sn74259::step`] is a reading of the steady state the pins are in.
//!
//! **This crate is the logic symbol.** The opening prints no logic diagram; the
//! function table on 3-864 states four modes and the ANSI/IEEE symbol on 3-863
//! states, per latch, when its data is enabled and when its reset is asserted
//! (`SN74259-013`). An output is always its latch's state and never a path
//! from the data pin.
//!
//! **There are no nanoseconds here.** The setup, hold and pulse-width figures
//! on 3-865 are conditions a board meets so that releasing the enable captures
//! what `SN74259-009` says it captures, and this crate assumes they are met.
//! Every call is one steady state, so two control pins that move together are
//! seen at once (`SN74259-OPEN-03`), and an address that changes under the
//! enable writes the newly addressed latch and leaves the one before holding
//! (`SN74259-OPEN-02`). `Default` is the part cleared (`SN74259-OPEN-04`).
//!
//! # Examples
//!
//! `G` Low writes `D` into the latch `S0`–`S2` address.
//!
//! ```
//! use sn74259::{Inputs, Level, Sn74259};
//!
//! let mut part = Sn74259::default();
//! part.step(Inputs { s: 3, d: Level::High, g_n: Level::Low, ..Inputs::default() });
//! assert_eq!(part.step(Inputs::default()).q, 0b0000_1000);
//! ```

#![no_std]

/// How many pins the package presents (`SN74259-002`).
pub const PIN_COUNT: usize = 16;

/// How many latches the package holds (`SN74259-001`).
pub const LATCH_COUNT: usize = 8;

/// The eight latches of the package, as a byte. Bit `i` is latch `i`, as the
/// data sheet numbers them, `Q0` at bit 0.
pub const LATCH_MASK: u8 = 0xFF;

/// The three select pins `S0` to `S2`, `S0` in bit 0, in the low three bits of
/// a byte (`SN74259-014`).
pub const S_MASK: u8 = 0x07;

/// A pin's level, in a pin word's field.
pub use signal::Level;

/// The levels the outside world holds the part's inputs at.
///
/// **A field is a level, not a condition.** `Ḡ` and `CLR̄` are drawn with bars
/// on the package (`SN74259-002`); here each field is the pin's level, so that
/// reading this needs no negation carried in the head. The part is enabled with
/// `g_n` Low and cleared with `clr_n` Low.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `S0`–`S2`, `S0` at bit 0: which latch is addressed (`SN74259-014`).
    /// Bits above the three pins do not exist and are masked off.
    pub s: u8,
    /// The `D` pin.
    pub d: Level,
    /// The `Ḡ` pin; High is the state in which no latch is addressed
    /// (`SN74259-010`).
    pub g_n: Level,
    /// The `CLR̄` pin; High is the state in which the part is *not* being
    /// cleared (`SN74259-011`).
    pub clr_n: Level,
}

impl Inputs {
    /// Both active-Low pins High and everything else Low: the memory mode,
    /// which is what a board doing nothing to this part is holding it in.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            s: 0,
            d: Level::Low,
            g_n: Level::High,
            clr_n: Level::High,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

impl Inputs {
    /// The latch the address pins name, 0 to 7 (`SN74259-014`).
    fn addressed(self) -> usize {
        usize::from(self.s & S_MASK)
    }
}

/// What the part drives on its eight output pins.
///
/// Always a level on every pin: every output is its latch's state
/// (`SN74259-016`) and a totem pole with no output-control pin
/// (`SN74259-006`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `Q0`–`Q7`, `Q0` at bit 0.
    pub q: u8,
}

/// The part.
///
/// `Default` stands at power-up. The data sheet fixes no level there
/// (`SN74259-OPEN-04`), so this crate starts the part in its reset state: all
/// eight latches cleared. A board that needs a known level asserts `CLR̄`
/// (`SN74259-011`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Sn74259 {
    /// The eight bits the package holds, which are also its eight outputs
    /// (`SN74259-016`). There is no remembered clock level, because there is
    /// no clock.
    q: u8,
}

impl Sn74259 {
    /// The part with a given eight bits held, for a caller that has to reach
    /// a state.
    #[must_use]
    pub const fn with_state(q: u8) -> Self {
        Self { q }
    }

    /// The eight bits the package holds, which are the levels on `Q0`–`Q7`:
    /// every output is its latch's state (`SN74259-016`) and every output is a
    /// totem pole with no output-control pin (`SN74259-006`), so this is a
    /// level on every output pin at all times.
    #[must_use]
    pub const fn q(&self) -> u8 {
        self.q
    }

    /// What the part drives, which is what it holds (`SN74259-016`).
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        Outputs { q: self.q }
    }

    /// Show the part one set of input levels, and read back what it drives.
    ///
    /// This is the logic symbol's two dependencies per latch (`SN74259-013`),
    /// evaluated for all eight at once:
    ///
    /// - a latch's data input is enabled when `Ḡ` is Low and the address
    ///   names it, and an enabled latch follows `D`;
    /// - a latch's reset is asserted when `CLR̄` is Low and the latch is not
    ///   addressed under the enable — so with `Ḡ` High no latch is addressed
    ///   and the clear resets all eight.
    ///
    /// Read against the function table that is: the addressed latch follows
    /// `D` and the others hold (`SN74259-009`); everything holds
    /// (`SN74259-010`); the addressed latch follows `D` and the others are
    /// Low (`SN74259-012`); everything is Low (`SN74259-011`).
    pub fn step(&mut self, inputs: Inputs) -> Outputs {
        let addressed = if inputs.g_n.is_high() {
            None
        } else {
            Some(inputs.addressed())
        };
        for latch in 0..LATCH_COUNT {
            let mask = 1u8 << latch;
            if addressed == Some(latch) {
                if inputs.d.is_high() {
                    self.q |= mask;
                } else {
                    self.q &= !mask;
                }
            } else if inputs.clr_n.is_low() {
                self.q &= !mask;
            }
        }
        self.outputs()
    }
}
