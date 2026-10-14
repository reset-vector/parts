//! The operation codes of Tables 3 to 6 and the execution times of Table 7
//! (`MC6808-016`, `MC6808-013`).
//!
//! The sheet prints no opcode map; this module is the four tables read row
//! by row. A code no table names is [`Kind::Undefined`], and what the crate
//! does with it is `MC6808-OPEN-01`.
use core::num::NonZeroUsize;

/// An accumulator.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Accumulator {
    /// Accumulator A.
    A,
    /// Accumulator B.
    B,
}

/// An accumulator, or memory, as the target of a unary operation.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Target {
    /// Accumulator A.
    A,
    /// Accumulator B.
    B,
    /// The byte at the effective address.
    Memory,
}

/// A sixteen-bit register a load or store names.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Reg16 {
    /// The index register.
    X,
    /// The stack pointer.
    S,
}

/// Where the operand is (`MC6808-017`): the sequencer's, and not part of a
/// decoded row's public face.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum Addressing {
    /// Accumulator or implied: no operand bytes.
    Inherent,
    /// The operand follows the opcode.
    Immediate,
    /// One address byte, page zero.
    Direct,
    /// One unsigned byte added to `X`.
    Indexed,
    /// Two address bytes, high first.
    Extended,
    /// One signed byte added to the address after it.
    Relative,
}

/// The eight-bit operations of Table 3 that read an operand into an
/// accumulator, or compare against one.
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

/// The one-operand operations of Table 3, on an accumulator or memory.
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

/// The branch tests of Table 5 (`MC6808-023`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Cond {
    /// `BRA`: always.
    Always,
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

/// The one-byte instructions that are not on an accumulator's unary row.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Inherent {
    /// `NOP`: no operation.
    Nop,
    /// `TAP`: `A` to the condition codes.
    Tap,
    /// `TPA`: the condition codes to `A`.
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
    /// `SBA`: `A` minus `B` into `A`.
    Sba,
    /// `CBA`: compare `A` with `B`.
    Cba,
    /// `TAB`: `A` to `B`.
    Tab,
    /// `TBA`: `B` to `A`.
    Tba,
    /// `DAA`: decimal adjust `A`.
    Daa,
    /// `ABA`: `A` plus `B` into `A`.
    Aba,
    /// `TSX`: the stack pointer plus one to `X`.
    Tsx,
    /// `INS`: changes no flag, so it is neither `INX` nor a unary row.
    Ins,
    /// `PULA`, `PULB`.
    Pull(Accumulator),
    /// `DES`: as `INS`.
    Des,
    /// `TXS`: `X` minus one to the stack pointer.
    Txs,
    /// `PSHA`, `PSHB`.
    Push(Accumulator),
    /// `RTS`: return from subroutine.
    Rts,
    /// `RTI`: return from interrupt, every register pulled.
    Rti,
    /// `WAI`: stack every register and wait for an interrupt.
    Wai,
    /// `SWI`: software interrupt.
    Swi,
}

/// The sixteen-bit arithmetic, each on the one register its opcode names.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Alu16 {
    /// `CPX`: compare with `X`.
    CmpX,
}

/// What an operation code does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    /// Table 3's two-operand row into an accumulator.
    Alu8(Alu8, Accumulator),
    /// `STA`: a store of an accumulator.
    St8(Accumulator),
    /// `CPX`.
    Alu16(Alu16),
    /// `LDX`, `LDS`.
    Ld16(Reg16),
    /// `STX`, `STS`.
    St16(Reg16),
    /// A unary operation on an accumulator or on memory.
    Unary(Unary, Target),
    /// `JMP`.
    Jmp,
    /// `JSR`.
    Jsr,
    /// `BSR`.
    Bsr,
    /// A relative branch on the condition.
    Branch(Cond),
    /// An instruction of no operand that is not on a unary row.
    Inherent(Inherent),
    /// A code no table names: a two-cycle no-operation (`MC6808-OPEN-01`).
    Undefined,
}

/// One decoded operation code: its row.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub struct Decoded {
    /// What the operation does.
    pub kind: Kind,
    /// Where its operand is.
    pub(crate) addressing: Addressing,
    /// Bytes the instruction occupies, opcode included.
    pub len: NonZeroUsize,
    /// E cycles the instruction takes, as Table 7 prints them
    /// (`MC6808-013`). For `WAI` the nine that stack; the wait itself is open
    /// ended.
    pub cycles: NonZeroUsize,
}

const fn instr(kind: Kind, addressing: Addressing) -> Decoded {
    Decoded {
        kind,
        addressing,
        len: NonZeroUsize::new(len(kind, addressing))
            .expect("every instruction is at least its opcode"),
        cycles: NonZeroUsize::new(cycles(kind, addressing))
            .expect("every instruction takes at least two cycles"),
    }
}

const UNDEFINED: Decoded = instr(Kind::Undefined, Addressing::Inherent);

/// The `$x0`–`$xF` column of the unary rows `$4x`–`$7x`.
const fn unary(low: u8) -> Option<Unary> {
    Some(match low {
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
        _ => return None,
    })
}

/// The `$x0`–`$xB` columns of the rows `$8x`–`$Fx`.
const fn alu8(low: u8) -> Option<Alu8> {
    Some(match low {
        0x0 => Alu8::Sub,
        0x1 => Alu8::Cmp,
        0x2 => Alu8::Sbc,
        0x4 => Alu8::And,
        0x5 => Alu8::Bit,
        0x6 => Alu8::Ld,
        0x8 => Alu8::Eor,
        0x9 => Alu8::Adc,
        0xa => Alu8::Or,
        0xb => Alu8::Add,
        _ => return None,
    })
}

/// The instruction at `op`.
#[must_use]
pub const fn decode(op: u8) -> Decoded {
    use Addressing::{Direct, Extended, Immediate, Indexed, Relative};
    let high = op >> 4;
    let low = op & 0x0f;
    match high {
        0x0 | 0x1 | 0x3 => {
            let i = match op {
                0x01 => Inherent::Nop,
                0x06 => Inherent::Tap,
                0x07 => Inherent::Tpa,
                0x08 => Inherent::Inx,
                0x09 => Inherent::Dex,
                0x0a => Inherent::Clv,
                0x0b => Inherent::Sev,
                0x0c => Inherent::Clc,
                0x0d => Inherent::Sec,
                0x0e => Inherent::Cli,
                0x0f => Inherent::Sei,
                0x10 => Inherent::Sba,
                0x11 => Inherent::Cba,
                0x16 => Inherent::Tab,
                0x17 => Inherent::Tba,
                0x19 => Inherent::Daa,
                0x1b => Inherent::Aba,
                0x30 => Inherent::Tsx,
                0x31 => Inherent::Ins,
                0x32 => Inherent::Pull(Accumulator::A),
                0x33 => Inherent::Pull(Accumulator::B),
                0x34 => Inherent::Des,
                0x35 => Inherent::Txs,
                0x36 => Inherent::Push(Accumulator::A),
                0x37 => Inherent::Push(Accumulator::B),
                0x39 => Inherent::Rts,
                0x3b => Inherent::Rti,
                0x3e => Inherent::Wai,
                0x3f => Inherent::Swi,
                _ => return UNDEFINED,
            };
            instr(Kind::Inherent(i), Addressing::Inherent)
        }
        0x2 => {
            let c = match low {
                0x0 => Cond::Always,
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
                0xf => Cond::Le,
                _ => return UNDEFINED,
            };
            instr(Kind::Branch(c), Relative)
        }
        0x4..=0x7 => {
            if op == 0x6e || op == 0x7e {
                return instr(Kind::Jmp, if op == 0x6e { Indexed } else { Extended });
            }
            let Some(u) = unary(low) else {
                return UNDEFINED;
            };
            match high {
                0x4 => instr(Kind::Unary(u, Target::A), Addressing::Inherent),
                0x5 => instr(Kind::Unary(u, Target::B), Addressing::Inherent),
                0x6 => instr(Kind::Unary(u, Target::Memory), Indexed),
                _ => instr(Kind::Unary(u, Target::Memory), Extended),
            }
        }
        _ => {
            let acc = if high < 0xc {
                Accumulator::A
            } else {
                Accumulator::B
            };
            let reg = if high < 0xc { Reg16::S } else { Reg16::X };
            let addressing = match high & 0x3 {
                0 => Immediate,
                1 => Direct,
                2 => Indexed,
                _ => Extended,
            };
            if let Some(a) = alu8(low) {
                return instr(Kind::Alu8(a, acc), addressing);
            }
            let immediate = matches!(addressing, Immediate);
            let kind = match low {
                0x7 if !immediate => Kind::St8(acc),
                0xc if high < 0xc => Kind::Alu16(Alu16::CmpX),
                0xd if op == 0x8d => return instr(Kind::Bsr, Relative),
                0xd if high == 0xa || high == 0xb => Kind::Jsr,
                0xe => Kind::Ld16(reg),
                0xf if !immediate => Kind::St16(reg),
                _ => return UNDEFINED,
            };
            instr(kind, addressing)
        }
    }
}

/// Bytes the instruction occupies, opcode included.
const fn len(kind: Kind, addressing: Addressing) -> usize {
    match addressing {
        Addressing::Inherent => 1,
        Addressing::Direct | Addressing::Indexed | Addressing::Relative => 2,
        Addressing::Extended => 3,
        Addressing::Immediate => match kind {
            Kind::Alu16(..) | Kind::Ld16(_) => 3,
            _ => 2,
        },
    }
}

/// Table 7's E cycles for a row (`MC6808-013`).
const fn cycles(kind: Kind, addressing: Addressing) -> usize {
    use Addressing::{Direct, Extended, Immediate, Indexed, Relative};
    match (kind, addressing) {
        (Kind::Alu8(..), Immediate) => 2,
        (Kind::Alu8(..), Direct) => 3,
        (Kind::Alu8(..), Indexed) => 5,
        (Kind::Alu8(..), Extended) => 4,
        (Kind::St8(_), Direct) => 4,
        (Kind::St8(_), Indexed) => 6,
        (Kind::St8(_), Extended) => 5,
        (Kind::Alu16(..) | Kind::Ld16(_), Immediate) => 3,
        (Kind::Alu16(..) | Kind::Ld16(_), Direct) => 4,
        (Kind::Alu16(..) | Kind::Ld16(_), Indexed) => 6,
        (Kind::Alu16(..) | Kind::Ld16(_), Extended) => 5,
        (Kind::St16(_), Direct) => 5,
        (Kind::St16(_), Indexed) => 7,
        (Kind::St16(_), Extended) => 6,
        (Kind::Unary(..), Addressing::Inherent) => 2,
        (Kind::Unary(..), Indexed) => 7,
        (Kind::Unary(..), Extended) => 6,
        (Kind::Jmp, Indexed) => 4,
        (Kind::Jmp, Extended) => 3,
        (Kind::Jsr, Indexed) => 8,
        (Kind::Jsr, Extended) => 9,
        (Kind::Bsr, _) => 8,
        (Kind::Branch(_), Relative) => 4,
        (Kind::Inherent(h), _) => match h {
            Inherent::Inx
            | Inherent::Dex
            | Inherent::Ins
            | Inherent::Des
            | Inherent::Tsx
            | Inherent::Txs
            | Inherent::Push(_)
            | Inherent::Pull(_) => 4,
            Inherent::Rts => 5,
            Inherent::Wai => 9,
            Inherent::Rti => 10,
            Inherent::Swi => 12,
            _ => 2,
        },
        _ => 2,
    }
}
