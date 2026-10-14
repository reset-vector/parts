//! The Motorola MC6821 peripheral interface adapter, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! # Examples
//!
//! ```
//! use mc6821::{Mc6821, Select};
//!
//! let mut pia = Mc6821::new();
//! // Control register A's bit 2 clear: `RS1`·`RS0` = `00` reaches the data
//! // direction register (`MC6821-008`). Every line of port A an output.
//! pia.write(Select::new(0b00), 0xff);
//! // Bit 2 set: the same select now reaches the peripheral register.
//! pia.write(Select::new(0b01), 0x04);
//! pia.write(Select::new(0b00), 0x5a);
//! pia.tick();
//! assert_eq!(pia.port_a().data, 0x5a);
//! assert_eq!(pia.port_a().direction, 0xff);
//! assert_eq!(pia.read(Select::new(0b00)), 0x5a);
//! ```
//!
//! # The shape
//!
//! `E` is the only timing signal the part receives (`MC6821-006`), so every
//! `E` cycle is one call, and which call says what the bus did during it:
//!
//! - [`Mc6821::read`] and [`Mc6821::write`] are an `E` cycle with the part
//!   selected — `CS0` and `CS1` High, `CS2` Low, which the board decodes
//!   (`MC6821-005`) — at the register `RS1`·`RS0` and the control
//!   registers' bit 2 choose (`MC6821-007`).
//! - [`Mc6821::tick`] is an `E` cycle with the part not selected. A host
//!   with the part on its bus calls one of the three on every `E` cycle: the
//!   edge-sense network is conditioned by `E` pulses (`MC6821-028`), the
//!   interrupt flags are re-enabled by a deselected one (`MC6821-029`), and
//!   the strobes on `CA2` and `CB2` are restored by one (`MC6821-032`,
//!   `MC6821-034`).
//!
//! The peripheral side is set between calls: [`Mc6821::set_pa`],
//! [`Mc6821::set_pb`] and the four control-line setters, and so is `RESET`
//! ([`Mc6821::set_reset_n`]). An active transition
//! sets its flag when the setter is called and not at the next `E`
//! (`MC6821-030`), so `IRQA` and `IRQB` can fall between two cycles.
//!
//! Both bus calls take `&mut self` because the part is not side-effect free:
//! a read of a peripheral register clears that side's flags (`MC6821-022`) and
//! starts `CA2`'s read strobe (`MC6821-031`). [`Mc6821::control_a`],
//! [`Mc6821::control_b`], [`Mc6821::port_a`] and [`Mc6821::port_b`] are the
//! views that change nothing.
//!
//! # Within one call
//!
//! A call is the rising edge of `E`, the transfer, then the falling edge, and
//! each edge does what the sheet hangs on it: `CB2`'s write strobe moves on
//! the rising edge (`MC6821-033`, `MC6821-034`), and `CA2`'s read strobe, the
//! port outputs and the edge conditioning on the falling one (`MC6821-031`,
//! `MC6821-037`). Nothing finer than an `E` cycle is modelled, and no time
//! is in nanoseconds.
//!
//! `RESET`, `IRQA` and `IRQB` are active Low and carry `_n`.
//!
//! # What this crate chooses
//!
//! Each an open item: a write that changes an active edge is no transition
//! (`MC6821-OPEN-01`); `RESET` leaves the edge sense unconditioned
//! (`MC6821-OPEN-02`); a strobe mode starts with `CA2` or `CB2` High
//! (`MC6821-OPEN-03`); `C1` restore is the flag being set, and while bit 7 is
//! set `CB2` stays High (`MC6821-OPEN-04`); a transition while a cleared flag
//! is inhibited is lost and spends the conditioning (`MC6821-OPEN-05`);
//! power-up is the reset state (`MC6821-OPEN-06`); port A reads its output
//! register on an output line, having no load (`MC6821-OPEN-07`); every peripheral input
//! starts High (`MC6821-OPEN-08`); and only a read of port A and a write of
//! port B move a strobe (`MC6821-OPEN-09`).

#![no_std]

/// A pin's level, as a getter named for the pin returns it (`irqa_n()`).
pub use signal::Level;

/// The levels of the register select pins `RS1`·`RS0`, `RS1` in bit 1 and
/// `RS0` in bit 0, as [`Mc6821::read`] and [`Mc6821::write`] take them
/// (`MC6821-007`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Select(u8);

impl Select {
    /// The select at `levels`, masked to `RS1`·`RS0` (bits 1–0); the other
    /// bits are ignored.
    #[must_use]
    pub const fn new(levels: u8) -> Self {
        Self(levels & 0b11)
    }

    /// The levels of `RS1`·`RS0`, in bits 1–0.
    #[must_use]
    pub const fn levels(self) -> u8 {
        self.0
    }
}

/// The six locations the bus reaches (`MC6821-007`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Register {
    /// Peripheral register A: the port A pins on a read, output register A on
    /// a write.
    PeripheralA,
    /// Data direction register A.
    DirectionA,
    /// Control register A.
    ControlA,
    /// Peripheral register B.
    PeripheralB,
    /// Data direction register B.
    DirectionB,
    /// Control register B.
    ControlB,
}

impl Register {
    /// Table 1: `RS1`, `RS0` and the two control registers' bit 2
    /// (`MC6821-007`, `MC6821-008`).
    const fn decode(rs1: bool, rs0: bool, cra2: bool, crb2: bool) -> Self {
        match (rs1, rs0) {
            (false, false) if cra2 => Self::PeripheralA,
            (false, false) => Self::DirectionA,
            (false, true) => Self::ControlA,
            (true, false) if crb2 => Self::PeripheralB,
            (true, false) => Self::DirectionB,
            (true, true) => Self::ControlB,
        }
    }
}

/// What the part drives on one port: the output register, and which lines
/// carry it (`MC6821-012`, `MC6821-013`).
///
/// A line whose `direction` bit is clear is an input and the part does not
/// drive it. On port A that line is still pulled up inside the part
/// (`MC6821-017`); on port B it is high impedance (`MC6821-018`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Port {
    /// The output register.
    pub data: u8,
    /// The data direction register: a `1` is a line the part drives.
    pub direction: u8,
}

/// Which side a [`Side`] is, because `CA2` and `CB2` strobe differently
/// (`MC6821-036`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Which {
    A,
    B,
}

/// One port with its control register and its two control lines.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
struct Side {
    port: Which,
    /// The output register (`MC6821-013`).
    output: u8,
    /// The data direction register (`MC6821-012`).
    direction: u8,
    /// Control register bits 0 to 5; bits 6 and 7 are the two flags
    /// (`MC6821-020`).
    control: u8,
    /// Bit 7, the flag set by `C1` (`MC6821-022`).
    flag1: bool,
    /// Bit 6, the flag set by `C2` as an input (`MC6821-024`).
    flag2: bool,
    /// The levels the peripheral presents at the port's eight pins.
    pins: u8,
    /// The level at `C1`.
    c1: bool,
    /// The level the peripheral presents at `C2`.
    c2: bool,
    /// An `E` pulse has occurred with `C1` at its inactive level since its
    /// last active transition (`MC6821-028`).
    armed1: bool,
    /// The same for `C2` as an input.
    armed2: bool,
    /// A read of the peripheral register cleared the flags and no deselected
    /// `E` pulse has followed (`MC6821-029`).
    inhibit: bool,
    /// What the part drives on `C2` while it is an output.
    c2_out: bool,
    /// `CB2`'s write strobe is due on the next rising `E` (`MC6821-033`).
    strobe_due: bool,
    /// A deselected `E` pulse has occurred with `CB2`'s strobe low
    /// (`MC6821-034`).
    deselect_seen: bool,
}

impl Side {
    /// The side as `RESET` leaves it (`MC6821-010`, `MC6821-011`), with the
    /// peripheral's levels kept: those are not the part's.
    const fn reset(port: Which, pins: u8, c1: bool, c2: bool) -> Self {
        Self {
            port,
            output: 0,
            direction: 0,
            control: 0,
            flag1: false,
            flag2: false,
            pins,
            c1,
            c2,
            armed1: false,
            armed2: false,
            inhibit: false,
            c2_out: true,
            strobe_due: false,
            deselect_seen: false,
        }
    }

    const fn control_byte(&self) -> u8 {
        self.control | (self.flag2 as u8) << 6 | (self.flag1 as u8) << 7
    }

    const fn c1_active_level(&self) -> bool {
        self.control & 0x02 != 0
    }

    const fn c2_is_input(&self) -> bool {
        self.control & 0x20 == 0
    }

    const fn c2_active_level(&self) -> bool {
        self.control & 0x10 != 0
    }

    /// `b5`·`b4` = `10`: a strobe mode (`MC6821-031` to `MC6821-034`).
    const fn strobing(&self) -> bool {
        self.control & 0x30 == 0x20
    }

    /// `b3` in a strobe mode: `E` restore where clear is `C1` restore.
    const fn e_restore(&self) -> bool {
        self.control & 0x08 != 0
    }

    /// `MC6821-027`, with `MC6821-023` and `MC6821-026` being that each flag
    /// is gated by its enable at the pin and not where it is set.
    const fn irq(&self) -> bool {
        (self.flag1 && self.control & 0x01 != 0)
            || (self.flag2 && self.control & 0x08 != 0 && self.c2_is_input())
    }

    /// The port as a read of the peripheral register returns it: input lines
    /// from the pins (`MC6821-016`), output lines from the output register
    /// (`MC6821-014`, `MC6821-015`; `MC6821-OPEN-07` for port A under load).
    const fn read_port(&self) -> u8 {
        (self.output & self.direction) | (self.pins & !self.direction)
    }

    const fn set_flag1(&mut self) {
        self.flag1 = true;
        if self.strobing() && !self.e_restore() {
            self.c2_out = true;
            self.strobe_due = false;
        }
    }

    const fn set_c1(&mut self, level: bool) {
        if level == self.c1 {
            return;
        }
        self.c1 = level;
        if level == self.c1_active_level() && self.armed1 {
            self.armed1 = false;
            if !self.inhibit {
                self.set_flag1();
            }
        }
    }

    const fn set_c2(&mut self, level: bool) {
        if level == self.c2 {
            return;
        }
        self.c2 = level;
        if self.c2_is_input() && level == self.c2_active_level() && self.armed2 {
            self.armed2 = false;
            if !self.inhibit {
                self.flag2 = true;
            }
        }
    }

    const fn write_control(&mut self, value: u8) {
        let was_strobing = self.strobing();
        self.control = value & 0x3f;
        if !self.c2_is_input() {
            self.flag2 = false;
            self.armed2 = false;
        }
        if self.control & 0x30 == 0x30 {
            self.c2_out = self.control & 0x08 != 0;
        }
        if self.strobing() && !was_strobing {
            self.c2_out = true;
        }
        if !self.strobing() {
            self.strobe_due = false;
            self.deselect_seen = false;
        }
    }

    /// A read of the peripheral register (`MC6821-022`, `MC6821-029`,
    /// `MC6821-031`).
    const fn read_peripheral(&mut self) -> u8 {
        let value = self.read_port();
        self.flag1 = false;
        self.flag2 = false;
        self.inhibit = true;
        if matches!(self.port, Which::A) && self.strobing() {
            self.c2_out = false;
        }
        value
    }

    const fn write_peripheral(&mut self, value: u8) {
        self.output = value;
        if matches!(self.port, Which::B) && self.strobing() {
            self.strobe_due = true;
        }
    }

    /// The rising edge of `E` (`MC6821-033`, `MC6821-034`).
    const fn rise(&mut self) {
        if !matches!(self.port, Which::B) || !self.strobing() {
            return;
        }
        if self.strobe_due {
            self.strobe_due = false;
            if self.e_restore() || !self.flag1 {
                self.c2_out = false;
                self.deselect_seen = false;
            }
        } else if self.e_restore() && !self.c2_out && self.deselect_seen {
            self.c2_out = true;
            self.deselect_seen = false;
        }
    }

    /// The falling edge of `E`, with the part `selected` or not.
    const fn fall(&mut self, selected: bool) {
        if self.strobing() && self.e_restore() && !self.c2_out && !selected {
            match self.port {
                Which::A => self.c2_out = true,
                Which::B => self.deselect_seen = true,
            }
        }
        if self.c1 != self.c1_active_level() {
            self.armed1 = true;
        }
        if self.c2_is_input() && self.c2 != self.c2_active_level() {
            self.armed2 = true;
        }
        if !selected {
            self.inhibit = false;
        }
    }
}

/// The whole part.
///
/// [`Mc6821::new`] stands at power-up. The sheet fixes no level there
/// (`MC6821-OPEN-06`), so this crate starts the part in its reset state, as
/// `RESET` leaves it, every register zero (`MC6821-010`, `MC6821-011`), with
/// `RESET` High. A board that needs a known state asserts `RESET`
/// ([`Mc6821::set_reset_n`]). The peripheral side starts at the levels
/// `MC6821-OPEN-08` declares: every input High.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Mc6821 {
    a: Side,
    b: Side,
    /// The `RESET` pin's level as last set.
    reset_n: Level,
}

impl Default for Mc6821 {
    fn default() -> Self {
        Self::new()
    }
}

impl Mc6821 {
    /// Power-up, which the sheet does not fix: the part's reset state, as
    /// after `RESET`, with every peripheral input High.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            a: Side::reset(Which::A, 0xff, true, true),
            b: Side::reset(Which::B, 0xff, true, true),
            reset_n: Level::High,
        }
    }

    /// The `RESET` pin's level, held until set again.
    ///
    /// While it is Low every register is zero (`MC6821-010`) and an `E`
    /// cycle changes nothing: a write does nothing and a read presents the
    /// zeroed register. Setting it High leaves the part in that state. What
    /// the peripheral presents at the pins is not the part's and is kept.
    pub const fn set_reset_n(&mut self, level: Level) {
        if level.is_low() {
            self.a = Side::reset(Which::A, self.a.pins, self.a.c1, self.a.c2);
            self.b = Side::reset(Which::B, self.b.pins, self.b.c1, self.b.c2);
        }
        self.reset_n = level;
    }

    /// The register `rs` addresses now (`MC6821-007`).
    const fn register(&self, rs: Select) -> Register {
        Register::decode(
            rs.levels() & 2 != 0,
            rs.levels() & 1 != 0,
            self.a.control & 0x04 != 0,
            self.b.control & 0x04 != 0,
        )
    }

    /// An `E` cycle selected with `R/W` High: the byte on `D0`–`D7`
    /// (`MC6821-009`).
    ///
    /// The cycle is a period of `E`, so a read advances the part as
    /// [`Mc6821::tick`] does, and a read of a peripheral register clears that
    /// side's interrupt flags (`MC6821-022`, `MC6821-029`, `MC6821-031`); a
    /// read may be made for that effect alone.
    pub const fn read(&mut self, rs: Select) -> u8 {
        if self.reset_n.is_low() {
            return self.view(rs);
        }
        self.a.rise();
        self.b.rise();
        let value = match self.register(rs) {
            Register::PeripheralA => self.a.read_peripheral(),
            Register::DirectionA => self.a.direction,
            Register::ControlA => self.a.control_byte(),
            Register::PeripheralB => self.b.read_peripheral(),
            Register::DirectionB => self.b.direction,
            Register::ControlB => self.b.control_byte(),
        };
        self.fall(true);
        value
    }

    /// An `E` cycle selected with `R/W` Low (`MC6821-009`).
    pub const fn write(&mut self, rs: Select, d: u8) {
        if self.reset_n.is_low() {
            return;
        }
        self.a.rise();
        self.b.rise();
        match self.register(rs) {
            Register::PeripheralA => self.a.write_peripheral(d),
            Register::DirectionA => self.a.direction = d,
            Register::ControlA => self.a.write_control(d),
            Register::PeripheralB => self.b.write_peripheral(d),
            Register::DirectionB => self.b.direction = d,
            Register::ControlB => self.b.write_control(d),
        }
        self.fall(true);
    }

    /// One period of `E`, the part's clock, with the part not selected
    /// (`MC6821-005`); a selected one is [`Mc6821::read`] or
    /// [`Mc6821::write`].
    pub const fn tick(&mut self) {
        if self.reset_n.is_low() {
            return;
        }
        self.a.rise();
        self.b.rise();
        self.fall(false);
    }

    const fn fall(&mut self, selected: bool) {
        self.a.fall(selected);
        self.b.fall(selected);
    }

    /// What a read of `rs` would return, changing nothing.
    const fn view(&self, rs: Select) -> u8 {
        match self.register(rs) {
            Register::PeripheralA => self.a.read_port(),
            Register::DirectionA => self.a.direction,
            Register::ControlA => self.a.control_byte(),
            Register::PeripheralB => self.b.read_port(),
            Register::DirectionB => self.b.direction,
            Register::ControlB => self.b.control_byte(),
        }
    }

    /// The levels the peripheral presents at `PA0`–`PA7`. Only lines
    /// programmed as inputs are read (`MC6821-016`).
    pub const fn set_pa(&mut self, levels: u8) {
        self.a.pins = levels;
    }

    /// The levels the peripheral presents at `PB0`–`PB7`.
    pub const fn set_pb(&mut self, levels: u8) {
        self.b.pins = levels;
    }

    /// The level at `CA1` (`MC6821-021`, `MC6821-030`).
    pub const fn set_ca1(&mut self, level: Level) {
        self.a.set_c1(level.is_high());
    }

    /// The level the peripheral presents at `CA2`; read only while `CA2` is
    /// an input (`MC6821-024`).
    pub const fn set_ca2(&mut self, level: Level) {
        self.a.set_c2(level.is_high());
    }

    /// The level at `CB1`.
    pub const fn set_cb1(&mut self, level: Level) {
        self.b.set_c1(level.is_high());
    }

    /// The level the peripheral presents at `CB2`.
    pub const fn set_cb2(&mut self, level: Level) {
        self.b.set_c2(level.is_high());
    }

    /// Port A's output register and direction (`MC6821-013`).
    #[must_use]
    pub const fn port_a(&self) -> Port {
        Port {
            data: self.a.output,
            direction: self.a.direction,
        }
    }

    /// Port B's output register and direction.
    #[must_use]
    pub const fn port_b(&self) -> Port {
        Port {
            data: self.b.output,
            direction: self.b.direction,
        }
    }

    /// Control register A as a read returns it, flags in bits 7 and 6
    /// (`MC6821-020`), changing nothing.
    #[must_use]
    pub const fn control_a(&self) -> u8 {
        self.a.control_byte()
    }

    /// Control register B, as [`Mc6821::control_a`].
    #[must_use]
    pub const fn control_b(&self) -> u8 {
        self.b.control_byte()
    }

    /// What the part drives on `CA2`, `None` while it is an input
    /// (`MC6821-024`, `MC6821-025`).
    #[must_use]
    pub const fn ca2(&self) -> Option<Level> {
        if self.a.c2_is_input() {
            None
        } else {
            Some(Level::from_bool(self.a.c2_out))
        }
    }

    /// What the part drives on `CB2`, `None` while it is an input.
    #[must_use]
    pub const fn cb2(&self) -> Option<Level> {
        if self.b.c2_is_input() {
            None
        } else {
            Some(Level::from_bool(self.b.c2_out))
        }
    }

    /// The `IRQA` output's level: [`Level::Low`] while the open-drain pin
    /// pulls Low, [`Level::High`] while it is released to the board's pull-up
    /// (`MC6821-004`, `MC6821-027`).
    #[must_use]
    pub const fn irqa_n(&self) -> Level {
        Level::from_bool(!self.a.irq())
    }

    /// The `IRQB` output's level, as [`Mc6821::irqa_n`].
    #[must_use]
    pub const fn irqb_n(&self) -> Level {
        Level::from_bool(!self.b.irq())
    }
}
