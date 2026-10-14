//! The Namco `06xx` bus controller, at its pins.
//!
//! Every item below names the claim or the open item it implements.
//!
//! # Examples
//!
//! ```
//! use namco06xx::{Inputs, Level, Namco06xx};
//!
//! let mut part = Namco06xx::new();
//! // Pin 15 High: a control write selecting chip 0 for writing at rate 3,
//! // which pulses `NMI` at once and every 16 edges of `64H` after.
//! part.write(Level::High, 0x61);
//! assert_eq!(part.outputs().nmi_n, Level::Low);
//! let falls = (0..64)
//!     .map(|_| part.tick(Inputs::idle()).nmi_n)
//!     .collect::<Vec<_>>()
//!     .windows(2)
//!     .filter(|w| w[0].is_high() && w[1].is_low())
//!     .count();
//! assert_eq!(falls, 4);
//! ```
//!
//! # The shape
//!
//! The processor sees two registers and an `NMI` line; the chips on the far
//! side see an eight-line bus, a direction line and a strobe each. One clock
//! reaches the crate, `64H` at pin 17, and everything the part does on its
//! own it does on a rising edge of that clock:
//!
//! - [`Namco06xx::write`] and [`Namco06xx::read`] are one bus cycle each at
//!   the register the level at pin 15 selects (`NAMCO06XX-001`), with pin 18 Low
//!   (`NAMCO06XX-OPEN-01`); the host's own decode of the Z80's `RD` and `WR`
//!   says which, the part having no pin for either (`NAMCO06XX-OPEN-03`).
//! - [`Namco06xx::tick`] is one rising edge of `64H`, carrying the
//!   levels the chips hold pins 2–9 at in [`Inputs`], and returns the [`Outputs`] after it. The byte clock is `2^(k + 1)` of
//!   these (`NAMCO06XX-005`, `NAMCO06XX-OPEN-04`); on it the part pulses
//!   `NMI`, and in a read transfer latches the bus and strobes the chip
//!   (`NAMCO06XX-009`, `NAMCO06XX-010`).
//! - [`Namco06xx::outputs`] is the levels at pins 19, 13 to 10, 1, and 2–9
//!   between those events. A pulse on any output lasts until the second
//!   rising edge of `64H` after the event that started it, or the first at
//!   rate 0, so it ends before the next byte clock (`NAMCO06XX-OPEN-06`).
//!
//! Every pin level is a [`Level`]. `NMI`, the strobes and `I/O` are drawn
//! with a bar, so a pulse is High, then Low, then High.
//!
//! # The bus, and which bit is on which pin
//!
//! A byte on pins 2–9 is carried here as *lines*: bit *m* of [`Inputs::d`]
//! is pin 9 − *m*, so that the `51xx`'s `O7`–`O0` and the latch of a read
//! transfer (`NAMCO06XX-009`) are the identity. A **written** byte goes out
//! one line up — bit *n* at pin 8 − *n* — which `NAMCO06XX-008` establishes
//! for bits 0–2 and `NAMCO06XX-OPEN-07` chooses for the rest, with bit 7 at
//! pin 9, as [`Outputs::d`] shows it, and a host wiring an `MB8843`
//! takes its `K2`–`K0` from lines 3–1 and its `K3` from [`Outputs::io`].
//!
//! **What the crate also chooses**, each an open item: the cadence
//! `2^(k + 1)` at every rate (`NAMCO06XX-OPEN-05`); the one-line offset as
//! drawn (`NAMCO06XX-OPEN-08`); a register read answering the byte it holds
//! (`NAMCO06XX-OPEN-09`); the written byte's lines on pins 2–9 except in a
//! read transfer (`NAMCO06XX-OPEN-10`); pin 1 following bit 4 at the control
//! write (`NAMCO06XX-OPEN-11`); and pin 1 Low while no chip is selected
//! (`NAMCO06XX-OPEN-12`).
//!
//! # What is not here
//!
//! Pin 16, whose net ends at another custom (`NAMCO06XX-OPEN-02`); any pin
//! not drawn on the board; and any figure in nanoseconds. There is no reset:
//! [`Namco06xx::new`] is the only state the part starts from here
//! (`NAMCO06XX-OPEN-03`).

#![no_std]

/// A pin's level, as [`Namco06xx::write`] takes pin 15's and [`Outputs`]
/// holds the outputs'.
pub use signal::Level;

/// The two registers pin 15 selects (`NAMCO06XX-001`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Register {
    /// Pin 15 Low.
    Data,
    /// Pin 15 High.
    Control,
}

impl Register {
    /// The register the level at pin 15 selects.
    const fn at(pin15: Level) -> Self {
        match pin15 {
            Level::Low => Self::Data,
            Level::High => Self::Control,
        }
    }
}

/// The chips the control byte can address, bits 0–3 (`NAMCO06XX-003`).
pub const CHIP_COUNT: usize = 4;

/// The chip strobed at pin 13 — on Galaga the `51xx`.
pub const CHIP_AT_PIN_13: usize = 0;

/// The chip strobed at pin 10 — on Galaga the `54xx`.
pub const CHIP_AT_PIN_10: usize = 3;

/// Rising edges of `64H` an output stays Low after the event that pulled it
/// (`NAMCO06XX-OPEN-06`), where the byte clock's period is longer; at rate 0,
/// whose period is two edges, a pulse lasts one, so `NMI` and a strobe return
/// High between byte clocks.
pub const PULSE_TICKS: usize = 2;

/// The levels the chips hold pins 2–9 at, the pin word [`Namco06xx::tick`]
/// takes.
///
/// `Default` is [`Inputs::idle`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// Pins 2–9, bit *m* at pin 9 − *m*.
    pub d: u8,
}

impl Inputs {
    /// Pins 2–9 all High, as the lines stand with nothing pulling them Low.
    #[must_use]
    pub const fn idle() -> Self {
        Self { d: 0xFF }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// The lines a written byte is driven onto: bit *n* at pin 8 − *n* for
/// *n* = 0–6 and bit 7 at pin 9 (`NAMCO06XX-008`, `NAMCO06XX-OPEN-07`).
const fn lines_of_written(byte: u8) -> u8 {
    byte.rotate_left(1)
}

/// The control register's fields (`NAMCO06XX-003` to `NAMCO06XX-005`), as
/// [`Namco06xx::control`] reads it.
pub mod control {
    /// Bits 0–3: one bit per chip, bit 0 the chip at pin 13, bit 3 the chip
    /// at pin 10 (`NAMCO06XX-003`).
    pub const CHIPS_MASK: u8 = 0x0F;
    /// Bit 4, which pin 1 (`I/O`) shows: set, the transfer runs from the
    /// chips to the processor (`NAMCO06XX-004`).
    pub const IO: u8 = 0x10;
    /// Bits 5–7, the rate `k`: `NMI` pulses every `2^(k + 1)` rising edges
    /// of `64H` (`NAMCO06XX-005`, `NAMCO06XX-OPEN-04`, `NAMCO06XX-OPEN-05`).
    pub const RATE_MASK: u8 = 0xE0;
}

/// Rising edges of `64H` between `NMI` pulses under control byte `control`.
const fn period_ticks(control: u8) -> usize {
    1 << ((control >> 5) + 1)
}

/// The levels at the part's outputs after a rising edge of `64H`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// Pin 19, `NMI` (drawn with a bar): Low while a pulse is on.
    pub nmi_n: Level,
    /// The chips' strobes (drawn with a bar), chip 0's pin 13 to chip 3's
    /// pin 10 (`NAMCO06XX-003`): Low while a pulse is on.
    pub strobe_n: [Level; CHIP_COUNT],
    /// Pin 1, `I/O`, the direction line: High in a read transfer
    /// (`NAMCO06XX-008`, `NAMCO06XX-009`) and Low at every other time — a
    /// control byte that selects no chip leaves it Low whatever its bit 4
    /// (`NAMCO06XX-OPEN-12`).
    pub io: Level,
    /// Pins 2–9: `None` while a chip is selected for reading, and the last
    /// written byte's lines at every other time (`NAMCO06XX-OPEN-10`).
    pub d: Option<u8>,
}

/// The part.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Namco06xx {
    control: u8,
    data: u8,
    /// The last byte written to the data register, which is what the part
    /// drives on pins 2–9 (`NAMCO06XX-OPEN-10`).
    driven: u8,
    /// Rising edges of `64H` since the byte clock last fired, or since the
    /// control write that started the transfer.
    count: usize,
    /// Edges of `64H` `NMI` stays Low; 0 is High.
    nmi_low: usize,
    /// The same for each chip's strobe.
    strobe_low: [usize; CHIP_COUNT],
}

impl Default for Namco06xx {
    fn default() -> Self {
        Self::new()
    }
}

impl Namco06xx {
    /// Both registers clear, no chip selected, every output High
    /// (`NAMCO06XX-OPEN-03`).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            control: 0,
            data: 0,
            driven: 0,
            count: 0,
            nmi_low: 0,
            strobe_low: [0; CHIP_COUNT],
        }
    }

    /// One write cycle at the register `pin15`, the level at pin 15, selects:
    /// Low the data register, High the control register (`NAMCO06XX-001`).
    ///
    /// A control write starts a transfer: with a chip selected and bit 4
    /// clear, `NMI` pulses at once (`NAMCO06XX-006`); with bit 4 set, the
    /// selected chips are strobed and the first `NMI` waits a period
    /// (`NAMCO06XX-006`, `NAMCO06XX-010`). With no chip selected the byte
    /// clock stops (`NAMCO06XX-007`).
    ///
    /// A data write in a write transfer hands the byte to the selected chips
    /// with a strobe (`NAMCO06XX-008`, `NAMCO06XX-010`).
    pub fn write(&mut self, pin15: Level, d: u8) {
        let byte = d;
        match Register::at(pin15) {
            Register::Control => {
                self.control = byte;
                self.count = 0;
                if self.control & control::CHIPS_MASK == 0 {
                    return;
                }
                if self.control & control::IO != 0 {
                    self.strobe_selected();
                } else {
                    self.nmi_low = self.pulse_ticks();
                }
            }
            Register::Data => {
                self.data = byte;
                self.driven = byte;
                if self.transfer_out() {
                    self.strobe_selected();
                }
            }
        }
    }

    /// One read cycle at the register `pin15` selects, as for
    /// [`Namco06xx::write`]: the data register answers the byte it
    /// holds, the control register the byte written to it
    /// (`NAMCO06XX-009`, `NAMCO06XX-OPEN-09`).
    #[must_use]
    pub const fn read(&self, pin15: Level) -> u8 {
        match Register::at(pin15) {
            Register::Data => self.data,
            Register::Control => self.control,
        }
    }

    /// One rising edge of `64H` at pin 17, with `pins.d` the levels the chips
    /// hold pins 2–9 at (bit *m* is pin 9 − *m*).
    ///
    /// Pulses in flight run out first. Then, with a chip selected, the edge
    /// is counted, and on the `2^(k + 1)`th `NMI` pulses (`NAMCO06XX-005`);
    /// in a read transfer that same edge latches `pins.d` into the data
    /// register and strobes the selected chips (`NAMCO06XX-009`,
    /// `NAMCO06XX-010`).
    pub fn tick(&mut self, pins: Inputs) -> Outputs {
        self.edge(pins.d);
        self.outputs()
    }

    fn edge(&mut self, d: u8) {
        self.nmi_low = self.nmi_low.saturating_sub(1);
        for low in &mut self.strobe_low {
            *low = low.saturating_sub(1);
        }
        if self.control & control::CHIPS_MASK == 0 {
            return;
        }
        self.count += 1;
        if self.count < period_ticks(self.control) {
            return;
        }
        self.count = 0;
        self.nmi_low = self.pulse_ticks();
        if self.control & control::IO != 0 {
            self.data = d;
            self.strobe_selected();
        }
    }

    /// What the outputs stand at now.
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        let mut strobe_n = [Level::High; CHIP_COUNT];
        let mut chip = 0;
        while chip < CHIP_COUNT {
            strobe_n[chip] = self.strobe_n(chip);
            chip += 1;
        }
        Outputs {
            nmi_n: self.nmi_n(),
            strobe_n,
            io: self.io(),
            d: self.d(),
        }
    }

    /// Rising edges of `64H` a pulse started now stays Low: [`PULSE_TICKS`],
    /// or one fewer than the byte clock's period where that is shorter, so a
    /// pulse ends before the next byte clock starts one (`NAMCO06XX-OPEN-06`).
    const fn pulse_ticks(&self) -> usize {
        let room = period_ticks(self.control) - 1;
        if room < PULSE_TICKS {
            room
        } else {
            PULSE_TICKS
        }
    }

    fn strobe_selected(&mut self) {
        let pulse = self.pulse_ticks();
        for (chip, low) in self.strobe_low.iter_mut().enumerate() {
            if self.control & (1 << chip) != 0 {
                *low = pulse;
            }
        }
    }

    /// Pin 19, `NMI` (drawn with a bar): Low while a pulse is on.
    const fn nmi_n(&self) -> Level {
        Level::from_bool(self.nmi_low == 0)
    }

    /// The strobe of `chip` (0–3), pin 13 for chip 0 and pin 10 for chip 3
    /// (`NAMCO06XX-003`); Low while a pulse is on.
    ///
    /// # Panics
    ///
    /// On a chip beyond [`CHIP_COUNT`].
    const fn strobe_n(&self, chip: usize) -> Level {
        Level::from_bool(self.strobe_low[chip] == 0)
    }

    /// Pin 1, the direction line: High in a read transfer (`NAMCO06XX-008`,
    /// `NAMCO06XX-009`) and Low at every other time — a control byte that
    /// selects no chip leaves it Low whatever its bit 4
    /// (`NAMCO06XX-OPEN-12`).
    const fn io(&self) -> Level {
        Level::from_bool(self.transfer_in())
    }

    /// What the part drives on pins 2–9: nothing while a chip is selected
    /// for reading, and the last written byte's lines at every other time
    /// (`NAMCO06XX-OPEN-10`).
    const fn d(&self) -> Option<u8> {
        if self.transfer_in() {
            None
        } else {
            Some(lines_of_written(self.driven))
        }
    }

    /// A chip is selected and bit 4 is set: the chips drive the bus.
    const fn transfer_in(&self) -> bool {
        self.control & control::CHIPS_MASK != 0 && self.control & control::IO != 0
    }

    /// A chip is selected and bit 4 is clear: the part writes to the chips.
    const fn transfer_out(&self) -> bool {
        self.control & control::CHIPS_MASK != 0 && self.control & control::IO == 0
    }

    /// The control byte as written — a host's view, not a pin; its fields
    /// are [`control`]'s.
    #[must_use]
    pub const fn control(&self) -> u8 {
        self.control
    }

    /// The data register — a host's view; [`Namco06xx::read`] is the pin.
    #[must_use]
    pub const fn data(&self) -> u8 {
        self.data
    }

    /// Rising edges of `64H` counted toward the next `NMI` — a host's view.
    #[must_use]
    pub const fn elapsed_ticks(&self) -> usize {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin 15 Low, the data register.
    const DATA: Level = Level::Low;
    /// Pin 15 High, the control register.
    const CONTROL: Level = Level::High;

    #[test]
    fn tick_returns_what_the_accessors_read() {
        let mut part = Namco06xx::new();
        part.write(CONTROL, 0x11);
        for _ in 0..8 {
            let out = part.tick(Inputs { d: 0x5A });
            assert_eq!(out, part.outputs());
        }
    }

    #[test]
    fn rate_0_pulses_return_high_between_byte_clocks() {
        let mut part = Namco06xx::new();
        part.write(CONTROL, 0x01);
        let nmi: [Level; 16] = core::array::from_fn(|_| part.tick(Inputs::idle()).nmi_n);
        assert!(nmi.iter().any(|l| l.is_high()), "NMI High at some tick");
        let falls = nmi
            .windows(2)
            .filter(|w| w[0].is_high() && w[1].is_low())
            .count();
        assert_eq!(falls, 8, "a pulse every two edges: {nmi:?}");

        let mut part = Namco06xx::new();
        part.write(CONTROL, 0x11);
        let strobe: [Level; 16] = core::array::from_fn(|_| part.tick(Inputs::idle()).strobe_n[0]);
        assert!(
            strobe.iter().any(|l| l.is_high()),
            "strobe High at some tick"
        );
        assert!(strobe.iter().any(|l| l.is_low()), "and Low at another");
    }

    #[test]
    fn deselecting_with_bit_4_set_leaves_pin_1_low() {
        let mut part = Namco06xx::new();
        part.write(CONTROL, 0xA1);
        part.write(DATA, 0x05);
        part.write(CONTROL, 0x10);
        assert_eq!(part.outputs().io, Level::Low);
        part.write(CONTROL, 0xB1);
        assert_eq!(
            part.outputs().io,
            Level::High,
            "and a read transfer still raises it"
        );
        part.write(CONTROL, 0x10);
        assert_eq!(part.outputs().io, Level::Low);
    }
}
