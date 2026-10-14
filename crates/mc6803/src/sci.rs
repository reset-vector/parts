//! The serial communications interface: four registers, a transmitter on
//! `P24`, a receiver on `P23` and a bit clock that can leave or enter on
//! `P22`.
//!
//! The bi-phase format is not implemented (`MC6803-OPEN-07`): with
//! `CC1:CC0` = `00` the lines carry NRZ here. The receiver's eight
//! sub-intervals a bit are kept, and it samples once, in the fifth
//! (`MC6803-OPEN-08`).

use crate::timer::Unit as Timer;
use crate::{Level, reg};

/// Bits of the transmit/receive control and status register (`MC6803-046`).
pub mod trcsr {
    /// Bit 0: wake-up on idle line.
    pub const WU: u8 = 1 << 0;
    /// Bit 1: transmit enable.
    pub const TE: u8 = 1 << 1;
    /// Bit 2: transmit interrupt enable.
    pub const TIE: u8 = 1 << 2;
    /// Bit 3: receive enable.
    pub const RE: u8 = 1 << 3;
    /// Bit 4: receive interrupt enable.
    pub const RIE: u8 = 1 << 4;
    /// Bit 5: transmit data register empty, read-only.
    pub const TDRE: u8 = 1 << 5;
    /// Bit 6: overrun or framing error, read-only.
    pub const ORFE: u8 = 1 << 6;
    /// Bit 7: receive data register full, read-only.
    pub const RDRF: u8 = 1 << 7;
    /// The five bits software may write.
    pub const WRITABLE_MASK: u8 = 0x1f;
    /// What reset presets the register to (`MC6803-009`).
    pub const RESET: u8 = TDRE;
}

/// Bits of the rate and mode control register (`MC6803-047`).
pub mod rmcr {
    /// Bits 1:0: the bit rate as a division of E.
    pub const SS1_SS0_MASK: u8 = 0b0011;
    /// Bits 3:2: format and clock source.
    pub const CC1_CC0_MASK: u8 = 0b1100;
    /// `CC1`: with `CC0` this selects the clock's route through `P22`.
    pub const CC1: u8 = 1 << 3;
    /// `CC0`.
    pub const CC0: u8 = 1 << 2;
}

/// The port 2 lines the interface uses.
const P22: u8 = 1 << 2;
const P23: u8 = 1 << 3;
const P24: u8 = 1 << 4;

/// The transmitter's state, a field of [`Sci`] (`MC6803-048`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Transmitter {
    /// Sending ones.
    Idle,
    /// Sending the preamble of ones.
    Preamble {
        /// Ones still to send, at most nine.
        one_count: u8,
    },
    /// Shifting a frame out.
    Shift {
        /// The bits still to send, LSB first, nine at most.
        bits: u16,
        /// How many remain, at most nine.
        bit_count: u8,
    },
}

/// The receiver's state, a field of [`Sci`] (`MC6803-049`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Receiver {
    /// Waiting for a start bit.
    Idle,
    /// Inside a frame.
    Frame {
        /// Sub-intervals since the start edge.
        eighth_count: u8,
        /// The bits sampled so far, the latest in bit 9; ten bits.
        bits: u16,
        /// How many bits have been sampled, at most nine: the tenth ends the
        /// frame.
        bit_count: u8,
    },
}

/// The serial interface's registers, a field of [`crate::Registers`], with
/// its shifters and bit clock.
///
/// `Default` is the interface as reset leaves it: RMCR and the receive data
/// register cleared, TRCSR at `$20` (`MC6803-009`), both shifters idle, the
/// bit clock at the start of a bit and the lines last seen idle.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub struct Sci {
    /// The rate and mode control register, `$10`, four bits; its fields are
    /// [`rmcr`] (`MC6803-047`).
    pub rmcr: u8,
    /// The transmit/receive control and status register, `$11`; its bits are
    /// [`trcsr`] (`MC6803-046`).
    pub trcsr: u8,
    /// The receive data register, `$12`.
    pub rdr: u8,
    /// The transmit data register, `$13`.
    pub tdr: u8,
    /// TRCSR was read with `TDRE` set: the first half of its clearing
    /// sequence is done (`MC6803-048`).
    pub tdre_read: bool,
    /// TRCSR was read with `RDRF` set (`MC6803-049`).
    pub rdrf_read: bool,
    /// TRCSR was read with `ORFE` set (`MC6803-049`).
    pub orfe_read: bool,
    /// The sub-interval within the bit, 0 to 7 (`MC6803-047`).
    pub bit_eighth: u8,
    /// The level at `P22` last cycle, for an external clock's edge
    /// (`MC6803-047`).
    pub p22: Level,
    /// The transmitter.
    pub transmitter: Transmitter,
    /// `Tx`, the transmitter's output, which `P24` shows while
    /// `TE` is set (`MC6803-048`).
    pub tx: Level,
    /// The receiver.
    pub receiver: Receiver,
    /// Consecutive idle bit times seen on the line, for wake-up, held at 255
    /// once it gets there (`MC6803-050`).
    pub idle_bit_count: u8,
    /// The level at `P23`, the receive line, at the last sub-interval.
    pub p23: Level,
}

impl Default for Sci {
    fn default() -> Self {
        Unit::new().registers()
    }
}

/// The interface's state: its registers, the shifters and the bit clock.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) struct Unit {
    /// The rate and mode control register, four bits (`MC6803-047`).
    rmcr: u8,
    /// The transmit/receive control and status register (`MC6803-046`).
    trcsr: u8,
    /// The receive data register.
    rdr: u8,
    /// The transmit data register.
    tdr: u8,
    /// TRCSR was read with the flag set: the first half of each clearing
    /// sequence (`MC6803-048`, `MC6803-049`).
    tdre_read: bool,
    rdrf_read: bool,
    orfe_read: bool,
    /// The sub-interval within the bit, 0–7.
    sub: u8,
    /// The level at `P22` last cycle, for the external clock's edge.
    p22_last: bool,
    /// The transmitter.
    tx: Transmitter,
    /// The level the transmitter is driving on `P24`.
    tx_line: bool,
    /// The receiver.
    rx: Receiver,
    /// Consecutive one bits seen on the line, for wake-up (`MC6803-050`).
    ones: u8,
    /// The line level sampled last sub-interval.
    rx_last: bool,
}

impl Unit {
    /// The interface as reset leaves it (`MC6803-009`).
    pub(crate) const fn new() -> Self {
        Self {
            rmcr: 0,
            trcsr: trcsr::RESET,
            rdr: 0,
            tdr: 0,
            tdre_read: false,
            rdrf_read: false,
            orfe_read: false,
            sub: 0,
            p22_last: false,
            tx: Transmitter::Idle,
            tx_line: true,
            rx: Receiver::Idle,
            ones: 0,
            rx_last: true,
        }
    }

    /// RMCR cleared, TRCSR preset to `$20`, the receive data register
    /// cleared (`MC6803-009`).
    pub(crate) const fn reset(&mut self) {
        let p22 = self.p22_last;
        *self = Self::new();
        self.p22_last = p22;
    }

    /// The registers as they stand.
    pub(crate) const fn registers(&self) -> Sci {
        Sci {
            rmcr: self.rmcr,
            trcsr: self.trcsr,
            rdr: self.rdr,
            tdr: self.tdr,
            tdre_read: self.tdre_read,
            rdrf_read: self.rdrf_read,
            orfe_read: self.orfe_read,
            bit_eighth: self.sub,
            p22: Level::from_bool(self.p22_last),
            transmitter: self.tx,
            tx: Level::from_bool(self.tx_line),
            receiver: self.rx,
            idle_bit_count: self.ones,
            p23: Level::from_bool(self.rx_last),
        }
    }

    /// An interface holding `regs`.
    pub(crate) const fn from_registers(regs: Sci) -> Self {
        Self {
            rmcr: regs.rmcr & 0x0f,
            trcsr: regs.trcsr,
            rdr: regs.rdr,
            tdr: regs.tdr,
            tdre_read: regs.tdre_read,
            rdrf_read: regs.rdrf_read,
            orfe_read: regs.orfe_read,
            sub: regs.bit_eighth & 7,
            p22_last: regs.p22.is_high(),
            tx: Self::transmitter_held(regs.transmitter),
            tx_line: regs.tx.is_high(),
            rx: Self::receiver_held(regs.receiver),
            ones: regs.idle_bit_count,
            rx_last: regs.p23.is_high(),
        }
    }

    /// The transmitter as the shifter can hold it: a preamble of at most nine
    /// ones, a frame of at most nine bits still to send.
    const fn transmitter_held(tx: Transmitter) -> Transmitter {
        match tx {
            Transmitter::Idle => Transmitter::Idle,
            Transmitter::Preamble { one_count } => Transmitter::Preamble {
                one_count: if one_count > 9 { 9 } else { one_count },
            },
            Transmitter::Shift { bits, bit_count } => Transmitter::Shift {
                bits: bits & 0x1ff,
                bit_count: if bit_count > 9 { 9 } else { bit_count },
            },
        }
    }

    /// The receiver as the shifter can hold it: ten bits, of which at most
    /// nine sampled, since the tenth ends the frame.
    const fn receiver_held(rx: Receiver) -> Receiver {
        match rx {
            Receiver::Idle => Receiver::Idle,
            Receiver::Frame {
                eighth_count,
                bits,
                bit_count,
            } => Receiver::Frame {
                eighth_count,
                bits: bits & 0x3ff,
                bit_count: if bit_count > 9 { 9 } else { bit_count },
            },
        }
    }

    /// E cycles per bit under the internal clock (`MC6803-047`).
    const fn bit_period_cycles(&self) -> u16 {
        match self.rmcr & rmcr::SS1_SS0_MASK {
            0 => 16,
            1 => 128,
            2 => 1024,
            _ => 4096,
        }
    }

    /// The bit clock comes in on `P22` at eight times the bit rate
    /// (`MC6803-047`).
    const fn external_clock(&self) -> bool {
        self.rmcr & rmcr::CC1_CC0_MASK == rmcr::CC1_CC0_MASK
    }

    /// The bit clock goes out on `P22` (`MC6803-047`).
    const fn clock_out(&self) -> bool {
        self.rmcr & rmcr::CC1_CC0_MASK == rmcr::CC1
    }

    /// The port 2 direction register with the interface's forcing applied
    /// (`MC6803-047`, `MC6803-048`): `TE` forces bit 4 set, `RE` forces bit
    /// 3 clear, `CC1` forces bit 2 to the complement of `CC0`.
    pub(crate) const fn direction(&self, ddr: u8) -> u8 {
        let mut ddr = ddr;
        if self.trcsr & trcsr::TE != 0 {
            ddr |= P24;
        }
        if self.trcsr & trcsr::RE != 0 {
            ddr &= !P23;
        }
        if self.rmcr & rmcr::CC1 != 0 {
            if self.rmcr & rmcr::CC0 != 0 {
                ddr &= !P22;
            } else {
                ddr |= P22;
            }
        }
        ddr
    }

    /// The port 2 output word with the lines the interface drives replaced:
    /// `P24` by the transmitter while `TE` is set, `P22` by the bit clock
    /// while it is routed out.
    pub(crate) const fn drive(&self, out: u8) -> u8 {
        let mut out = out;
        if self.trcsr & trcsr::TE != 0 {
            out = (out & !P24) | ((self.tx_line as u8) << 4);
        }
        if self.clock_out() {
            out = (out & !P22) | (((self.sub >= 4) as u8) << 2);
        }
        out
    }

    /// The level the interface puts on `IRQ2` (`MC6803-037`).
    pub(crate) const fn irq2(&self) -> bool {
        let t = self.trcsr;
        (t & trcsr::TDRE != 0 && t & trcsr::TIE != 0)
            || ((t & (trcsr::RDRF | trcsr::ORFE) != 0) && t & trcsr::RIE != 0)
    }

    /// One E cycle. `p2_in` is the levels presented on port 2 and `ddr` the
    /// port's direction, for the receive line and the external clock.
    /// Returns the `IRQ2` level.
    pub(crate) fn step(&mut self, timer: &Timer, p2_in: u8, ddr: u8) -> bool {
        let ddr = self.direction(ddr);
        let p22 = p2_in & P22 != 0;
        let tick = if self.external_clock() {
            let edge = p22 && !self.p22_last;
            edge && ddr & P22 == 0
        } else {
            let subs = self.bit_period_cycles() / 8;
            timer.counter() & (subs - 1) == 0
        };
        self.p22_last = p22;

        if tick {
            self.sub = (self.sub + 1) & 7;
            if self.sub == 0 {
                self.transmit_bit();
            }
            let line = if ddr & P23 != 0 {
                true
            } else {
                p2_in & P23 != 0
            };
            self.receive_sub(line);
        }
        self.irq2()
    }

    /// The transmitter at a bit boundary (`MC6803-048`).
    fn transmit_bit(&mut self) {
        if self.trcsr & trcsr::TE == 0 {
            self.tx = Transmitter::Idle;
            self.tx_line = true;
            return;
        }
        match self.tx {
            Transmitter::Preamble { one_count } => {
                self.tx_line = true;
                self.tx = if one_count > 1 {
                    Transmitter::Preamble {
                        one_count: one_count - 1,
                    }
                } else {
                    Transmitter::Idle
                };
            }
            Transmitter::Shift { bits, bit_count } => {
                self.tx_line = bits & 1 != 0;
                self.tx = if bit_count > 1 {
                    Transmitter::Shift {
                        bits: bits >> 1,
                        bit_count: bit_count - 1,
                    }
                } else {
                    Transmitter::Idle
                };
            }
            Transmitter::Idle => {
                if self.trcsr & trcsr::TDRE == 0 {
                    let frame = ((self.tdr as u16) << 1) | 0x200;
                    self.trcsr |= trcsr::TDRE;
                    self.tx_line = false;
                    self.tx = Transmitter::Shift {
                        bits: frame >> 1,
                        bit_count: 9,
                    };
                } else {
                    self.tx_line = true;
                }
            }
        }
    }

    /// The receiver at a sub-interval (`MC6803-049`, `MC6803-050`).
    fn receive_sub(&mut self, line: bool) {
        if self.trcsr & trcsr::RE == 0 {
            self.rx = Receiver::Idle;
            self.rx_last = line;
            return;
        }
        match self.rx {
            Receiver::Idle => {
                if self.sub == 0 {
                    if line {
                        self.ones = self.ones.saturating_add(1);
                        if self.ones >= 10 {
                            self.trcsr &= !trcsr::WU;
                        }
                    } else {
                        self.ones = 0;
                    }
                }
                if !line && self.rx_last {
                    self.rx = Receiver::Frame {
                        eighth_count: 0,
                        bits: 0,
                        bit_count: 0,
                    };
                    self.ones = 0;
                }
            }
            Receiver::Frame {
                eighth_count,
                bits,
                bit_count,
            } => {
                let subs = eighth_count.wrapping_add(1);
                if subs % 8 == 4 {
                    let count = bit_count + 1;
                    let bits = (bits >> 1) | ((line as u16) << 9);
                    if count == 1 && line {
                        self.rx = Receiver::Idle;
                    } else if count == 10 {
                        self.frame_done(bits);
                        self.rx = Receiver::Idle;
                    } else {
                        self.rx = Receiver::Frame {
                            eighth_count: subs,
                            bits,
                            bit_count: count,
                        };
                    }
                } else {
                    self.rx = Receiver::Frame {
                        eighth_count: subs,
                        bits,
                        bit_count,
                    };
                }
            }
        }
        self.rx_last = line;
    }

    /// Ten bits are in: start at bit 0, data at 1–8, stop at 9.
    fn frame_done(&mut self, bits: u16) {
        let data = (bits >> 1) as u8;
        let stop = bits & 0x200 != 0;
        if self.trcsr & trcsr::WU != 0 {
            return;
        }
        if !stop {
            self.trcsr |= trcsr::ORFE;
            self.rdr = data;
        } else if self.trcsr & trcsr::RDRF != 0 {
            self.trcsr |= trcsr::ORFE;
        } else {
            self.rdr = data;
            self.trcsr |= trcsr::RDRF;
        }
    }

    /// A read of one of the four addresses, with its side effects.
    pub(crate) fn read(&mut self, address: u16) -> u8 {
        match address {
            reg::RMCR => self.rmcr,
            reg::TRCSR => {
                let t = self.trcsr;
                self.tdre_read = t & trcsr::TDRE != 0;
                self.rdrf_read = t & trcsr::RDRF != 0;
                self.orfe_read = t & trcsr::ORFE != 0;
                t
            }
            reg::RDR => {
                if self.rdrf_read {
                    self.trcsr &= !trcsr::RDRF;
                    self.rdrf_read = false;
                }
                if self.orfe_read {
                    self.trcsr &= !trcsr::ORFE;
                    self.orfe_read = false;
                }
                self.rdr
            }
            _ => self.tdr,
        }
    }

    /// A write to one of the four addresses.
    pub(crate) fn write(&mut self, address: u16, data: u8) {
        match address {
            reg::RMCR => self.rmcr = data & 0x0f,
            reg::TRCSR => {
                let was = self.trcsr;
                self.trcsr = (was & !trcsr::WRITABLE_MASK) | (data & trcsr::WRITABLE_MASK);
                if was & trcsr::TE == 0 && data & trcsr::TE != 0 {
                    self.tx = Transmitter::Preamble { one_count: 9 };
                }
                if data & trcsr::RE == 0 {
                    self.rx = Receiver::Idle;
                }
            }
            reg::TDR => {
                self.tdr = data;
                if self.tdre_read {
                    self.trcsr &= !trcsr::TDRE;
                    self.tdre_read = false;
                }
            }
            _ => {}
        }
    }
}
