//! One counter/timer channel: its two registers, its two counters and the
//! control logic between them (`Z80CTC-007` to `Z80CTC-022`).

use crate::control::{
    CONTROL, COUNTER, INTERRUPT, PRESCALE_256, RISING_EDGE, SOFTWARE_RESET, TIME_CONSTANT, TRIGGER,
};

/// Where a channel stands between programming and counting.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Run {
    /// Not counting: after a reset, or never given a time constant.
    Stopped,
    /// Counting starts with the clock after this many more.
    Arming(u8),
    /// A timer waiting for its `CLK/TRG` edge (`Z80CTC-021`). `armed` is
    /// false until the clock after the time constant was taken, before which
    /// no edge starts it.
    WaitTrigger { armed: bool },
    /// Counting.
    Running,
}

/// The channel's whole state.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Chan {
    /// The channel control register (`Z80CTC-007`).
    pub(crate) control: u8,
    /// The time constant register; `0` stands for 256 (`Z80CTC-009`).
    pub(crate) tc: u8,
    /// The down-counter. A load of `0` counts 256 before it is next zero.
    pub(crate) counter: u8,
    /// System clocks left before the prescaler next clocks the down-counter.
    prescaler: u16,
    pub(crate) run: Run,
    /// The next word written to this channel is its time constant
    /// (`Z80CTC-008`).
    tc_next: bool,
    /// `CLK/TRG` at the last clock, for its edges.
    clk_prev: bool,
    /// A control word changed the active edge while the channel operated,
    /// which counts as one active edge (`Z80CTC-014`).
    forced_edge: bool,
    /// A zero count with interrupts enabled, not yet a request: it becomes
    /// one with the next clock at which `M1` is High (`Z80CTC-023`,
    /// `Z80CTC-025`).
    pub(crate) raise: bool,
    /// An interrupt requested and not yet acknowledged.
    pub(crate) pending: bool,
    /// An interrupt acknowledged and not yet released by `RETI`.
    pub(crate) in_service: bool,
}

impl Chan {
    /// A channel as `RESET` leaves it (`Z80CTC-032`), seeing `clk` on its
    /// `CLK/TRG` pin.
    pub(crate) const fn reset(control: u8, tc: u8, counter: u8, clk: bool) -> Self {
        Self {
            control: control & !INTERRUPT,
            tc,
            counter,
            prescaler: 0,
            run: Run::Stopped,
            tc_next: false,
            clk_prev: clk,
            forced_edge: false,
            raise: false,
            pending: false,
            in_service: false,
        }
    }

    /// The prescaler's factor (`Z80CTC-019`).
    const fn factor(&self) -> u16 {
        if self.control & PRESCALE_256 != 0 {
            256
        } else {
            16
        }
    }

    /// A word the CPU wrote to this channel. Returns it as the vector where
    /// it is one (`Z80CTC-010`); which channel may take a vector is the
    /// caller's.
    pub(crate) fn write(&mut self, byte: u8) -> Option<u8> {
        if self.tc_next {
            self.tc_next = false;
            self.tc = byte;
            if self.run == Run::Stopped {
                self.counter = byte;
                self.prescaler = self.factor();
                self.run = if self.control & COUNTER == 0 && self.control & TRIGGER != 0 {
                    Run::WaitTrigger { armed: false }
                } else {
                    Run::Arming(1)
                };
            }
            return None;
        }
        if byte & CONTROL == 0 {
            return Some(byte);
        }
        let old = self.control;
        self.control = byte;
        self.tc_next = byte & TIME_CONSTANT != 0;
        if byte & SOFTWARE_RESET != 0 {
            self.run = Run::Stopped;
            self.forced_edge = false;
        } else if self.run != Run::Stopped && (old ^ byte) & RISING_EDGE != 0 {
            self.forced_edge = true;
        }
        None
    }

    /// Whether `level` on `CLK/TRG`, after the last, is the selected edge.
    const fn active_edge(&self, level: bool) -> bool {
        if self.control & RISING_EDGE != 0 {
            level && !self.clk_prev
        } else {
            !level && self.clk_prev
        }
    }

    /// One rising edge of the system clock, with `level` on `CLK/TRG`.
    /// Returns whether the down-counter reached zero at it.
    pub(crate) fn clock(&mut self, level: bool) -> bool {
        let edge = self.active_edge(level) | core::mem::take(&mut self.forced_edge);
        self.clk_prev = level;
        match self.run {
            Run::Stopped => false,
            Run::Arming(0) => {
                self.run = Run::Running;
                self.count(edge)
            }
            Run::Arming(n) => {
                self.run = Run::Arming(n - 1);
                false
            }
            Run::WaitTrigger { armed: false } => {
                self.run = Run::WaitTrigger { armed: true };
                false
            }
            Run::WaitTrigger { armed: true } => {
                if edge {
                    self.run = Run::Arming(0);
                }
                false
            }
            Run::Running => self.count(edge),
        }
    }

    fn count(&mut self, edge: bool) -> bool {
        if self.control & COUNTER != 0 {
            return edge && self.decrement();
        }
        self.prescaler -= 1;
        if self.prescaler == 0 {
            self.prescaler = self.factor();
            self.decrement()
        } else {
            false
        }
    }

    /// `Z80CTC-016`: the count from 1 to 0 reloads the time constant.
    const fn decrement(&mut self) -> bool {
        self.counter = self.counter.wrapping_sub(1);
        if self.counter == 0 {
            self.counter = self.tc;
            true
        } else {
            false
        }
    }
}
