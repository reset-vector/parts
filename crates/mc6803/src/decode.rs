//! The operation code map of `MC6803-014`, one cell at a time.
//!
//! Appendix B of the reference manual lays the 256 codes out as a 16 × 16
//! grid whose columns are the addressing modes and whose rows are the
//! operations; this module is that grid as a function. What each cell costs
//! in E cycles is Appendix H's and Table F-1's (`MC6803-016`), and it is
//! carried here beside the cell so that the sequencer never has to derive it.
use core::num::NonZeroUsize;

/// Which of the two accumulators an instruction names.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Accumulator {
    /// `ACCA`.
    A,
    /// `ACCB`.
    B,
}

/// A register `PSH` and `PUL` move to or from the stack.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Stacked {
    /// `ACCA`.
    A,
    /// `ACCB`.
    B,
    /// The index register, low byte pushed first.
    X,
}

/// A sixteen-bit register a load or store names.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Reg16 {
    /// The double accumulator, `ACCA:ACCB`.
    D,
    /// The index register.
    X,
    /// The stack pointer.
    S,
}

/// The addressing mode of a cell (`MC6803-015`): the sequencer's, and not
/// part of a decoded row's public face.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum Addressing {
    /// The operand is the register the code names, or none.
    Inherent,
    /// The operand follows the opcode.
    Immediate,
    /// One byte of address on page zero follows.
    Direct,
    /// Two bytes of address follow, high first.
    Extended,
    /// One unsigned byte follows and is added to `X`.
    Indexed,
    /// One signed byte follows and is added to the address after it.
    Relative,
}

/// The eight-bit accumulator-and-memory operations that read an operand.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Alu8 {
    /// `SUB`.
    Sub,
    /// `CMP`.
    Cmp,
    /// `SBC`.
    Sbc,
    /// `AND`.
    And,
    /// `BIT`.
    Bit,
    /// `LDA`: load.
    Ld,
    /// `EOR`.
    Eor,
    /// `ADC`.
    Adc,
    /// `ORA`: inclusive OR.
    Or,
    /// `ADD`.
    Add,
}

/// The sixteen-bit arithmetic, each on the one register its opcode names.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Alu16 {
    /// `SUBD`: subtract from `D`.
    SubD,
    /// `ADDD`: add to `D`.
    AddD,
    /// `CPX`: compare with `X`.
    CmpX,
}

/// An accumulator, or memory, as the target of a one-operand operation.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Target {
    /// `ACCA`.
    A,
    /// `ACCB`.
    B,
    /// The byte at the effective address.
    Memory,
}

/// The one-operand read-modify-write operations, on an accumulator or a
/// memory byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Unary {
    /// `NEG`: two's complement negate.
    Neg,
    /// `COM`: one's complement.
    Com,
    /// `LSR`: logical shift right, `0` in at bit 7.
    Lsr,
    /// `ROR`: rotate right through `C`.
    Ror,
    /// `ASR`: arithmetic shift right, bit 7 kept.
    Asr,
    /// `ASL`: arithmetic shift left.
    Asl,
    /// `ROL`: rotate left through `C`.
    Rol,
    /// `DEC`: decrement.
    Dec,
    /// `INC`: increment.
    Inc,
    /// `TST`: test, setting `N` and `Z` and clearing `V` and `C`.
    Tst,
    /// `CLR`: clear.
    Clr,
}

/// The conditions a relative branch tests (`MC6803-026`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Cond {
    /// `BRA`: always.
    Always,
    /// `BRN`: never.
    Never,
    /// `BHI`: higher, `C` and `Z` clear.
    Hi,
    /// `BLS`: lower or same, `C` or `Z` set.
    Ls,
    /// `BCC`: carry clear.
    Cc,
    /// `BCS`: carry set.
    Cs,
    /// `BNE`: not equal, `Z` clear.
    Ne,
    /// `BEQ`: equal, `Z` set.
    Eq,
    /// `BVC`: overflow clear.
    Vc,
    /// `BVS`: overflow set.
    Vs,
    /// `BPL`: plus, `N` clear.
    Pl,
    /// `BMI`: minus, `N` set.
    Mi,
    /// `BGE`: greater or equal, `N` equals `V`.
    Ge,
    /// `BLT`: less than, `N` differs from `V`.
    Lt,
    /// `BGT`: greater than, `Z` clear and `N` equals `V`.
    Gt,
    /// `BLE`: less or equal, `Z` set or `N` differs from `V`.
    Le,
}

/// The inherent instructions that name no memory.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Inherent {
    /// `NOP`: no operation.
    Nop,
    /// `LSRD`: a shift of the double accumulator, which no
    /// [`Kind::Unary`] row's target names.
    Lsrd,
    /// `ASLD`: as `LSRD`.
    Asld,
    /// `TAP`: `ACCA` to the condition codes.
    Tap,
    /// `TPA`: the condition codes to `ACCA`.
    Tpa,
    /// `INX`: not [`Kind::Unary`]'s increment, which sets `N`, `Z` and `V`;
    /// this sets `Z` only.
    Inx,
    /// `DEX`: as `INX`.
    Dex,
    /// `CLV`: clear `V`.
    Clv,
    /// `SEV`: set `V`.
    Sev,
    /// `CLC`: clear `C`.
    Clc,
    /// `SEC`: set `C`.
    Sec,
    /// `CLI`: clear `I`, enabling `IRQ`.
    Cli,
    /// `SEI`: set `I`, masking `IRQ`.
    Sei,
    /// `SBA`: `ACCA` minus `ACCB` into `ACCA`.
    Sba,
    /// `CBA`: compare `ACCA` with `ACCB`.
    Cba,
    /// `TAB`: `ACCA` to `ACCB`.
    Tab,
    /// `TBA`: `ACCB` to `ACCA`.
    Tba,
    /// `DAA`: decimal adjust `ACCA`.
    Daa,
    /// `ABA`: `ACCA` plus `ACCB` into `ACCA`.
    Aba,
    /// `TSX`: the stack pointer plus one to `X`.
    Tsx,
    /// `INS`: changes no flag, so it is neither `INX` nor a unary row.
    Ins,
    /// `PULA`, `PULB` and `PULX`.
    Pull(Stacked),
    /// `DES`: as `INS`.
    Des,
    /// `TXS`: `X` minus one to the stack pointer.
    Txs,
    /// `PSHA`, `PSHB` and `PSHX`.
    Push(Stacked),
    /// `RTS`: return from subroutine.
    Rts,
    /// `ABX`: `ACCB`, unsigned, added to `X`.
    Abx,
    /// `RTI`: return from interrupt, every register pulled.
    Rti,
    /// `MUL`: `ACCA` times `ACCB` into `D`.
    Mul,
    /// `WAI`: stack every register and wait for an interrupt.
    Wai,
    /// `SWI`: software interrupt.
    Swi,
}

/// What a code does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    /// An eight-bit operation reading memory into an accumulator's flags or
    /// value.
    Alu8(Alu8, Accumulator),
    /// A store of an accumulator.
    St8(Accumulator),
    /// A sixteen-bit load.
    Ld16(Reg16),
    /// A sixteen-bit store.
    St16(Reg16),
    /// `SUBD`, `ADDD`, `CPX`.
    Alu16(Alu16),
    /// A read-modify-write on an accumulator or a memory byte.
    Unary(Unary, Target),
    /// `JMP`.
    Jmp,
    /// `JSR`.
    Jsr,
    /// `BSR`.
    Bsr,
    /// A conditional or unconditional relative branch.
    Branch(Cond),
    /// An inherent instruction.
    Inherent(Inherent),
    /// A code the map leaves empty: a two-cycle no-operation
    /// (`MC6803-OPEN-02`).
    Undefined,
}

/// One cell of the map: a decoded operation code.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub struct Decoded {
    /// What it does.
    pub kind: Kind,
    /// How it finds its operand.
    pub(crate) addressing: Addressing,
    /// Bytes the cell occupies, opcode included.
    pub len: NonZeroUsize,
    /// E cycles from opcode fetch to the cycle before the next opcode fetch
    /// (`MC6803-016`). For `WAI` the nine that stack; the wait itself is open
    /// ended.
    pub cycles: NonZeroUsize,
}

const fn instr(kind: Kind, addressing: Addressing, cycles: usize) -> Decoded {
    Decoded {
        kind,
        addressing,
        len: NonZeroUsize::new(len(kind, addressing)).expect("every cell is at least its opcode"),
        cycles: NonZeroUsize::new(cycles).expect("every cell takes at least two cycles"),
    }
}

const UNDEFINED: Decoded = instr(Kind::Undefined, Addressing::Inherent, 2);

/// Decode one opcode.
#[must_use]
pub const fn decode(op: u8) -> Decoded {
    use Addressing::{Direct, Extended, Immediate, Indexed, Relative};
    let hi = op >> 4;
    let lo = op & 0x0f;
    match hi {
        0x0 => {
            let inh = match lo {
                0x1 => Inherent::Nop,
                0x4 => Inherent::Lsrd,
                0x5 => Inherent::Asld,
                0x6 => Inherent::Tap,
                0x7 => Inherent::Tpa,
                0x8 => Inherent::Inx,
                0x9 => Inherent::Dex,
                0xa => Inherent::Clv,
                0xb => Inherent::Sev,
                0xc => Inherent::Clc,
                0xd => Inherent::Sec,
                0xe => Inherent::Cli,
                0xf => Inherent::Sei,
                _ => return UNDEFINED,
            };
            instr(
                Kind::Inherent(inh),
                Addressing::Inherent,
                inherent_cycles(inh),
            )
        }
        0x1 => {
            let inh = match lo {
                0x0 => Inherent::Sba,
                0x1 => Inherent::Cba,
                0x6 => Inherent::Tab,
                0x7 => Inherent::Tba,
                0x9 => Inherent::Daa,
                0xb => Inherent::Aba,
                _ => return UNDEFINED,
            };
            instr(
                Kind::Inherent(inh),
                Addressing::Inherent,
                inherent_cycles(inh),
            )
        }
        0x2 => {
            let cond = match lo {
                0x0 => Cond::Always,
                0x1 => Cond::Never,
                0x2 => Cond::Hi,
                0x3 => Cond::Ls,
                0x4 => Cond::Cc,
                0x5 => Cond::Cs,
                0x6 => Cond::Ne,
                0x7 => Cond::Eq,
                0x8 => Cond::Vc,
                0x9 => Cond::Vs,
                0xa => Cond::Pl,
                0xb => Cond::Mi,
                0xc => Cond::Ge,
                0xd => Cond::Lt,
                0xe => Cond::Gt,
                _ => Cond::Le,
            };
            instr(Kind::Branch(cond), Relative, 3)
        }
        0x3 => {
            let inh = match lo {
                0x0 => Inherent::Tsx,
                0x1 => Inherent::Ins,
                0x2 => Inherent::Pull(Stacked::A),
                0x3 => Inherent::Pull(Stacked::B),
                0x4 => Inherent::Des,
                0x5 => Inherent::Txs,
                0x6 => Inherent::Push(Stacked::A),
                0x7 => Inherent::Push(Stacked::B),
                0x8 => Inherent::Pull(Stacked::X),
                0x9 => Inherent::Rts,
                0xa => Inherent::Abx,
                0xb => Inherent::Rti,
                0xc => Inherent::Push(Stacked::X),
                0xd => Inherent::Mul,
                0xe => Inherent::Wai,
                0xf => Inherent::Swi,
                _ => return UNDEFINED,
            };
            instr(
                Kind::Inherent(inh),
                Addressing::Inherent,
                inherent_cycles(inh),
            )
        }
        0x4..=0x7 => {
            if lo == 0xe {
                return match hi {
                    0x6 => instr(Kind::Jmp, Indexed, 3),
                    0x7 => instr(Kind::Jmp, Extended, 3),
                    _ => UNDEFINED,
                };
            }
            let rmw = match lo {
                0x0 => Unary::Neg,
                0x3 => Unary::Com,
                0x4 => Unary::Lsr,
                0x6 => Unary::Ror,
                0x7 => Unary::Asr,
                0x8 => Unary::Asl,
                0x9 => Unary::Rol,
                0xa => Unary::Dec,
                0xc => Unary::Inc,
                0xd => Unary::Tst,
                0xf => Unary::Clr,
                _ => return UNDEFINED,
            };
            match hi {
                0x4 => instr(Kind::Unary(rmw, Target::A), Addressing::Inherent, 2),
                0x5 => instr(Kind::Unary(rmw, Target::B), Addressing::Inherent, 2),
                0x6 => instr(Kind::Unary(rmw, Target::Memory), Indexed, 6),
                _ => instr(Kind::Unary(rmw, Target::Memory), Extended, 6),
            }
        }
        _ => {
            let acc = if hi < 0xc {
                Accumulator::A
            } else {
                Accumulator::B
            };
            let addressing = match hi & 0x3 {
                0x0 => Immediate,
                0x1 => Direct,
                0x2 => Indexed,
                _ => Extended,
            };
            let c8 = match addressing {
                Immediate => 2,
                Direct => 3,
                _ => 4,
            };
            let c16 = match addressing {
                Immediate => 3,
                Direct => 4,
                _ => 5,
            };
            let c16a = match addressing {
                Immediate => 4,
                Direct => 5,
                _ => 6,
            };
            match lo {
                0x0 => instr(Kind::Alu8(Alu8::Sub, acc), addressing, c8),
                0x1 => instr(Kind::Alu8(Alu8::Cmp, acc), addressing, c8),
                0x2 => instr(Kind::Alu8(Alu8::Sbc, acc), addressing, c8),
                0x3 => {
                    let op = if hi < 0xc { Alu16::SubD } else { Alu16::AddD };
                    instr(Kind::Alu16(op), addressing, c16a)
                }
                0x4 => instr(Kind::Alu8(Alu8::And, acc), addressing, c8),
                0x5 => instr(Kind::Alu8(Alu8::Bit, acc), addressing, c8),
                0x6 => instr(Kind::Alu8(Alu8::Ld, acc), addressing, c8),
                0x7 => match addressing {
                    Immediate => UNDEFINED,
                    _ => instr(Kind::St8(acc), addressing, c8),
                },
                0x8 => instr(Kind::Alu8(Alu8::Eor, acc), addressing, c8),
                0x9 => instr(Kind::Alu8(Alu8::Adc, acc), addressing, c8),
                0xa => instr(Kind::Alu8(Alu8::Or, acc), addressing, c8),
                0xb => instr(Kind::Alu8(Alu8::Add, acc), addressing, c8),
                0xc => {
                    if hi < 0xc {
                        instr(Kind::Alu16(Alu16::CmpX), addressing, c16a)
                    } else {
                        instr(Kind::Ld16(Reg16::D), addressing, c16)
                    }
                }
                0xd => {
                    if hi < 0xc {
                        match addressing {
                            Immediate => instr(Kind::Bsr, Relative, 6),
                            Direct => instr(Kind::Jsr, addressing, 5),
                            _ => instr(Kind::Jsr, addressing, 6),
                        }
                    } else {
                        match addressing {
                            Immediate => UNDEFINED,
                            _ => instr(Kind::St16(Reg16::D), addressing, c16),
                        }
                    }
                }
                0xe => {
                    let reg = if hi < 0xc { Reg16::S } else { Reg16::X };
                    instr(Kind::Ld16(reg), addressing, c16)
                }
                _ => {
                    let reg = if hi < 0xc { Reg16::S } else { Reg16::X };
                    match addressing {
                        Immediate => UNDEFINED,
                        _ => instr(Kind::St16(reg), addressing, c16),
                    }
                }
            }
        }
    }
}

/// Cycle counts of the inherent instructions, Table F-1's `INHERENT` sheets.
const fn inherent_cycles(inh: Inherent) -> usize {
    match inh {
        Inherent::Abx
        | Inherent::Asld
        | Inherent::Lsrd
        | Inherent::Des
        | Inherent::Ins
        | Inherent::Inx
        | Inherent::Dex => 3,
        Inherent::Push(Stacked::A | Stacked::B) | Inherent::Tsx | Inherent::Txs => 3,
        Inherent::Pull(Stacked::A | Stacked::B) | Inherent::Push(Stacked::X) => 4,
        Inherent::Pull(Stacked::X) | Inherent::Rts => 5,
        Inherent::Wai => 9,
        Inherent::Mul | Inherent::Rti => 10,
        Inherent::Swi => 12,
        _ => 2,
    }
}

/// The count of bytes a cell occupies, from its mode.
const fn len(kind: Kind, addressing: Addressing) -> usize {
    match addressing {
        Addressing::Inherent => 1,
        Addressing::Immediate => match kind {
            Kind::Ld16(_) | Kind::Alu16(..) => 3,
            _ => 2,
        },
        Addressing::Direct | Addressing::Indexed | Addressing::Relative => 2,
        Addressing::Extended => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_columns_take_their_addressing_modes() {
        assert_eq!(decode(0x60).addressing, Addressing::Indexed);
        assert_eq!(decode(0x7f).addressing, Addressing::Extended);
        assert_eq!(decode(0x86).addressing, Addressing::Immediate);
        assert_eq!(decode(0x96).addressing, Addressing::Direct);
        assert_eq!(decode(0xa6).addressing, Addressing::Indexed);
        assert_eq!(decode(0xb6).addressing, Addressing::Extended);
        assert_eq!(decode(0x9d).addressing, Addressing::Direct);
    }
}
