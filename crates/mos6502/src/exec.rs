//! The sequencer: which bus cycle each cycle of each instruction performs, and
//! what the part does with the byte that comes back.
//!
//! The cycle lists are Appendix A of MOS Technology's *MCS6500 Microcomputer
//! Family Hardware Manual* (6500-10A), *Summary of Single Cycle Execution*,
//! which prints the address bus, the data bus and `R/W` for every cycle of
//! every addressing mode.

use crate::alu::{self, FLAG_B, FLAG_C, FLAG_D, FLAG_I, FLAG_N, FLAG_U, FLAG_V, FLAG_Z, nz, set};
use crate::decode::{Access, Addressing, Alu8, Cond, Inherent, Kind, Reg, Stacked, Unary, decode};
use crate::{Bus, Finish, Interrupt, Mos6502};

/// Page one. `MOS6502-026`: the part puts `$01` on the high address lines
/// during every stack operation.
const STACK: u16 = 0x0100;

impl Mos6502 {
    /// One cycle of the sequencer: the bus operation to perform now.
    pub(crate) fn step(&mut self) -> Bus {
        self.fetching = false;
        if self.res_held {
            return Bus::Read(self.pc);
        }
        if self.stopped {
            return Bus::Read(0xffff);
        }
        if self.cycle == 1 {
            self.complete();
            return self.begin();
        }
        if self.cycle == 2 && self.interrupt.is_none() {
            self.op = self.dl;
        }
        let row = decode(self.op);
        let addressing = if self.interrupt.is_some() {
            Addressing::Brk
        } else {
            row.addressing
        };
        match addressing {
            Addressing::Implied | Addressing::Accumulator => {
                self.implied_cycle(row.kind, addressing)
            }
            Addressing::Immediate => self.immediate_cycle(),
            Addressing::ZeroPage => self.zero_page_cycle(row.kind, row.access),
            Addressing::ZeroPageX => self.zero_page_indexed_cycle(row.kind, row.access, self.x),
            Addressing::ZeroPageY => self.zero_page_indexed_cycle(row.kind, row.access, self.y),
            Addressing::Absolute => self.absolute_cycle(row.kind, row.access),
            Addressing::AbsoluteX => self.absolute_indexed_cycle(row.kind, row.access, self.x),
            Addressing::AbsoluteY => self.absolute_indexed_cycle(row.kind, row.access, self.y),
            Addressing::IndirectX => self.indexed_indirect_cycle(row.kind, row.access),
            Addressing::IndirectY => self.indirect_indexed_cycle(row.kind, row.access),
            Addressing::Relative => self.relative_cycle(row.kind),
            Addressing::JmpAbsolute => self.jump_absolute_cycle(),
            Addressing::Indirect => self.jump_indirect_cycle(),
            Addressing::Brk => self.interrupt_cycle(),
            Addressing::Jsr => self.jsr_cycle(),
            Addressing::Rts => self.rts_cycle(),
            Addressing::Rti => self.rti_cycle(),
            Addressing::Push => self.push_cycle(row.kind),
            Addressing::Pull => self.pull_cycle(),
            Addressing::Undefined => self.stop_cycle(),
        }
    }

    /// The tail of the instruction that has just ended (`MOS6502-041`).
    pub(crate) fn complete(&mut self) {
        match self.finish {
            Finish::None => {}
            Finish::Operand => self.apply_operand(self.dl),
            Finish::Pull => match decode(self.op).kind {
                Kind::Inherent(Inherent::Pull(Stacked::A)) => {
                    self.a = self.dl;
                    self.p = nz(self.p, self.a);
                }
                Kind::Inherent(Inherent::Pull(Stacked::P)) => {
                    self.p = (self.dl & !(FLAG_B | FLAG_U)) | FLAG_U
                }
                _ => {}
            },
            Finish::PcHigh => self.pc = u16::from_le_bytes([self.pcl, self.dl]),
        }
        self.finish = Finish::None;
    }

    /// Cycle one: an interrupt sequence, or an op-code fetch.
    fn begin(&mut self) -> Bus {
        self.cycle = 2;
        self.cross = false;
        if self.res_pending {
            self.res_pending = false;
            return self.begin_interrupt(Interrupt::Reset);
        }
        if self.nmi_pending {
            self.nmi_pending = false;
            return self.begin_interrupt(Interrupt::Nmi);
        }
        if self.irq_line && self.p & FLAG_I == 0 {
            return self.begin_interrupt(Interrupt::Irq);
        }
        self.interrupt = None;
        self.fetching = true;
        let at = self.pc;
        self.pc = self.pc.wrapping_add(1);
        Bus::Read(at)
    }

    /// `MOS6502-074`: a hardware interrupt runs `BRK`'s cycles, and the byte it
    /// reads at `PC` is discarded rather than decoded.
    fn begin_interrupt(&mut self, kind: Interrupt) -> Bus {
        self.interrupt = Some(kind);
        self.op = 0x00;
        Bus::Read(self.pc)
    }

    /// Where the sequence that is running takes its program counter from
    /// (`MOS6502-015`).
    fn vector(&self) -> u16 {
        match self.interrupt {
            Some(Interrupt::Reset) => crate::vector::RESET,
            Some(Interrupt::Nmi) => crate::vector::NMI,
            _ => crate::vector::IRQ,
        }
    }

    /// The effective address the addressing mode has assembled.
    fn address(&self) -> u16 {
        u16::from_le_bytes([self.adl, self.adh])
    }

    /// A write, unless a reset is running — `MOS6502-013`: while `RES` is Low
    /// writing is inhibited, so the three stack cycles happen and store
    /// nothing.
    fn store(&mut self, address: u16, value: u8) -> Bus {
        if self.interrupt == Some(Interrupt::Reset) {
            return Bus::Read(address);
        }
        Bus::Write(address, value)
    }

    /// Finish an instruction that ends on a read of the operand.
    fn read_operand(&mut self, address: u16) -> Bus {
        self.cycle = 1;
        self.finish = Finish::Operand;
        Bus::Read(address)
    }

    /// Two cycles, the second a discarded read from `PC + 1` (`MOS6502-040`).
    fn implied_cycle(&mut self, op: Kind, addressing: Addressing) -> Bus {
        self.cycle = 1;
        if addressing == Addressing::Accumulator {
            let value = self.modify(op, self.a);
            self.a = value;
        } else {
            self.implied(op);
        }
        Bus::Read(self.pc)
    }

    /// Two cycles; the operand is the byte after the opcode.
    fn immediate_cycle(&mut self) -> Bus {
        let at = self.pc;
        self.pc = self.pc.wrapping_add(1);
        self.read_operand(at)
    }

    fn zero_page_cycle(&mut self, op: Kind, access: Access) -> Bus {
        match self.cycle {
            2 => {
                let at = self.pc;
                self.pc = self.pc.wrapping_add(1);
                self.cycle = 3;
                Bus::Read(at)
            }
            3 => {
                self.adl = self.dl;
                self.adh = 0;
                self.access(op, access, 4)
            }
            _ => self.modify_cycles(op, 4),
        }
    }

    fn zero_page_indexed_cycle(&mut self, op: Kind, access: Access, index: u8) -> Bus {
        match self.cycle {
            2 => {
                let at = self.pc;
                self.pc = self.pc.wrapping_add(1);
                self.cycle = 3;
                Bus::Read(at)
            }
            3 => {
                self.base = self.dl;
                self.cycle = 4;
                Bus::Read(u16::from(self.base))
            }
            4 => {
                self.adl = self.base.wrapping_add(index);
                self.adh = 0;
                self.access(op, access, 5)
            }
            _ => self.modify_cycles(op, 5),
        }
    }

    fn absolute_cycle(&mut self, op: Kind, access: Access) -> Bus {
        match self.cycle {
            2 => {
                let at = self.pc;
                self.pc = self.pc.wrapping_add(1);
                self.cycle = 3;
                Bus::Read(at)
            }
            3 => {
                self.adl = self.dl;
                let at = self.pc;
                self.pc = self.pc.wrapping_add(1);
                self.cycle = 4;
                Bus::Read(at)
            }
            4 => {
                self.adh = self.dl;
                self.access(op, access, 5)
            }
            _ => self.modify_cycles(op, 5),
        }
    }

    fn absolute_indexed_cycle(&mut self, op: Kind, access: Access, index: u8) -> Bus {
        match self.cycle {
            2 => {
                let at = self.pc;
                self.pc = self.pc.wrapping_add(1);
                self.cycle = 3;
                Bus::Read(at)
            }
            3 => {
                self.base = self.dl;
                let at = self.pc;
                self.pc = self.pc.wrapping_add(1);
                self.cycle = 4;
                Bus::Read(at)
            }
            4 => {
                self.adh = self.dl;
                let (adl, cross) = self.base.overflowing_add(index);
                self.adl = adl;
                self.cross = cross;
                if access == Access::Read && !cross {
                    return self.read_operand(self.address());
                }
                self.cycle = 5;
                Bus::Read(self.address())
            }
            5 => {
                let h = self.adh;
                if self.cross {
                    self.adh = self.adh.wrapping_add(1);
                }
                match access {
                    Access::Read => self.read_operand(self.address()),
                    Access::Write => {
                        let value = self.unstable_store(op, h.wrapping_add(1));
                        self.cycle = 1;
                        let at = self.address();
                        self.store(at, value)
                    }
                    _ => {
                        self.cycle = 6;
                        Bus::Read(self.address())
                    }
                }
            }
            _ => self.modify_cycles(op, 6),
        }
    }

    /// `MOS6502-045`: the un-indexed pointer is read first, and both halves of
    /// the pointer come from page zero.
    fn indexed_indirect_cycle(&mut self, op: Kind, access: Access) -> Bus {
        match self.cycle {
            2 => {
                let at = self.pc;
                self.pc = self.pc.wrapping_add(1);
                self.cycle = 3;
                Bus::Read(at)
            }
            3 => {
                self.base = self.dl;
                self.cycle = 4;
                Bus::Read(u16::from(self.base))
            }
            4 => {
                self.base = self.base.wrapping_add(self.x);
                self.cycle = 5;
                Bus::Read(u16::from(self.base))
            }
            5 => {
                self.adl = self.dl;
                self.cycle = 6;
                Bus::Read(u16::from(self.base.wrapping_add(1)))
            }
            6 => {
                self.adh = self.dl;
                self.access(op, access, 7)
            }
            _ => self.modify_cycles(op, 7),
        }
    }

    fn indirect_indexed_cycle(&mut self, op: Kind, access: Access) -> Bus {
        match self.cycle {
            2 => {
                let at = self.pc;
                self.pc = self.pc.wrapping_add(1);
                self.cycle = 3;
                Bus::Read(at)
            }
            3 => {
                self.base = self.dl;
                self.cycle = 4;
                Bus::Read(u16::from(self.base))
            }
            4 => {
                self.adl = self.dl;
                self.cycle = 5;
                Bus::Read(u16::from(self.base.wrapping_add(1)))
            }
            5 => {
                self.adh = self.dl;
                let (adl, cross) = self.adl.overflowing_add(self.y);
                self.adl = adl;
                self.cross = cross;
                if access == Access::Read && !cross {
                    return self.read_operand(self.address());
                }
                self.cycle = 6;
                Bus::Read(self.address())
            }
            6 => {
                let h = self.adh;
                if self.cross {
                    self.adh = self.adh.wrapping_add(1);
                }
                match access {
                    Access::Read => self.read_operand(self.address()),
                    Access::Write => {
                        let value = self.unstable_store(op, h.wrapping_add(1));
                        self.cycle = 1;
                        let at = self.address();
                        self.store(at, value)
                    }
                    _ => {
                        self.cycle = 7;
                        Bus::Read(self.address())
                    }
                }
            }
            _ => self.modify_cycles(op, 7),
        }
    }

    /// The branches (`MOS6502-037`).
    fn relative_cycle(&mut self, op: Kind) -> Bus {
        match self.cycle {
            2 => {
                let at = self.pc;
                self.pc = self.pc.wrapping_add(1);
                self.cycle = if self.branch_taken(op) { 3 } else { 1 };
                Bus::Read(at)
            }
            3 => {
                let here = self.pc;
                #[allow(clippy::cast_sign_loss)]
                let target = here.wrapping_add(self.dl as i8 as u16);
                self.pc = (here & 0xff00) | (target & 0x00ff);
                self.cross = target != self.pc;
                [self.adl, self.adh] = target.to_le_bytes();
                self.cycle = if self.cross { 4 } else { 1 };
                Bus::Read(here)
            }
            _ => {
                let stale = self.pc;
                self.pc = self.address();
                self.cycle = 1;
                Bus::Read(stale)
            }
        }
    }

    fn jump_absolute_cycle(&mut self) -> Bus {
        match self.cycle {
            2 => {
                let at = self.pc;
                self.pc = self.pc.wrapping_add(1);
                self.cycle = 3;
                Bus::Read(at)
            }
            _ => {
                self.pcl = self.dl;
                self.cycle = 1;
                self.finish = Finish::PcHigh;
                Bus::Read(self.pc)
            }
        }
    }

    fn jump_indirect_cycle(&mut self) -> Bus {
        match self.cycle {
            2 => {
                let at = self.pc;
                self.pc = self.pc.wrapping_add(1);
                self.cycle = 3;
                Bus::Read(at)
            }
            3 => {
                self.adl = self.dl;
                let at = self.pc;
                self.pc = self.pc.wrapping_add(1);
                self.cycle = 4;
                Bus::Read(at)
            }
            4 => {
                self.adh = self.dl;
                self.cycle = 5;
                Bus::Read(self.address())
            }
            _ => {
                self.pcl = self.dl;
                self.adl = self.adl.wrapping_add(1);
                self.cycle = 1;
                self.finish = Finish::PcHigh;
                Bus::Read(self.address())
            }
        }
    }

    /// `BRK`, and the three hardware sequences (`MOS6502-023`, `MOS6502-074`).
    fn interrupt_cycle(&mut self) -> Bus {
        match self.cycle {
            2 => {
                self.cycle = 3;
                if self.interrupt.is_some() {
                    return Bus::Read(self.pc);
                }
                let at = self.pc;
                self.pc = self.pc.wrapping_add(1);
                Bus::Read(at)
            }
            3 => {
                let at = STACK | u16::from(self.s);
                self.s = self.s.wrapping_sub(1);
                self.cycle = 4;
                self.store(at, self.pc.to_le_bytes()[1])
            }
            4 => {
                let at = STACK | u16::from(self.s);
                self.s = self.s.wrapping_sub(1);
                self.cycle = 5;
                self.store(at, self.pc.to_le_bytes()[0])
            }
            5 => {
                let pushed = if self.interrupt.is_some() {
                    (self.p | FLAG_U) & !FLAG_B
                } else {
                    self.p | FLAG_U | FLAG_B
                };
                let at = STACK | u16::from(self.s);
                self.s = self.s.wrapping_sub(1);
                self.p |= FLAG_I;
                self.cycle = 6;
                self.store(at, pushed)
            }
            6 => {
                self.cycle = 7;
                Bus::Read(self.vector())
            }
            _ => {
                self.pcl = self.dl;
                self.cycle = 1;
                self.finish = Finish::PcHigh;
                Bus::Read(self.vector() | 1)
            }
        }
    }

    /// `MOS6502-049`: the third cycle is a discarded read of the stack, and the
    /// target's high byte is fetched after both pushes.
    fn jsr_cycle(&mut self) -> Bus {
        match self.cycle {
            2 => {
                let at = self.pc;
                self.pc = self.pc.wrapping_add(1);
                self.cycle = 3;
                Bus::Read(at)
            }
            3 => {
                self.adl = self.dl;
                self.cycle = 4;
                Bus::Read(STACK | u16::from(self.s))
            }
            4 => {
                let at = STACK | u16::from(self.s);
                self.s = self.s.wrapping_sub(1);
                self.cycle = 5;
                self.store(at, self.pc.to_le_bytes()[1])
            }
            5 => {
                let at = STACK | u16::from(self.s);
                self.s = self.s.wrapping_sub(1);
                self.cycle = 6;
                self.store(at, self.pc.to_le_bytes()[0])
            }
            _ => {
                self.pcl = self.adl;
                self.cycle = 1;
                self.finish = Finish::PcHigh;
                Bus::Read(self.pc)
            }
        }
    }

    /// `MOS6502-051`.
    fn rts_cycle(&mut self) -> Bus {
        match self.cycle {
            2 => {
                self.cycle = 3;
                Bus::Read(self.pc)
            }
            3 => {
                let at = STACK | u16::from(self.s);
                self.s = self.s.wrapping_add(1);
                self.cycle = 4;
                Bus::Read(at)
            }
            4 => {
                let at = STACK | u16::from(self.s);
                self.s = self.s.wrapping_add(1);
                self.cycle = 5;
                Bus::Read(at)
            }
            5 => {
                self.pcl = self.dl;
                self.cycle = 6;
                Bus::Read(STACK | u16::from(self.s))
            }
            _ => {
                self.pc = u16::from_le_bytes([self.pcl, self.dl]);
                let at = self.pc;
                self.pc = self.pc.wrapping_add(1);
                self.cycle = 1;
                Bus::Read(at)
            }
        }
    }

    /// `MOS6502-052`.
    fn rti_cycle(&mut self) -> Bus {
        match self.cycle {
            2 => {
                self.cycle = 3;
                Bus::Read(self.pc)
            }
            3 => {
                let at = STACK | u16::from(self.s);
                self.s = self.s.wrapping_add(1);
                self.cycle = 4;
                Bus::Read(at)
            }
            4 => {
                let at = STACK | u16::from(self.s);
                self.s = self.s.wrapping_add(1);
                self.cycle = 5;
                Bus::Read(at)
            }
            5 => {
                self.p = (self.dl & !(FLAG_B | FLAG_U)) | FLAG_U;
                let at = STACK | u16::from(self.s);
                self.s = self.s.wrapping_add(1);
                self.cycle = 6;
                Bus::Read(at)
            }
            _ => {
                self.pcl = self.dl;
                self.cycle = 1;
                self.finish = Finish::PcHigh;
                Bus::Read(STACK | u16::from(self.s))
            }
        }
    }

    /// `MOS6502-053`: three cycles, the second a discarded fetch.
    fn push_cycle(&mut self, op: Kind) -> Bus {
        match self.cycle {
            2 => {
                self.cycle = 3;
                Bus::Read(self.pc)
            }
            _ => {
                let value = if op == Kind::Inherent(Inherent::Push(Stacked::A)) {
                    self.a
                } else {
                    self.p | FLAG_U | FLAG_B
                };
                let at = STACK | u16::from(self.s);
                self.s = self.s.wrapping_sub(1);
                self.cycle = 1;
                self.store(at, value)
            }
        }
    }

    /// `MOS6502-050`, `MOS6502-053`: four cycles, and the stack is read once
    /// before the byte the instruction wants.
    fn pull_cycle(&mut self) -> Bus {
        match self.cycle {
            2 => {
                self.cycle = 3;
                Bus::Read(self.pc)
            }
            3 => {
                let at = STACK | u16::from(self.s);
                self.s = self.s.wrapping_add(1);
                self.cycle = 4;
                Bus::Read(at)
            }
            _ => {
                self.cycle = 1;
                self.finish = Finish::Pull;
                Bus::Read(STACK | u16::from(self.s))
            }
        }
    }

    /// `MOS6502-OPEN-01`: the byte after the opcode is fetched, and then
    /// nothing. Which opcodes reach here, and whether a part does this at all,
    /// is this crate's choice.
    fn stop_cycle(&mut self) -> Bus {
        self.stopped = true;
        Bus::Read(self.pc)
    }

    /// The cycle that touches the effective address, once the mode has it.
    fn access(&mut self, op: Kind, access: Access, modify_from: u8) -> Bus {
        let at = self.address();
        match access {
            Access::Read => self.read_operand(at),
            Access::Write => {
                self.cycle = 1;
                let value = self.store_value(op);
                self.store(at, value)
            }
            _ => {
                self.cycle = modify_from;
                Bus::Read(at)
            }
        }
    }

    /// `MOS6502-047`: a read-modify-write writes the byte back unchanged before
    /// it writes the new one.
    fn modify_cycles(&mut self, op: Kind, first: u8) -> Bus {
        let at = self.address();
        if self.cycle == first {
            self.tmp = self.dl;
            self.cycle = first + 1;
            return self.store(at, self.tmp);
        }
        let value = self.modify(op, self.tmp);
        self.cycle = 1;
        self.store(at, value)
    }

    /// The byte a store instruction puts on the bus.
    fn store_value(&self, op: Kind) -> u8 {
        match op {
            Kind::St(Reg::A) => self.a,
            Kind::St(Reg::X) => self.x,
            Kind::St(Reg::Y) => self.y,
            Kind::Sax => self.a & self.x,
            Kind::Alu8(_)
            | Kind::Unary(_)
            | Kind::Branch(_)
            | Kind::Jmp
            | Kind::Jsr
            | Kind::Inherent(_)
            | Kind::Slo
            | Kind::Rla
            | Kind::Sre
            | Kind::Rra
            | Kind::Lax
            | Kind::Dcp
            | Kind::Isc
            | Kind::Anc
            | Kind::Alr
            | Kind::Arr
            | Kind::Sbx
            | Kind::Las
            | Kind::Sha
            | Kind::Shx
            | Kind::Shy
            | Kind::Tas
            | Kind::Undefined
            | Kind::Ane
            | Kind::Lxa => 0,
        }
    }

    /// The byte an indexed store puts on the bus, where `h_plus_1` is the high
    /// half of the address in the opcode plus one.
    ///
    /// `MOS6502-069`: for five opcodes the value is ANDed with that byte, and
    /// where indexing crossed a page the same AND lands on the address as well.
    fn unstable_store(&mut self, op: Kind, h_plus_1: u8) -> u8 {
        let mask = match op {
            Kind::Sha | Kind::Tas => self.a & self.x,
            Kind::Shx => self.x,
            Kind::Shy => self.y,
            _ => return self.store_value(op),
        };
        if op == Kind::Tas {
            self.s = self.a & self.x;
        }
        if self.cross {
            self.adh &= mask;
        }
        mask & h_plus_1
    }

    /// A read-modify-write's new byte, and the flags and registers it leaves.
    fn modify(&mut self, op: Kind, m: u8) -> u8 {
        match op {
            Kind::Unary(Unary::AslA | Unary::AslMemory) => {
                let value = m << 1;
                self.p = set(nz(self.p, value), FLAG_C, m & 0x80 != 0);
                value
            }
            Kind::Unary(Unary::LsrA | Unary::LsrMemory) => {
                let value = m >> 1;
                self.p = set(nz(self.p, value), FLAG_C, m & 1 != 0);
                value
            }
            Kind::Unary(Unary::RolA | Unary::RolMemory) => {
                let value = (m << 1) | (self.p & FLAG_C);
                self.p = set(nz(self.p, value), FLAG_C, m & 0x80 != 0);
                value
            }
            Kind::Unary(Unary::RorA | Unary::RorMemory) => {
                let value = (m >> 1) | ((self.p & FLAG_C) << 7);
                self.p = set(nz(self.p, value), FLAG_C, m & 1 != 0);
                value
            }
            Kind::Unary(Unary::IncX | Unary::IncY | Unary::IncMemory) => {
                let value = m.wrapping_add(1);
                self.p = nz(self.p, value);
                value
            }
            Kind::Unary(Unary::DecX | Unary::DecY | Unary::DecMemory) => {
                let value = m.wrapping_sub(1);
                self.p = nz(self.p, value);
                value
            }
            Kind::Slo => {
                let value = self.modify(Kind::Unary(Unary::AslMemory), m);
                self.apply_operand_op(Kind::Alu8(Alu8::Ora), value);
                value
            }
            Kind::Rla => {
                let value = self.modify(Kind::Unary(Unary::RolMemory), m);
                self.apply_operand_op(Kind::Alu8(Alu8::And), value);
                value
            }
            Kind::Sre => {
                let value = self.modify(Kind::Unary(Unary::LsrMemory), m);
                self.apply_operand_op(Kind::Alu8(Alu8::Eor), value);
                value
            }
            Kind::Rra => {
                let value = self.modify(Kind::Unary(Unary::RorMemory), m);
                self.apply_operand_op(Kind::Alu8(Alu8::Adc), value);
                value
            }
            Kind::Dcp => {
                let value = m.wrapping_sub(1);
                self.apply_operand_op(Kind::Alu8(Alu8::Cmp), value);
                value
            }
            Kind::Isc => {
                let value = m.wrapping_add(1);
                self.apply_operand_op(Kind::Alu8(Alu8::Sbc), value);
                value
            }
            Kind::Alu8(_)
            | Kind::St(_)
            | Kind::Branch(_)
            | Kind::Jmp
            | Kind::Jsr
            | Kind::Inherent(_)
            | Kind::Sax
            | Kind::Lax
            | Kind::Anc
            | Kind::Alr
            | Kind::Arr
            | Kind::Sbx
            | Kind::Las
            | Kind::Sha
            | Kind::Shx
            | Kind::Shy
            | Kind::Tas
            | Kind::Undefined
            | Kind::Ane
            | Kind::Lxa => m,
        }
    }

    /// An implied or accumulator instruction that needs no operand.
    fn implied(&mut self, op: Kind) {
        match op {
            Kind::Inherent(Inherent::Clc) => self.p &= !FLAG_C,
            Kind::Inherent(Inherent::Sec) => self.p |= FLAG_C,
            Kind::Inherent(Inherent::Cli) => self.p &= !FLAG_I,
            Kind::Inherent(Inherent::Sei) => self.p |= FLAG_I,
            Kind::Inherent(Inherent::Clv) => self.p &= !FLAG_V,
            Kind::Inherent(Inherent::Cld) => self.p &= !FLAG_D,
            Kind::Inherent(Inherent::Sed) => self.p |= FLAG_D,
            Kind::Unary(Unary::IncX) => {
                self.x = self.x.wrapping_add(1);
                self.p = nz(self.p, self.x);
            }
            Kind::Unary(Unary::DecX) => {
                self.x = self.x.wrapping_sub(1);
                self.p = nz(self.p, self.x);
            }
            Kind::Unary(Unary::IncY) => {
                self.y = self.y.wrapping_add(1);
                self.p = nz(self.p, self.y);
            }
            Kind::Unary(Unary::DecY) => {
                self.y = self.y.wrapping_sub(1);
                self.p = nz(self.p, self.y);
            }
            Kind::Inherent(Inherent::Tax) => {
                self.x = self.a;
                self.p = nz(self.p, self.x);
            }
            Kind::Inherent(Inherent::Tay) => {
                self.y = self.a;
                self.p = nz(self.p, self.y);
            }
            Kind::Inherent(Inherent::Tsx) => {
                self.x = self.s;
                self.p = nz(self.p, self.x);
            }
            Kind::Inherent(Inherent::Txa) => {
                self.a = self.x;
                self.p = nz(self.p, self.a);
            }
            Kind::Inherent(Inherent::Tya) => {
                self.a = self.y;
                self.p = nz(self.p, self.a);
            }
            Kind::Inherent(Inherent::Txs) => self.s = self.x,
            _ => {}
        }
    }

    /// Whether the branch this opcode names is taken (`MOS6502-037`).
    fn branch_taken(&self, op: Kind) -> bool {
        match op {
            Kind::Branch(Cond::Pl) => self.p & FLAG_N == 0,
            Kind::Branch(Cond::Mi) => self.p & FLAG_N != 0,
            Kind::Branch(Cond::Vc) => self.p & FLAG_V == 0,
            Kind::Branch(Cond::Vs) => self.p & FLAG_V != 0,
            Kind::Branch(Cond::Cc) => self.p & FLAG_C == 0,
            Kind::Branch(Cond::Cs) => self.p & FLAG_C != 0,
            Kind::Branch(Cond::Ne) => self.p & FLAG_Z == 0,
            Kind::Branch(Cond::Eq) => self.p & FLAG_Z != 0,
            Kind::Alu8(_)
            | Kind::St(_)
            | Kind::Unary(_)
            | Kind::Jmp
            | Kind::Jsr
            | Kind::Inherent(_)
            | Kind::Slo
            | Kind::Rla
            | Kind::Sre
            | Kind::Rra
            | Kind::Sax
            | Kind::Lax
            | Kind::Dcp
            | Kind::Isc
            | Kind::Anc
            | Kind::Alr
            | Kind::Arr
            | Kind::Sbx
            | Kind::Las
            | Kind::Sha
            | Kind::Shx
            | Kind::Shy
            | Kind::Tas
            | Kind::Undefined
            | Kind::Ane
            | Kind::Lxa => false,
        }
    }

    fn apply_operand(&mut self, m: u8) {
        self.apply_operand_op(decode(self.op).kind, m);
    }

    /// What an instruction that ends on a read does with the byte.
    fn apply_operand_op(&mut self, op: Kind, m: u8) {
        match op {
            Kind::Alu8(Alu8::Ld) => {
                self.a = m;
                self.p = nz(self.p, self.a);
            }
            Kind::Alu8(Alu8::LdX) => {
                self.x = m;
                self.p = nz(self.p, self.x);
            }
            Kind::Alu8(Alu8::LdY) => {
                self.y = m;
                self.p = nz(self.p, self.y);
            }
            Kind::Alu8(Alu8::Ora) => {
                self.a |= m;
                self.p = nz(self.p, self.a);
            }
            Kind::Alu8(Alu8::And) => {
                self.a &= m;
                self.p = nz(self.p, self.a);
            }
            Kind::Alu8(Alu8::Eor) => {
                self.a ^= m;
                self.p = nz(self.p, self.a);
            }
            Kind::Alu8(Alu8::Adc) => {
                let out = alu::adc(self.a, m, self.p);
                self.a = out.value;
                self.p = out.p;
            }
            Kind::Alu8(Alu8::Sbc) => {
                let out = alu::sbc(self.a, m, self.p);
                self.a = out.value;
                self.p = out.p;
            }
            Kind::Alu8(Alu8::Cmp) => self.p = alu::compare(self.a, m, self.p),
            Kind::Alu8(Alu8::CmpX) => self.p = alu::compare(self.x, m, self.p),
            Kind::Alu8(Alu8::CmpY) => self.p = alu::compare(self.y, m, self.p),
            Kind::Alu8(Alu8::Bit) => {
                self.p = set(self.p, FLAG_Z, self.a & m == 0);
                self.p = set(self.p, FLAG_N, m & FLAG_N != 0);
                self.p = set(self.p, FLAG_V, m & FLAG_V != 0);
            }
            Kind::Lax => {
                self.a = m;
                self.x = m;
                self.p = nz(self.p, self.a);
            }
            Kind::Las => {
                let value = m & self.s;
                self.a = value;
                self.x = value;
                self.s = value;
                self.p = nz(self.p, value);
            }
            Kind::Anc => {
                self.a &= m;
                self.p = nz(self.p, self.a);
                self.p = set(self.p, FLAG_C, self.a & FLAG_N != 0);
            }
            Kind::Alr => {
                let and = self.a & m;
                self.a = and >> 1;
                self.p = nz(self.p, self.a);
                self.p = set(self.p, FLAG_C, and & 1 != 0);
            }
            Kind::Sbx => {
                let lhs = self.a & self.x;
                self.x = lhs.wrapping_sub(m);
                self.p = set(nz(self.p, self.x), FLAG_C, lhs >= m);
            }
            Kind::Arr => {
                let out = alu::arr(self.a, m, self.p);
                self.a = out.value;
                self.p = out.p;
            }
            Kind::Ane => {
                self.a = (self.a | self.magic) & self.x & m;
                self.p = nz(self.p, self.a);
            }
            Kind::Lxa => {
                let value = (self.a | self.magic) & m;
                self.a = value;
                self.x = value;
                self.p = nz(self.p, value);
            }
            _ => {}
        }
    }
}
