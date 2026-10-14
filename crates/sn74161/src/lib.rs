//! The Texas Instruments SN74161 synchronous 4-bit binary counter, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part is edge-triggered** (`SN74161-013`), so [`Sn74161`] holds the four
//! bits the package holds and the clock level it last saw: an edge is a change
//! between two levels and a part at its pins is only ever shown one at a time.
//!
//! **`RCO` is not state.** `SN74161-016` and `SN74161-017` make it a decode of
//! the four outputs and the `ENT` pin, with no flip-flop behind it, so
//! [`Sn74161::outputs`] decodes the four bits against the `ENT` level the part
//! was last shown, a pin level it remembers as it remembers the clock's.
//!
//! **The two restrictions the sheet places on the board are not modelled
//! here**, and nothing in this crate refuses to compute an answer for the
//! levels they forbid: `SN74161-021` and `SN74161-022` are conditions on *when
//! a board may move a pin*, and a core that is only ever shown settled levels
//! has no way to see one being broken. `SN74161-OPEN-03` is what the sheet does
//! not say about them either.
//!
//! **There are no nanoseconds here.** The setup, hold and pulse-width figures
//! on page 12 are conditions a board meets so that an edge means what
//! `SN74161-013` says it means, and this crate assumes they are met: a clear
//! released on the call an edge arrives lets the edge through
//! (`SN74161-OPEN-04`).
//!
//! # Examples
//!
//! Two rising clock edges count from power-up's 0 to 2.
//!
//! ```
//! use sn74161::{Inputs, Level, Sn74161};
//!
//! let mut part = Sn74161::default();
//! for _ in 0..2 {
//!     part.step(Inputs::default());
//!     part.step(Inputs { clk: Level::High, ..Inputs::default() });
//! }
//! assert_eq!(part.q(), 2);
//! ```

#![no_std]

/// How many pins the package presents (`SN74161-002`).
pub const PIN_COUNT: usize = 16;

/// The counter's four outputs, in the low four bits of a byte.
///
/// Bit 0 is the output the data sheet calls `QA`, which is the least
/// significant, as the part's own logic symbol weights it (`SN74161-001`).
/// Nothing above bit 3 leaves a pin, and every value this crate produces is
/// masked with this.
pub const Q_MASK: u8 = 0x0F;

/// The count the part's ripple carry decode fires on (`SN74161-016`).
pub const LAST_STATE: u8 = 0x0F;

/// A pin's level, in a pin word's field, an argument and a getter's result.
pub use signal::Level;

/// The levels the outside world holds the part's inputs at.
///
/// **A field is a level, not a condition.** `CLR` and `LOAD` are drawn with a
/// bar over them on the package (`SN74161-002`); here each field is the pin's
/// level, so that reading this needs no negation carried in the head.
///
/// There is one clock, one clear and one load for the whole package
/// rather than one per flip-flop, which is `SN74161-013`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `A`–`D`, in the low four bits, `A` at bit 0.
    pub abcd: u8,
    /// The `CLR` pin; High is the state in which the part is *not* being
    /// cleared (`SN74161-009`).
    pub clr_n: Level,
    /// The `LOAD` pin; High is the state in which the part is *not* being
    /// loaded (`SN74161-010`).
    pub load_n: Level,
    /// The `ENP` pin. It reaches the counting logic and nothing else, which is
    /// half of `SN74161-018`.
    pub enp: Level,
    /// The `ENT` pin. It reaches the carry decode as well as the counting
    /// logic, which is the other half.
    pub ent: Level,
    /// The `CLK` pin.
    pub clk: Level,
}

impl Inputs {
    /// Every control pin High and the clock Low: the part enabled to count, with
    /// no edge yet shown and nothing on the data inputs.
    ///
    /// The two active-Low pins default High because a board that is doing
    /// nothing to this part is holding them there, and a default in which the
    /// part is held cleared would read as the sheet's first mode rather than as
    /// its absence.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            abcd: 0,
            clr_n: Level::High,
            load_n: Level::High,
            enp: Level::High,
            ent: Level::High,
            clk: Level::Low,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// What the part drives, on all five output pins at once.
///
/// Both fields are always a level: the sheet gives the part no output-control
/// pin and no high-impedance state, so neither of these is an `Option`. What it
/// does grant is that `RCO` carries counting spikes (`SN74161-020`), which this
/// type cannot express.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `QA`–`QD`, in the low four bits.
    pub q: u8,
    /// The `RCO` pin (`SN74161-016`).
    pub rco: Level,
}

/// The part.
///
/// `Default` stands at power-up. The data sheet fixes no level there, so this
/// crate starts the part in its reset state: cleared, with the clock remembered
/// Low. A board that needs a known level asserts `CLR` (`SN74161-009`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Sn74161 {
    /// The four bits the package holds, in the low four bits.
    q: u8,
    /// The clock level this part was last shown.
    ///
    /// A part at its pins cannot see an edge, only two levels in succession; the
    /// edge is the change between them.
    clk: Level,
    /// The `ENT` level this part was last shown, which the terminal-count
    /// decode in [`Sn74161::outputs`] reads.
    ent: Level,
}

/// The next state of a modulo-16 binary counter (`SN74161-014`).
///
/// Every one of the sixteen values is on the one cycle (`SN74161-015`), so this
/// is total and needs no table: the part has no state to recover from.
fn next(q: u8) -> u8 {
    q.wrapping_add(1) & Q_MASK
}

impl Default for Sn74161 {
    /// Power-up, which the sheet does not fix: this crate's reset state,
    /// cleared, with `CLK` remembered Low and `ENT` High.
    fn default() -> Self {
        Self::with_state(0, Level::Low, Level::High)
    }
}

impl Sn74161 {
    /// The part with a given four bits held and given `CLK` and `ENT` levels
    /// remembered.
    ///
    /// For a caller that has to reach a state rather than clock its way to one.
    /// `q` is masked to the pins that exist; `with_state(p.q(), p.clk(),
    /// p.ent())` rebuilds `p`.
    #[must_use]
    pub const fn with_state(q: u8, clk: Level, ent: Level) -> Self {
        Self {
            q: q & Q_MASK,
            clk,
            ent,
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

    /// The `ENT` level the part was last shown.
    #[must_use]
    pub const fn ent(&self) -> Level {
        self.ent
    }

    /// What the part drives, given what it holds and the `ENT` level it was
    /// last shown.
    ///
    /// `RCO` is a decode and not a flip-flop (`SN74161-017`): the pin reaches
    /// the output with no clock edge in between, so the same count drives two
    /// different `RCO` levels under two different `ENT` levels.
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        Outputs {
            q: self.q,
            rco: Level::from_bool(self.ent.is_high() && self.q == LAST_STATE),
        }
    }

    /// Show the part one set of input levels, and read back what it drives.
    ///
    /// The branches are the four modes in the order of precedence
    /// `SN74161-008` states:
    ///
    /// - `CLR` Low clears all four whatever else is happening (`SN74161-009`),
    ///   and the clock level is still taken up, because the pin is what it is
    ///   regardless of what the part is doing about it;
    /// - on a rising edge with `CLR` High, `LOAD` Low loads the data inputs
    ///   (`SN74161-010`);
    /// - on a rising edge with `CLR` and `LOAD` High, both enables High count
    ///   (`SN74161-011`);
    /// - anything else holds (`SN74161-012`) — either enable Low, a clock
    ///   standing at either level, and a falling edge (`SN74161-013`).
    pub fn step(&mut self, inputs: Inputs) -> Outputs {
        let rising = inputs.clk.is_high() && self.clk.is_low();
        self.clk = inputs.clk;
        self.ent = inputs.ent;
        if inputs.clr_n.is_low() {
            self.q = 0;
        } else if rising {
            if inputs.load_n.is_low() {
                self.q = inputs.abcd & Q_MASK;
            } else if inputs.enp.is_high() && inputs.ent.is_high() {
                self.q = next(self.q);
            }
        }
        self.outputs()
    }
}
