//! The sequencer: what the part does with its bus in each cycle of each
//! instruction, Table F-1 of the reference manual cycle for cycle
//! (`MC6803-016`), and the reset, interrupt and wait sequences around it
//! (`MC6803-011`, `MC6803-035`, `MC6803-029`).
//!
//! `step` is called once per E cycle with `cycle` naming which cycle of the
//! thing in flight comes next, and returns the bus operation for it. The
//! byte a read produces arrives at the next tick as `dl`, so an instruction
//! whose last cycle is a read leaves a [`Finish`] behind, and `complete`
//! applies it before the next `step`.

use crate::alu;
use crate::decode::{
    Accumulator, Addressing, Alu8, Alu16, Inherent, Kind, Reg16, Stacked, Target, Unary, decode,
};
use crate::{Bus, Ctrl, Finish, Mc6803, Pins, Seq, vector};

impl Mc6803 {
    /// Apply the tail of the instruction that has just ended, using `dl`.
    pub(crate) fn complete(&mut self) {
        let finish = core::mem::replace(&mut self.finish, Finish::None);
        match finish {
            Finish::None => {}
            Finish::Alu8(op, acc) => self.alu8(op, acc, self.dl),
            Finish::Pull(acc) => {
                let v = self.dl;
                self.set_acc(acc, v);
            }
            Finish::Ld16(reg) => {
                let v = ((self.tmp as u16) << 8) | self.dl as u16;
                self.ccr = alu::nzv0_16(self.ccr, v);
                self.set_reg16(reg, v);
            }
            Finish::Pc => self.pc = ((self.tmp as u16) << 8) | self.dl as u16,
            Finish::X => self.x = ((self.tmp as u16) << 8) | self.dl as u16,
        }
    }

    /// Take the outstanding read's byte — `inp.d`, or the byte an internal
    /// read produced — and finish the instruction that has just ended without
    /// starting the next, so [`Mc6803::registers`] reads the boundary. An
    /// `NMI` edge on `inp` is latched into the `NMI` flip-flop as the next
    /// tick latches it, at that tick's end:
    /// [`crate::Nmi::Unseen`] holds it back from that tick's decision
    /// (`MC6803-032`, `MC6803-034`). Idempotent; ticking on afterwards, with
    /// the same `inp.d`, is correct.
    ///
    /// Returns whether the part now stands at a boundary that
    /// [`Mc6803::at_boundary`] rebuilds from [`Mc6803::registers`]: `false`,
    /// changing nothing, unless [`Mc6803::is_at_instruction_boundary`], and with
    /// [`Ctrl::RESET`] on `inp`, under which that tick drops the byte and the
    /// instruction's end (`MC6803-009`). A part is rebuilt at a later boundary
    /// instead.
    #[must_use]
    pub fn settle(&mut self, inp: Pins) -> bool {
        if inp.ctrl.contains(Ctrl::RESET) || !self.is_at_instruction_boundary() {
            return false;
        }
        let nmi = inp.ctrl.contains(Ctrl::NMI);
        if nmi && !self.nmi_last && self.nmi.is_none() && !matches!(self.seq, Seq::Held) {
            self.nmi = Some(crate::Nmi::Unseen);
        }
        self.nmi_last = nmi;
        if let Bus::Read(_) | Bus::Dummy(_) | Bus::Idle = self.outstanding
            && self.started
        {
            self.dl = self.internal.unwrap_or(inp.d.unwrap_or(0));
        }
        self.complete();
        true
    }

    const fn acc(&self, acc: Accumulator) -> u8 {
        match acc {
            Accumulator::A => self.a,
            Accumulator::B => self.b,
        }
    }

    const fn set_acc(&mut self, acc: Accumulator, v: u8) {
        match acc {
            Accumulator::A => self.a = v,
            Accumulator::B => self.b = v,
        }
    }

    const fn reg16(&self, reg: Reg16) -> u16 {
        match reg {
            Reg16::D => self.d(),
            Reg16::X => self.x,
            Reg16::S => self.sp,
        }
    }

    const fn set_reg16(&mut self, reg: Reg16, v: u16) {
        match reg {
            Reg16::D => self.set_d(v),
            Reg16::X => self.x = v,
            Reg16::S => self.sp = v,
        }
    }

    /// The eight-bit accumulator-and-memory group (`MC6803-018`).
    fn alu8(&mut self, op: Alu8, acc: Accumulator, m: u8) {
        let x = self.acc(acc);
        match op {
            Alu8::Sub => {
                let (ccr, r) = alu::sub8(self.ccr, x, m, false);
                self.ccr = ccr;
                self.set_acc(acc, r);
            }
            Alu8::Cmp => {
                let (ccr, _) = alu::sub8(self.ccr, x, m, false);
                self.ccr = ccr;
            }
            Alu8::Sbc => {
                let (ccr, r) = alu::sub8(self.ccr, x, m, self.ccr & alu::C != 0);
                self.ccr = ccr;
                self.set_acc(acc, r);
            }
            Alu8::And => {
                let r = x & m;
                self.ccr = alu::nzv0_8(self.ccr, r);
                self.set_acc(acc, r);
            }
            Alu8::Bit => {
                self.ccr = alu::nzv0_8(self.ccr, x & m);
            }
            Alu8::Ld => {
                self.ccr = alu::nzv0_8(self.ccr, m);
                self.set_acc(acc, m);
            }
            Alu8::Eor => {
                let r = x ^ m;
                self.ccr = alu::nzv0_8(self.ccr, r);
                self.set_acc(acc, r);
            }
            Alu8::Adc => {
                let (ccr, r) = alu::add8(self.ccr, x, m, self.ccr & alu::C != 0);
                self.ccr = ccr;
                self.set_acc(acc, r);
            }
            Alu8::Or => {
                let r = x | m;
                self.ccr = alu::nzv0_8(self.ccr, r);
                self.set_acc(acc, r);
            }
            Alu8::Add => {
                let (ccr, r) = alu::add8(self.ccr, x, m, false);
                self.ccr = ccr;
                self.set_acc(acc, r);
            }
        }
    }

    /// The sixteen-bit arithmetic (`MC6803-021`).
    fn alu16(&mut self, op: Alu16, m: u16) {
        match op {
            Alu16::SubD => {
                let (ccr, r) = alu::sub16(self.ccr, self.d(), m);
                self.ccr = ccr;
                self.set_d(r);
            }
            Alu16::AddD => {
                let (ccr, r) = alu::add16(self.ccr, self.d(), m);
                self.ccr = ccr;
                self.set_d(r);
            }
            Alu16::CmpX => {
                let (ccr, _) = alu::sub16(self.ccr, self.x, m);
                self.ccr = ccr;
            }
        }
    }

    /// The read-modify-write group on one byte (`MC6803-019`, `MC6803-020`).
    fn rmw(&mut self, op: Unary, m: u8) -> u8 {
        let (ccr, r) = match op {
            Unary::Neg => alu::neg(self.ccr, m),
            Unary::Com => alu::com(self.ccr, m),
            Unary::Lsr => alu::lsr(self.ccr, m),
            Unary::Ror => alu::ror(self.ccr, m),
            Unary::Asr => alu::asr(self.ccr, m),
            Unary::Asl => alu::asl(self.ccr, m),
            Unary::Rol => alu::rol(self.ccr, m),
            Unary::Dec => alu::dec(self.ccr, m),
            Unary::Inc => alu::inc(self.ccr, m),
            Unary::Tst => (alu::tst(self.ccr, m), m),
            Unary::Clr => (alu::clr(self.ccr), 0),
        };
        self.ccr = ccr;
        r
    }

    /// Read the byte at `pc` and step past it.
    fn fetch(&mut self) -> Bus {
        let bus = self.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        bus
    }

    /// Push a byte: written at `sp`, then `sp` decrements (`MC6803-025`).
    fn push(&mut self, v: u8) -> Bus {
        let bus = self.write(self.sp, v);
        self.sp = self.sp.wrapping_sub(1);
        bus
    }

    /// Pull a byte: `sp` increments, then the byte at `sp` is read
    /// (`MC6803-025`).
    fn pull(&mut self) -> Bus {
        self.sp = self.sp.wrapping_add(1);
        self.read(self.sp)
    }

    /// The idle cycle: a read of `$FFFF` (`MC6803-007`).
    const fn idle(&mut self) -> Bus {
        Bus::Idle
    }

    /// The CCR as pushed: bits 7 and 6 set (`MC6803-024`).
    const fn ccr_pushed(&self) -> u8 {
        self.ccr | alu::UPPER
    }

    /// Cycles 3 to 9 of every stacking sequence: `PCL`, `PCH`, `XL`, `XH`,
    /// `A`, `B`, `CCR` (`MC6803-027`, `MC6803-035`).
    fn stack_cycle(&mut self, cycle: u8) -> Bus {
        match cycle {
            3 => self.push(self.pc as u8),
            4 => self.push((self.pc >> 8) as u8),
            5 => self.push(self.x as u8),
            6 => self.push((self.x >> 8) as u8),
            7 => self.push(self.a),
            8 => self.push(self.b),
            _ => self.push(self.ccr_pushed()),
        }
    }

    /// The bus operation for the cycle `self.cycle`, advancing to the next.
    pub(crate) fn step(&mut self) -> Bus {
        let cycle = self.cycle;
        match self.seq {
            Seq::Held => self.read(vector::RESET),
            Seq::Reset => self.reset_cycle(cycle),
            Seq::Interrupt => self.interrupt_cycle(cycle),
            Seq::Wait => self.wait_cycle(),
            Seq::WaitExit => self.wait_exit_cycle(cycle),
            Seq::Instruction => self.instruction_cycle(cycle),
        }
    }

    /// After `RESET` rises: one idle read of `$FFFE`, then the vector's two
    /// bytes (`MC6803-011`, `MC6803-OPEN-05`).
    fn reset_cycle(&mut self, cycle: u8) -> Bus {
        match cycle {
            1 => {
                self.cycle = 2;
                self.dummy(vector::RESET)
            }
            2 => {
                self.cycle = 3;
                self.read(vector::RESET)
            }
            _ => {
                self.tmp = self.dl;
                self.finish = Finish::Pc;
                self.seq = Seq::Instruction;
                self.cycle = 1;
                self.read(vector::RESET + 1)
            }
        }
    }

    /// The twelve cycles of `MC6803-035`.
    fn interrupt_cycle(&mut self, cycle: u8) -> Bus {
        self.cycle = cycle + 1;
        match cycle {
            1 | 2 => self.dummy(self.pc),
            3..=9 => self.stack_cycle(cycle),
            10 => {
                let bus = self.dummy(self.sp);
                self.vector = self.select_vector();
                self.set_i();
                self.clear_nmi();
                bus
            }
            11 => self.read(self.vector),
            _ => {
                self.tmp = self.dl;
                self.finish = Finish::Pc;
                self.seq = Seq::Instruction;
                self.cycle = 1;
                self.read(self.vector + 1)
            }
        }
    }

    /// The wait state: reads of `SP` (seven below the original) until a
    /// flip-flop is set (`MC6803-029`).
    fn wait_cycle(&mut self) -> Bus {
        if self.interrupt_pending() {
            self.seq = Seq::WaitExit;
            self.cycle = 2;
        }
        self.dummy(self.sp)
    }

    /// Leaving the wait state: one more read of `SP` with I set and the
    /// vector chosen, then the vector's two bytes — the five cycles from
    /// receipt to the service routine's opcode fetch (`MC6803-029`,
    /// `MC6803-OPEN-10`).
    fn wait_exit_cycle(&mut self, cycle: u8) -> Bus {
        self.cycle = cycle + 1;
        match cycle {
            2 => {
                let bus = self.dummy(self.sp);
                self.vector = self.select_vector();
                self.set_i();
                self.clear_nmi();
                bus
            }
            3 => self.read(self.vector),
            _ => {
                self.tmp = self.dl;
                self.finish = Finish::Pc;
                self.seq = Seq::Instruction;
                self.cycle = 1;
                self.read(self.vector + 1)
            }
        }
    }

    /// One cycle of an instruction.
    fn instruction_cycle(&mut self, cycle: u8) -> Bus {
        if cycle == 1 {
            if self.interrupt_pending() {
                self.seq = Seq::Interrupt;
                self.cycle = 2;
                return self.dummy(self.pc);
            }
            self.cycle = 2;
            return self.fetch();
        }
        if cycle == 2 {
            self.instr = decode(self.dl);
        }
        let instr = self.instr;
        let last = cycle as usize == instr.cycles.get();
        self.cycle = if last { 1 } else { cycle + 1 };

        match instr.kind {
            Kind::Alu8(op, acc) => {
                self.operand_read(cycle, instr.addressing, Finish::Alu8(op, acc))
            }
            Kind::St8(acc) => {
                let v = self.acc(acc);
                let bus = self.operand_write(cycle, instr.addressing, v);
                if last {
                    self.ccr = alu::nzv0_8(self.ccr, v);
                }
                bus
            }
            Kind::Ld16(reg) => self.load16(cycle, instr.addressing, reg),
            Kind::St16(reg) => self.store16(cycle, instr.addressing, reg),
            Kind::Alu16(op) => self.arith16(cycle, instr.addressing, op),
            Kind::Unary(op, t @ (Target::A | Target::B)) => {
                let acc = if matches!(t, Target::A) {
                    Accumulator::A
                } else {
                    Accumulator::B
                };
                let r = self.rmw(op, self.acc(acc));
                if op != Unary::Tst {
                    self.set_acc(acc, r);
                }
                self.read(self.pc)
            }
            Kind::Unary(op, Target::Memory) => self.rmw_mem(cycle, instr.addressing, op),
            Kind::Jmp => self.jmp(cycle, instr.addressing),
            Kind::Jsr => self.jsr(cycle, instr.addressing),
            Kind::Bsr => self.bsr(cycle),
            Kind::Branch(cond) => match cycle {
                2 => self.fetch(),
                _ => {
                    if alu::branch(self.ccr, cond) {
                        self.pc = self.pc.wrapping_add(self.dl as i8 as i16 as u16);
                    }
                    self.idle()
                }
            },
            Kind::Inherent(inh) => self.inherent(cycle, inh),
            Kind::Undefined => self.inherent(cycle, Inherent::Nop),
        }
    }

    /// Assemble the effective address over the cycles before the operand
    /// cycle: direct, extended and indexed (`MC6803-015`). Returns the bus
    /// operation for this cycle if it is one of those, or none once the
    /// address is ready.
    fn effective(&mut self, cycle: u8, addressing: Addressing) -> Option<Bus> {
        match (addressing, cycle) {
            (Addressing::Direct, 2) => Some(self.fetch()),
            (Addressing::Direct, 3) => {
                self.ea = self.dl as u16;
                None
            }
            (Addressing::Extended, 2) => Some(self.fetch()),
            (Addressing::Extended, 3) => {
                self.tmp = self.dl;
                Some(self.fetch())
            }
            (Addressing::Extended, 4) => {
                self.ea = ((self.tmp as u16) << 8) | self.dl as u16;
                None
            }
            (Addressing::Indexed, 2) => Some(self.fetch()),
            (Addressing::Indexed, 3) => {
                self.ea = self.x.wrapping_add(self.dl as u16);
                Some(self.idle())
            }
            _ => None,
        }
    }

    /// The cycle at which the operand is first touched, by mode.
    const fn operand_cycle(addressing: Addressing) -> u8 {
        match addressing {
            Addressing::Immediate => 2,
            Addressing::Direct => 3,
            _ => 4,
        }
    }

    /// An eight-bit read operand (`MC6803-016`, `IMMEDIATE`/`DIRECT`/
    /// `EXTENDED`/`INDEXED`).
    fn operand_read(&mut self, cycle: u8, addressing: Addressing, finish: Finish) -> Bus {
        if addressing == Addressing::Immediate {
            self.finish = finish;
            return self.fetch();
        }
        if let Some(bus) = self.effective(cycle, addressing) {
            return bus;
        }
        self.finish = finish;
        self.read(self.ea)
    }

    /// An eight-bit store.
    fn operand_write(&mut self, cycle: u8, addressing: Addressing, v: u8) -> Bus {
        if let Some(bus) = self.effective(cycle, addressing) {
            return bus;
        }
        self.write(self.ea, v)
    }

    /// `LDS`, `LDX`, `LDD` (`MC6803-021`).
    fn load16(&mut self, cycle: u8, addressing: Addressing, reg: Reg16) -> Bus {
        let first = Self::operand_cycle(addressing);
        if addressing == Addressing::Immediate {
            if cycle == 2 {
                return self.fetch();
            }
            self.tmp = self.dl;
            self.finish = Finish::Ld16(reg);
            return self.fetch();
        }
        if let Some(bus) = self.effective(cycle, addressing) {
            return bus;
        }
        if cycle == first {
            return self.read(self.ea);
        }
        self.tmp = self.dl;
        self.finish = Finish::Ld16(reg);
        self.read(self.ea.wrapping_add(1))
    }

    /// `STS`, `STX`, `STD` (`MC6803-021`).
    fn store16(&mut self, cycle: u8, addressing: Addressing, reg: Reg16) -> Bus {
        let first = Self::operand_cycle(addressing);
        if let Some(bus) = self.effective(cycle, addressing) {
            return bus;
        }
        let v = self.reg16(reg);
        if cycle == first {
            return self.write(self.ea, (v >> 8) as u8);
        }
        self.ccr = alu::nzv0_16(self.ccr, v);
        self.write(self.ea.wrapping_add(1), v as u8)
    }

    /// `SUBD`, `ADDD`, `CPX`: two operand bytes then an idle cycle in which
    /// the arithmetic is done (`MC6803-021`).
    fn arith16(&mut self, cycle: u8, addressing: Addressing, op: Alu16) -> Bus {
        let first = Self::operand_cycle(addressing);
        if addressing == Addressing::Immediate {
            match cycle {
                2 => return self.fetch(),
                3 => {
                    self.tmp = self.dl;
                    return self.fetch();
                }
                _ => {
                    let m = ((self.tmp as u16) << 8) | self.dl as u16;
                    self.alu16(op, m);
                    return self.idle();
                }
            }
        }
        if let Some(bus) = self.effective(cycle, addressing) {
            return bus;
        }
        if cycle == first {
            self.read(self.ea)
        } else if cycle == first + 1 {
            self.tmp = self.dl;
            self.read(self.ea.wrapping_add(1))
        } else {
            let m = ((self.tmp as u16) << 8) | self.dl as u16;
            self.alu16(op, m);
            self.idle()
        }
    }

    /// The memory read-modify-write group: read, an idle cycle in which the
    /// new value is formed, write (`MC6803-016`).
    fn rmw_mem(&mut self, cycle: u8, addressing: Addressing, op: Unary) -> Bus {
        if let Some(bus) = self.effective(cycle, addressing) {
            return bus;
        }
        match cycle {
            4 => self.read(self.ea),
            5 => {
                self.tmp = self.rmw(op, self.dl);
                self.idle()
            }
            _ => self.write(self.ea, self.tmp),
        }
    }

    /// `JMP`: the address is the destination (`MC6803-021`).
    fn jmp(&mut self, cycle: u8, addressing: Addressing) -> Bus {
        match (addressing, cycle) {
            (Addressing::Extended, 2) => self.fetch(),
            (Addressing::Extended, _) => {
                self.tmp = self.dl;
                self.finish = Finish::Pc;
                self.fetch()
            }
            (_, 2) => self.fetch(),
            _ => {
                self.pc = self.x.wrapping_add(self.dl as u16);
                self.idle()
            }
        }
    }

    /// `JSR`: the destination's first byte is read, then the return address
    /// is pushed low byte first (`MC6803-027`).
    fn jsr(&mut self, cycle: u8, addressing: Addressing) -> Bus {
        if let Some(bus) = self.effective(cycle, addressing) {
            return bus;
        }
        let first = Self::operand_cycle(addressing);
        if cycle == first {
            self.read(self.ea)
        } else if cycle == first + 1 {
            self.push(self.pc as u8)
        } else {
            let bus = self.push((self.pc >> 8) as u8);
            self.pc = self.ea;
            bus
        }
    }

    /// `BSR`: the offset, an idle cycle, the destination's first byte, then
    /// the two pushes (`MC6803-027`).
    fn bsr(&mut self, cycle: u8) -> Bus {
        match cycle {
            2 => self.fetch(),
            3 => {
                self.ea = self.pc.wrapping_add(self.dl as i8 as i16 as u16);
                self.idle()
            }
            4 => self.read(self.ea),
            5 => self.push(self.pc as u8),
            _ => {
                let bus = self.push((self.pc >> 8) as u8);
                self.pc = self.ea;
                bus
            }
        }
    }

    /// The inherent instructions (`MC6803-017`, `MC6803-022` to
    /// `MC6803-025`, `MC6803-028`, `MC6803-029`).
    fn inherent(&mut self, cycle: u8, inh: Inherent) -> Bus {
        match inh {
            Inherent::Nop => self.read(self.pc),
            Inherent::Tap => {
                self.set_ccr(self.a);
                self.read(self.pc)
            }
            Inherent::Tpa => {
                self.a = self.ccr | alu::UPPER;
                self.read(self.pc)
            }
            Inherent::Clv => {
                self.ccr &= !alu::V;
                self.read(self.pc)
            }
            Inherent::Sev => {
                self.ccr |= alu::V;
                self.read(self.pc)
            }
            Inherent::Clc => {
                self.ccr &= !alu::C;
                self.read(self.pc)
            }
            Inherent::Sec => {
                self.ccr |= alu::C;
                self.read(self.pc)
            }
            Inherent::Cli => {
                self.set_ccr(self.ccr & !alu::I);
                self.read(self.pc)
            }
            Inherent::Sei => {
                self.set_i();
                self.read(self.pc)
            }
            Inherent::Sba => {
                let (ccr, r) = alu::sub8(self.ccr, self.a, self.b, false);
                self.ccr = ccr;
                self.a = r;
                self.read(self.pc)
            }
            Inherent::Cba => {
                let (ccr, _) = alu::sub8(self.ccr, self.a, self.b, false);
                self.ccr = ccr;
                self.read(self.pc)
            }
            Inherent::Tab => {
                self.b = self.a;
                self.ccr = alu::nzv0_8(self.ccr, self.b);
                self.read(self.pc)
            }
            Inherent::Tba => {
                self.a = self.b;
                self.ccr = alu::nzv0_8(self.ccr, self.a);
                self.read(self.pc)
            }
            Inherent::Daa => {
                let (ccr, r) = alu::daa(self.ccr, self.a);
                self.ccr = ccr;
                self.a = r;
                self.read(self.pc)
            }
            Inherent::Aba => {
                let (ccr, r) = alu::add8(self.ccr, self.a, self.b, false);
                self.ccr = ccr;
                self.a = r;
                self.read(self.pc)
            }

            Inherent::Abx => match cycle {
                2 => self.read(self.pc),
                _ => {
                    self.x = self.x.wrapping_add(self.b as u16);
                    self.idle()
                }
            },
            Inherent::Asld => match cycle {
                2 => self.read(self.pc),
                _ => {
                    let (ccr, r) = alu::asld(self.ccr, self.d());
                    self.ccr = ccr;
                    self.set_d(r);
                    self.idle()
                }
            },
            Inherent::Lsrd => match cycle {
                2 => self.read(self.pc),
                _ => {
                    let (ccr, r) = alu::lsrd(self.ccr, self.d());
                    self.ccr = ccr;
                    self.set_d(r);
                    self.idle()
                }
            },
            Inherent::Inx => match cycle {
                2 => self.read(self.pc),
                _ => {
                    self.x = self.x.wrapping_add(1);
                    self.ccr = if self.x == 0 {
                        self.ccr | alu::Z
                    } else {
                        self.ccr & !alu::Z
                    };
                    self.idle()
                }
            },
            Inherent::Dex => match cycle {
                2 => self.read(self.pc),
                _ => {
                    self.x = self.x.wrapping_sub(1);
                    self.ccr = if self.x == 0 {
                        self.ccr | alu::Z
                    } else {
                        self.ccr & !alu::Z
                    };
                    self.idle()
                }
            },
            Inherent::Txs => match cycle {
                2 => self.read(self.pc),
                _ => {
                    self.sp = self.x.wrapping_sub(1);
                    self.idle()
                }
            },

            Inherent::Des => match cycle {
                2 => self.read(self.pc),
                _ => {
                    let bus = self.dummy(self.sp);
                    self.sp = self.sp.wrapping_sub(1);
                    bus
                }
            },
            Inherent::Ins => match cycle {
                2 => self.read(self.pc),
                _ => {
                    let bus = self.dummy(self.sp);
                    self.sp = self.sp.wrapping_add(1);
                    bus
                }
            },
            Inherent::Tsx => match cycle {
                2 => self.read(self.pc),
                _ => {
                    let bus = self.dummy(self.sp);
                    self.x = self.sp.wrapping_add(1);
                    bus
                }
            },
            Inherent::Push(Stacked::A) => match cycle {
                2 => self.read(self.pc),
                _ => self.push(self.acc(Accumulator::A)),
            },
            Inherent::Push(Stacked::B) => match cycle {
                2 => self.read(self.pc),
                _ => self.push(self.acc(Accumulator::B)),
            },

            Inherent::Pull(Stacked::A) => match cycle {
                2 => self.read(self.pc),
                3 => self.dummy(self.sp),
                _ => {
                    self.finish = Finish::Pull(Accumulator::A);
                    self.pull()
                }
            },
            Inherent::Pull(Stacked::B) => match cycle {
                2 => self.read(self.pc),
                3 => self.dummy(self.sp),
                _ => {
                    self.finish = Finish::Pull(Accumulator::B);
                    self.pull()
                }
            },
            Inherent::Push(Stacked::X) => match cycle {
                2 => self.read(self.pc),
                3 => self.push(self.x as u8),
                _ => self.push((self.x >> 8) as u8),
            },
            Inherent::Pull(Stacked::X) => match cycle {
                2 => self.read(self.pc),
                3 => self.dummy(self.sp),
                4 => self.pull(),
                _ => {
                    self.tmp = self.dl;
                    self.finish = Finish::X;
                    self.pull()
                }
            },
            Inherent::Rts => match cycle {
                2 => self.read(self.pc),
                3 => self.dummy(self.sp),
                4 => self.pull(),
                _ => {
                    self.tmp = self.dl;
                    self.finish = Finish::Pc;
                    self.pull()
                }
            },

            Inherent::Mul => match cycle {
                2 => self.read(self.pc),
                10 => {
                    let (ccr, r) = alu::mul(self.ccr, self.a, self.b);
                    self.ccr = ccr;
                    self.set_d(r);
                    self.idle()
                }
                _ => self.idle(),
            },

            Inherent::Rti => match cycle {
                2 => self.read(self.pc),
                3 => self.dummy(self.sp),
                4 => self.pull(),
                5 => {
                    self.set_ccr(self.dl);
                    self.pull()
                }
                6 => {
                    self.b = self.dl;
                    self.pull()
                }
                7 => {
                    self.a = self.dl;
                    self.pull()
                }
                8 => {
                    self.tmp = self.dl;
                    self.pull()
                }
                9 => {
                    self.x = ((self.tmp as u16) << 8) | self.dl as u16;
                    self.pull()
                }
                _ => {
                    self.tmp = self.dl;
                    self.finish = Finish::Pc;
                    self.pull()
                }
            },

            Inherent::Swi => match cycle {
                2 => self.read(self.pc),
                3..=9 => self.stack_cycle(cycle),
                10 => {
                    let bus = self.dummy(self.sp);
                    self.set_i();
                    bus
                }
                11 => self.read(vector::SWI),
                _ => {
                    self.tmp = self.dl;
                    self.finish = Finish::Pc;
                    self.read(vector::SWI + 1)
                }
            },

            Inherent::Wai => match cycle {
                2 => self.read(self.pc),
                9 => {
                    let bus = self.stack_cycle(9);
                    self.seq = Seq::Wait;
                    self.cycle = 1;
                    bus
                }
                _ => self.stack_cycle(cycle),
            },
        }
    }
}
