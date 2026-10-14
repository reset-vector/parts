//! The serial port: an eight-bit shift register that clocks in on `CNT` or
//! clocks out from timer A.
//!
//! The phase of the first bit after a write, and the levels the two pins stand
//! at in input mode, and what a change of `SPMODE` does to a byte under way,
//! are `MOS6526-OPEN-10`'s choices.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Serial {
    /// The serial data register, register `C` (`MOS6526-038`).
    pub sdr: u8,
    /// The shift register behind it.
    shift: u8,
    /// Input mode: bits received so far. Output mode: bits still to send.
    bits: u8,
    /// Output mode: `SDR` holds a byte not yet moved into the shift register
    /// (`MOS6526-041`).
    pending: bool,
    /// Output mode: a byte is in flight.
    active: bool,
    /// Output mode: the part's own drive on `CNT`, `false` pulling the line
    /// Low (`MOS6526-040`).
    cnt_drive: bool,
    /// Output mode: the part's own drive on `SP` (`MOS6526-041`).
    sp_drive: bool,
}

impl Serial {
    /// Empty, idle, both open-drain lines released (`MOS6526-009`).
    pub(crate) const fn reset() -> Self {
        Self {
            sdr: 0,
            shift: 0,
            bits: 0,
            pending: false,
            active: false,
            cnt_drive: true,
            sp_drive: true,
        }
    }

    /// Input mode, one cycle: `cnt_rose` is a Low-to-High transition on `CNT`
    /// this cycle, `sp` the level on `SP`. Returns whether the eighth bit just
    /// arrived and `SDR` was loaded (`MOS6526-039`).
    pub(crate) const fn input(&mut self, cnt_rose: bool, sp: bool) -> bool {
        if !cnt_rose {
            return false;
        }
        self.shift = (self.shift << 1) | (sp as u8);
        self.bits += 1;
        if self.bits < 8 {
            return false;
        }
        self.bits = 0;
        self.sdr = self.shift;
        true
    }

    /// Output mode, one timer A underflow. Returns whether the eighth `CNT`
    /// pulse just completed (`MOS6526-041`).
    ///
    /// Every underflow moves `CNT` once, so a bit takes two of them
    /// (`MOS6526-040`). A byte waiting in `SDR` is loaded on the underflow that
    /// starts it, and that same underflow takes `CNT` Low with the first bit
    /// on `SP` — `MOS6526-OPEN-10`'s choice. Bits are most significant first
    /// and each becomes valid on the falling edge of `CNT`.
    pub(crate) const fn output_underflow(&mut self) -> bool {
        if !self.active {
            if !self.pending {
                return false;
            }
            self.load();
        }
        if self.cnt_drive {
            self.cnt_drive = false;
            self.sp_drive = self.shift & 0x80 != 0;
            self.shift <<= 1;
            self.bits -= 1;
            return false;
        }
        self.cnt_drive = true;
        if self.bits != 0 {
            return false;
        }
        if self.pending {
            self.load();
        } else {
            self.active = false;
        }
        true
    }

    const fn load(&mut self) {
        self.pending = false;
        self.shift = self.sdr;
        self.bits = 8;
        self.active = true;
    }

    /// A write to `SDR`: the byte waits for the shift register
    /// (`MOS6526-040`, `MOS6526-041`).
    pub(crate) const fn write(&mut self, value: u8) {
        self.sdr = value;
        self.pending = true;
    }

    /// A write to `CRA` that changes `SPMODE`: a byte in flight or waiting is
    /// dropped, the bits received so far are forgotten, and both lines are
    /// released — `MOS6526-OPEN-10`'s choice.
    pub(crate) const fn mode_change(&mut self) {
        self.bits = 0;
        self.pending = false;
        self.active = false;
        self.cnt_drive = true;
        self.sp_drive = true;
    }

    /// The part's own drive on `CNT` and `SP`: what it holds in output mode,
    /// and nothing at all in input mode, where both lines are the outside's
    /// (`MOS6526-011`, `MOS6526-OPEN-10`).
    pub(crate) const fn drives(self, output_mode: bool) -> (bool, bool) {
        if output_mode {
            (self.cnt_drive, self.sp_drive)
        } else {
            (true, true)
        }
    }
}
