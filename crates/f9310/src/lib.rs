//! The Fairchild 9310 synchronous BCD decade counter, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The counter is a table and not an increment.** `F9310-013` gives ten states
//! in the BCD sequence and `F9310-015` gives the successor of each of the six
//! that are outside it, as the sheet's state diagram draws them; there is no
//! arithmetic that produces the second set from the first, which is why the
//! crate writes its successor table out.
//!
//! **`TC` is not state**, and is not the same decode as the binary counter's:
//! `F9310-016` puts it High only at 9 and never in 11, 13 or 15, which are the
//! three states the same book's `'160` page says it *can* be High in
//! (`F9310-024`). [`F9310::outputs`] decodes against the `CET` level the part
//! was last shown, because the pin reaches `TC` with no clock edge in between,
//! so one count drives two different `TC` levels under two different `CET`
//! levels.
//!
//! **There are no nanoseconds here.** The setup, hold, pulse-width and recovery
//! figures on the sheet are conditions a board meets so that a clock edge means
//! what the mode select table says it means, and this crate assumes they are
//! met.
//!
//! # Examples
//!
//! Two rising clock edges count from power-up's 0 to 2.
//!
//! ```
//! use f9310::{F9310, Inputs, Level};
//!
//! let mut part = F9310::default();
//! for _ in 0..2 {
//!     part.step(Inputs::default());
//!     part.step(Inputs { cp: Level::High, ..Inputs::default() });
//! }
//! assert_eq!(part.q(), 2);
//! ```

#![no_std]

/// How many pins the package presents (`F9310-002`).
pub const PIN_COUNT: usize = 16;

/// The counter's four outputs, in the low four bits of a byte.
///
/// Bit 0 is the output the data sheet calls `Q0`. `F9310-001` gives the part
/// four; nothing above bit 3 leaves a pin, and every value this crate produces
/// is masked with this.
pub const Q_MASK: u8 = 0x0F;

/// The last state of the counting sequence, on which the terminal-count decode
/// fires (`F9310-016`).
pub const LAST_STATE: u8 = 9;

/// How many states the counting sequence passes through (`F9310-013`).
pub const STATE_COUNT: usize = 10;

/// The successor of every one of the sixteen states, index by state.
///
/// Entries 0 to 9 are the BCD sequence of `F9310-013`; entries 10 to 15 are the
/// six of `F9310-015`, each an arrow on the `'10` state diagram and none of them
/// derivable from the modulus. Entry 13 is `F9310-023`: the second printing
/// draws only the diagonal to 4.
const NEXT: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 11, 4, 13, 4, 15, 0];

/// A pin's level, in a pin word, in [`F9310::with_state`] and from a getter.
pub use signal::Level;

/// The levels the outside world holds the part's inputs at.
///
/// **A field is a level, not a condition.** `MR` and `PE` are drawn with a bar
/// over them on the package (`F9310-002`), so `High` on either is the state in
/// which it does nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `P0`–`P3`, in the low four bits. All sixteen values load, six of them off
    /// the counting sequence (`F9310-009`).
    pub p: u8,
    /// The `MR` pin, High while the part is *not* being reset (`F9310-008`).
    pub mr_n: Level,
    /// The `PE` pin, High while the part is *not* being loaded (`F9310-009`).
    pub pe_n: Level,
    /// The `CEP` pin.
    pub cep: Level,
    /// The `CET` pin. It reaches the terminal-count decode as
    /// well as the counting logic, which `F9310-019` is.
    pub cet: Level,
    /// The `CP` pin.
    pub cp: Level,
}

impl Inputs {
    /// Every control pin High and the clock Low: the part enabled to count,
    /// with no edge yet shown. The active-Low pins default High because that is
    /// what a board doing nothing to this part holds them at.
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
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `Q0`–`Q3`, in the low four bits.
    pub q: u8,
    /// The `TC` pin (`F9310-016`).
    pub tc: Level,
}

/// The part.
///
/// `Default` stands at power-up. The data sheet fixes no level there, so this
/// crate starts the part in its reset state: cleared, with the clock remembered
/// Low. For this part that matters more than for the binary counter beside it:
/// `F9310-014` is written for a counter that assumes a state off its sequence
/// when power is applied, so a board that needs a known level asserts `MR`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct F9310 {
    /// The four bits the package holds, in the low four bits.
    q: u8,
    /// The clock level this part was last shown.
    cp: bool,
    /// The `CET` level this part was last shown, which the terminal-count
    /// decode in [`F9310::outputs`] reads.
    cet: bool,
}

/// The next state of the counter, off [`NEXT`].
///
/// Total over the four bits a caller can hold, which is what makes the six
/// states off the sequence ordinary here: the part is in one of sixteen states
/// and every one of them has a successor.
const fn next(q: u8) -> u8 {
    NEXT[(q & Q_MASK) as usize]
}

impl Default for F9310 {
    /// Power-up, which the sheet does not fix: this crate's reset state,
    /// cleared, with `CP` remembered Low and `CET` High.
    fn default() -> Self {
        Self::with_state(0, Level::Low, Level::High)
    }
}

impl F9310 {
    /// The part with a given four bits held and given `CP` and `CET` levels
    /// remembered.
    #[must_use]
    pub const fn with_state(q: u8, cp: Level, cet: Level) -> Self {
        Self {
            q: q & Q_MASK,
            cp: cp.is_high(),
            cet: cet.is_high(),
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
        Level::from_bool(self.cp)
    }

    /// The `CET` level the part was last shown.
    #[must_use]
    pub const fn cet(&self) -> Level {
        Level::from_bool(self.cet)
    }

    /// What the part drives, given what it holds and the `CET` level it was
    /// last shown.
    ///
    /// The decode is `F9310-016`'s: the maximum count and nothing else, so
    /// states 11, 13 and 15 drive `TC` Low however `CET` stands.
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        Outputs {
            q: self.q,
            tc: Level::from_bool(self.cet && self.q == LAST_STATE),
        }
    }

    /// Show the part one set of input levels, and read back what it drives.
    ///
    /// The branches are the mode select table's rows in the order of precedence
    /// `F9310-007` states. What differs from the 9316 is the successor table
    /// and the decode in [`F9310::outputs`].
    pub const fn step(&mut self, inputs: Inputs) -> Outputs {
        let rising = inputs.cp.is_high() && !self.cp;
        self.cp = inputs.cp.is_high();
        self.cet = inputs.cet.is_high();
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
