//! What each instruction does, and which machine cycles it takes to do it.
//!
//! Table 2-4 of Intel's *MCS-80 User's Manual* (98-153D, October 1977) gives
//! the order the effects happen in. The state counts here are `I8080-052`
//! to `I8080-057` and `I8080-059` to `I8080-073` — the manual's own Summary of Processor Instructions —
//! expressed as `M1`'s length plus one three-state machine cycle per bus
//! transaction.

use crate::alu;
use crate::decode::{Alu8, Inherent, Kind, Pair, Reg};
use crate::seq::{Cycle, CycleKind};
use crate::{FLAG_CY, I8080, Mode};

/// How many states `M1` has for this instruction.
///
/// Four normally; five where Table 2-4 gives the row a `T5`, which is exactly
/// the instructions whose whole work happens inside the fetch or which have a
/// stack pointer to decrement before the first write.
pub(crate) fn m1_states(op: Kind) -> u8 {
    match op {
        Kind::Mov(mv) if mv.ddd() != Reg::M && mv.sss() != Reg::M => 5,
        Kind::Inr(r) | Kind::Dcr(r) if r != Reg::M => 5,
        Kind::Inherent(Inherent::Sphl)
        | Kind::Inherent(Inherent::Pchl)
        | Kind::Inx(_)
        | Kind::Dcx(_)
        | Kind::Call
        | Kind::Ccond(_)
        | Kind::Rcond(_)
        | Kind::Rst(_)
        | Kind::Push(_) => 5,
        _ => 4,
    }
}

impl I8080 {
    /// The effects of the machine cycle that has just ended.
    pub(crate) fn apply_cycle(&mut self) {
        if self.mstep == 0 {
            self.m1_effects();
            return;
        }
        let step = self.mstep;
        let byte = self.latch;
        match self.dec.kind {
            Kind::Mov(mv) if mv.sss() == Reg::M => self.set_reg(mv.ddd(), byte),
            Kind::Mvi(r) => {
                if r == Reg::M {
                    self.tmp = byte;
                } else {
                    self.set_reg(r, byte);
                }
            }
            Kind::Lxi(p) => {
                if step == 1 {
                    self.set_pair_low(p, byte);
                } else {
                    self.set_pair_high(p, byte);
                }
            }
            Kind::Lda | Kind::Sta | Kind::Lhld | Kind::Shld => match step {
                1 => self.z = byte,
                2 => self.w = byte,
                3 => match self.dec.kind {
                    Kind::Lda => self.a = byte,
                    Kind::Lhld => {
                        self.l = byte;
                        self.set_wz(self.wz().wrapping_add(1));
                    }
                    Kind::Shld => self.set_wz(self.wz().wrapping_add(1)),
                    _ => {}
                },
                _ => {
                    if self.dec.kind == Kind::Lhld {
                        self.h = byte;
                    }
                }
            },
            Kind::Ldax(_) => self.a = byte,
            Kind::Alu(op, Reg::M) | Kind::AluImmediate(op) => {
                self.tmp = byte;
                self.stage_alu(op);
            }
            Kind::Inr(Reg::M) | Kind::Dcr(Reg::M) => {
                if step == 1 {
                    let carry = self.flags & FLAG_CY != 0;
                    let (value, flags) = if matches!(self.dec.kind, Kind::Inr(_)) {
                        alu::inr(byte, carry)
                    } else {
                        alu::dcr(byte, carry)
                    };
                    self.tmp = value;
                    self.flags = flags;
                }
            }
            Kind::Dad(p) => {
                if step == 2 {
                    let (value, flags) = alu::dad(self.hl(), self.pair(p), self.flags);
                    self.set_pair(Pair::Hl, value);
                    self.flags = flags;
                }
            }
            Kind::Jmp | Kind::Jcond(_) => match step {
                1 => self.z = byte,
                _ => {
                    self.w = byte;
                    self.transfer = self.dec.kind == Kind::Jmp || self.taken;
                }
            },
            Kind::Call | Kind::Ccond(_) => match step {
                1 => self.z = byte,
                2 => self.w = byte,
                3 => self.sp = self.sp.wrapping_sub(1),
                _ => self.transfer = true,
            },
            Kind::Inherent(Inherent::Ret) | Kind::Rcond(_) => match step {
                1 => {
                    self.z = byte;
                    self.sp = self.sp.wrapping_add(1);
                }
                _ => {
                    self.w = byte;
                    self.sp = self.sp.wrapping_add(1);
                    self.transfer = true;
                }
            },
            Kind::Rst(_) => match step {
                1 => self.sp = self.sp.wrapping_sub(1),
                _ => self.transfer = true,
            },
            Kind::Push(_) => {
                if step == 1 {
                    self.sp = self.sp.wrapping_sub(1);
                }
            }
            Kind::Pop(p) => match step {
                1 => {
                    if p == Pair::SpPsw {
                        self.flags = alu::fix_constant_bits(byte);
                    } else {
                        self.set_pair_low(p, byte);
                    }
                    self.sp = self.sp.wrapping_add(1);
                }
                _ => {
                    if p == Pair::SpPsw {
                        self.a = byte;
                    } else {
                        self.set_pair_high(p, byte);
                    }
                    self.sp = self.sp.wrapping_add(1);
                }
            },
            Kind::Inherent(Inherent::Xthl) => match step {
                1 => self.z = byte,
                2 => self.w = byte,
                4 => {
                    let wz = self.wz();
                    self.set_pair(Pair::Hl, wz);
                }
                _ => {}
            },
            Kind::In => match step {
                1 => {
                    self.z = byte;
                    self.w = byte;
                }
                _ => self.a = byte,
            },
            Kind::Out => {
                if step == 1 {
                    self.z = byte;
                    self.w = byte;
                }
            }
            Kind::Inherent(Inherent::Hlt) => self.mode = Mode::Halted,
            Kind::Inherent(
                Inherent::Nop
                | Inherent::Xchg
                | Inherent::Daa
                | Inherent::Rlc
                | Inherent::Rrc
                | Inherent::Ral
                | Inherent::Rar
                | Inherent::Cma
                | Inherent::Stc
                | Inherent::Cmc
                | Inherent::Pchl
                | Inherent::Sphl
                | Inherent::Ei
                | Inherent::Di,
            )
            | Kind::Mov(_)
            | Kind::Stax(_)
            | Kind::Alu(..)
            | Kind::Inr(_)
            | Kind::Dcr(_)
            | Kind::Inx(_)
            | Kind::Dcx(_)
            | Kind::Undefined => {}
        }
    }

    /// The `T4` and `T5` work of a fetch: everything an instruction can do
    /// without touching the bus again.
    fn m1_effects(&mut self) {
        match self.dec.kind {
            Kind::Mov(mv) if mv.ddd() != Reg::M && mv.sss() != Reg::M => {
                let value = self.reg(mv.sss());
                self.set_reg(mv.ddd(), value);
            }
            Kind::Inherent(Inherent::Xchg) => {
                let hl = self.hl();
                let de = self.pair(Pair::De);
                self.set_pair(Pair::Hl, de);
                self.set_pair(Pair::De, hl);
            }
            Kind::Inherent(Inherent::Daa) => {
                let (value, flags) = alu::daa(self.a, self.flags);
                self.a = value;
                self.flags = flags;
            }
            Kind::Inherent(Inherent::Rlc) => self.late = Some(alu::rlc(self.a, self.flags)),
            Kind::Inherent(Inherent::Rrc) => self.late = Some(alu::rrc(self.a, self.flags)),
            Kind::Inherent(Inherent::Ral) => self.late = Some(alu::ral(self.a, self.flags)),
            Kind::Inherent(Inherent::Rar) => self.late = Some(alu::rar(self.a, self.flags)),
            Kind::Inherent(Inherent::Cma) => self.a = !self.a,
            Kind::Inherent(Inherent::Stc) => self.flags |= FLAG_CY,
            Kind::Inherent(Inherent::Cmc) => self.flags ^= FLAG_CY,
            Kind::Inr(r) | Kind::Dcr(r) if r != Reg::M => {
                let carry = self.flags & FLAG_CY != 0;
                let (value, flags) = if matches!(self.dec.kind, Kind::Inr(_)) {
                    alu::inr(self.reg(r), carry)
                } else {
                    alu::dcr(self.reg(r), carry)
                };
                self.set_reg(r, value);
                self.flags = flags;
            }
            Kind::Inx(p) => {
                let value = self.pair(p).wrapping_add(1);
                self.set_pair(p, value);
            }
            Kind::Dcx(p) => {
                let value = self.pair(p).wrapping_sub(1);
                self.set_pair(p, value);
            }
            Kind::Inherent(Inherent::Pchl) => self.pc = self.hl(),
            Kind::Inherent(Inherent::Sphl) => self.sp = self.hl(),
            Kind::Inherent(Inherent::Ei) => {
                self.inte = true;
                self.ei_defer = true;
            }
            Kind::Inherent(Inherent::Di) => self.inte = false,
            Kind::Alu(op, r) if r != Reg::M => {
                self.tmp = self.reg(r);
                self.stage_alu(op);
            }
            Kind::Jcond(cond) | Kind::Ccond(cond) | Kind::Rcond(cond) => {
                self.taken = self.condition(cond);
                if self.taken && matches!(self.dec.kind, Kind::Ccond(_)) {
                    self.sp = self.sp.wrapping_sub(1);
                }
            }
            Kind::Call | Kind::Push(_) => self.sp = self.sp.wrapping_sub(1),
            Kind::Rst(restart) => {
                self.set_wz(u16::from(restart.n()) * 8);
                self.sp = self.sp.wrapping_sub(1);
            }
            Kind::Inherent(Inherent::Nop | Inherent::Ret | Inherent::Xthl | Inherent::Hlt)
            | Kind::Mov(_)
            | Kind::Inr(_)
            | Kind::Dcr(_)
            | Kind::Alu(..)
            | Kind::Mvi(_)
            | Kind::Lxi(_)
            | Kind::Lda
            | Kind::Sta
            | Kind::Lhld
            | Kind::Shld
            | Kind::Ldax(_)
            | Kind::Stax(_)
            | Kind::AluImmediate(_)
            | Kind::Dad(_)
            | Kind::Jmp
            | Kind::Pop(_)
            | Kind::In
            | Kind::Out
            | Kind::Undefined => {}
        }
    }

    /// The next machine cycle of the instruction, or `None` when it is over.
    ///
    /// Called once per cycle, after `self.mstep` has counted the one that just
    /// ended, so `mstep == 1` asks for `M2`.
    pub(crate) fn next_cycle(&mut self) -> Option<Cycle> {
        let step = self.mstep;
        match self.dec.kind {
            Kind::Mov(mv) if step == 1 => {
                let (d, s) = (mv.ddd(), mv.sss());
                if s == Reg::M {
                    Some(Cycle::new(CycleKind::MemRead, self.hl(), 0, 3))
                } else if d == Reg::M {
                    Some(Cycle::new(CycleKind::MemWrite, self.hl(), self.reg(s), 3))
                } else {
                    None
                }
            }
            Kind::Mvi(r) => match step {
                1 => Some(self.read_operand()),
                2 if r == Reg::M => Some(Cycle::new(CycleKind::MemWrite, self.hl(), self.tmp, 3)),
                _ => None,
            },
            Kind::Lxi(_) if step <= 2 => Some(self.read_operand()),
            Kind::Lda | Kind::Sta | Kind::Lhld | Kind::Shld => match step {
                1 | 2 => Some(self.read_operand()),
                3 => Some(match self.dec.kind {
                    Kind::Lda | Kind::Lhld => Cycle::new(CycleKind::MemRead, self.wz(), 0, 3),
                    Kind::Sta => Cycle::new(CycleKind::MemWrite, self.wz(), self.a, 3),
                    _ => Cycle::new(CycleKind::MemWrite, self.wz(), self.l, 3),
                }),
                4 => match self.dec.kind {
                    Kind::Lhld => Some(Cycle::new(CycleKind::MemRead, self.wz(), 0, 3)),
                    Kind::Shld => Some(Cycle::new(CycleKind::MemWrite, self.wz(), self.h, 3)),
                    _ => None,
                },
                _ => None,
            },
            Kind::Ldax(p) if step == 1 => {
                Some(Cycle::new(CycleKind::MemRead, self.pair(p.into()), 0, 3))
            }
            Kind::Stax(p) if step == 1 => Some(Cycle::new(
                CycleKind::MemWrite,
                self.pair(p.into()),
                self.a,
                3,
            )),
            Kind::Alu(_, Reg::M) if step == 1 => {
                Some(Cycle::new(CycleKind::MemRead, self.hl(), 0, 3))
            }
            Kind::AluImmediate(_) if step == 1 => Some(self.read_operand()),
            Kind::Inr(Reg::M) | Kind::Dcr(Reg::M) => match step {
                1 => Some(Cycle::new(CycleKind::MemRead, self.hl(), 0, 3)),
                2 => Some(Cycle::new(CycleKind::MemWrite, self.hl(), self.tmp, 3)),
                _ => None,
            },
            Kind::Dad(_) if step <= 2 => Some(Cycle::new(CycleKind::Internal, self.hl(), 0, 3)),
            Kind::Jmp | Kind::Jcond(_) if step <= 2 => Some(self.read_operand()),
            Kind::Call | Kind::Ccond(_) => match step {
                1 | 2 => Some(self.read_operand()),
                3 if self.dec.kind == Kind::Call || self.taken => Some(Cycle::new(
                    CycleKind::StackWrite,
                    self.sp,
                    self.pc.to_be_bytes()[0],
                    3,
                )),
                4 if self.dec.kind == Kind::Call || self.taken => Some(Cycle::new(
                    CycleKind::StackWrite,
                    self.sp,
                    self.pc.to_be_bytes()[1],
                    3,
                )),
                _ => None,
            },
            Kind::Inherent(Inherent::Ret) | Kind::Rcond(_)
                if step <= 2 && (self.dec.kind == Kind::Inherent(Inherent::Ret) || self.taken) =>
            {
                Some(Cycle::new(CycleKind::StackRead, self.sp, 0, 3))
            }
            Kind::Rst(_) => match step {
                1 => Some(Cycle::new(
                    CycleKind::StackWrite,
                    self.sp,
                    self.pc.to_be_bytes()[0],
                    3,
                )),
                2 => Some(Cycle::new(
                    CycleKind::StackWrite,
                    self.sp,
                    self.pc.to_be_bytes()[1],
                    3,
                )),
                _ => None,
            },
            Kind::Push(p) => match step {
                1 => Some(Cycle::new(
                    CycleKind::StackWrite,
                    self.sp,
                    self.push_high(p),
                    3,
                )),
                2 => Some(Cycle::new(
                    CycleKind::StackWrite,
                    self.sp,
                    self.push_low(p),
                    3,
                )),
                _ => None,
            },
            Kind::Pop(_) if step <= 2 => Some(Cycle::new(CycleKind::StackRead, self.sp, 0, 3)),
            Kind::Inherent(Inherent::Xthl) => match step {
                1 => Some(Cycle::new(CycleKind::StackRead, self.sp, 0, 3)),
                2 => Some(Cycle::new(
                    CycleKind::StackRead,
                    self.sp.wrapping_add(1),
                    0,
                    3,
                )),
                3 => Some(Cycle::new(
                    CycleKind::StackWrite,
                    self.sp.wrapping_add(1),
                    self.h,
                    3,
                )),
                4 => Some(Cycle::new(CycleKind::StackWrite, self.sp, self.l, 5)),
                _ => None,
            },
            Kind::In => match step {
                1 => Some(self.read_operand()),
                2 => Some(Cycle::new(CycleKind::Input, self.wz(), 0, 3)),
                _ => None,
            },
            Kind::Out => match step {
                1 => Some(self.read_operand()),
                2 => Some(Cycle::new(CycleKind::Output, self.wz(), self.a, 3)),
                _ => None,
            },
            Kind::Inherent(Inherent::Hlt) if step == 1 => {
                Some(Cycle::new(CycleKind::Halt, self.pc, 0, 2))
            }
            Kind::Inherent(
                Inherent::Nop
                | Inherent::Xchg
                | Inherent::Daa
                | Inherent::Rlc
                | Inherent::Rrc
                | Inherent::Ral
                | Inherent::Rar
                | Inherent::Cma
                | Inherent::Stc
                | Inherent::Cmc
                | Inherent::Ret
                | Inherent::Pchl
                | Inherent::Sphl
                | Inherent::Ei
                | Inherent::Di
                | Inherent::Hlt,
            )
            | Kind::Mov(_)
            | Kind::Lxi(_)
            | Kind::Ldax(_)
            | Kind::Stax(_)
            | Kind::Alu(..)
            | Kind::AluImmediate(_)
            | Kind::Inr(_)
            | Kind::Dcr(_)
            | Kind::Inx(_)
            | Kind::Dcx(_)
            | Kind::Dad(_)
            | Kind::Jmp
            | Kind::Jcond(_)
            | Kind::Rcond(_)
            | Kind::Pop(_)
            | Kind::Undefined => None,
        }
    }

    /// A memory read of the byte the program counter points at, stepping it on.
    fn read_operand(&mut self) -> Cycle {
        let addr = self.pc;
        self.pc = self.pc.wrapping_add(1);
        Cycle::new(CycleKind::MemRead, addr, 0, 3)
    }

    /// The eight accumulator operations, staged for `T2` of the next machine
    /// cycle (`I8080-074`).
    fn stage_alu(&mut self, op: Alu8) {
        let carry = self.flags & FLAG_CY != 0;
        let (a, tmp) = (self.a, self.tmp);
        self.late = Some(match op {
            Alu8::Add => alu::add(a, tmp, false),
            Alu8::Adc => alu::add(a, tmp, carry),
            Alu8::Sub => alu::sub(a, tmp, false),
            Alu8::Sbb => alu::sub(a, tmp, carry),
            Alu8::Ana => alu::and(a, tmp),
            Alu8::Xra => alu::xor(a, tmp),
            Alu8::Ora => alu::or(a, tmp),
            Alu8::Cmp => (a, alu::cmp(a, tmp)),
        });
    }

    /// The byte `PUSH` writes first, at `SP-1` (`I8080-069`). For the processor
    /// status word that is the accumulator (`I8080-047`).
    fn push_high(&self, p: Pair) -> u8 {
        if p == Pair::SpPsw {
            self.a
        } else {
            self.pair(p).to_be_bytes()[0]
        }
    }

    /// The byte `PUSH` writes second, at `SP-2`.
    fn push_low(&self, p: Pair) -> u8 {
        if p == Pair::SpPsw {
            self.flags
        } else {
            self.pair(p).to_be_bytes()[1]
        }
    }

    fn set_pair_low(&mut self, p: Pair, value: u8) {
        let current = self.pair(p);
        self.set_pair(p, (current & 0xff00) | u16::from(value));
    }

    fn set_pair_high(&mut self, p: Pair, value: u8) {
        let current = self.pair(p);
        self.set_pair(p, (current & 0x00ff) | (u16::from(value) << 8));
    }
}
