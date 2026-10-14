//! The Texas Instruments SN7474 dual D-type positive-edge-triggered flip-flop
//! with preset and clear, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part is edge-triggered** (`SN7474-008`), so this is a machine and not a
//! function: [`FlipFlop`] holds the one bit the sheet gives it and the clock
//! level it last saw, because an edge is a change between two levels and a part
//! at its pins is only ever shown one at a time.
//!
//! **This crate is a reading of the function table and the description, not of
//! the logic diagram.** `SDLS119` p. 1 draws one flip-flop as six three-input
//! NAND gates (`SN7474-021`) with three feedback loops in them, so what they
//! settle to depends on an order of settling the page does not give.
//!
//! **There are no nanoseconds here**, and no rate either. Setup, hold and
//! pulse-width figures are conditions a board meets so that an edge means what
//! `SN7474-008` says it means, and this crate assumes they are met: `D` is
//! taken as it stands on the call an edge arrives (`SN7474-OPEN-06`). Unlike the
//! `'174`, this part's own pages state no minimum clock frequency at all
//! (`SN7474-027`), so counting edges and not time is what is left rather than a
//! reading of a `DC` row.
//!
//! **What the sheet does not say, this crate does not say either.** [`step`]
//! returns [`Outputs`] whose pins are [`Logic::Undetermined`] in two places: at
//! power-up, which the sheet gives no state (`SN7474-OPEN-03`), until `PRE`,
//! `CLR` or a clock edge sets a level; and after the part leaves the
//! configuration in which both asynchronous inputs are low by raising both of
//! them together (`SN7474-OPEN-04`).
//!
//! [`step`]: FlipFlop::step
//!
//! # Examples
//!
//! A rising clock edge stores `D`, fixing a `Q` power-up left undetermined.
//!
//! ```
//! use sn7474::{Inputs, Level, Logic, Sn7474};
//!
//! let mut part = Sn7474::default();
//! assert_eq!(part.flip_flop(0).q(), Logic::Undetermined);
//! let d = Inputs { d: Level::High, ..Inputs::default() };
//! part.step([d, Inputs::default()]);
//! let out = part.step([Inputs { clk: Level::High, ..d }, Inputs::default()]);
//! assert_eq!((out[0].q, out[0].q_bar), (Logic::High, Logic::Low));
//! ```

#![no_std]

/// How many flip-flops the package carries (`SN7474-001`).
pub const FLIP_FLOP_COUNT: usize = 2;

/// How many pins the package presents (`SN7474-002`).
pub const PIN_COUNT: usize = 14;

/// The levels the outside world holds one flip-flop's four input pins at.
///
/// **A field is a level, not a condition.** `PRE` and `CLR` are drawn with a bar
/// over them on the package (`SN7474-002`), so `High` on either is the state
/// in which the part is *not* being preset or cleared.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// The `PRE` pin.
    pub pre_n: Level,
    /// The `CLR` pin.
    pub clr_n: Level,
    /// The `CLK` pin.
    pub clk: Level,
    /// The `D` pin.
    pub d: Level,
}

impl Inputs {
    /// Both asynchronous inputs inactive, the clock low and the data low.
    ///
    /// The two supplies aside, that is the only resting combination of the four
    /// that leaves the part alone: `PRE` or `CLR` low is an action
    /// (`SN7474-006`, `SN7474-007`).
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            pre_n: Level::High,
            clr_n: Level::High,
            clk: Level::Low,
            d: Level::Low,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// What one flip-flop drives on its two output pins.
///
/// A field is a level, [`Logic::Undetermined`] where the flip-flop holds
/// nothing the sheet names (`SN7474-OPEN-04`). The outputs are totem poles with no output-control pin
/// and no high-impedance state (`SN7474-022`), which is why neither field is an
/// `Option`; that `q_bar` is not simply `!q` is `SN7474-012`, the one row of the
/// function table on which the sheet prints the same level in both columns.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// The `Q` pin.
    pub q: Logic,
    /// The `Q` pin drawn with a bar.
    pub q_bar: Logic,
}

/// A pin's level where the part always determines it: an input pin, in a pin
/// word, in [`FlipFlop::with_state`] and from a getter.
pub use signal::Level;
/// A level: what an output pin stands at, and the one bit a flip-flop holds —
/// or that it holds none that the sheet names.
///
/// `SN7474-014` gives the part one bit per flip-flop. [`Logic::Undetermined`] is not
/// a third level and not a third bit: it is the state the part is in while both
/// asynchronous inputs are held low, which `SN7474-012` says the sheet declines
/// to call a state at all, and it is what the part is left holding if that
/// configuration is left by raising both inputs together. `SN7474-OPEN-04` is
/// that question and this crate does not answer it.
pub use signal::Logic;

/// One of the package's two flip-flops.
///
/// `Default` is [`Logic::Undetermined`] with the clock remembered low and both
/// asynchronous inputs remembered High, because the
/// data sheet gives no power-on state at all (`SN7474-OPEN-03`) — a board that
/// needs one asserts `PRE` or `CLR`, and that is what a real one does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct FlipFlop {
    /// The bit the flip-flop holds.
    held: Logic,
    /// The clock level this flip-flop was last shown.
    ///
    /// A part at its pins cannot see an edge, only two levels in succession; the
    /// edge is the change between them. `SN7474-010` is why holding one level is
    /// enough — the trigger is a level and is not directly related to the rise
    /// time of the clock pulse, so there is no duration for this to be missing.
    clk: Level,
    /// The `PRE` level this flip-flop was last shown, which
    /// [`FlipFlop::outputs`] reads.
    pre_n: Level,
    /// The `CLR` level this flip-flop was last shown, likewise.
    clr_n: Level,
}

impl Default for FlipFlop {
    fn default() -> Self {
        Self::with_state(Logic::Undetermined, Level::Low)
    }
}

impl FlipFlop {
    /// A flip-flop holding a given bit with a given clock level remembered,
    /// and `PRE` and `CLR` remembered High.
    ///
    /// For a caller that has to reach a state rather than clock its way to one.
    #[must_use]
    pub const fn with_state(held: Logic, clk: Level) -> Self {
        Self {
            held,
            clk,
            pre_n: Level::High,
            clr_n: Level::High,
        }
    }

    /// The bit the flip-flop holds, which `Q` drives except while `PRE` and
    /// `CLR` are both low; [`FlipFlop::outputs`] reads the pins.
    #[must_use]
    pub const fn q(&self) -> Logic {
        self.held
    }

    /// The clock level the flip-flop was last shown.
    #[must_use]
    pub const fn clk(&self) -> Level {
        self.clk
    }

    /// The `PRE` level the flip-flop was last shown.
    #[must_use]
    pub const fn pre_n(&self) -> Level {
        self.pre_n
    }

    /// The `CLR` level the flip-flop was last shown.
    #[must_use]
    pub const fn clr_n(&self) -> Level {
        self.clr_n
    }

    /// What the flip-flop drives, given what it holds and the `PRE` and `CLR`
    /// levels it was last shown, both pins [`Logic::Undetermined`] where the
    /// sheet does not say.
    ///
    /// The asynchronous inputs are read here as well as in [`FlipFlop::step`]
    /// because both of them low is a configuration the part is *driven* into and
    /// does not hold (`SN7474-012`): the outputs go High together while the pins
    /// stay there, and that is the sheet's one row on which `Q` bar is not the
    /// complement of `Q` (`SN7474-013`).
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        if self.pre_n.is_low() && self.clr_n.is_low() {
            return Outputs {
                q: Logic::High,
                q_bar: Logic::High,
            };
        }
        match self.held {
            Logic::High => Outputs {
                q: Logic::High,
                q_bar: Logic::Low,
            },
            Logic::Low => Outputs {
                q: Logic::Low,
                q_bar: Logic::High,
            },
            Logic::Undetermined => Outputs {
                q: Logic::Undetermined,
                q_bar: Logic::Undetermined,
            },
        }
    }

    /// Show the flip-flop one set of input levels, and read back what it drives.
    ///
    /// The four branches are the four documented behaviours, in the order the
    /// function table gives them:
    ///
    /// - `PRE` low with `CLR` high sets, whatever the clock and the data are
    ///   doing (`SN7474-006`);
    /// - `CLR` low with `PRE` high resets, likewise (`SN7474-007`);
    /// - both low leaves the part holding nothing the sheet names
    ///   (`SN7474-012`), and what it holds when that configuration is left is
    ///   `SN7474-OPEN-04`;
    /// - with both high, a clock that has gone from low to high since the last
    ///   call transfers `D` (`SN7474-008`) and anything else holds
    ///   (`SN7474-009`, `SN7474-015`) — a clock standing at either level, and a
    ///   falling edge.
    ///
    /// The clock level is taken up on every call, because the pin is what it is
    /// regardless of what the part is doing about it.
    pub fn step(&mut self, inputs: Inputs) -> Outputs {
        let rising = inputs.clk.is_high() && self.clk.is_low();
        self.clk = inputs.clk;
        self.pre_n = inputs.pre_n;
        self.clr_n = inputs.clr_n;
        self.held = match (inputs.pre_n, inputs.clr_n) {
            (Level::Low, Level::Low) => Logic::Undetermined,
            (Level::Low, Level::High) => Logic::High,
            (Level::High, Level::Low) => Logic::Low,
            (Level::High, Level::High) if rising => Logic::from(inputs.d),
            (Level::High, Level::High) => self.held,
        };
        self.outputs()
    }
}

/// The package: two flip-flops that share the two supply pins and nothing else
/// (`SN7474-001`, `SN7474-016`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Sn7474 {
    /// Index 0 is the flip-flop the data sheet numbers 1.
    flip_flops: [FlipFlop; FLIP_FLOP_COUNT],
}

impl Sn7474 {
    /// A package built from two flip-flops in known states.
    #[must_use]
    pub const fn with_flip_flops(flip_flops: [FlipFlop; FLIP_FLOP_COUNT]) -> Self {
        Self { flip_flops }
    }

    /// One of the two flip-flops, index 0 being the one the sheet numbers 1.
    ///
    /// # Panics
    ///
    /// If `index` is not 0 or 1.
    #[must_use]
    pub const fn flip_flop(&self, index: usize) -> FlipFlop {
        self.flip_flops[index]
    }

    /// What the package drives, given what it holds and the levels it was last
    /// shown.
    #[must_use]
    pub const fn outputs(&self) -> [Outputs; FLIP_FLOP_COUNT] {
        [self.flip_flops[0].outputs(), self.flip_flops[1].outputs()]
    }

    /// Show the package one set of input levels on each half.
    ///
    /// The two halves are stepped independently and neither is passed anything
    /// of the other's, which is `SN7474-016` written as code rather than
    /// asserted: there is no term here through which one could reach the other.
    pub fn step(&mut self, inputs: [Inputs; FLIP_FLOP_COUNT]) -> [Outputs; FLIP_FLOP_COUNT] {
        [
            self.flip_flops[0].step(inputs[0]),
            self.flip_flops[1].step(inputs[1]),
        ]
    }
}
