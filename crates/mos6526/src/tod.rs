//! The time-of-day clock and its alarm.
//!
//! Four of its behaviours are this crate's choices — the edge counted and the
//! divider's behaviour while stopped (`MOS6526-OPEN-04`), the hours arithmetic
//! (`MOS6526-OPEN-05`), when the alarm compares (`MOS6526-OPEN-06`) and whether
//! the clock runs after `RES` (`MOS6526-OPEN-11`) — and each is taken at the
//! line that takes it.

/// The bits each of the four registers holds, tenths first
/// (`MOS6526-032`). Everything outside is unused and reads zero
/// (`MOS6526-014`).
const MASK: [u8; 4] = [0x0f, 0x7f, 0x7f, 0x9f];

/// The `PM` flag, bit 7 of the hours register (`MOS6526-031`).
const PM: u8 = 0x80;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Tod {
    /// Tenths, seconds, minutes, hours, each as the BCD byte its register
    /// shows (`MOS6526-032`).
    time: [u8; 4],
    /// The alarm, same layout, write-only (`MOS6526-035`).
    alarm: [u8; 4],
    /// The copy a read of hours took, held until a read of tenths
    /// (`MOS6526-037`).
    latched: Option<[u8; 4]>,
    /// Stopped by a write to hours, started by a write to tenths
    /// (`MOS6526-036`).
    running: bool,
    /// Input periods since the last tenths advance (`MOS6526-034`).
    divider: u8,
    /// The `TOD` pin's level last cycle, for the edge.
    prev_pin: bool,
}

impl Tod {
    /// Every register zero (`MOS6526-009`), and stopped — `MOS6526-OPEN-11`'s
    /// choice.
    pub(crate) const fn reset() -> Self {
        Self {
            time: [0; 4],
            alarm: [0; 4],
            latched: None,
            running: false,
            divider: 0,
            prev_pin: false,
        }
    }

    /// One `Ø2` cycle, given the `TOD` pin's level. Returns whether the alarm
    /// matched on an advance this cycle.
    ///
    /// A Low-to-High transition is one input period (`MOS6526-OPEN-04`), and
    /// six of them at 60 Hz or five at 50 Hz advance the tenths
    /// (`MOS6526-034`). The divider holds while the clock is stopped.
    pub(crate) fn tick(&mut self, pin: bool, fifty_hz: bool) -> bool {
        let rising = pin && !self.prev_pin;
        self.prev_pin = pin;
        if !rising || !self.running {
            return false;
        }
        self.divider += 1;
        if self.divider < if fifty_hz { 5 } else { 6 } {
            return false;
        }
        self.divider = 0;
        self.advance();
        self.time == self.alarm
    }

    /// One tenth of a second, in BCD with the carries (`MOS6526-031`).
    fn advance(&mut self) {
        if self.time[0] < 9 {
            self.time[0] += 1;
            return;
        }
        self.time[0] = 0;
        if bcd_increment(&mut self.time[1], 0x59) {
            return;
        }
        if bcd_increment(&mut self.time[2], 0x59) {
            return;
        }
        let flag = self.time[3] & PM;
        let hour = self.time[3] & !PM;
        self.time[3] = match hour {
            0x11 => (flag ^ PM) | 0x12,
            0x12 => flag | 0x01,
            0x09 => flag | 0x10,
            _ => flag | ((hour + 1) & MASK[3] & !PM),
        };
    }

    /// A read of register `reg`, `0` tenths to `3` hours.
    ///
    /// A read of hours latches all four, unless they already are; a read of
    /// tenths releases them (`MOS6526-037`). Whatever `ALARM` says, a read is
    /// of the time (`MOS6526-035`).
    pub(crate) fn read(&mut self, reg: usize) -> u8 {
        if reg == 3 && self.latched.is_none() {
            self.latched = Some(self.time);
        }
        let value = self.latched.unwrap_or(self.time)[reg];
        if reg == 0 {
            self.latched = None;
        }
        value
    }

    /// A write of `value` to register `reg`, to the alarm when `alarm` is set
    /// (`MOS6526-035`).
    ///
    /// A write of the time to hours stops the clock and a write to tenths
    /// starts it (`MOS6526-036`); an alarm write does neither, which is
    /// `MOS6526-OPEN-06`'s choice.
    pub(crate) const fn write(&mut self, reg: usize, value: u8, alarm: bool) {
        let value = value & MASK[reg];
        if alarm {
            self.alarm[reg] = value;
            return;
        }
        self.time[reg] = value;
        match reg {
            3 => self.running = false,
            0 => {
                self.running = true;
                self.divider = 0;
            }
            _ => {}
        }
    }

    /// Whether the clock is counting (`MOS6526-036`).
    pub(crate) const fn running(self) -> bool {
        self.running
    }
}

/// Add one to a two-digit BCD byte, wrapping to zero past `max`. Returns
/// whether the count stayed inside — `false` is a carry out.
const fn bcd_increment(byte: &mut u8, max: u8) -> bool {
    if *byte >= max {
        *byte = 0;
        return false;
    }
    if *byte & 0x0f == 9 {
        *byte = (*byte & 0xf0) + 0x10;
    } else {
        *byte += 1;
    }
    true
}
