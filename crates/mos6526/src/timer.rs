//! One interval timer: a sixteen-bit counter, a sixteen-bit latch, and the
//! control register that drives it.
//!
//! Timer A and timer B are two of these; what differs between them — which
//! clocks each may count, which port line each may drive, and the three bits
//! of `CRA` that are not timer A's at all — is decided by the caller.

/// The bits of the two control registers, `CRA` and `CRB` (`MOS6526-021`).
pub mod control {
    /// `START`, bit 0 of either control register (`MOS6526-022`).
    pub const START: u8 = 0x01;
    /// `PBON`, bit 1 (`MOS6526-023`).
    pub const PBON: u8 = 0x02;
    /// `OUTMODE`, bit 2 — set for toggle, clear for pulse (`MOS6526-024`).
    pub const OUTMODE: u8 = 0x04;
    /// `RUNMODE`, bit 3 — set for one-shot, clear for continuous
    /// (`MOS6526-025`).
    pub const RUNMODE: u8 = 0x08;
    /// `LOAD`, bit 4 — a strobe that stores nothing (`MOS6526-026`).
    pub const LOAD: u8 = 0x10;
    /// `INMODE`, bit 5 of `CRA` — set to count `CNT` (`MOS6526-027`). In `CRB`
    /// this is the low bit of a two-bit field (`MOS6526-028`).
    pub const INMODE: u8 = 0x20;
    /// `SPMODE`, bit 6 of `CRA` — set for serial output (`MOS6526-038`). In `CRB`
    /// this is the high bit of the `INMODE` field.
    pub const SPMODE: u8 = 0x40;
    /// `TODIN`, bit 7 of `CRA` — set for a 50 Hz `TOD` input (`MOS6526-033`). In
    /// `CRB` this is `ALARM` (`MOS6526-035`).
    pub const TODIN: u8 = 0x80;
    /// `ALARM`, bit 7 of `CRB` — set for a write to the TOD registers to
    /// set the alarm (`MOS6526-035`, `MOS6526-047`). In `CRA` this is
    /// `TODIN`.
    pub const ALARM: u8 = 0x80;
}

use control::{LOAD, OUTMODE, PBON, RUNMODE, START};

/// One timer.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Timer {
    /// The write-only latch (`MOS6526-020`).
    pub latch: u16,
    /// The read-only counter (`MOS6526-020`).
    pub counter: u16,
    /// The control register as stored: bit 4 is never held (`MOS6526-026`).
    pub cr: u8,
    /// The toggle flip-flop behind `OUTMODE` = 1 (`MOS6526-024`).
    toggle: bool,
    /// The one-cycle pulse behind `OUTMODE` = 0, for the current cycle
    /// (`MOS6526-024`).
    pulse: bool,
}

impl Timer {
    /// The state `RES` leaves: control register zero, latch all ones
    /// (`MOS6526-009`). The counter is loaded from the latch, this crate's
    /// choice; the toggle output is Low (`MOS6526-024`).
    pub(crate) const fn reset() -> Self {
        Self {
            latch: 0xffff,
            counter: 0xffff,
            cr: 0,
            toggle: false,
            pulse: false,
        }
    }

    pub(crate) const fn running(self) -> bool {
        self.cr & START != 0
    }

    pub(crate) const fn one_shot(self) -> bool {
        self.cr & RUNMODE != 0
    }

    pub(crate) const fn pb_on(self) -> bool {
        self.cr & PBON != 0
    }

    /// The level the timer output would put on its port B line this cycle
    /// (`MOS6526-024`).
    pub(crate) const fn level(self) -> bool {
        if self.cr & OUTMODE != 0 {
            self.toggle
        } else {
            self.pulse
        }
    }

    /// One `Ø2` cycle, with `pulse` saying whether the timer's selected clock
    /// delivered a count this cycle. Returns whether the timer underflowed.
    ///
    /// The counter takes the values `N` down to `0`, one per counted pulse,
    /// and the pulse after `0` is the underflow: reload from the latch
    /// (`MOS6526-029`), stop if one-shot (`MOS6526-022`, `MOS6526-025`), and
    /// drive the output (`MOS6526-024`). That makes a period `N + 1`
    /// (`MOS6526-030`); whether a part spends a pulse on the reload is
    /// `MOS6526-OPEN-02`.
    pub(crate) fn count(&mut self, pulse: bool) -> bool {
        self.pulse = false;
        if !pulse || !self.running() {
            return false;
        }
        if self.counter == 0 {
            self.counter = self.latch;
            if self.one_shot() {
                self.cr &= !START;
            }
            self.toggle = !self.toggle;
            self.pulse = true;
            true
        } else {
            self.counter -= 1;
            false
        }
    }

    /// A write to the control register.
    ///
    /// Starting a stopped timer sets the toggle output High (`MOS6526-024`);
    /// a one in bit 4 loads the latch into the counter and is not stored
    /// (`MOS6526-026`).
    pub(crate) fn write_cr(&mut self, value: u8) {
        if value & START != 0 && !self.running() {
            self.toggle = true;
        }
        if value & LOAD != 0 {
            self.counter = self.latch;
        }
        self.cr = value & !LOAD;
    }

    /// A write to the low byte: the latch only (`MOS6526-021`, `MOS6526-029`).
    pub(crate) const fn write_lo(&mut self, value: u8) {
        self.latch = (self.latch & 0xff00) | u16::from_le_bytes([value, 0]);
    }

    /// A write to the high byte: the latch, and the counter too while the
    /// timer is stopped (`MOS6526-029`).
    pub(crate) const fn write_hi(&mut self, value: u8) {
        self.latch = (self.latch & 0x00ff) | u16::from_le_bytes([0, value]);
        if !self.running() {
            self.counter = self.latch;
        }
    }

    /// A read of the low byte: the counter (`MOS6526-021`).
    pub(crate) const fn read_lo(self) -> u8 {
        self.counter.to_le_bytes()[0]
    }

    /// A read of the high byte: the counter (`MOS6526-021`).
    pub(crate) const fn read_hi(self) -> u8 {
        self.counter.to_le_bytes()[1]
    }
}
