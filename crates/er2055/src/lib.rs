//! The General Instrument `ER2055`, a 64 × 8 EAROM, at its pins.
//!
//! # Examples
//!
//! ```
//! use er2055::{Cell, Er2055, Inputs, Level, TW_RANGE_PS};
//! use units::Picoseconds;
//!
//! let mut earom = Er2055::new(TW_RANGE_PS.0);
//! // Selected in Read, then `C1` Low and `C2` Low: a write of 0x5A to word 3.
//! let read = Inputs {
//!     cs1: Level::High,
//!     cs2_n: Level::Low,
//!     c1: Level::High,
//!     ..Inputs::default()
//! };
//! let write = Inputs { c1: Level::Low, a: 3, d: 0x5A, ..read };
//! earom.step(Picoseconds(0), read);
//! earom.step(Picoseconds(0), write);
//! // Held for the sheet's shortest write, it lands when the mode changes back.
//! earom.step(TW_RANGE_PS.0, read);
//! assert_eq!(earom.cells()[3], Cell::Written(0x5A));
//! ```
//!
//! # The shape
//!
//! The part has no internal clock to step. [`Er2055::step`] is one change of
//! any input pin, with the time it happened at in picoseconds; everything the
//! part does follows from the edges between one call and the next, which is
//! what the sheet's "dynamic edge triggered circuits" say it does
//! (`ER2055-009`). Between calls the pins hold.
//!
//! - An **erase** or a **write** begins at the edge that puts a selected part
//!   into that mode, or at a clock pulse in it, and ends at the next edge on a
//!   chip select or a mode line, or the clock's next rising edge. It takes
//!   effect when it ends if it was held the shortest of [`TE_RANGE_PS`] or
//!   [`TW_RANGE_PS`], which the sheet gives alike, or longer
//!   (`ER2055-010`), or as long as a host passes to [`Er2055::new`]
//!   (`ER2055-OPEN-02`).
//! - A **read** takes the address at the clock's rising edge and drives the
//!   word from its falling edge until the part is deselected or the mode lines
//!   change (`ER2055-007`, `ER2055-OPEN-06`).
//!
//! [`Er2055::cells`] and [`Er2055::set_cells`] are the contents, for a host that
//! keeps them between runs.
//!
//! **What the crate chooses**, each an open item: an erased word reads
//! [`ERASED`] (`ER2055-OPEN-01`); a write over a word not erased stores the
//! new byte and is counted in [`Er2055::unerased_write_count`]
//! (`ER2055-OPEN-03`); an erase or write takes the address and data at
//! the edge that starts it (`ER2055-OPEN-04`); and a part never written holds
//! every word erased (`ER2055-OPEN-05`).
//!
//! # What is not here
//!
//! Levels, currents, the nanosecond margins, and wear: the part is taken to
//! meet its sheet, and a word never tires.

#![no_std]

pub use signal::Level;
use units::Picoseconds;

/// The array's size, in its eight-bit words (`ER2055-001`).
pub const ROM_WORDS: usize = 64;

/// What an erased word reads — **the crate's choice**, `ER2055-OPEN-01`. The
/// sheet says only that no valid data is present.
pub const ERASED: u8 = 0xFF;

/// What one word of the array holds (`ER2055-006`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Cell {
    /// Erased: it presents no valid data, and a read gives [`ERASED`].
    Erased,
    /// Written with this byte since it was last erased.
    Written(u8),
}

/// `tE`, the shortest and longest erase the sheet allows (`ER2055-010`).
/// Nothing here enforces the longest (`ER2055-OPEN-02`).
pub const TE_RANGE_PS: (Picoseconds, Picoseconds) =
    (Picoseconds(50_000_000_000), Picoseconds(200_000_000_000));

/// `tW`, the shortest and longest write the sheet allows (`ER2055-010`).
/// Nothing here enforces the longest (`ER2055-OPEN-02`).
pub const TW_RANGE_PS: (Picoseconds, Picoseconds) =
    (Picoseconds(50_000_000_000), Picoseconds(200_000_000_000));

/// `tACC`, the longest a read takes from the clock (`ER2055-011`).
pub const TACC_MAX_PS: Picoseconds = Picoseconds(2_000_000);

/// `tPW`, the shortest clock pulse (`ER2055-011`).
pub const TPW_MIN_PS: Picoseconds = Picoseconds(2_000_000);

/// The input pins. The default deselects the part, in Read; it is not the
/// every-pin-Low word [`Er2055::new`] remembers (`ER2055-OPEN-05`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `A0`–`A5`; bits 6 and 7 are ignored.
    pub a: u8,
    /// `D0`–`D7` as the host drives them. Taken only by a write.
    pub d: u8,
    /// Selects the part while High (`ER2055-004`).
    pub cs1: Level,
    /// `CS2`, which selects the part while Low (`ER2055-004`).
    pub cs2_n: Level,
    /// Mode line `C1` (`ER2055-005`).
    pub c1: Level,
    /// Mode line `C2` (`ER2055-005`).
    pub c2: Level,
    /// `CLK`.
    pub clk: Level,
}

impl Inputs {
    /// The part idle; see [`Inputs`].
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            a: 0,
            d: 0,
            cs1: Level::Low,
            cs2_n: Level::High,
            c1: Level::High,
            c2: Level::Low,
            clk: Level::Low,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

impl Inputs {
    /// Whether these levels select the part (`ER2055-004`).
    const fn is_selected(self) -> bool {
        self.cs1.is_high() && self.cs2_n.is_low()
    }

    /// The mode `C1` and `C2` choose (`ER2055-005`).
    const fn mode(self) -> Mode {
        match (self.c1, self.c2) {
            (Level::High, _) => Mode::Read,
            (Level::Low, Level::High) => Mode::Erase,
            (Level::Low, Level::Low) => Mode::Write,
        }
    }
}

/// What `C1` and `C2` select (`ER2055-005`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Mode {
    /// `C1` Low, `C2` High.
    Erase,
    /// `C1` Low, `C2` Low.
    Write,
    /// `C1` High.
    Read,
}

/// The output pins.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub struct Outputs {
    /// `D0`–`D7` as the part drives them, or `None` while they float
    /// (`ER2055-004`, `ER2055-007`).
    pub d: Option<u8>,
}

/// An erase or write in progress.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Cycle {
    write: bool,
    addr: usize,
    data: u8,
    start: u64,
}

/// The part.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Er2055 {
    cells: [Cell; ROM_WORDS],
    pins: Inputs,
    cycle: Option<Cycle>,
    /// The address a read took at the clock's rising edge.
    read_addr: Option<usize>,
    /// The word on `D0`–`D7` while the part is selected in read mode.
    out: Option<u8>,
    unerased_write_count: usize,
    /// The shortest hold that lands (`ER2055-OPEN-02`).
    min_hold_ps: Picoseconds,
}

impl Er2055 {
    /// A part with every word erased and every pin Low, which deselects it
    /// (`ER2055-OPEN-05`), on which an erase or write lands once held
    /// `min_hold_ps`. The sheet's shortest is [`TE_RANGE_PS`]`.0`
    /// and [`TW_RANGE_PS`]`.0`, alike; what a real part does with a hold
    /// under 50 ms is `ER2055-OPEN-02`, and a host that passes less is taking
    /// that reading as its own.
    #[must_use]
    pub const fn new(min_hold_ps: Picoseconds) -> Self {
        Self {
            cells: [Cell::Erased; ROM_WORDS],
            pins: Inputs {
                a: 0,
                d: 0,
                cs1: Level::Low,
                cs2_n: Level::Low,
                c1: Level::Low,
                c2: Level::Low,
                clk: Level::Low,
            },
            cycle: None,
            read_addr: None,
            out: None,
            unerased_write_count: 0,
            min_hold_ps,
        }
    }

    /// The shortest hold that lands.
    #[must_use]
    pub const fn min_hold_ps(&self) -> Picoseconds {
        self.min_hold_ps
    }

    /// The inputs as last set.
    #[must_use]
    pub const fn inputs(&self) -> Inputs {
        self.pins
    }

    /// The input pins change to `pins` at `now_ps`, on the host's own clock;
    /// returns where the outputs then stand. Times must not go backwards.
    pub fn step(&mut self, now_ps: Picoseconds, pins: Inputs) -> Outputs {
        self.apply(now_ps.0, pins);
        self.outputs()
    }

    fn apply(&mut self, now: u64, pins: Inputs) {
        let was = self.pins;
        self.pins = pins;

        let selection = was.is_selected() != pins.is_selected();
        let mode = was.mode() != pins.mode();
        let rise = was.clk.is_low() && pins.clk.is_high();
        let fall = was.clk.is_high() && pins.clk.is_low();

        if selection || mode || rise {
            self.finish(now);
        }
        if selection || mode {
            self.out = None;
            self.read_addr = None;
        }

        if !pins.is_selected() {
            return;
        }
        let addr = usize::from(pins.a & 0x3F);
        match pins.mode() {
            Mode::Read => {
                if rise {
                    self.read_addr = Some(addr);
                }
                if fall && let Some(at) = self.read_addr.take() {
                    self.out = Some(match self.cells[at] {
                        Cell::Erased => ERASED,
                        Cell::Written(d) => d,
                    });
                }
            }
            m @ (Mode::Erase | Mode::Write) => {
                if selection || mode || rise {
                    self.cycle = Some(Cycle {
                        write: m == Mode::Write,
                        addr,
                        data: pins.d,
                        start: now,
                    });
                }
            }
        }
    }

    /// Ends the erase or write in progress at `now`.
    fn finish(&mut self, now: u64) {
        let Some(c) = self.cycle.take() else {
            return;
        };
        if now.saturating_sub(c.start) < self.min_hold_ps.0 {
            return;
        }
        if c.write {
            if self.cells[c.addr] != Cell::Erased {
                self.unerased_write_count = self.unerased_write_count.saturating_add(1);
            }
            self.cells[c.addr] = Cell::Written(c.data);
        } else {
            self.cells[c.addr] = Cell::Erased;
        }
    }

    /// What the outputs stand at now.
    #[must_use]
    pub fn outputs(&self) -> Outputs {
        Outputs {
            d: if self.pins.is_selected() && self.pins.mode() == Mode::Read {
                self.out
            } else {
                None
            },
        }
    }

    /// Whether an erase or write is being held.
    #[must_use]
    pub const fn is_busy(&self) -> bool {
        self.cycle.is_some()
    }

    /// The contents. A cycle still being held is not in them.
    #[must_use]
    pub const fn cells(&self) -> [Cell; ROM_WORDS] {
        self.cells
    }

    /// Sets the contents, as a host restoring them at power-up would.
    pub fn set_cells(&mut self, cells: [Cell; ROM_WORDS]) {
        self.cells = cells;
    }

    /// How many writes landed on a word that had not been erased — a thing
    /// the sheet forbids and whose result is `ER2055-OPEN-03`.
    #[must_use]
    pub const fn unerased_write_count(&self) -> usize {
        self.unerased_write_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sheet's shortest write, in picoseconds.
    const TW: u64 = TW_RANGE_PS.0.0;

    /// `ps` picoseconds.
    const fn at(ps: u64) -> Picoseconds {
        Picoseconds(ps)
    }

    #[test]
    fn default_pins_deselect_in_read() {
        let p = Inputs {
            cs1: Level::High,
            ..Inputs::default()
        };
        assert!(!p.is_selected());
        assert_eq!(p.mode(), Mode::Read);
    }

    const SEL: Inputs = Inputs {
        a: 0,
        d: 0,
        cs1: Level::High,
        cs2_n: Level::Low,
        c1: Level::High,
        c2: Level::Low,
        clk: Level::Low,
    };

    /// `ER2055-OPEN-02`: a short hold is this crate's choice to ignore.
    #[test]
    fn a_write_held_under_fifty_ms_does_nothing() {
        let mut p = Er2055::new(TE_RANGE_PS.0);
        let _ = p.step(at(0), SEL);
        let _ = p.step(
            at(1),
            Inputs {
                c1: Level::Low,
                d: 0x5A,
                a: 3,
                ..SEL
            },
        );
        let _ = p.step(at(TW), SEL);
        assert_eq!(p.cells()[3], Cell::Erased);
    }

    /// `ER2055-OPEN-02`: a host that lowers the floor has a 48 ms write land,
    /// and one just under its floor still does nothing.
    #[test]
    fn a_host_floor_lands_a_shorter_write() {
        const FLOOR: Picoseconds = Picoseconds(48_000_000_000);
        let w = Inputs {
            c1: Level::Low,
            d: 0x5A,
            a: 3,
            ..SEL
        };
        let mut p = Er2055::new(FLOOR);
        let _ = p.step(at(0), SEL);
        let _ = p.step(at(1), w);
        let _ = p.step(FLOOR, SEL);
        assert_eq!(p.cells()[3], Cell::Erased);
        let _ = p.step(FLOOR, w);
        let _ = p.step(FLOOR * 2, SEL);
        assert_eq!(p.cells()[3], Cell::Written(0x5A));
    }

    /// `ER2055-OPEN-03`: a write over a written word stores the new byte and
    /// is counted.
    #[test]
    fn a_write_without_an_erase_is_counted() {
        let mut p = Er2055::new(TE_RANGE_PS.0);
        let w = Inputs {
            c1: Level::Low,
            a: 9,
            ..SEL
        };
        let _ = p.step(at(0), SEL);
        let _ = p.step(at(0), Inputs { d: 1, ..w });
        let _ = p.step(at(TW), SEL);
        let _ = p.step(at(TW), Inputs { d: 2, ..w });
        let _ = p.step(at(2 * TW), SEL);
        assert_eq!(p.cells()[9], Cell::Written(2));
        assert_eq!(p.unerased_write_count(), 1);
    }

    /// `ER2055-OPEN-04`: the address is the one at the starting edge.
    #[test]
    fn a_cycle_takes_its_address_at_the_start() {
        let mut p = Er2055::new(TE_RANGE_PS.0);
        let _ = p.step(at(0), SEL);
        let _ = p.step(
            at(0),
            Inputs {
                c1: Level::Low,
                a: 4,
                d: 7,
                ..SEL
            },
        );
        let _ = p.step(
            at(10),
            Inputs {
                c1: Level::Low,
                a: 5,
                d: 8,
                ..SEL
            },
        );
        let _ = p.step(at(TW), SEL);
        assert_eq!(p.cells()[4], Cell::Written(7));
        assert_eq!(p.cells()[5], Cell::Erased);
    }
}
