//! The timer: an 8-bit counter behind a 7-bit prescaler (`MC68705P5-022` to
//! `MC68705P5-028`).

/// The bits of the timer control register, `TCR` at `$009`
/// (`MC68705P5-023`).
pub mod tcr {
    /// Bit 7, `TIR`: the counter has reached zero.
    pub const TIR: u8 = 0x80;
    /// Bit 6, `TIM`: the timer interrupt is masked.
    pub const TIM: u8 = 0x40;
    /// Bit 5, `TIN`: the counter's input select.
    pub const TIN: u8 = 0x20;
    /// Bit 4, `TIE`: the `TIMER` pin's enable.
    pub const TIE: u8 = 0x10;
    /// Bit 3, `PSC`: a write of 1 clears the prescaler; it reads 0.
    pub const PSC: u8 = 0x08;
    /// Bits 2–0, `PS2`–`PS0`: the prescaler's division, a power of two.
    pub const PRESCALE_MASK: u8 = 0x07;
}

/// The bits of the mask option register, `MOR` at `$784`
/// (`MC68705P5-034`).
pub mod mor {
    /// Bit 6, `TOPT`: the timer's options come from `MOR`, not `TCR`.
    pub const TOPT: u8 = 0x40;
    /// Bit 5, `CLS`: the counter's input select, read with `TOPT` at 1; with
    /// `TOPT` at 0, reset copies it into `TCR`'s `TIN`.
    pub const CLS: u8 = 0x20;
    /// Bit 4, `TIE`: with `TOPT` at 0, reset copies it into `TCR`'s `TIE`.
    /// With `TOPT` at 1 it is not read, and the `TIMER` pin is enabled.
    pub const TIE: u8 = 0x10;
    /// Bits 2–0, `P2`–`P0`: the prescaler's division, read with `TOPT` at 1;
    /// with `TOPT` at 0, reset copies them into `TCR`'s `PS2`–`PS0`.
    pub const PRESCALE_MASK: u8 = 0x07;
}

use mor::{CLS as MOR_CLS, PRESCALE_MASK as MOR_P, TIE as MOR_TIE, TOPT as MOR_TOPT};
use tcr::{PRESCALE_MASK as PS, PSC, TIE, TIM, TIN, TIR};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) struct Timer {
    /// `TDR`.
    pub(crate) data: u8,
    /// The stored bits of `TCR`: `TIR`, `TIM`, and with `TOPT` = 0 also
    /// `TIN`, `TIE` and `PS`. `PSC` is never stored.
    pub(crate) control: u8,
    /// The prescaler, seven bits.
    pub(crate) prescaler: u8,
    /// `MOR`, latched at reset (`MC68705P5-029`).
    pub(crate) mor: u8,
    /// The `TIMER` pin at the previous step, for the rising edge.
    pub(crate) pin_before: bool,
}

impl Timer {
    pub(crate) const fn reset(mor: u8) -> Self {
        let options = if mor & MOR_TOPT == 0 {
            (mor & MOR_CLS) | (mor & MOR_TIE) | (mor & MOR_P)
        } else {
            0
        };
        Self {
            data: 0xFF,
            control: TIM | options,
            prescaler: 0x7F,
            mor,
            pin_before: false,
        }
    }

    /// The counter, the stored `TCR` bits and the prescaler set from
    /// [`Registers`](crate::Registers), masked as the part stores them.
    pub(crate) const fn load(&mut self, data: u8, control: u8, prescaler: u8) {
        self.data = data;
        self.control = if self.mor_controlled() {
            control & (TIR | TIM)
        } else {
            control & !PSC
        };
        self.prescaler = prescaler & 0x7F;
    }

    const fn mor_controlled(&self) -> bool {
        self.mor & MOR_TOPT != 0
    }

    /// `TIN`, `TIE` and the division as the counter sees them: the `TCR`'s
    /// with `TOPT` = 0, the `MOR`'s with `TOPT` = 1 (`MC68705P5-025`).
    const fn mode(&self) -> (bool, bool, u8) {
        if self.mor_controlled() {
            (self.mor & MOR_CLS != 0, true, self.mor & MOR_P)
        } else {
            (
                self.control & TIN != 0,
                self.control & TIE != 0,
                self.control & PS,
            )
        }
    }

    /// `TCR` as a read returns it (`MC68705P5-023`, `MC68705P5-025`).
    pub(crate) const fn read_control(&self) -> u8 {
        if self.mor_controlled() {
            (self.control & (TIR | TIM)) | 0x3F
        } else {
            self.control & !PSC
        }
    }

    /// A write to `TCR`.
    pub(crate) const fn write_control(&mut self, value: u8) {
        if self.mor_controlled() {
            self.control = value & (TIR | TIM);
        } else {
            self.control = value & !PSC;
            if value & PSC != 0 {
                self.prescaler = 0x7F;
            }
        }
    }

    /// Whether the timer asks for an interrupt (`MC68705P5-028`).
    pub(crate) const fn requesting(&self) -> bool {
        self.control & TIR != 0 && self.control & TIM == 0
    }

    /// One instruction cycle with the `TIMER` pin at `pin`
    /// (`MC68705P5-024`).
    pub(crate) const fn cycle(&mut self, pin: bool) {
        let (tin, tie, division) = self.mode();
        let tick = match (tin, tie) {
            (false, false) => true,
            (false, true) => pin,
            (true, false) => false,
            (true, true) => pin && !self.pin_before,
        };
        self.pin_before = pin;
        if !tick {
            return;
        }
        self.prescaler = self.prescaler.wrapping_sub(1) & 0x7F;
        let mask = (1u8 << division) - 1;
        if self.prescaler & mask == mask {
            self.data = self.data.wrapping_sub(1);
            if self.data == 0 {
                self.control |= TIR;
            }
        }
    }
}
