//! The Fairchild 9316 synchronous 4-bit binary counter, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part is edge-triggered** (`F9316-012`), so [`F9316`] holds the four
//! bits the package holds and the clock level it last saw: an edge is a change
//! between two levels and a part at its pins is only ever shown one at a time.
//!
//! **`TC` is not state.** `F9316-015` and `F9316-016` make it a decode of the
//! four outputs and the `CET` pin, with no flip-flop behind it, so
//! [`F9316::outputs`] decodes the four bits against the `CET` level the part
//! was last shown, a pin level it remembers as it remembers the clock's.
//!
//! **There are no nanoseconds here.** The setup, hold, pulse-width and recovery
//! figures on printed 6-35 are conditions a board meets so that an edge means
//! what `F9316-012` says it means, and this crate assumes they are met.
//!
//! # Examples
//!
//! Two rising clock edges count from power-up's 0 to 2.
//!
//! ```
//! use f9316::{F9316, Inputs, Level};
//!
//! let mut part = F9316::default();
//! for _ in 0..2 {
//!     part.step(Inputs::default());
//!     part.step(Inputs { cp: Level::High, ..Inputs::default() });
//! }
//! assert_eq!(part.q(), 2);
//! ```

#![no_std]

/// How many pins the package presents (`F9316-002`).
pub const PIN_COUNT: usize = 16;

/// The counter's four outputs, in the low four bits of a byte.
///
/// Bit 0 is the output the data sheet calls `Q0`. `F9316-001` gives the part
/// four; nothing above bit 3 leaves a pin, and every value this crate produces
/// is masked with this.
pub const Q_MASK: u8 = 0x0F;

/// The count the part's terminal-count decode fires on (`F9316-015`).
pub const LAST_STATE: u8 = 0x0F;

/// A pin's level, in a pin word's field, an argument and a getter's result.
pub use signal::Level;

/// The levels the outside world holds the part's inputs at.
///
/// **A field is a level, not a condition.** `MR` and `PE` are drawn with a bar
/// over them on the package (`F9316-002`); here each field is the pin's level,
/// so that reading this needs no negation carried in the head.
///
/// There is one clock, one reset and one parallel enable for the whole package
/// rather than one per flip-flop, which is `F9316-012`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `P0`–`P3`, in the low four bits.
    pub p: u8,
    /// The `MR` pin; High is the state in which the part is *not* being reset
    /// (`F9316-008`).
    pub mr_n: Level,
    /// The `PE` pin; High is the state in which the part is *not* being loaded
    /// (`F9316-009`).
    pub pe_n: Level,
    /// The `CEP` pin.
    pub cep: Level,
    /// The `CET` pin. It reaches the terminal-count decode as well as the
    /// counting logic, which `F9316-018` is.
    pub cet: Level,
    /// The `CP` pin.
    pub cp: Level,
}

impl Inputs {
    /// Every control pin High and the clock Low: the part enabled to count, with
    /// no edge yet shown and nothing on the parallel inputs.
    ///
    /// The two active-Low pins default High because a board that is doing
    /// nothing to this part is holding them there, and a default in which the
    /// part is held in reset would read as the sheet's first mode rather than
    /// as its absence.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            p: 0,
            mr_n: Level::High,
            pe_n: Level::High,
            cep: Level::High,
            cet: Level::High,
            cp: Level::Low,
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
/// pin and no high-impedance state, so neither of these is an `Option`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `Q0`–`Q3`, in the low four bits.
    pub q: u8,
    /// The `TC` pin (`F9316-015`).
    pub tc: Level,
}

/// The part.
///
/// `Default` stands at power-up. The data sheet fixes no level there, so this
/// crate starts the part in its reset state: cleared, with the clock remembered
/// Low. A board that needs a known level asserts `MR` (`F9316-008`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct F9316 {
    /// The four bits the package holds, in the low four bits.
    q: u8,
    /// The clock level this part was last shown.
    ///
    /// A part at its pins cannot see an edge, only two levels in succession;
    /// the edge is the change between them. What the part does with an input
    /// that moves *between* two of them is `F9316-019`, which this crate does
    /// not model.
    cp: Level,
    /// The `CET` level this part was last shown, which the terminal-count
    /// decode in [`F9316::outputs`] reads.
    cet: Level,
}

/// The next state of a modulo-16 binary counter (`F9316-013`).
///
/// Every one of the sixteen values is on the one cycle (`F9316-014`), so this is
/// total and needs no table: the part has no state to recover from.
const fn next(q: u8) -> u8 {
    ((q & Q_MASK) + 1) & Q_MASK
}

impl Default for F9316 {
    /// Power-up, which the sheet does not fix: this crate's reset state,
    /// cleared, with `CP` remembered Low and `CET` High.
    fn default() -> Self {
        Self::with_state(0, Level::Low, Level::High)
    }
}

impl F9316 {
    /// The part with a given four bits held and given `CP` and `CET` levels
    /// remembered.
    ///
    /// For a caller that has to reach a state rather than clock its way to one.
    /// `q` is masked to the pins that exist; `with_state(p.q(), p.cp(),
    /// p.cet())` rebuilds `p`.
    #[must_use]
    pub const fn with_state(q: u8, cp: Level, cet: Level) -> Self {
        Self {
            q: q & Q_MASK,
            cp,
            cet,
        }
    }

    /// The four bits the package holds.
    #[must_use]
    pub const fn q(&self) -> u8 {
        self.q
    }

    /// The clock level the part was last shown.
    #[must_use]
    pub const fn cp(&self) -> Level {
        self.cp
    }

    /// The `CET` level the part was last shown.
    #[must_use]
    pub const fn cet(&self) -> Level {
        self.cet
    }

    /// What the part drives, given what it holds and the `CET` level it was
    /// last shown.
    ///
    /// `TC` is a decode and not a flip-flop (`F9316-016`): the pin reaches the
    /// output with no clock edge in between, so the same count drives two
    /// different `TC` levels under two different `CET` levels.
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        Outputs {
            q: self.q,
            tc: Level::from_bool(self.cet.is_high() && self.q == LAST_STATE),
        }
    }

    /// Show the part one set of input levels, and read back what it drives.
    ///
    /// The branches are the mode select table's rows in the order of precedence
    /// `F9316-007` states:
    ///
    /// - `MR` Low clears all four whatever else is happening (`F9316-008`), and
    ///   the clock level is still taken up, because the pin is what it is
    ///   regardless of what the part is doing about it;
    /// - on a rising edge with `MR` High, `PE` Low loads the parallel inputs
    ///   (`F9316-009`);
    /// - on a rising edge with `MR` and `PE` High, both enables High count
    ///   (`F9316-010`);
    /// - anything else holds (`F9316-011`) — either enable Low, a clock standing
    ///   at either level, and a falling edge (`F9316-012`).
    pub const fn step(&mut self, inputs: Inputs) -> Outputs {
        let rising = inputs.cp.is_high() && self.cp.is_low();
        self.cp = inputs.cp;
        self.cet = inputs.cet;
        if inputs.mr_n.is_low() {
            self.q = 0;
        } else if rising {
            if inputs.pe_n.is_low() {
                self.q = inputs.p & Q_MASK;
            } else if inputs.cep.is_high() && inputs.cet.is_high() {
                self.q = next(self.q);
            }
        }
        self.outputs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_ignores_the_bits_above_the_outputs() {
        assert_eq!(next(0xFF), 0);
        assert_eq!(next(0xF3), 4);
    }
}
