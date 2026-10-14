//! The programmable timer: a free-running counter, an output compare, an
//! input capture and the control and status register over them
//! (`MC6803-041` to `MC6803-045`).

use core::num::NonZeroU8;

use crate::{Level, reg, vector};

/// Bits of the timer control and status register (`MC6803-045`).
pub mod tcsr {
    /// Bit 0: the level clocked to the output level register on a compare.
    pub const OLVL: u8 = 1 << 0;
    /// Bit 1: capture on a rising edge when set, a falling edge when clear.
    pub const IEDG: u8 = 1 << 1;
    /// Bit 2: overflow interrupt enable.
    pub const ETOI: u8 = 1 << 2;
    /// Bit 3: output compare interrupt enable.
    pub const EOCI: u8 = 1 << 3;
    /// Bit 4: input capture interrupt enable.
    pub const EICI: u8 = 1 << 4;
    /// Bit 5: timer overflow flag, read-only.
    pub const TOF: u8 = 1 << 5;
    /// Bit 6: output compare flag, read-only.
    pub const OCF: u8 = 1 << 6;
    /// Bit 7: input capture flag, read-only.
    pub const ICF: u8 = 1 << 7;
    /// The five bits software may write.
    pub const WRITABLE_MASK: u8 = 0x1f;
}

/// The value a write to the counter's high byte presets it to
/// (`MC6803-042`).
pub const COUNTER_PRESET: u16 = 0xfff8;

/// The timer's registers, a field of [`crate::Registers`], with the
/// one-cycle latches around them that delay a write's or an edge's effect.
///
/// `Default` is the timer as reset leaves it: counter, buffer, input capture
/// and TCSR cleared, the output compare register at `$FFFF`, the output level
/// register cleared (`MC6803-009`, `MC6803-044`), nothing in flight and `P20`
/// last seen Low.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub struct Timer {
    /// The free-running counter, `$09`–`$0A` (`MC6803-042`).
    pub counter: u16,
    /// The buffer a read of `$09` fills with the counter's low byte and a
    /// read of `$0A` returns (`MC6803-042`).
    pub counter_latch: u8,
    /// The output compare register, `$0B`–`$0C` (`MC6803-043`).
    pub output_compare: u16,
    /// The input capture register, `$0D`–`$0E` (`MC6803-044`).
    pub input_capture: u16,
    /// The control and status register, `$08`; its bits are [`tcsr`]
    /// (`MC6803-045`).
    pub tcsr: u8,
    /// The output level register, clocked from `OLVL` on a compare, which
    /// `P21` shows when its direction is output (`MC6803-043`).
    pub output_level_register: Level,
    /// TCSR was read with `TOF` set: the first half of its clearing
    /// sequence is done (`MC6803-045`).
    pub tof_read: bool,
    /// TCSR was read with `OCF` set.
    pub ocf_read: bool,
    /// TCSR was read with `ICF` set.
    pub icf_read: bool,
    /// The counter's high byte was written: the next clock presets the
    /// counter instead of counting (`MC6803-042`).
    pub preset_deferred: bool,
    /// The output compare register's high byte was written: the next
    /// compare is inhibited (`MC6803-043`).
    pub compare_inhibit: bool,
    /// The input capture register's high byte was read: the next capture
    /// transfer is inhibited (`MC6803-044`).
    pub capture_inhibit: bool,
    /// E cycles until a seen edge's capture transfer, `None` while no edge
    /// is waiting for one (`MC6803-044`).
    pub capture_remaining_cycles: Option<NonZeroU8>,
    /// The level at `P20` last cycle, for the capture edge (`MC6803-044`).
    pub p20: Level,
}

impl Default for Timer {
    fn default() -> Self {
        Unit::new().registers()
    }
}

/// The timer's state: its registers and the one-cycle latches around them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) struct Unit {
    /// The free-running counter (`MC6803-042`).
    counter: u16,
    /// A write to `$09` this cycle: the next clock presets instead of
    /// counting.
    preset: bool,
    /// The buffer a read of `$09` fills and a read of `$0A` empties.
    lsb: u8,
    /// The output compare register (`MC6803-043`).
    ocr: u16,
    /// The input capture register (`MC6803-044`).
    icr: u16,
    /// The control and status register (`MC6803-045`).
    tcsr: u8,
    /// The output level register, clocked from `OLVL` on a compare.
    olvl: bool,
    /// A write to `$0B` this cycle: the next compare is inhibited.
    oc_inhibit: bool,
    /// A read of `$0D` this cycle: the next capture transfer is inhibited.
    ic_inhibit: bool,
    /// An edge has been seen and its transfer is this many cycles away.
    ic_due: u8,
    /// The level at `P20` last cycle, for the edge detector.
    p20_last: bool,
    /// The TCSR was read with each flag set: the first half of each clearing
    /// sequence.
    tof_read: bool,
    ocf_read: bool,
    icf_read: bool,
}

impl Unit {
    /// The timer as reset leaves it (`MC6803-009`).
    pub(crate) const fn new() -> Self {
        Self {
            counter: 0,
            preset: false,
            lsb: 0,
            ocr: 0xffff,
            icr: 0,
            tcsr: 0,
            olvl: false,
            oc_inhibit: false,
            ic_inhibit: false,
            ic_due: 0,
            p20_last: false,
            tof_read: false,
            ocf_read: false,
            icf_read: false,
        }
    }

    /// Everything reset does to the timer: counter, LSB buffer, output level
    /// register and TCSR cleared, the output compare register preset to
    /// `$FFFF` (`MC6803-009`). The input capture register is cleared too
    /// (`MC6803-044`).
    pub(crate) const fn reset(&mut self) {
        let p20 = self.p20_last;
        *self = Self::new();
        self.p20_last = p20;
    }

    /// The registers as they stand.
    pub(crate) const fn registers(&self) -> Timer {
        Timer {
            counter: self.counter,
            counter_latch: self.lsb,
            output_compare: self.ocr,
            input_capture: self.icr,
            tcsr: self.tcsr,
            output_level_register: Level::from_bool(self.olvl),
            tof_read: self.tof_read,
            ocf_read: self.ocf_read,
            icf_read: self.icf_read,
            preset_deferred: self.preset,
            compare_inhibit: self.oc_inhibit,
            capture_inhibit: self.ic_inhibit,
            capture_remaining_cycles: NonZeroU8::new(self.ic_due),
            p20: Level::from_bool(self.p20_last),
        }
    }

    /// A timer holding `regs`.
    pub(crate) const fn from_registers(regs: Timer) -> Self {
        Self {
            counter: regs.counter,
            preset: regs.preset_deferred,
            lsb: regs.counter_latch,
            ocr: regs.output_compare,
            icr: regs.input_capture,
            tcsr: regs.tcsr,
            olvl: regs.output_level_register.is_high(),
            oc_inhibit: regs.compare_inhibit,
            ic_inhibit: regs.capture_inhibit,
            ic_due: match regs.capture_remaining_cycles {
                Some(n) => n.get(),
                None => 0,
            },
            p20_last: regs.p20.is_high(),
            tof_read: regs.tof_read,
            ocf_read: regs.ocf_read,
            icf_read: regs.icf_read,
        }
    }

    /// The counter as it stands this cycle.
    pub(crate) const fn counter(&self) -> u16 {
        self.counter
    }

    /// The output level register, which `P21` shows when its direction is
    /// output (`MC6803-043`).
    pub(crate) const fn output_level(&self) -> bool {
        self.olvl
    }

    /// One E cycle: the counter clocks in the Low half, the compare and the
    /// overflow test follow, and the capture edge detector looks at `P20`.
    /// Returns the level the timer puts on `IRQ2` (`MC6803-037`).
    pub(crate) fn step(&mut self, p20: bool) -> bool {
        if self.preset {
            self.counter = COUNTER_PRESET;
            self.preset = false;
        } else {
            self.counter = self.counter.wrapping_add(1);
        }
        if self.counter == 0xffff {
            self.tcsr |= tcsr::TOF;
        }

        if self.oc_inhibit {
            self.oc_inhibit = false;
        } else if self.counter == self.ocr {
            self.tcsr |= tcsr::OCF;
            self.olvl = self.tcsr & tcsr::OLVL != 0;
        }

        let rising = self.tcsr & tcsr::IEDG != 0;
        let edge = if rising {
            p20 && !self.p20_last
        } else {
            !p20 && self.p20_last
        };
        self.p20_last = p20;
        if self.ic_due > 0 {
            self.ic_due -= 1;
            if self.ic_due == 0 {
                if self.ic_inhibit {
                    self.ic_due = 1;
                } else {
                    self.icr = self.counter;
                    self.tcsr |= tcsr::ICF;
                }
            }
        }
        if edge && self.ic_due == 0 {
            self.ic_due = 2;
        }
        self.ic_inhibit = false;

        self.irq2()
    }

    /// The three enabled flags, ORed (`MC6803-037`).
    pub(crate) const fn irq2(&self) -> bool {
        let t = self.tcsr;
        (t & tcsr::ICF != 0 && t & tcsr::EICI != 0)
            || (t & tcsr::OCF != 0 && t & tcsr::EOCI != 0)
            || (t & tcsr::TOF != 0 && t & tcsr::ETOI != 0)
    }

    /// The vector the priority encoder would pick for the timer's part of
    /// `IRQ2`, or none if no enabled flag is set (`MC6803-036`).
    pub(crate) const fn vector(&self) -> Option<u16> {
        let t = self.tcsr;
        if t & tcsr::ICF != 0 && t & tcsr::EICI != 0 {
            Some(vector::ICF)
        } else if t & tcsr::OCF != 0 && t & tcsr::EOCI != 0 {
            Some(vector::OCF)
        } else if t & tcsr::TOF != 0 && t & tcsr::ETOI != 0 {
            Some(vector::TOF)
        } else {
            None
        }
    }

    /// A read of one of the seven timer addresses, with its side effects
    /// (`MC6803-042`, `MC6803-044`, `MC6803-045`).
    pub(crate) fn read(&mut self, address: u16) -> u8 {
        match address {
            reg::TCSR => {
                let t = self.tcsr;
                self.tof_read = t & tcsr::TOF != 0;
                self.ocf_read = t & tcsr::OCF != 0;
                self.icf_read = t & tcsr::ICF != 0;
                t
            }
            reg::COUNTER_UPPER => {
                self.lsb = self.counter as u8;
                if self.tof_read {
                    self.tcsr &= !tcsr::TOF;
                    self.tof_read = false;
                }
                (self.counter >> 8) as u8
            }
            reg::COUNTER_LOWER => self.lsb,
            reg::OCR_UPPER => (self.ocr >> 8) as u8,
            reg::OCR_LOWER => self.ocr as u8,
            reg::ICR_UPPER => {
                self.ic_inhibit = true;
                if self.icf_read {
                    self.tcsr &= !tcsr::ICF;
                    self.icf_read = false;
                }
                (self.icr >> 8) as u8
            }
            _ => self.icr as u8,
        }
    }

    /// A write to one of the seven timer addresses.
    pub(crate) fn write(&mut self, address: u16, data: u8) {
        match address {
            reg::TCSR => {
                self.tcsr = (self.tcsr & !tcsr::WRITABLE_MASK) | (data & tcsr::WRITABLE_MASK);
            }
            reg::COUNTER_UPPER => self.preset = true,
            reg::COUNTER_LOWER => {}
            reg::OCR_UPPER => {
                self.ocr = (self.ocr & 0x00ff) | ((data as u16) << 8);
                self.oc_inhibit = true;
                self.clear_ocf();
            }
            reg::OCR_LOWER => {
                self.ocr = (self.ocr & 0xff00) | data as u16;
                self.clear_ocf();
            }
            _ => {}
        }
    }

    /// The second half of OCF's clearing sequence (`MC6803-045`).
    const fn clear_ocf(&mut self) {
        if self.ocf_read {
            self.tcsr &= !tcsr::OCF;
            self.ocf_read = false;
        }
    }
}
