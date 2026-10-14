//! The Sega 32X's PWM sound source, at its five registers and its two pulse
//! outputs.
//!
//! Every rule here is stated by Sega's *32X Hardware Manual* (MAR-32-R4-072294)
//! or by the four hardware statements in its *PWM Driver Ver. 1.06*. What
//! neither says is the crate's choice, and each choice names its open item.
//!
//! **What the crate chooses**, each an open item: a width past the period
//! holding the output high for the whole period (`SEGA32X-PWM-OPEN-02`); all
//! twelve bits of a width (`SEGA32X-PWM-OPEN-06`); `LMD` 0,1 as L ch and 1,0
//! as R ch (`SEGA32X-PWM-OPEN-03`); a setting the manual does not allow
//! leaving the output off (`SEGA32X-PWM-OPEN-04`); a write taking effect at
//! once and each output high for the first `width` clocks of a period
//! (`SEGA32X-PWM-OPEN-07`); MONO's `FULL` if either FIFO is full and `EMPTY`
//! if both are empty (`SEGA32X-PWM-OPEN-09`); the FIFO after a write while
//! full or an off-and-on (`SEGA32X-PWM-OPEN-10`); and an output that is off
//! held low (`SEGA32X-PWM-OPEN-11`).
//!
//! # Examples
//!
//! A period of 1474 clocks with half of it high on both outputs, the duty
//! [`Sega32xPwm::audio`] reads as 0:
//!
//! ```
//! use sega32x_pwm::{Select, Sega32xPwm, reg};
//!
//! let mut pwm = Sega32xPwm::new();
//! pwm.write(Select::new(reg::CONTROL), 0x105);
//! pwm.write(Select::new(reg::CYCLE), 1475);
//! pwm.write(Select::new(reg::MONO), 738);
//! pwm.advance(1474);
//! assert_eq!(pwm.widths().cycles_per_sample.map(|p| p.get()), Some(1474));
//! assert_eq!(pwm.widths().left_cycles, Some(737));
//! assert_eq!(pwm.audio().map(|s| s.0), [0, 0]);
//! ```
//!
//! # The shape
//!
//! - [`Sega32xPwm::write`] and [`Sega32xPwm::read`] are word accesses to the five registers,
//!   by a [`Select`] of the address lines ([`reg`]). Which bus and address reach them is the
//!   machine's (`SEGA32X-PWM-002`).
//! - [`Sega32xPwm::advance`] moves the part on by a count of its base clock,
//!   23.01 MHz NTSC (`SEGA32X-PWM-006`). [`Sega32xPwm::tick`] is one clock.
//! - [`Sega32xPwm::outputs`] is the two outputs' levels at the current clock, and
//!   [`Sega32xPwm::widths`] the width each is putting out in this sampling period,
//!   over the period: the pulse the board integrates (`SEGA32X-PWM-022`).
//!   [`Sega32xPwm::audio`] is that duty as a signed sample.
//! - [`Sega32xPwm::is_irq_pending`] and [`Sega32xPwm::take_dreq`] are the timer's two outputs
//!   (`SEGA32X-PWM-020`, `-021`); the clear is the interface chip's.
//!
//! # What is not here
//!
//! Byte access and the Mega Drive side's read-only bits (`SEGA32X-PWM-OPEN-08`),
//! anything electrical (`SEGA32X-PWM-OPEN-01`), and the undefined FIFO data
//! of the first enable after power-up (`SEGA32X-PWM-OPEN-05`).

#![no_std]

/// The register addresses, each one 16-bit word, as the address lines
/// `A3`–`A1` carry them, which [`Select::new`] takes: the Mega Drive side's
/// `A15130H` + 2 × index, the SH2 side's `20004030H` + 2 × index
/// (`SEGA32X-PWM-002`).
pub mod reg {
    /// PWM control (`SEGA32X-PWM-003`).
    pub const CONTROL: u8 = 0;
    /// Cycle (`SEGA32X-PWM-007`).
    pub const CYCLE: u8 = 1;
    /// L ch pulse width (`SEGA32X-PWM-009`).
    pub const LEFT: u8 = 2;
    /// R ch pulse width (`SEGA32X-PWM-009`).
    pub const RIGHT: u8 = 3;
    /// MONO pulse width, written to both (`SEGA32X-PWM-009`).
    pub const MONO: u8 = 4;
}

/// The PWM control register's fields ([`reg::CONTROL`]).
pub mod control {
    /// Bits 11–8, the timer interval (`SEGA32X-PWM-020`).
    pub const TM_MASK: u16 = 0x0F00;
    /// Bit 7, DREQ1 enable (`SEGA32X-PWM-019`).
    pub const RTP: u16 = 0x0080;
    /// Bits 3–2, `RMD0` and `RMD1`: what the right output carries
    /// (`SEGA32X-PWM-005`).
    pub const RMD_MASK: u16 = 0x000C;
    /// Bits 1–0, `LMD0` and `LMD1`: what the left output carries
    /// (`SEGA32X-PWM-004`).
    pub const LMD_MASK: u16 = 0x0003;
}

/// The pulse width registers' bits as read ([`reg::LEFT`], [`reg::RIGHT`],
/// [`reg::MONO`]).
pub mod pulse_width {
    /// Bit 15: the FIFO has no space (`SEGA32X-PWM-013`).
    pub const FULL: u16 = 0x8000;
    /// Bit 14: the FIFO holds no data (`SEGA32X-PWM-013`).
    pub const EMPTY: u16 = 0x4000;
}

use core::num::NonZeroUsize;

use control::{LMD_MASK, RMD_MASK, RTP, TM_MASK};
use pulse_width::{EMPTY, FULL};
use units::Audio;

/// A pin's logic level.
pub use signal::Level;

/// Words each FIFO holds (`SEGA32X-PWM-009`).
pub const FIFO_WORDS: usize = 3;

/// The levels of the word address lines `A3`–`A1` an access presents, `A1`
/// in bit 0: a register's index in [`reg`] (`SEGA32X-PWM-002`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Select(u8);

impl Select {
    /// The select `levels` name, masked to `A3`–`A1`, the low three bits.
    #[must_use]
    pub const fn new(levels: u8) -> Self {
        Self(levels & 0b111)
    }

    /// The levels of `A3`–`A1`, `A1` in bit 0.
    #[must_use]
    pub const fn levels(self) -> u8 {
        self.0
    }
}

/// A channel, the data one FIFO carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Channel {
    /// The L ch FIFO.
    Left,
    /// The R ch FIFO.
    Right,
}

/// The largest magnitude either channel of [`Sega32xPwm::audio`] returns.
pub const AUDIO_FULL_SCALE: i32 = 32_767;

/// The two pulse outputs' levels at the current base clock.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// The left output.
    pub left: Level,
    /// The right output.
    pub right: Level,
}

impl Default for Outputs {
    /// Both outputs Low.
    fn default() -> Self {
        Self {
            left: Level::Low,
            right: Level::Low,
        }
    }
}

/// What each output is putting out in the current sampling period, in base
/// clocks.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
#[non_exhaustive]
pub struct Widths {
    /// Base clocks in the sampling period; `None` while the counter is
    /// stopped.
    pub cycles_per_sample: Option<NonZeroUsize>,
    /// The left output's high time, at most `cycles_per_sample` and 0 while
    /// that is `None`; `None` while the output is off.
    pub left_cycles: Option<usize>,
    /// The right output's high time, likewise.
    pub right_cycles: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Fifo {
    words: [u16; FIFO_WORDS],
    len: u8,
    /// A write landed while `FULL` read 1, which turns `FULL` off until the
    /// FIFO next drains (`SEGA32X-PWM-012`, `SEGA32X-PWM-OPEN-10`).
    overrun: bool,
    /// `FULL` held on over an emptied FIFO by an off-and-on
    /// (`SEGA32X-PWM-017`, `SEGA32X-PWM-OPEN-10`).
    stale_full: bool,
}

impl Fifo {
    const fn full(&self) -> bool {
        self.stale_full || (self.len as usize == FIFO_WORDS && !self.overrun)
    }

    const fn empty(&self) -> bool {
        self.len == 0
    }

    /// `SEGA32X-PWM-011`, `-012`.
    fn push(&mut self, w: u16) {
        let was_full = self.full();
        if self.len as usize == FIFO_WORDS {
            self.words.copy_within(1.., 0);
            self.words[FIFO_WORDS - 1] = w;
        } else {
            self.words[self.len as usize] = w;
            self.len += 1;
        }
        if was_full {
            self.overrun = self.len as usize == FIFO_WORDS;
            self.stale_full = false;
        }
    }

    /// `SEGA32X-PWM-010`.
    fn pop(&mut self) -> Option<u16> {
        if self.len == 0 {
            return None;
        }
        let w = self.words[0];
        self.words.copy_within(1.., 0);
        self.len -= 1;
        self.overrun = false;
        Some(w)
    }
}

/// The part.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Sega32xPwm {
    control: u16,
    cycle: u16,
    fifo: [Fifo; 2],
    /// Each channel's width in force, already `value − 1` (`SEGA32X-PWM-008`).
    width: [u16; 2],
    /// Base clocks into the current sampling period.
    phase: u16,
    /// Sampling periods since the timer last fired.
    periods: u8,
    irq: bool,
    dreq: bool,
}

impl Default for Sega32xPwm {
    fn default() -> Self {
        Self::new()
    }
}

/// A 12-bit register value less one, modulo 4096 (`SEGA32X-PWM-007`, `-008`).
const fn less_one(v: u16) -> u16 {
    v.wrapping_sub(1) & 0x0FFF
}

impl Sega32xPwm {
    /// The part after reset: both FIFOs empty and both widths 0
    /// (`SEGA32X-PWM-018`), `RTP` off (`SEGA32X-PWM-019`), and every other
    /// bit 0, this crate's choice under `SEGA32X-PWM-OPEN-05`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            control: 0,
            cycle: 0,
            fifo: [Fifo {
                words: [0; FIFO_WORDS],
                len: 0,
                overrun: false,
                stale_full: false,
            }; 2],
            width: [0; 2],
            phase: 0,
            periods: 0,
            irq: false,
            dreq: false,
        }
    }

    /// What one output's two control bits select; `None` is off, and so is
    /// the setting the manual does not allow (`SEGA32X-PWM-004`, `-005`,
    /// `SEGA32X-PWM-OPEN-04`).
    const fn source(field: u16, one: Channel, two: Channel) -> Option<Channel> {
        match field {
            0b01 => Some(one),
            0b10 => Some(two),
            _ => None,
        }
    }

    /// The channel the left output carries (`SEGA32X-PWM-004`).
    #[must_use]
    pub const fn lmd(&self) -> Option<Channel> {
        Self::source(self.control & LMD_MASK, Channel::Left, Channel::Right)
    }

    /// The channel the right output carries (`SEGA32X-PWM-005`).
    #[must_use]
    pub const fn rmd(&self) -> Option<Channel> {
        Self::source(
            (self.control & RMD_MASK) >> 2,
            Channel::Right,
            Channel::Left,
        )
    }

    /// Sound production is on: at least one output is (`SEGA32X-PWM-014`).
    #[must_use]
    pub const fn is_producing(&self) -> bool {
        self.lmd().is_some() || self.rmd().is_some()
    }

    /// A word written to the register `a` selects (the word index of
    /// [`reg`]); an address past 4 is ignored.
    pub fn write(&mut self, a: Select, d: u16) {
        let a = a.levels();
        match a {
            reg::CONTROL => {
                let was = self.is_producing();
                self.control = d & (TM_MASK | RTP | RMD_MASK | LMD_MASK);
                if !was && self.is_producing() {
                    self.start();
                }
            }
            reg::CYCLE => self.cycle = d & 0x0FFF,
            reg::LEFT | reg::RIGHT | reg::MONO => {
                if !self.is_producing() {
                    return;
                }
                let w = d & 0x0FFF;
                if a != reg::RIGHT {
                    self.fifo[0].push(w);
                }
                if a != reg::LEFT {
                    self.fifo[1].push(w);
                }
            }
            _ => {}
        }
    }

    /// A word read from the register at `a`, as [`write`](Self::write) takes
    /// it. The pulse width registers' bits
    /// 11–0 are undefined and read 0 here (`SEGA32X-PWM-013`); the MONO
    /// register's flags are the crate's choice (`SEGA32X-PWM-OPEN-09`).
    #[must_use]
    pub fn read(&self, a: Select) -> u16 {
        let a = a.levels();
        let flags = |full: bool, empty: bool| {
            (if full { FULL } else { 0 }) | (if empty { EMPTY } else { 0 })
        };
        let [l, r] = &self.fifo;
        match a {
            reg::CONTROL => self.control,
            reg::CYCLE => self.cycle,
            reg::LEFT => flags(l.full(), l.empty()),
            reg::RIGHT => flags(r.full(), r.empty()),
            reg::MONO => flags(l.full() || r.full(), l.empty() && r.empty()),
            _ => 0,
        }
    }

    /// Production turned on: the counter starts a period, and a FIFO left
    /// full by the last turn-off reads full and empty both
    /// (`SEGA32X-PWM-017`).
    fn start(&mut self) {
        self.phase = 0;
        self.periods = 0;
        for f in &mut self.fifo {
            if f.full() {
                *f = Fifo {
                    stale_full: true,
                    ..Fifo::default()
                };
            }
        }
    }

    /// The end of a sampling period: each FIFO gives up a word, or its
    /// channel's width holds (`SEGA32X-PWM-010`, `-016`), and the timer
    /// counts (`SEGA32X-PWM-020`, `-021`).
    fn boundary(&mut self) {
        for (f, w) in self.fifo.iter_mut().zip(&mut self.width) {
            if let Some(v) = f.pop() {
                *w = less_one(v);
            }
        }
        self.periods += 1;
        let tm = ((self.control & TM_MASK) >> 8) as u8;
        if self.periods >= if tm == 0 { 16 } else { tm } {
            self.periods = 0;
            self.irq = true;
            if self.control & RTP != 0 {
                self.dreq = true;
            }
        }
        self.phase = 0;
    }

    /// Base clocks in one sampling period, read off the cycle register at
    /// every clock, so a write takes effect at once (`SEGA32X-PWM-007`,
    /// `SEGA32X-PWM-OPEN-07`); `None` while the counter is stopped, with
    /// production off (`SEGA32X-PWM-014`) or the cycle register holding 1
    /// (`SEGA32X-PWM-007`).
    #[must_use]
    pub const fn cycles_per_sample(&self) -> Option<NonZeroUsize> {
        if self.is_producing() {
            NonZeroUsize::new(less_one(self.cycle) as usize)
        } else {
            None
        }
    }

    /// Move on `n` base clocks. Nothing moves while production is off
    /// (`SEGA32X-PWM-014`) or the cycle register holds 1
    /// (`SEGA32X-PWM-007`).
    pub fn advance(&mut self, n: usize) {
        let mut clocks = n;
        while clocks > 0 {
            let Some(period) = self.cycles_per_sample() else {
                return;
            };
            let period = period.get();
            let left = period.saturating_sub(usize::from(self.phase)).max(1);
            if clocks < left {
                self.phase += clocks as u16;
                return;
            }
            clocks -= left;
            self.boundary();
        }
    }

    /// One base clock.
    pub fn tick(&mut self) {
        self.advance(1);
    }

    fn high(&self, source: Option<Channel>) -> Option<usize> {
        let period = self.cycles_per_sample().map_or(0, NonZeroUsize::get);
        source.map(|c| usize::from(self.width[c as usize]).min(period))
    }

    /// Each output's high time in this period (`SEGA32X-PWM-022`).
    #[must_use]
    pub fn widths(&self) -> Widths {
        Widths {
            cycles_per_sample: self.cycles_per_sample(),
            left_cycles: self.high(self.lmd()),
            right_cycles: self.high(self.rmd()),
        }
    }

    /// The two outputs' levels at this clock: high for the first `width`
    /// clocks of each period (`SEGA32X-PWM-022`, `SEGA32X-PWM-OPEN-07`); an
    /// output that is off is low (`SEGA32X-PWM-OPEN-11`).
    #[must_use]
    pub fn outputs(&self) -> Outputs {
        let w = self.widths();
        let level =
            |h: Option<usize>| Level::from_bool(h.is_some_and(|h| usize::from(self.phase) < h));
        Outputs {
            left: level(w.left_cycles),
            right: level(w.right_cycles),
        }
    }

    /// The timer interrupt has fired and not been cleared
    /// (`SEGA32X-PWM-020`).
    #[must_use]
    pub const fn is_irq_pending(&self) -> bool {
        self.irq
    }

    /// The interface chip's PWM interrupt clear.
    pub fn clear_irq(&mut self) {
        self.irq = false;
    }

    /// A DREQ1 transfer request has been raised since the last call, which
    /// takes it (`SEGA32X-PWM-021`).
    #[must_use]
    pub fn take_dreq(&mut self) -> bool {
        core::mem::take(&mut self.dreq)
    }

    /// Each output's pulse width over its period, as `[left, right]`: signed,
    /// −[`AUDIO_FULL_SCALE`] for a pulse that is never high, +[`AUDIO_FULL_SCALE`]
    /// for one high the whole period, truncated toward 0 between. An output that is off, and both
    /// while the counter is stopped, read 0, as does a pulse high for half the
    /// period. It is the duty the board integrates (`SEGA32X-PWM-022`), not a
    /// level.
    #[must_use]
    pub fn audio(&self) -> [Audio<AUDIO_FULL_SCALE>; 2] {
        let w = self.widths();
        let duty = |h: Option<usize>| match (h, w.cycles_per_sample) {
            (Some(h), Some(p)) => {
                let p = p.get() as i64;
                Audio(((2 * h as i64 - p) * i64::from(AUDIO_FULL_SCALE) / p) as i32)
            }
            _ => Audio(0),
        };
        [duty(w.left_cycles), duty(w.right_cycles)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// After Burner Complete's three set-up writes give a 1474-clock period
    /// and carry L to the left output and R to the right.
    #[test]
    fn after_burner_setup() {
        let mut p = Sega32xPwm::new();
        p.write(Select::new(reg::CONTROL), 0x105);
        p.write(Select::new(reg::CYCLE), 0x5C3);
        p.write(Select::new(reg::LEFT), 0x2E2);
        p.write(Select::new(reg::RIGHT), 0x2E2);
        p.advance(1474);
        assert_eq!(
            p.widths(),
            Widths {
                cycles_per_sample: NonZeroUsize::new(1474),
                left_cycles: Some(0x2E1),
                right_cycles: Some(0x2E1)
            }
        );
    }
}
