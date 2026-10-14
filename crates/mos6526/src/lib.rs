//! The MOS 6526 Complex Interface Adapter, emulated at its pins.
//!
//! The crate depends only on `signal`, allocates nothing, does no I/O and is
//! `no_std`: a [`Mos6526`] is a plain value that advances one `Ø2` cycle at a
//! time and shows the outside world its [`Pins`].
//!
//! # Examples
//!
//! ```
//! use mos6526::{Ctrl, Mos6526, Pins, Select};
//!
//! let mut cia = Mos6526::new();
//! // Timer A: latch 3, continuous, counting Ø2, its interrupt enabled.
//! let _ = cia.tick(Pins::write_access(Select::new(0x4), 3));
//! let _ = cia.tick(Pins::write_access(Select::new(0x5), 0));
//! let _ = cia.tick(Pins::write_access(Select::new(0xd), 0x81));
//! let _ = cia.tick(Pins::write_access(Select::new(0xe), 0x01));
//! // MOS6526-030: an underflow every N + 1 counted pulses.
//! let mut irq_at = None;
//! for cycle in 1..=8 {
//!     let out = cia.tick(Pins::idle());
//!     if out.ctrl.contains(Ctrl::IRQ) && irq_at.is_none() {
//!         irq_at = Some(cycle);
//!     }
//! }
//! assert_eq!(irq_at, Some(4));
//! // MOS6526-044: a read of the data register clears it and releases IRQ.
//! let out = cia.tick(Pins::read_access(Select::new(0xd)));
//! assert_eq!(out.d, Some(0x81));
//! assert!(!out.ctrl.contains(Ctrl::IRQ));
//! ```
//!
//! # The part, not the machine
//!
//! Sixteen registers behind `CS`, `R/W` and `RS3`–`RS0`, and nothing about
//! where a machine puts them (`MOS6526-004`). What the port lines, `FLAG`,
//! `CNT`, `SP` and `TOD` are wired to is a board's; this crate sees levels at
//! pins and drives levels at pins. That a Commodore 64 fits two of these is
//! two values of this type and a board that tells them apart.
//!
//! # One tick is one `Ø2` cycle, and the bus access rides in the pins
//!
//! [`Mos6526::tick`] takes the pins for one cycle and returns them. A read or
//! write the part is selected for in that cycle is carried in the same word —
//! [`Pins::read_access`] and [`Pins::write_access`] build one — and the byte a read returns
//! is in the returned word's [`Pins::d`], **resolved in the cycle it is
//! taken**. A read has side effects the part performs in that cycle: the
//! interrupt data register clears (`MOS6526-044`), the TOD registers latch or
//! release (`MOS6526-037`), and `PC` pulses after port B (`MOS6526-018`). So
//! there is no side-effect-free read at the pins, because the part has none.
//! [`Mos6526::interrupt_control`], [`Mos6526::interrupt_control_mask`],
//! [`Mos6526::timer_a`] and [`Mos6526::timer_b`] are the debugger's window
//! and do not exist on the package.
//!
//! Within one cycle the part **counts first and serves the bus after**, which
//! is `MOS6526-OPEN-07`'s choice: a write in cycle `t` is applied after the
//! pulse of cycle `t` has been counted, and a read in cycle `t` sees the count
//! of cycle `t`.
//!
//! # What this crate chooses
//!
//! Twelve behaviours here are the crate's choice, each under an open item, and
//! each is marked where it is written. The ones a consumer most needs to know
//! are the first three.
//!
//! - The first decrement follows the cycle that wrote `START`
//!   (`MOS6526-OPEN-01`).
//! - A period is `N + 1` cycles: the reload spends no pulse of its own
//!   (`MOS6526-OPEN-02`).
//! - `IRQ` follows the data register in the same cycle in both directions,
//!   and an underflow in a read's own cycle is returned and cleared by that
//!   read (`MOS6526-OPEN-03`).
//! - The `TOD` input counts rising edges, and its divider holds while the
//!   clock is stopped (`MOS6526-OPEN-04`).
//! - The hours run 12, 1, 2 … 11, 12, with `PM` toggled on the step from 11
//!   to 12; an hour that count never reaches counts on in binary within the
//!   five hours bits (`MOS6526-OPEN-05`).
//! - The alarm is compared on all four registers after every tenths advance,
//!   and an alarm write stops and starts nothing (`MOS6526-OPEN-06`).
//! - The part counts before it serves the bus, and timer B's `11` mode
//!   samples `CNT` at the underflow (`MOS6526-OPEN-07`).
//! - A read of the counter in the underflow cycle returns the reloaded latch
//!   (`MOS6526-OPEN-08`).
//! - `PC` is Low in the cycle after the port B access (`MOS6526-OPEN-09`).
//! - The shift register loads on the first timer A underflow after a write to
//!   `SDR`, which also drives `CNT` Low with the first bit on `SP`; in input
//!   mode neither pin is driven; a change of `SPMODE` drops a byte under way
//!   and releases both pins (`MOS6526-OPEN-10`).
//! - The TOD clock stays stopped after `RES` until the first write to the
//!   tenths register (`MOS6526-OPEN-11`).
//! - `IR` is the AND of the data and mask registers at all times, so setting
//!   a mask bit over a standing flag brings `IRQ` Low (`MOS6526-OPEN-12`).

#![no_std]

#[cfg(test)]
extern crate std;

mod pins;
mod serial;
mod timer;
mod tod;

pub use pins::{Ctrl, Pins, Select};

use serial::Serial;
use timer::Timer;
use tod::Tod;

use control::{ALARM, INMODE, SPMODE, TODIN};
use interrupt_control::{ALRM, FLG, IR, SP, TA, TB};

/// A pin's level, as a getter named for the pin returns it (`irq_n()`).
pub use signal::Level;

/// The interrupt control register's bits, as a read of the data register
/// and a write of the mask register take them (`MOS6526-042`).
pub mod interrupt_control {
    /// `TA`, interrupt bit 0 — timer A underflow (`MOS6526-042`).
    pub const TA: u8 = 0x01;
    /// `TB`, interrupt bit 1 — timer B underflow (`MOS6526-042`).
    pub const TB: u8 = 0x02;
    /// `ALRM`, interrupt bit 2 — the TOD alarm (`MOS6526-042`).
    pub const ALRM: u8 = 0x04;
    /// `SP`, interrupt bit 3 — the serial port full or empty (`MOS6526-042`).
    pub const SP: u8 = 0x08;
    /// `FLG`, interrupt bit 4 — a negative transition on `FLAG`
    /// (`MOS6526-042`).
    pub const FLG: u8 = 0x10;
    /// `IR`, bit 7 of the data register on read — an enabled interrupt stands
    /// (`MOS6526-043`). On write, `S/C`: whether the ones in the byte set or
    /// clear mask bits (`MOS6526-045`).
    pub const IR: u8 = 0x80;
}

pub use timer::control;

/// The five interrupt sources, as a mask over the data and mask registers.
const SOURCES: u8 = TA | TB | ALRM | SP | FLG;

/// The whole part.
///
/// [`Mos6526::new`] stands at power-up, which this crate starts in the
/// part's reset state, as `RES` leaves it (`MOS6526-009`), and has no other
/// constructor. A board that needs a known state asserts `RES`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Mos6526 {
    /// `PRA`, register 0 — the output register's bits (`MOS6526-013`).
    pra: u8,
    /// `PRB`, register 1.
    prb: u8,
    /// `DDRA`, register 2 (`MOS6526-015`).
    ddra: u8,
    /// `DDRB`, register 3.
    ddrb: u8,
    /// Timer A, registers 4, 5 and `E`.
    ta: Timer,
    /// Timer B, registers 6, 7 and `F`.
    tb: Timer,
    /// Registers 8 to `B`.
    tod: Tod,
    /// Register `C`.
    serial: Serial,
    /// The read-only interrupt data register, bits 0–4 (`MOS6526-042`).
    icr_data: u8,
    /// The write-only interrupt mask register, bits 0–4 (`MOS6526-042`).
    icr_mask: u8,
    /// `FLAG` was Low last cycle, for the edge (`MOS6526-019`).
    prev_flag_asserted: bool,
    /// The `CNT` line was High last cycle, for the edge (`MOS6526-027`,
    /// `MOS6526-039`).
    prev_cnt: bool,
    /// Port B was accessed last cycle, so `PC` is Low this cycle
    /// (`MOS6526-018`).
    pc_pending: bool,
}

impl Default for Mos6526 {
    fn default() -> Self {
        Self::new()
    }
}

impl Mos6526 {
    /// Power-up, this crate's choice: the part's reset state, as `RES` leaves
    /// it (`MOS6526-009`): every port line an input, every port register
    /// zero, both control registers zero, both timer latches all ones,
    /// everything else zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pra: 0,
            prb: 0,
            ddra: 0,
            ddrb: 0,
            ta: Timer::reset(),
            tb: Timer::reset(),
            tod: Tod::reset(),
            serial: Serial::reset(),
            icr_data: 0,
            icr_mask: 0,
            prev_flag_asserted: false,
            prev_cnt: true,
            pc_pending: false,
        }
    }

    /// One `Ø2` cycle.
    ///
    /// See the crate documentation for what rides in `pins` and what comes
    /// back. `RES` Low resets the part in this cycle and it drives nothing.
    #[must_use]
    pub fn tick(&mut self, pins: Pins) -> Pins {
        if pins.ctrl.contains(Ctrl::RES) {
            *self = Self::new();
            self.prev_flag_asserted = pins.ctrl.contains(Ctrl::FLAG);
            self.prev_cnt = !pins.ctrl.contains(Ctrl::CNT_LOW);
            return self.outputs(pins, None, false);
        }

        let (cnt_drive, _) = self.serial.drives(self.serial_output());
        let cnt = !pins.ctrl.contains(Ctrl::CNT_LOW) && cnt_drive;
        let cnt_rose = cnt && !self.prev_cnt;
        self.prev_cnt = cnt;
        let sp = !pins.ctrl.contains(Ctrl::SP_LOW);

        let flag_low = pins.ctrl.contains(Ctrl::FLAG);
        if flag_low && !self.prev_flag_asserted {
            self.icr_data |= FLG;
        }
        self.prev_flag_asserted = flag_low;

        let ta_pulse = if self.ta.cr & INMODE != 0 {
            cnt_rose
        } else {
            true
        };
        let ta_under = self.ta.count(ta_pulse);
        if ta_under {
            self.icr_data |= TA;
            if self.serial_output() && !self.ta.one_shot() && self.serial.output_underflow() {
                self.icr_data |= SP;
            }
        }

        let tb_pulse = match (self.tb.cr >> 5) & 0b11 {
            0b00 => true,
            0b01 => cnt_rose,
            0b10 => ta_under,
            _ => ta_under && cnt,
        };
        if self.tb.count(tb_pulse) {
            self.icr_data |= TB;
        }

        if self
            .tod
            .tick(pins.ctrl.contains(Ctrl::TOD), self.ta.cr & TODIN != 0)
        {
            self.icr_data |= ALRM;
        }

        if !self.serial_output() && self.serial.input(cnt_rose, sp) {
            self.icr_data |= SP;
        }

        let pc_low = self.pc_pending;
        self.pc_pending = false;

        let mut data = None;
        if pins.ctrl.contains(Ctrl::CS) {
            let rs = pins.rs & 0x0f;
            if pins.ctrl.contains(Ctrl::WRITE) {
                self.write(rs, pins.d.unwrap_or(0));
            } else {
                data = Some(self.read(rs, pins));
            }
            if rs == 1 {
                self.pc_pending = true;
            }
        }

        self.outputs(pins, data, pc_low)
    }

    /// The word the part drives at the end of a cycle.
    fn outputs(&self, pins: Pins, data: Option<u8>, pc_low: bool) -> Pins {
        let (cnt_drive, sp_drive) = self.serial.drives(self.serial_output());
        let mut ctrl = pins.ctrl.without(Ctrl::IRQ | Ctrl::PC);
        if !cnt_drive {
            ctrl |= Ctrl::CNT_LOW;
        }
        if !sp_drive {
            ctrl |= Ctrl::SP_LOW;
        }
        if self.irq_asserted() {
            ctrl |= Ctrl::IRQ;
        }
        if pc_low {
            ctrl |= Ctrl::PC;
        }
        Pins {
            rs: pins.rs,
            d: data,
            pa: self.pa_pins(pins.pa),
            pb: self.pb_pins(pins.pb),
            ctrl,
        }
    }

    const fn serial_output(&self) -> bool {
        self.ta.cr & SPMODE != 0
    }

    /// The level each port A line stands at, given what the outside holds
    /// (`MOS6526-015`, `MOS6526-016`): an output line is its register bit,
    /// an input line is pulled High (`MOS6526-010`), and either is Low when
    /// the outside pulls it Low.
    const fn pa_pins(&self, outside: u8) -> u8 {
        ((self.pra & self.ddra) | !self.ddra) & outside
    }

    /// Port B on the same terms, with the timer outputs overriding bits 6
    /// and 7 when switched on (`MOS6526-017`, `MOS6526-023`).
    const fn pb_pins(&self, outside: u8) -> u8 {
        let mut driven = (self.prb & self.ddrb) | !self.ddrb;
        if self.ta.pb_on() {
            driven = (driven & !0x40) | if self.ta.level() { 0x40 } else { 0 };
        }
        if self.tb.pb_on() {
            driven = (driven & !0x80) | if self.tb.level() { 0x80 } else { 0 };
        }
        driven & outside
    }

    /// A read of register `rs` in this cycle (`MOS6526-013`).
    fn read(&mut self, rs: u8, pins: Pins) -> u8 {
        match rs {
            0x0 => self.pa_pins(pins.pa),
            0x1 => self.pb_pins(pins.pb),
            0x2 => self.ddra,
            0x3 => self.ddrb,
            0x4 => self.ta.read_lo(),
            0x5 => self.ta.read_hi(),
            0x6 => self.tb.read_lo(),
            0x7 => self.tb.read_hi(),
            0x8..=0xb => self.tod.read(usize::from(rs - 8)),
            0xc => self.serial.sdr,
            0xd => {
                let value = self.interrupt_control();
                self.icr_data = 0;
                value
            }
            0xe => self.ta.cr,
            _ => self.tb.cr,
        }
    }

    /// A write of `value` to register `rs` in this cycle (`MOS6526-013`).
    fn write(&mut self, rs: u8, value: u8) {
        match rs {
            0x0 => self.pra = value,
            0x1 => self.prb = value,
            0x2 => self.ddra = value,
            0x3 => self.ddrb = value,
            0x4 => self.ta.write_lo(value),
            0x5 => self.ta.write_hi(value),
            0x6 => self.tb.write_lo(value),
            0x7 => self.tb.write_hi(value),
            0x8..=0xb => {
                self.tod
                    .write(usize::from(rs - 8), value, self.tb.cr & ALARM != 0);
            }
            0xc => self.serial.write(value),
            0xd => {
                let bits = value & SOURCES;
                if value & IR != 0 {
                    self.icr_mask |= bits;
                } else {
                    self.icr_mask &= !bits;
                }
            }
            0xe => {
                if (self.ta.cr ^ value) & SPMODE != 0 {
                    self.serial.mode_change();
                }
                self.ta.write_cr(value);
            }
            _ => self.tb.write_cr(value),
        }
    }

    /// The level of `IRQ`: [`Level::Low`] while an enabled interrupt stands in
    /// the data register (`MOS6526-043`). Evaluated continuously against the
    /// mask, which is `MOS6526-OPEN-12`'s choice.
    #[must_use]
    pub const fn irq_n(&self) -> Level {
        Level::from_bool(!self.irq_asserted())
    }

    const fn irq_asserted(&self) -> bool {
        self.icr_data & self.icr_mask & SOURCES != 0
    }

    /// The interrupt data register as a read would return it, without the
    /// read's clearing (`MOS6526-042`, `MOS6526-043`). Not a pin.
    #[must_use]
    pub const fn interrupt_control(&self) -> u8 {
        let flags = self.icr_data & SOURCES;
        if self.irq_asserted() {
            flags | IR
        } else {
            flags
        }
    }

    /// The interrupt mask register, which the package never shows
    /// (`MOS6526-042`). Not a pin.
    #[must_use]
    pub const fn interrupt_control_mask(&self) -> u8 {
        self.icr_mask
    }

    /// Timer A's counter (`MOS6526-020`). Not a pin.
    #[must_use]
    pub const fn timer_a(&self) -> u16 {
        self.ta.counter
    }

    /// Timer B's counter (`MOS6526-020`). Not a pin.
    #[must_use]
    pub const fn timer_b(&self) -> u16 {
        self.tb.counter
    }

    /// Whether the TOD clock is counting (`MOS6526-036`). Not a pin.
    #[must_use]
    pub const fn is_tod_running(&self) -> bool {
        self.tod.running()
    }
}

#[cfg(test)]
mod tests {
    use super::control::{SPMODE, START};
    use super::interrupt_control::SP;
    use super::{Ctrl, Mos6526, Pins, Select};

    /// `MOS6526-OPEN-10`: however far a byte had got, in either direction, a
    /// change of `SPMODE` abandons it, and the next byte written in output mode
    /// takes its full sixteen underflows.
    #[test]
    fn spmode_change_abandons_a_byte_under_way() {
        for underflows in 0..18 {
            for pulses in 0..9 {
                let mut cia = Mos6526::new();
                for (rs, value) in [(0x4, 0), (0x5, 0), (0xe, START | SPMODE)] {
                    let _ = cia.tick(Pins::write_access(Select::new(rs), value));
                }
                let _ = cia.tick(Pins::write_access(Select::new(0xc), 0xa5));
                for _ in 0..underflows {
                    let _ = cia.tick(Pins::idle());
                }
                let _ = cia.tick(Pins::write_access(Select::new(0xe), START));
                for _ in 0..pulses {
                    let low = Pins::idle();
                    let _ = cia.tick(Pins {
                        ctrl: low.ctrl.with(Ctrl::CNT_LOW),
                        ..low
                    });
                    let _ = cia.tick(Pins::idle());
                }
                let _ = cia.tick(Pins::write_access(Select::new(0xe), START | SPMODE));
                let _ = cia.tick(Pins::read_access(Select::new(0xd)));
                for _ in 0..20 {
                    let out = cia.tick(Pins::idle());
                    assert!(!out.ctrl.contains(Ctrl::CNT_LOW), "nothing left to send");
                }
                assert_eq!(cia.interrupt_control() & SP, 0);
                let _ = cia.tick(Pins::write_access(Select::new(0xc), 0x5a));
                let mut cycles = 0;
                while cia.interrupt_control() & SP == 0 {
                    let _ = cia.tick(Pins::idle());
                    cycles += 1;
                    assert!(cycles <= 16, "{underflows} underflows, {pulses} pulses");
                }
                assert_eq!(cycles, 16, "{underflows} underflows, {pulses} pulses");
            }
        }
    }
}
