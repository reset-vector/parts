//! The Texas Instruments SN74166 parallel-load 8-bit shift register, at its
//! pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part is edge-triggered** (`SN74166-012`), so this is a machine and not
//! a function: [`Sn74166`] holds the eight bits the package holds
//! (`SN74166-022`) and the clock level it last saw, because an edge is a change
//! between two levels and a part at its pins is only ever shown one at a time.
//!
//! **The level it remembers is not a pin.** `SN74166-015` makes the clock the
//! OR of two pins, so what one edge means depends on where that OR was last
//! time and on neither pin alone.
//!
//! **There are no nanoseconds here**, and no rate either. Setup, hold and
//! pulse-width figures are conditions a board meets so that an edge means what
//! `SN74166-012` says it means, and this crate assumes they are met.
//! `SN74166-023` gives the part a specified lower bound of DC, so counting
//! edges and not time is a reading of the sheet rather than a simplification
//! of it.
//!
//! **What the crate chooses**, two open items: an input pin is High or Low
//! and nothing between (`SN74166-OPEN-03`), and a clear released on the call
//! an edge arrives lets the edge through (`SN74166-OPEN-01`).
//!
//! # Examples
//!
//! A rising clock edge with `SH/LD` Low loads `A`–`H`; `QH` shows `H`.
//!
//! ```
//! use sn74166::{Inputs, Level, Sn74166};
//!
//! let mut part = Sn74166::default();
//! let load = Inputs { abcdefgh: 0x80, sh_ld: Level::Low, ..Inputs::default() };
//! part.step(load);
//! assert_eq!(part.step(Inputs { clk: Level::High, ..load }).q_h, Level::High);
//! ```

#![no_std]

/// How many pins the package presents (`SN74166-002`).
pub const PIN_COUNT: usize = 16;

/// How many flip-flops the register has (`SN74166-001`).
pub const STAGE_COUNT: usize = 8;

/// A pin's level, in a pin word's field, an argument and a getter's result.
pub use signal::Level;

/// The levels the outside world holds the part's inputs at.
///
/// **A field is a level, not a condition.** `CLR` and the `LD` half of `SH/LD`
/// are drawn with bars over them on the package (`SN74166-002`); here each field
/// is the pin's level, so that reading this needs no negation carried in the
/// head. No level lies between High and Low (`SN74166-OPEN-03`).
///
/// There is one of each control for the whole register rather than one per
/// stage, which is `SN74166-021`.
///
/// [`Inputs::default`] holds every pin Low but `CLR` and `SH/LD`, which are
/// High: neither clearing nor loading.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// The `SER` pin, pin 1 — the bit a shift moves into the first stage
    /// (`SN74166-016`), and nothing at all while `sh_ld` is Low
    /// (`SN74166-019`).
    pub ser: Level,
    /// `A`–`H`, the broadside inputs, `A` in bit 0 and `H` in bit 7 — the same
    /// order as [`Sn74166::q`], because a load puts one into the other
    /// (`SN74166-018`).
    pub abcdefgh: u8,
    /// The `CLK` pin.
    pub clk: Level,
    /// The `CLK INH` pin. It is not a qualifier on `clk`:
    /// `SN74166-013` makes the two pins interchangeable and `SN74166-015` is
    /// the one event they produce between them.
    pub clk_inh: Level,
    /// The `SH/LD` pin; High selects a shift, Low a load (`SN74166-016`, `SN74166-018`). It carries no `_n`: the pin picks
    /// between two actions, each of them active at one level.
    pub sh_ld: Level,
    /// The `CLR` pin; High is the state in which the part is *not* being
    /// cleared (`SN74166-020`).
    pub clr_n: Level,
}

impl Inputs {
    /// Every pin Low but `SH/LD` and `CLR`.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            ser: Level::Low,
            abcdefgh: 0,
            clk: Level::Low,
            clk_inh: Level::Low,
            sh_ld: Level::High,
            clr_n: Level::High,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// What the part drives, which is one pin.
///
/// One field, because `SN74166-001` brings out the last stage and no other:
/// seven of the eight bits the part holds reach no pin, and a board that wants
/// them clocks them along to this one. They are still the part's, and
/// [`Sn74166::q`] is where they are.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `Q_H`, pin 13. Always a level — the output is a totem pole with no
    /// output-control pin and no high-impedance state (`SN74166-009`), which is
    /// why it is not an `Option`.
    pub q_h: Level,
}

/// The part.
///
/// `Default` stands at power-up. The data sheet fixes no level there, so this
/// crate starts the part in its reset state: cleared, with the clock remembered
/// Low. A board that needs a known level asserts `CLR` (`SN74166-020`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Sn74166 {
    /// The eight bits the package holds: `Q_A` in bit 0 through `Q_H` in bit 7.
    /// `SN74166-022` says this is the whole of its state, and `clk` below is
    /// the other half of that sentence: a level, not a ninth bit of storage.
    q: u8,
    /// The level of the OR of `CLK` and `CLK INH` this part was last shown.
    ///
    /// A part at its pins cannot see an edge, only two levels in succession; the
    /// edge is the change between them. What is remembered is the OR rather than
    /// either pin because `SN74166-015` is what the two of them produce, and the
    /// same fact is why `SN74166-014` is an instruction to a board rather than a
    /// property of the part.
    clk: Level,
}

impl Default for Sn74166 {
    /// Power-up: cleared, with the clock remembered Low.
    fn default() -> Self {
        Self::with_state(0, Level::Low)
    }
}

impl Sn74166 {
    /// The part with a given eight bits held and a given clock level remembered.
    ///
    /// For a caller that has to reach a state rather than clock its way to one,
    /// which for this part is most of them: seven of the eight bits are not at a
    /// pin. `clk` is the level of the OR of `CLK` and `CLK INH`
    /// (`SN74166-015`); `with_state(p.q(), p.internal_clock_level())` rebuilds
    /// `p`.
    #[must_use]
    pub const fn with_state(q: u8, clk: Level) -> Self {
        Self { q, clk }
    }

    /// The eight bits the package holds, `Q_A` in bit 0 through `Q_H` in bit 7.
    ///
    /// Seven of them are the `INTERNAL OUTPUTS` the data sheet's own function
    /// table names, and they are inside the part: its boundary is the
    /// package (`SN74166-022`).
    #[must_use]
    pub const fn q(&self) -> u8 {
        self.q
    }

    /// The level of the OR of `CLK` and `CLK INH` the part was last shown
    /// (`SN74166-015`), which no pin carries.
    #[must_use]
    pub const fn internal_clock_level(&self) -> Level {
        self.clk
    }

    /// What the part drives, given what it holds.
    ///
    /// The one output pin is the eighth stage and no computation
    /// (`SN74166-001`).
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        Outputs {
            q_h: Level::from_bool(self.q & 0x80 != 0),
        }
    }

    /// Show the part one set of input levels, and read back what it drives.
    ///
    /// The branches are the function table's rows, in the order it gives them:
    ///
    /// - `CLR` Low clears all eight whatever every other pin is doing
    ///   (`SN74166-020`), and the clock level is still taken up, because the
    ///   pins are what they are regardless of what the part is doing about them;
    /// - an edge — the OR of the two clock pins having gone from Low to High
    ///   since the last call (`SN74166-015`) — does one of two things, and
    ///   `SH/LD` is what picks: a shift, which moves each stage into the next
    ///   and `SER` into the first (`SN74166-016`), or a load, which takes all
    ///   eight parallel pins at once (`SN74166-018`);
    /// - anything else holds — a clock at either level, and a falling edge.
    ///
    /// The shift drops the eighth stage's old contents, there being nothing
    /// drawn past it (`SN74166-016`), and `SER` reaches nothing on the load
    /// branch, which is `SN74166-019` being an absence of code rather than a
    /// line of it.
    pub fn step(&mut self, inputs: Inputs) -> Outputs {
        let clock = Level::from_bool(inputs.clk.is_high() || inputs.clk_inh.is_high());
        let rising = clock.is_high() && self.clk.is_low();
        self.clk = clock;
        if inputs.clr_n.is_low() {
            self.q = 0;
        } else if rising {
            self.q = if inputs.sh_ld.is_high() {
                (self.q << 1) | u8::from(inputs.ser.is_high())
            } else {
                inputs.abcdefgh
            };
        }
        self.outputs()
    }
}
