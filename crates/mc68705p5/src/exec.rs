//! The instruction set: Table 7's map with Tables 1–5's cycles
//! (`MC68705P5-009`), the addressing modes (`MC68705P5-010`) and Table 6's
//! condition codes (`MC68705P5-011` to `MC68705P5-015`).

use crate::{FLAG_C, FLAG_H, FLAG_I, FLAG_N, FLAG_Z, Mc68705p5};

impl Mc68705p5 {
    fn fetch(&mut self) -> u8 {
        let v = self.read(self.pc);
        self.pc = (self.pc + 1) & 0x7FF;
        v
    }

    fn fetch_word(&mut self) -> u16 {
        let hi = u16::from(self.fetch());
        (hi << 8) | u16::from(self.fetch())
    }

    fn set_nz(&mut self, v: u8) {
        self.cc &= !(FLAG_N | FLAG_Z);
        if v & 0x80 != 0 {
            self.cc |= FLAG_N;
        }
        if v == 0 {
            self.cc |= FLAG_Z;
        }
    }

    fn set(&mut self, bit: u8, on: bool) {
        if on {
            self.cc |= bit;
        } else {
            self.cc &= !bit;
        }
    }

    pub(crate) fn push(&mut self, v: u8) {
        self.write(u16::from(self.sp), v);
        self.sp = 0x60 | (self.sp.wrapping_sub(1) & 0x1F);
    }

    fn pull(&mut self) -> u8 {
        self.sp = 0x60 | (self.sp.wrapping_add(1) & 0x1F);
        self.read(u16::from(self.sp))
    }

    /// Push `PC` low then high, the high byte with its five unused bits set
    /// (`MC68705P5-008`).
    pub(crate) fn push_pc(&mut self) {
        self.push(self.pc as u8);
        self.push(0xF8 | (self.pc >> 8) as u8);
    }

    /// Push the whole state and take `vector` (`MC68705P5-030`).
    pub(crate) fn interrupt(&mut self, vector: u16) {
        self.push_pc();
        self.push(self.x);
        self.push(self.a);
        self.push(0xE0 | self.cc);
        self.cc |= FLAG_I;
        self.pc = self.vector(vector);
    }

    /// The operand address of a register/memory or read-modify-write
    /// instruction whose opcode's high nibble is `mode` (`MC68705P5-010`,
    /// `MC68705P5-OPEN-06`).
    fn address(&mut self, mode: u8) -> u16 {
        match mode {
            0x3 | 0xB => u16::from(self.fetch()),
            0xC => self.fetch_word() & 0x7FF,
            0xD => (self.fetch_word().wrapping_add(u16::from(self.x))) & 0x7FF,
            0x6 | 0xE => u16::from(self.fetch()) + u16::from(self.x),
            _ => u16::from(self.x),
        }
    }

    fn branch(&mut self, taken: bool) {
        let offset = self.fetch() as i8;
        if taken {
            self.pc = (self.pc.wrapping_add_signed(i16::from(offset))) & 0x7FF;
        }
    }

    fn add(&mut self, m: u8, carry: bool) {
        let c = u8::from(carry);
        let r = u16::from(self.a) + u16::from(m) + u16::from(c);
        self.set(FLAG_H, (self.a & 0x0F) + (m & 0x0F) + c > 0x0F);
        self.set(FLAG_C, r > 0xFF);
        self.a = r as u8;
        self.set_nz(self.a);
    }

    fn subtract(&mut self, left: u8, m: u8, borrow: bool) -> u8 {
        let b = u16::from(borrow);
        let r = u16::from(left).wrapping_sub(u16::from(m)).wrapping_sub(b);
        self.set(FLAG_C, u16::from(left) < u16::from(m) + b);
        let r = r as u8;
        self.set_nz(r);
        r
    }

    /// One read-modify-write operation, by the opcode's low nibble.
    fn modify(&mut self, op: u8, v: u8) -> Option<u8> {
        let c = self.cc & FLAG_C != 0;
        let r = match op {
            0x0 => {
                let r = 0u8.wrapping_sub(v);
                self.set(FLAG_C, r != 0);
                r
            }
            0x3 => {
                self.set(FLAG_C, true);
                !v
            }
            0x4 => {
                self.set(FLAG_C, v & 1 != 0);
                v >> 1
            }
            0x6 => {
                self.set(FLAG_C, v & 1 != 0);
                (v >> 1) | if c { 0x80 } else { 0 }
            }
            0x7 => {
                self.set(FLAG_C, v & 1 != 0);
                (v >> 1) | (v & 0x80)
            }
            0x8 => {
                self.set(FLAG_C, v & 0x80 != 0);
                v << 1
            }
            0x9 => {
                self.set(FLAG_C, v & 0x80 != 0);
                (v << 1) | u8::from(c)
            }
            0xA => v.wrapping_sub(1),
            0xC => v.wrapping_add(1),
            0xD => {
                self.set_nz(v);
                return None;
            }
            _ => 0,
        };
        self.set_nz(r);
        Some(r)
    }

    /// Execute one defined instruction, its opcode already fetched.
    pub(crate) fn execute(&mut self, opcode: u8) {
        let hi = opcode >> 4;
        let low = opcode & 0x0F;
        match hi {
            0x0 => {
                let at = u16::from(self.fetch());
                let bit = self.read(at) >> (low >> 1) & 1 != 0;
                self.set(FLAG_C, bit);
                self.branch(bit == (low & 1 == 0));
            }
            0x1 => {
                let at = u16::from(self.fetch());
                let mask = 1u8 << (low >> 1);
                let v = self.read(at);
                self.write(at, if low & 1 == 0 { v | mask } else { v & !mask });
            }
            0x2 => {
                let f = self.cc;
                let z = f & FLAG_Z != 0;
                let c = f & FLAG_C != 0;
                let taken = match low >> 1 {
                    0 => true,
                    1 => !(c || z),
                    2 => !c,
                    3 => !z,
                    4 => f & FLAG_H == 0,
                    5 => f & FLAG_N == 0,
                    6 => f & FLAG_I == 0,
                    _ => self.pins.int_n.is_low(),
                };
                self.branch(taken == (low & 1 == 0));
            }
            0x3..=0x7 => match hi {
                0x4 => {
                    if let Some(r) = self.modify(low, self.a) {
                        self.a = r;
                    }
                }
                0x5 => {
                    if let Some(r) = self.modify(low, self.x) {
                        self.x = r;
                    }
                }
                _ => {
                    let at = self.address(hi);
                    let v = self.read(at);
                    if let Some(r) = self.modify(low, v) {
                        self.write(at, r);
                    }
                }
            },
            0x8 => match low {
                0x0 => {
                    self.cc = self.pull() & 0x1F;
                    self.a = self.pull();
                    self.x = self.pull();
                    self.pull_pc();
                }
                0x1 => self.pull_pc(),
                _ => self.interrupt(crate::vector::SWI),
            },
            0x9 => match low {
                0x7 => self.x = self.a,
                0x8 => self.cc &= !FLAG_C,
                0x9 => self.cc |= FLAG_C,
                0xA => self.cc &= !FLAG_I,
                0xB => self.cc |= FLAG_I,
                0xC => self.sp = 0x7F,
                0xF => self.a = self.x,
                _ => {}
            },
            _ => self.register_memory(hi, low),
        }
    }

    fn pull_pc(&mut self) {
        let hi = u16::from(self.pull() & 0x07);
        self.pc = (hi << 8) | u16::from(self.pull());
    }

    fn register_memory(&mut self, hi: u8, low: u8) {
        if hi == 0xA && low == 0xD {
            let offset = self.fetch() as i8;
            self.push_pc();
            self.pc = (self.pc.wrapping_add_signed(i16::from(offset))) & 0x7FF;
            return;
        }
        let at = if hi == 0xA {
            let at = self.pc;
            self.pc = (self.pc + 1) & 0x7FF;
            at
        } else {
            self.address(hi)
        };
        match low {
            0x7 => {
                self.write(at, self.a);
                self.set_nz(self.a);
            }
            0xF => {
                self.write(at, self.x);
                self.set_nz(self.x);
            }
            0xC => self.pc = at & 0x7FF,
            0xD => {
                self.push_pc();
                self.pc = at & 0x7FF;
            }
            _ => {
                let m = self.read(at);
                match low {
                    0x0 => self.a = self.subtract(self.a, m, false),
                    0x1 => {
                        self.subtract(self.a, m, false);
                    }
                    0x2 => {
                        let c = self.cc & FLAG_C != 0;
                        self.a = self.subtract(self.a, m, c);
                    }
                    0x3 => {
                        self.subtract(self.x, m, false);
                    }
                    0x4 => {
                        self.a &= m;
                        self.set_nz(self.a);
                    }
                    0x5 => self.set_nz(self.a & m),
                    0x6 => {
                        self.a = m;
                        self.set_nz(m);
                    }
                    0x8 => {
                        self.a ^= m;
                        self.set_nz(self.a);
                    }
                    0x9 => {
                        let c = self.cc & FLAG_C != 0;
                        self.add(m, c);
                    }
                    0xA => {
                        self.a |= m;
                        self.set_nz(self.a);
                    }
                    0xB => self.add(m, false),
                    _ => {
                        self.x = m;
                        self.set_nz(m);
                    }
                }
            }
        }
    }
}
