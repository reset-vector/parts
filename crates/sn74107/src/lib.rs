//! The Texas Instruments SN74107 dual J-K flip-flop with clear, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part is pulse-triggered, not edge-triggered** (`SN74107-008`), and
//! that is the whole shape of this crate. The `'74` and the `'174` take their
//! data at one edge; this part takes it over the whole
//! of a clock pulse — the J-K input is loaded into a *master* while the clock
//! is high and transferred to the *slave* on the high-to-low transition — so
//! one [`FlipFlop`] holds three things: the bit the slave drives, what the
//! master was loaded with, and the clock level it last saw.
//!
//! **The sheet makes the J and K inputs a condition and not a data path**
//! (`SN74107-009`): they must be stable while the clock is high. This crate
//! holds the part to that sentence rather than guessing past it. A flip-flop
//! whose `J` or `K` is seen to move while its clock stands high has its master
//! [`Master::Disturbed`] (`SN74107-OPEN-03`), and the transfer at the next fall leaves the slave
//! [`Logic::Undetermined`] — which is why [`step`] returns [`Outputs`] of
//! [`Logic`]s. The other way to [`Logic::Undetermined`] is power-up, which the sheet gives no
//! state (`SN74107-OPEN-02`). A clear puts it back, and a clear released while
//! the clock stands high leaves the master loaded with what is present then
//! (`SN74107-OPEN-04`).
//!
//! **There are no nanoseconds here**, and no rate either. Setup, hold and
//! pulse-duration figures are conditions a board meets so that a pulse means
//! what `SN74107-008` says it means; this crate assumes they are met.
//!
//! [`step`]: FlipFlop::step
//!
//! # Examples
//!
//! A clock pulse with `J` High and `K` Low sets `Q` on its falling edge.
//!
//! ```
//! use sn74107::{Inputs, Level, Logic, Sn74107};
//!
//! let mut part = Sn74107::default();
//! let set = Inputs { j: Level::High, ..Inputs::default() };
//! part.step([Inputs { clk: Level::High, ..set }, Inputs::default()]);
//! let out = part.step([set, Inputs::default()]);
//! assert_eq!((out[0].q, out[0].q_bar), (Logic::High, Logic::Low));
//! ```

#![no_std]

/// How many flip-flops the package carries (`SN74107-001`).
pub const FLIP_FLOP_COUNT: usize = 2;

/// How many pins the package presents (`SN74107-002`).
pub const PIN_COUNT: usize = 14;

/// Where one flip-flop's six pins are on the package.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct FlipFlopPinout {
    /// `J`.
    pub j: u8,
    /// `Q` bar.
    pub q_bar: u8,
    /// `Q`.
    pub q: u8,
    /// `K`.
    pub k: u8,
    /// `CLK`.
    pub clk: u8,
    /// `CLR` bar.
    pub clr_n: u8,
}

/// Each flip-flop's six pins, flip-flop 1 first (`SN74107-002`,
/// `SN74107-003`).
///
/// The part decodes no pin number; the table is here because the two halves
/// are not mirror images of each other.
pub const FLIP_FLOP_PINS: [FlipFlopPinout; FLIP_FLOP_COUNT] = [
    FlipFlopPinout {
        j: 1,
        q_bar: 2,
        q: 3,
        k: 4,
        clk: 12,
        clr_n: 13,
    },
    FlipFlopPinout {
        j: 8,
        q_bar: 6,
        q: 5,
        k: 11,
        clk: 9,
        clr_n: 10,
    },
];

/// The levels the outside world holds one flip-flop's four input pins at.
///
/// **A field is a level, not a condition.** `CLR` is drawn with a bar over it
/// on the package (`SN74107-002`), so `High` on it is the state in which the
/// part is *not* being cleared.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// The `J` pin.
    pub j: Level,
    /// The `K` pin.
    pub k: Level,
    /// The `CLK` pin.
    pub clk: Level,
    /// The `CLR` pin.
    pub clr_n: Level,
}

impl Inputs {
    /// The clear inactive, the clock low and both data pins low.
    ///
    /// The two supplies aside, that is the only resting combination of the four
    /// that leaves the part alone: `CLR` low is an action (`SN74107-006`), and
    /// a clock standing high is the middle of a pulse.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            j: Level::Low,
            k: Level::Low,
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

/// What one flip-flop drives on its two output pins.
///
/// A field is a level, [`Logic::Undetermined`] where the slave holds nothing
/// the sheet names (`SN74107-OPEN-03`). The outputs are totem poles with no output-control pin
/// and no high-impedance state (`SN74107-019`), and `q_bar` is the complement
/// of `q` on every row of the function table (`SN74107-011`), which is why the
/// pair is two fields and not one bit: a consumer wires two pins.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// The `Q` pin.
    pub q: Logic,
    /// The `Q` pin drawn with a bar.
    pub q_bar: Logic,
}

/// A pin's level where the part always determines it: an input pin, in a pin
/// word, in [`Master`], in [`FlipFlop::with_state`] and from a getter.
pub use signal::Level;
/// A level: what an output pin stands at, and the one bit a flip-flop's slave
/// holds — or that it holds none the sheet names.
///
/// `SN74107-012` gives the part one bit per flip-flop. [`Logic::Undetermined`] is
/// not a third level and not a third bit: it is what the slave is left with
/// when the master was transferred after the sheet's condition on `J` and `K`
/// was broken (`SN74107-009`, `SN74107-OPEN-03`), and what a flip-flop holds
/// at power-up, which the sheet does not give (`SN74107-OPEN-02`). `Low` and
/// `High` held are `Q` low and high.
pub use signal::Logic;

/// What the master has been loaded with since the clock last rose.
///
/// The master is not a pin and the sheet draws it only as gates and two
/// coupling transistors (`SN74107-017`); what this crate keeps of it is the
/// `J` and `K` levels the description says it is loaded with while the clock
/// is high (`SN74107-008`), and nothing else.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Master {
    /// The clock is low, or the flip-flop has not seen it high since it was
    /// cleared with the clock low: nothing is loaded.
    #[default]
    Idle,
    /// Latched with the `J` and `K` levels present while the clock has stood
    /// high, which have not changed since.
    Latched {
        /// The `J` level the master latched.
        j: Level,
        /// The `K` level the master latched.
        k: Level,
    },
    /// `J` or `K` moved while the clock stood high, which the sheet forbids
    /// and does not describe (`SN74107-009`).
    Disturbed,
}

/// One of the package's two flip-flops.
///
/// `Default` is power-up: [`Logic::Undetermined`], the master idle and the clock
/// remembered low, because the data sheet gives no power-on state at all
/// (`SN74107-OPEN-02`) — a board that needs one asserts the clear, or clocks a
/// level in with `J` and `K` unequal.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct FlipFlop {
    /// The bit the slave holds, which is what the two output pins show.
    held: Logic,
    /// What the master holds.
    master: Master,
    /// The clock level this flip-flop was last shown.
    ///
    /// A part at its pins cannot see a pulse, only levels in succession; the
    /// rise and the fall are changes between them.
    clk: Level,
}

impl Default for FlipFlop {
    fn default() -> Self {
        Self {
            held: Logic::Undetermined,
            master: Master::Idle,
            clk: Level::Low,
        }
    }
}

impl FlipFlop {
    /// A flip-flop in a given state.
    ///
    /// For a caller that has to reach a state rather than clock its way to one.
    #[must_use]
    pub const fn with_state(held: Logic, master: Master, clk: Level) -> Self {
        Self { held, master, clk }
    }

    /// The bit the slave holds, which `Q` drives.
    #[must_use]
    pub const fn q(&self) -> Logic {
        self.held
    }

    /// What the master holds.
    #[must_use]
    pub const fn master(&self) -> Master {
        self.master
    }

    /// The clock level the flip-flop was last shown.
    #[must_use]
    pub const fn clk(&self) -> Level {
        self.clk
    }

    /// What the flip-flop drives, given what its slave holds, both pins
    /// [`Logic::Undetermined`] where the sheet does not say.
    ///
    /// A flip-flop being cleared is driving Low whatever its slave held a
    /// moment ago (`SN74107-006`); [`FlipFlop::step`] puts the slave there
    /// while `CLR` is Low, so the slave is what this reads.
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
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

    /// What the slave takes from a master loaded with `j` and `k`: the four
    /// clocked rows of the function table (`SN74107-007`), applied to what the
    /// slave holds now.
    fn transfer(held: Logic, j: Level, k: Level) -> Logic {
        match (j, k) {
            (Level::Low, Level::Low) => held,
            (Level::High, Level::Low) => Logic::High,
            (Level::Low, Level::High) => Logic::Low,
            (Level::High, Level::High) => !held,
        }
    }

    /// Show the flip-flop one set of input levels, and read back what it drives.
    ///
    /// The branches are the documented behaviours, in the order the sheet gives
    /// them:
    ///
    /// - `CLR` low drives `Q` Low whatever the clock, `J` and `K` are doing
    ///   (`SN74107-006`). It reaches the master too: with the clock standing
    ///   high the master is reloaded from the pins, so that a clear released
    ///   mid-pulse leaves the part loaded with what is present then
    ///   (`SN74107-OPEN-04`), and with the clock low there is nothing to load;
    /// - a clock that has risen since the last call loads the master with `J`
    ///   and `K` (`SN74107-008`);
    /// - a clock standing high keeps the master loaded with what it was loaded
    ///   with, unless `J` or `K` has moved, which disturbs it (`SN74107-009`);
    /// - a clock that has fallen since the last call transfers the master to
    ///   the slave — the four clocked rows of the function table
    ///   (`SN74107-007`) — and a disturbed master transfers nothing the sheet
    ///   names;
    /// - a clock standing low holds (`SN74107-013`).
    ///
    /// The clock level is taken up on every call, because the pin is what it is
    /// regardless of what the part is doing about it.
    pub fn step(&mut self, inputs: Inputs) -> Outputs {
        let rising = inputs.clk.is_high() && self.clk.is_low();
        let falling = inputs.clk.is_low() && self.clk.is_high();
        self.clk = inputs.clk;
        let loaded = Master::Latched {
            j: inputs.j,
            k: inputs.k,
        };
        if inputs.clr_n.is_low() {
            self.held = Logic::Low;
            self.master = if inputs.clk.is_high() {
                loaded
            } else {
                Master::Idle
            };
            return self.outputs();
        }
        if rising {
            self.master = loaded;
        } else if inputs.clk.is_high() {
            self.master = match self.master {
                Master::Idle => loaded,
                Master::Latched { .. } if self.master != loaded => Master::Disturbed,
                unchanged => unchanged,
            };
        } else if falling {
            self.held = match self.master {
                Master::Latched { j, k } => Self::transfer(self.held, j, k),
                Master::Disturbed => Logic::Undetermined,
                Master::Idle => self.held,
            };
            self.master = Master::Idle;
        }
        self.outputs()
    }
}

/// The package: two flip-flops that share the two supply pins and nothing else
/// (`SN74107-001`, `SN74107-015`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Sn74107 {
    /// Index 0 is the flip-flop the data sheet numbers 1.
    flip_flops: [FlipFlop; FLIP_FLOP_COUNT],
}

impl Sn74107 {
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

    /// What the package drives, given what it holds.
    #[must_use]
    pub const fn outputs(&self) -> [Outputs; FLIP_FLOP_COUNT] {
        [self.flip_flops[0].outputs(), self.flip_flops[1].outputs()]
    }

    /// Show the package one set of input levels on each half.
    ///
    /// The two halves are stepped independently and neither is passed anything
    /// of the other's, which is `SN74107-015` written as code rather than
    /// asserted: there is no term here through which one could reach the other.
    /// A board that wires `1Q` to `2CLK` to divide by four does so outside the
    /// package, and so does a consumer of this crate.
    pub fn step(&mut self, inputs: [Inputs; FLIP_FLOP_COUNT]) -> [Outputs; FLIP_FLOP_COUNT] {
        [
            self.flip_flops[0].step(inputs[0]),
            self.flip_flops[1].step(inputs[1]),
        ]
    }
}
