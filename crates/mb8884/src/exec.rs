//! What each opcode does: how many cycles it takes, what its second cycle
//! puts on the bus, and its effect when its last cycle ends.

use crate::{FLAG_AC, FLAG_BS, FLAG_C, FLAG_F0, FLAG_ONE, Inputs, Mb8884, SP_MASK, TimerMode};

/// Machine cycles per opcode (`MB8884-003`).
#[rustfmt::skip]
pub(crate) const CYCLES: [u8; 256] = [
    1, 1, 2, 2, 2, 1, 1, 1, 2, 2, 2, 1, 2, 2, 2, 2,
    1, 1, 2, 2, 2, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 2, 2, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 2, 1, 2, 1, 2, 1, 1, 2, 2, 1, 2, 2, 2, 2,
    1, 1, 1, 2, 2, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 2, 2, 2, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 2, 1, 2, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    2, 2, 1, 2, 2, 1, 2, 1, 2, 2, 2, 1, 2, 2, 2, 2,
    2, 2, 2, 2, 2, 1, 2, 1, 2, 2, 2, 1, 2, 2, 2, 2,
    1, 1, 1, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    2, 2, 2, 2, 2, 1, 2, 1, 2, 2, 2, 2, 2, 2, 2, 2,
    1, 1, 1, 1, 2, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 2, 2, 1, 2, 1, 2, 2, 2, 2, 2, 2, 2, 2,
    1, 1, 2, 1, 2, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1,
];

/// The machine cycles one opcode takes (`MB8884-003`): one or two. The four
/// opcodes `STRT CNT`, `STRT T`, `MOV T,A` and `STOP TCNT` are given one
/// cycle (`MB8884-OPEN-03`), and so is every opcode the map leaves blank
/// (`MB8884-020`).
#[must_use]
pub(crate) const fn cycles(opcode: u8) -> usize {
    CYCLES[opcode as usize] as usize
}

/// What an instruction's second cycle does at the pins.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum Second {
    /// `ALE` alone: no strobe.
    Idle,
    /// `/PSEN` at the program counter, which then advances: an operand.
    Operand,
    /// `/PSEN` at an address the instruction names, the counter left alone.
    FetchAt(u16),
    /// `/RD` with the byte on `DB` during `ALE`, or with `DB` floating.
    Read(Option<u8>),
    /// `/WR` with the byte on `DB` during `ALE`, then the data.
    Write(Option<u8>, u8),
    /// `PROG` with the expander's nibble on `P2`'s low four lines.
    Prog(u8),
}

impl Mb8884 {
    /// The second cycle of `op`, decided when the opcode has been fetched;
    /// `pc` already stands past the opcode.
    pub(crate) fn second(&self, op: u8) -> Second {
        let page = self.pc & 0xF00;
        match op {
            0xA3 | 0xB3 => Second::FetchAt(page | u16::from(self.a)),
            0xE3 => Second::FetchAt(0x300 | u16::from(self.a)),
            0x80 | 0x81 => Second::Read(Some(self.reg(op & 1))),
            0x90 | 0x91 => Second::Write(Some(self.reg(op & 1)), self.a),
            0x08 => Second::Read(None),
            0x02 => Second::Write(None, self.a),
            0x0C..=0x0F => Second::Prog(0x0F),
            0x3C..=0x3F | 0x8C..=0x8F | 0x9C..=0x9F => Second::Prog(self.a & 0x0F),
            0x09 | 0x0A | 0x39 | 0x3A | 0x83 | 0x93 => Second::Idle,
            _ => Second::Operand,
        }
    }

    pub(crate) fn reg(&self, r: u8) -> u8 {
        self.ram[self.bank() + usize::from(r & 7)]
    }

    fn set_reg(&mut self, r: u8, v: u8) {
        let at = self.bank() + usize::from(r & 7);
        self.ram[at] = v;
    }

    fn bank(&self) -> usize {
        if self.psw & FLAG_BS != 0 { 24 } else { 0 }
    }

    /// `@Ri`: the register's value taken modulo 64 (`MB8884-004`).
    fn ind(&self, r: u8) -> usize {
        usize::from(self.reg(r) & 0x3F)
    }

    fn carry(&self) -> u8 {
        u8::from(self.psw & FLAG_C != 0)
    }

    fn set_flag(&mut self, flag: u8, on: bool) {
        if on {
            self.psw |= flag;
        } else {
            self.psw &= !flag;
        }
    }

    fn add(&mut self, v: u8, c: u8) {
        let sum = u16::from(self.a) + u16::from(v) + u16::from(c);
        let half = (self.a & 0x0F) + (v & 0x0F) + c;
        self.set_flag(FLAG_C, sum > 0xFF);
        self.set_flag(FLAG_AC, half > 0x0F);
        self.a = sum as u8;
    }

    /// `MB8884-013`.
    fn decimal_adjust(&mut self) {
        let mut a = u16::from(self.a);
        if a & 0x0F > 9 || self.psw & FLAG_AC != 0 {
            a += 6;
            if a > 0xFF {
                self.psw |= FLAG_C;
            }
        }
        if a & 0xF0 > 0x90 || self.psw & FLAG_C != 0 {
            a += 0x60;
            self.psw |= FLAG_C;
        }
        self.a = a as u8;
    }

    /// The stack at `$08` + 2·SP: the counter's low byte, then the PSW's
    /// upper four bits over the counter's upper four (`MB8884-008`).
    pub(crate) fn push(&mut self) {
        let sp = usize::from(self.psw & SP_MASK);
        self.ram[8 + 2 * sp] = self.pc as u8;
        self.ram[9 + 2 * sp] = (self.psw & 0xF0) | ((self.pc >> 8) as u8 & 0x0F);
        self.psw = (self.psw & !SP_MASK) | ((sp as u8 + 1) & SP_MASK);
    }

    fn pop(&mut self, restore: bool) {
        let sp = (self.psw.wrapping_sub(1)) & SP_MASK;
        let at = 8 + 2 * usize::from(sp);
        let hi = self.ram[at + 1];
        self.pc = (u16::from(hi & 0x0F) << 8) | u16::from(self.ram[at]);
        let upper = if restore { hi & 0xF0 } else { self.psw & 0xF0 };
        self.psw = upper | sp;
    }

    /// `JMP` and `CALL`: eleven bits from the instruction, A11 from `MBF`
    /// outside an interrupt routine and 0 inside one (`MB8884-017`,
    /// `MB8884-026`).
    fn long_target(&self, op: u8, lo: u8) -> u16 {
        let a11 = if self.mbf && !self.in_irq { 0x800 } else { 0 };
        a11 | (u16::from(op & 0xE0) << 3) | u16::from(lo)
    }

    /// A conditional jump lands in the page of its operand byte
    /// (`MB8884-015`).
    fn short(&mut self, cond: bool, lo: u8) {
        if cond {
            self.pc = (self.operand_at & 0xF00) | u16::from(lo);
        }
    }

    /// The effect of `op` when its last cycle ends; `b` is its second
    /// cycle's byte where there was one.
    pub(crate) fn execute(&mut self, op: u8, b: u8, pins: &Inputs) {
        let r = op & 7;
        let i = op & 1;
        match op {
            0x00 => {}
            0x02 => self.bus = self.a,
            0x03 => self.add(b, 0),
            0x04 | 0x24 | 0x44 | 0x64 | 0x84 | 0xA4 | 0xC4 | 0xE4 => {
                self.pc = self.long_target(op, b);
            }
            0x05 => self.xie = true,
            0x07 => self.a = self.a.wrapping_sub(1),
            0x08 => self.a = b,
            0x09 => self.a = self.p1 & pins.p1,
            0x0A => self.a = self.p2 & pins.p2,
            0x0C..=0x0F => {
                self.a = pins.p2 & 0x0F;
                self.p2 |= 0x0F;
            }
            0x10 | 0x11 => {
                let at = self.ind(i);
                self.ram[at] = self.ram[at].wrapping_add(1);
            }
            0x12 | 0x32 | 0x52 | 0x72 | 0x92 | 0xB2 | 0xD2 | 0xF2 => {
                self.short(self.a & (1 << (op >> 5)) != 0, b);
            }
            0x13 => self.add(b, self.carry()),
            0x14 | 0x34 | 0x54 | 0x74 | 0x94 | 0xB4 | 0xD4 | 0xF4 => {
                self.push();
                self.pc = self.long_target(op, b);
            }
            0x15 => self.xie = false,
            0x16 => {
                let tf = self.tf;
                self.tf = false;
                self.short(tf, b);
            }
            0x17 => self.a = self.a.wrapping_add(1),
            0x18..=0x1F => self.set_reg(r, self.reg(r).wrapping_add(1)),
            0x20 | 0x21 => {
                let at = self.ind(i);
                core::mem::swap(&mut self.a, &mut self.ram[at]);
            }
            0x23 => self.a = b,
            0x25 => self.tie = true,
            0x26 => self.short(pins.t0.is_low(), b),
            0x27 => self.a = 0,
            0x28..=0x2F => {
                let v = self.reg(r);
                self.set_reg(r, self.a);
                self.a = v;
            }
            0x30 | 0x31 => {
                let at = self.ind(i);
                let m = self.ram[at];
                self.ram[at] = (m & 0xF0) | (self.a & 0x0F);
                self.a = (self.a & 0xF0) | (m & 0x0F);
            }
            0x35 => {
                self.tie = false;
                self.tirq = false;
            }
            0x36 => self.short(pins.t0.is_high(), b),
            0x37 => self.a = !self.a,
            0x39 => self.p1 = self.a,
            0x3A => self.p2 = self.a,
            0x3C..=0x3F => self.p2 = (self.p2 & 0xF0) | (self.a & 0x0F),
            0x40 | 0x41 => self.a |= self.ram[self.ind(i)],
            0x42 => self.a = self.t,
            0x43 => self.a |= b,
            0x45 => self.mode = TimerMode::Counter,
            0x46 => self.short(pins.t1.is_low(), b),
            0x47 => self.a = self.a.rotate_left(4),
            0x48..=0x4F => self.a |= self.reg(r),
            0x50 | 0x51 => self.a &= self.ram[self.ind(i)],
            0x53 => self.a &= b,
            0x55 => {
                self.mode = TimerMode::Timer;
                self.prescaler = 0;
            }
            0x56 => self.short(pins.t1.is_high(), b),
            0x57 => self.decimal_adjust(),
            0x58..=0x5F => self.a &= self.reg(r),
            0x60 | 0x61 => self.add(self.ram[self.ind(i)], 0),
            0x62 => self.t = self.a,
            0x65 => self.mode = TimerMode::Stopped,
            0x67 => {
                let c = self.carry();
                self.set_flag(FLAG_C, self.a & 1 != 0);
                self.a = (self.a >> 1) | (c << 7);
            }
            0x68..=0x6F => self.add(self.reg(r), 0),
            0x70 | 0x71 => self.add(self.ram[self.ind(i)], self.carry()),
            0x75 => self.t0_clock = true,
            0x76 => self.short(self.f1, b),
            0x77 => self.a = self.a.rotate_right(1),
            0x78..=0x7F => self.add(self.reg(r), self.carry()),
            0x80 | 0x81 => self.a = b,
            0x83 => self.pop(false),
            0x85 => self.psw &= !FLAG_F0,
            0x86 => self.short(pins.int_n.is_low(), b),
            0x88 => self.bus |= b,
            0x89 => self.p1 |= b,
            0x8A => self.p2 |= b,
            0x8C..=0x8F => self.p2 = (self.p2 & 0xF0) | (self.a & 0x0F),
            0x90 | 0x91 => {}
            0x93 => {
                self.pop(true);
                self.in_irq = false;
            }
            0x95 => self.psw ^= FLAG_F0,
            0x96 => self.short(self.a != 0, b),
            0x97 => self.psw &= !FLAG_C,
            0x98 => self.bus &= b,
            0x99 => self.p1 &= b,
            0x9A => self.p2 &= b,
            0x9C..=0x9F => self.p2 = (self.p2 & 0xF0) | (self.a & 0x0F),
            0xA0 | 0xA1 => {
                let at = self.ind(i);
                self.ram[at] = self.a;
            }
            0xA3 | 0xE3 => self.a = b,
            0xA5 => self.f1 = false,
            0xA7 => self.psw ^= FLAG_C,
            0xA8..=0xAF => self.set_reg(r, self.a),
            0xB0 | 0xB1 => {
                let at = self.ind(i);
                self.ram[at] = b;
            }
            0xB3 => self.pc = (self.pc & 0xF00) | u16::from(b),
            0xB5 => self.f1 = !self.f1,
            0xB6 => self.short(self.psw & FLAG_F0 != 0, b),
            0xB8..=0xBF => self.set_reg(r, b),
            0xC5 => self.psw &= !FLAG_BS,
            0xC6 => self.short(self.a == 0, b),
            0xC7 => self.a = self.psw | FLAG_ONE,
            0xC8..=0xCF => self.set_reg(r, self.reg(r).wrapping_sub(1)),
            0xD0 | 0xD1 => self.a ^= self.ram[self.ind(i)],
            0xD3 => self.a ^= b,
            0xD5 => self.psw |= FLAG_BS,
            0xD7 => self.psw = self.a & !FLAG_ONE,
            0xD8..=0xDF => self.a ^= self.reg(r),
            0xE5 => self.mbf = false,
            0xE6 => self.short(self.psw & FLAG_C == 0, b),
            0xE7 => self.a = self.a.rotate_left(1),
            0xE8..=0xEF => {
                let v = self.reg(r).wrapping_sub(1);
                self.set_reg(r, v);
                self.short(v != 0, b);
            }
            0xF0 | 0xF1 => self.a = self.ram[self.ind(i)],
            0xF5 => self.mbf = true,
            0xF6 => self.short(self.psw & FLAG_C != 0, b),
            0xF7 => {
                let c = self.carry();
                self.set_flag(FLAG_C, self.a & 0x80 != 0);
                self.a = (self.a << 1) | c;
            }
            0xF8..=0xFF => self.a = self.reg(r),
            _ => {}
        }
    }
}
