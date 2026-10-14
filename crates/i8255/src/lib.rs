//! The Intel 8255A programmable peripheral interface, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! # Examples
//!
//! ```
//! use i8255::{I8255, Select};
//!
//! let mut ppi = I8255::new();
//! // Mode 0, port A an output and every other line an input.
//! ppi.write(Select::new(0b11), 0x8B);
//! ppi.write(Select::new(0b00), 0x5A);
//! assert_eq!(ppi.outputs().pa, Some(0x5A));
//! ppi.set_pb(0x3C);
//! assert_eq!(ppi.read(Select::new(0b01)), Some(0x3C));
//! ```
//!
//! # The shape
//!
//! The part has no clock. Everything it does happens on an edge of `RD` or
//! `WR` with `CS` Low, or while `RESET` is High, so the crate is three calls
//! and a view:
//!
//! - [`I8255::write`] is one `WR` pulse, taking effect at its trailing edge
//!   (`I8255-025`).
//! - [`I8255::read`] is one `RD` pulse, and presents the levels the outside
//!   world holds the twenty-four lines at during it, because in Mode 0 inputs
//!   are not latched (`I8255-022`, `I8255-026`).
//! - [`I8255::set_reset`] is the `RESET` pin's level, and [`I8255::set_pa`],
//!   [`I8255::set_pb`] and [`I8255::set_pc`] the ports' lines, each held until
//!   set again (`I8255-010`).
//! - [`I8255::outputs`] is what the part holds its lines at between those
//!   events: a byte for a port or a half-port programmed as an output, and
//!   nothing for one programmed as an input.
//!
//! Which of the calls above a cycle is, if any, is the host's decode of the
//! five control pins by the `8255 BASIC OPERATION` table (`I8255-005` to
//! `I8255-009`): `CS` Low with one of `RD` and `WR` Low, at the `A1`·`A0`
//! select `a`; a read of the control register is illegal, and `RD` and `WR`
//! both Low has no call (`I8255-OPEN-06`).
//!
//! # What is not here
//!
//! **Modes 1 and 2.** `I8255-OPEN-01` and `I8255-OPEN-02` say what would change
//! that. A mode word selecting either is accepted into the control register,
//! [`I8255::is_modelled`] turns `false`, and the group in that mode is held as the
//! reset state — every line an input, nothing driven, a read presenting the
//! pins — until a mode word puts it back in Mode 0. That is a refusal and not a
//! model, and a host that reaches it should know.
//!
//! **Four readings the sheet does not make**, each an open item that names
//! this crate's choice: a read of an output port presents the latch
//! (`I8255-OPEN-03`); a read of the control register drives nothing
//! (`I8255-OPEN-04`); every mode word resets the latches, changed or not
//! (`I8255-OPEN-05`); a write to an input port still writes its latch
//! (`I8255-OPEN-07`).

#![no_std]

pub use signal::Level;

/// The four addresses `A1`·`A0` reach (`I8255-007`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Register {
    /// `A1`·`A0` = `00`.
    PortA,
    /// `01`.
    PortB,
    /// `10`.
    PortC,
    /// `11` — write-only (`I8255-009`, `I8255-012`).
    Control,
}

impl Register {
    /// The register `A1`·`A0` select.
    const fn at(a: Select) -> Self {
        match a.levels() {
            0b00 => Self::PortA,
            0b01 => Self::PortB,
            0b10 => Self::PortC,
            _ => Self::Control,
        }
    }
}

/// The levels of the select pins `A1`·`A0`, `A1` in bit 1 and `A0` in bit 0,
/// as [`I8255::read`] and [`I8255::write`] take them (`I8255-007`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Select(u8);

impl Select {
    /// The select at `levels`, masked to `A1`·`A0` (bits 1–0); the other
    /// bits are ignored.
    #[must_use]
    pub const fn new(levels: u8) -> Self {
        Self(levels & 0b11)
    }

    /// The levels of `A1`·`A0`, in bits 1–0.
    #[must_use]
    pub const fn levels(self) -> u8 {
        self.0
    }
}

/// Which way a port, or a half of port C, faces (`I8255-014`, `I8255-015`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Direction {
    /// The mode word's bit is 1; the lines are high-impedance and a read
    /// presents them.
    Input,
    /// The mode word's bit is 0; the lines are driven from the latch.
    Output,
}

impl Direction {
    const fn from_bit(bit: bool) -> Self {
        if bit { Self::Input } else { Self::Output }
    }
}

/// `lines` where `direction` is an output, and none where an input.
const fn output_mask(direction: Direction, lines: u8) -> u8 {
    match direction {
        Direction::Output => lines,
        Direction::Input => 0,
    }
}

/// The three modes group A can be in (`I8255-014`); group B's two are
/// [`ModeB`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Mode {
    /// Basic input/output (`I8255-021`). The one mode this crate models.
    Mode0,
    /// Strobed input/output (`I8255-028`). Declined; `I8255-OPEN-01`.
    Mode1,
    /// Bidirectional bus, group A only (`I8255-029`). Declined;
    /// `I8255-OPEN-02`.
    Mode2,
}

/// The two modes group B can be in (`I8255-015`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ModeB {
    /// Basic input/output (`I8255-021`). The one mode this crate models.
    Mode0,
    /// Strobed input/output (`I8255-028`). Declined; `I8255-OPEN-01`.
    Mode1,
}

/// What the part drives its lines to, one entry per unit of direction.
///
/// `None` is the high-impedance state of a port or half-port programmed as
/// an input, and of every line after reset (`I8255-010`). Port C is two
/// entries because its halves are directed by different groups
/// (`I8255-004`); each carries its lines at their places in the port, as
/// [`I8255::port_c`] does, the other half's bits 0.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Outputs {
    /// `PA7`–`PA0`, or nothing.
    pub pa: Option<u8>,
    /// `PB7`–`PB0`, or nothing.
    pub pb: Option<u8>,
    /// `PC7`–`PC4` in bits 7–4, or nothing.
    pub pc_upper: Option<u8>,
    /// `PC3`–`PC0` in bits 3–0, or nothing.
    pub pc_lower: Option<u8>,
}

/// A port's output latch (`I8255-023`) and the direction the mode word gives
/// its lines (`I8255-014`, `I8255-015`), as [`I8255::port_a`] reads them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Port {
    /// The output latch, bit 0 the port's line 0.
    pub data: u8,
    /// A bit set where the line is an output. The mode word's direction bit
    /// is 0 for an output, so this is its complement; every line is an
    /// input after reset and in a group in a declined mode. Port C's halves
    /// are directed by different groups (`I8255-004`), each as a nibble.
    pub direction: u8,
}

/// The whole part.
///
/// [`I8255::new`] stands at power-up. The data sheet fixes no level there, so
/// this crate starts the part in its reset state, as `RESET` leaves it: the
/// control register cleared and every line an input (`I8255-010`), usable so
/// without a mode word (`I8255-011`), with `RESET` Low. A board that needs a
/// known state asserts `RESET` ([`I8255::set_reset`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct I8255 {
    /// The `RESET` pin's level as last set.
    reset: Level,
    /// The control word register (`I8255-012`): the last mode word written,
    /// or `0` after reset. A bit set/reset word is a command and does not
    /// replace it. With `D7` clear no mode has been set and every line is an
    /// input (`I8255-010`).
    control: u8,
    /// Port A's output latch (`I8255-023`).
    latch_a: u8,
    /// Port B's output latch (`I8255-023`).
    latch_b: u8,
    /// Port C's output latch, both four-bit halves in one byte (`I8255-023`).
    latch_c: u8,
    /// The levels the outside world holds `PA7`–`PA0` at, as last set.
    pa: u8,
    /// The same for `PB7`–`PB0`.
    pb: u8,
    /// The same for `PC7`–`PC0`.
    pc: u8,
}

impl Default for I8255 {
    fn default() -> Self {
        Self::new()
    }
}

impl I8255 {
    /// The part as `RESET` leaves it (`I8255-010`), with `RESET` Low and
    /// every port line held Low from outside.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            reset: Level::Low,
            control: 0,
            latch_a: 0,
            latch_b: 0,
            latch_c: 0,
            pa: 0,
            pb: 0,
            pc: 0,
        }
    }

    /// The `RESET` pin's level, held until set again.
    ///
    /// While it is High every internal register is cleared and every line is
    /// an input (`I8255-010`), and a [`I8255::write`] does nothing; setting it
    /// Low leaves the part in that state.
    pub const fn set_reset(&mut self, level: Level) {
        if level.is_high() {
            let (pa, pb, pc) = (self.pa, self.pb, self.pc);
            *self = Self::new();
            self.pa = pa;
            self.pb = pb;
            self.pc = pc;
        }
        self.reset = level;
    }

    /// The levels the outside world holds `PA7`–`PA0` at, `PA0` in bit 0,
    /// held until set again. Only the lines programmed as inputs are read
    /// (`I8255-026`); what a peripheral holds an output line at while the
    /// part drives it is the peripheral's business, and the sheet does not
    /// reach it.
    pub const fn set_pa(&mut self, levels: u8) {
        self.pa = levels;
    }

    /// The levels the outside world holds `PB7`–`PB0` at, as
    /// [`I8255::set_pa`].
    pub const fn set_pb(&mut self, levels: u8) {
        self.pb = levels;
    }

    /// The levels the outside world holds `PC7`–`PC0` at, as
    /// [`I8255::set_pa`]; each half is read only while it is an input.
    pub const fn set_pc(&mut self, levels: u8) {
        self.pc = levels;
    }

    /// The control word register's contents: the last mode word written, or
    /// `0` after reset, so `D7` clear is a part no mode word has reached
    /// since (`I8255-010`, `I8255-012`).
    ///
    /// Not a read of the register — the part allows none (`I8255-009`) — but
    /// a host's view of what it holds.
    #[must_use]
    pub const fn control(&self) -> u8 {
        self.control
    }

    /// Group A's mode, from `D6`·`D5` of the mode word (`I8255-014`); Mode 0
    /// after reset.
    #[must_use]
    pub const fn mode_a(&self) -> Mode {
        match (self.control >> 5) & 0b11 {
            0b00 => Mode::Mode0,
            0b01 => Mode::Mode1,
            _ => Mode::Mode2,
        }
    }

    /// Group B's mode, from `D2` of the mode word (`I8255-015`); Mode 0 after
    /// reset.
    #[must_use]
    pub const fn mode_b(&self) -> ModeB {
        if self.control & 0x04 == 0 {
            ModeB::Mode0
        } else {
            ModeB::Mode1
        }
    }

    /// Whether both groups are in the one mode this crate models.
    ///
    /// `false` is the refusal the crate's documentation describes: a group in
    /// Mode 1 or Mode 2 is held as the reset state and nothing about that
    /// group is a reading of the sheet.
    #[must_use]
    pub const fn is_modelled(&self) -> bool {
        matches!(self.mode_a(), Mode::Mode0) && matches!(self.mode_b(), ModeB::Mode0)
    }

    /// Whether a mode word has been written and group A is in Mode 0, which
    /// is when its direction bits mean anything here.
    const fn group_a_live(&self) -> bool {
        self.control & 0x80 != 0 && matches!(self.mode_a(), Mode::Mode0)
    }

    const fn group_b_live(&self) -> bool {
        self.control & 0x80 != 0 && matches!(self.mode_b(), ModeB::Mode0)
    }

    /// Port A's direction, `D4` of the mode word (`I8255-014`); `Input` after
    /// reset and for a group in a declined mode.
    const fn direction_a(&self) -> Direction {
        if self.group_a_live() {
            Direction::from_bit(self.control & 0x10 != 0)
        } else {
            Direction::Input
        }
    }

    /// Port C upper's direction, `D3` of the mode word (`I8255-014`).
    const fn direction_c_upper(&self) -> Direction {
        if self.group_a_live() {
            Direction::from_bit(self.control & 0x08 != 0)
        } else {
            Direction::Input
        }
    }

    /// Port B's direction, `D1` of the mode word (`I8255-015`).
    const fn direction_b(&self) -> Direction {
        if self.group_b_live() {
            Direction::from_bit(self.control & 0x02 != 0)
        } else {
            Direction::Input
        }
    }

    /// Port C lower's direction, `D0` of the mode word (`I8255-015`).
    const fn direction_c_lower(&self) -> Direction {
        if self.group_b_live() {
            Direction::from_bit(self.control & 0x01 != 0)
        } else {
            Direction::Input
        }
    }

    /// Port A's output latch and direction, as a host's view.
    ///
    /// What the lines carry is [`I8255::outputs`]; the latch is what the part
    /// holds behind lines that may not be driven, which `I8255-OPEN-03` and
    /// `I8255-OPEN-07` are about.
    #[must_use]
    pub const fn port_a(&self) -> Port {
        Port {
            data: self.latch_a,
            direction: output_mask(self.direction_a(), 0xFF),
        }
    }

    /// Port B's, as [`I8255::port_a`].
    #[must_use]
    pub const fn port_b(&self) -> Port {
        Port {
            data: self.latch_b,
            direction: output_mask(self.direction_b(), 0xFF),
        }
    }

    /// Port C's, as [`I8255::port_a`]: both four-bit halves in one byte,
    /// `PC7` in bit 7, the upper half directed by group A and the lower by
    /// group B.
    #[must_use]
    pub const fn port_c(&self) -> Port {
        Port {
            data: self.latch_c,
            direction: output_mask(self.direction_c_upper(), 0xF0)
                | output_mask(self.direction_c_lower(), 0x0F),
        }
    }

    /// One `WR` pulse with `CS` Low, taking effect at its trailing edge
    /// (`I8255-025`).
    ///
    /// At a port the byte enters that port's output latch (`I8255-022`,
    /// `I8255-023`) whatever its direction (`I8255-OPEN-07`). At the control
    /// register a byte with `D7` set is a mode word (`I8255-013`) and resets
    /// all three latches (`I8255-017`, `I8255-OPEN-05`); one with `D7` clear
    /// is a bit set/reset of port C (`I8255-019`, `I8255-020`). `a` is the
    /// levels of `A1`·`A0`. With `RESET` High
    /// the pulse does nothing (`I8255-010`).
    pub const fn write(&mut self, a: Select, d: u8) {
        if self.reset.is_high() {
            return;
        }
        match Register::at(a) {
            Register::PortA => self.latch_a = d,
            Register::PortB => self.latch_b = d,
            Register::PortC => self.latch_c = d,
            Register::Control => {
                if d & 0x80 != 0 {
                    self.control = d;
                    self.latch_a = 0;
                    self.latch_b = 0;
                    self.latch_c = 0;
                } else {
                    let bit = (d >> 1) & 0b111;
                    if d & 0x01 != 0 {
                        self.latch_c |= 1 << bit;
                    } else {
                        self.latch_c &= !(1 << bit);
                    }
                }
            }
        }
    }

    /// One `RD` pulse with `CS` Low: the byte the data bus carries, or
    /// `None` where the part drives nothing.
    ///
    /// The lines stand where [`I8255::set_pa`] and its siblings last held
    /// them; a port or half-port programmed as an input presents
    /// them, unlatched (`I8255-026`), and one programmed as an output presents
    /// its latch (`I8255-OPEN-03`). Port C is composed half and half
    /// (`I8255-004`, `I8255-024`). The control register is the illegal
    /// condition and presents nothing (`I8255-009`, `I8255-OPEN-04`). `a` is
    /// the levels of `A1`·`A0`.
    #[must_use]
    pub const fn read(&self, a: Select) -> Option<u8> {
        match Register::at(a) {
            Register::PortA => Some(match self.direction_a() {
                Direction::Input => self.pa,
                Direction::Output => self.latch_a,
            }),
            Register::PortB => Some(match self.direction_b() {
                Direction::Input => self.pb,
                Direction::Output => self.latch_b,
            }),
            Register::PortC => {
                let upper = match self.direction_c_upper() {
                    Direction::Input => self.pc,
                    Direction::Output => self.latch_c,
                } & 0xF0;
                let lower = match self.direction_c_lower() {
                    Direction::Input => self.pc,
                    Direction::Output => self.latch_c,
                } & 0x0F;
                Some(upper | lower)
            }
            Register::Control => None,
        }
    }

    /// What the part drives its lines to now: the latch behind every port or
    /// half-port programmed as an output, nothing behind an input
    /// (`I8255-010`, `I8255-022`).
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        Outputs {
            pa: match self.direction_a() {
                Direction::Output => Some(self.latch_a),
                Direction::Input => None,
            },
            pb: match self.direction_b() {
                Direction::Output => Some(self.latch_b),
                Direction::Input => None,
            },
            pc_upper: match self.direction_c_upper() {
                Direction::Output => Some(self.latch_c & 0xF0),
                Direction::Input => None,
            },
            pc_lower: match self.direction_c_lower() {
                Direction::Output => Some(self.latch_c & 0x0F),
                Direction::Input => None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_select_keeps_only_a1_and_a0() {
        assert_eq!(Select::new(0xFF).levels(), 0b11);
        assert_eq!(Select::new(0b110).levels(), 0b10);
    }
}
