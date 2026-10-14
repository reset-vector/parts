//! The operation codes of Table 9 and Figure 18 (`MC6809E-013`), read row by
//! row into what each code does and where its operand is.
//!
//! A code Table 9 marks unused is [`Kind::Undefined`] from [`decode`]; what
//! the crate does with one is `MC6809E-OPEN-01`.

/// An accumulator, or memory, as the target of a one-operand operation.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Target {
    /// Accumulator A.
    A,
    /// Accumulator B.
    B,
    /// The byte at the effective address.
    Memory,
}

/// Which of the three software interrupts.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Swi {
    /// `SWI`, which also sets `F` and `I`.
    Swi,
    /// `SWI2`.
    Swi2,
    /// `SWI3`.
    Swi3,
}

/// An eight-bit accumulator.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Accumulator {
    /// Accumulator A.
    A,
    /// Accumulator B.
    B,
}

/// A sixteen-bit register a load, store, compare or `LEA` names.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Reg16 {
    /// The double accumulator, `A:B`.
    D,
    /// Index register `X`.
    X,
    /// Index register `Y`.
    Y,
    /// The user stack pointer.
    U,
    /// The hardware stack pointer.
    S,
}

/// The register a `LEA` loads: the four an indexed address can be formed
/// from.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum LeaRegister {
    /// `LEAX`.
    X,
    /// `LEAY`.
    Y,
    /// `LEAU`.
    U,
    /// `LEAS`.
    S,
}

impl From<LeaRegister> for Reg16 {
    fn from(reg: LeaRegister) -> Self {
        match reg {
            LeaRegister::X => Self::X,
            LeaRegister::Y => Self::Y,
            LeaRegister::U => Self::U,
            LeaRegister::S => Self::S,
        }
    }
}

/// One of the two stacks (`MC6809E-022`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Stack {
    /// The hardware stack.
    S,
    /// The user stack.
    U,
}

/// Where the operand is (`MC6809E-021`): the sequencer's, and not part of a
/// decoded row's public face.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum Addressing {
    /// Accumulator or implied.
    Inherent,
    /// The bytes after the opcode; a postbyte, for the register forms.
    Immediate,
    /// One address byte, the high byte from `DP`.
    Direct,
    /// A postbyte, then what it names.
    Indexed,
    /// Two address bytes, high first.
    Extended,
    /// One signed offset byte.
    Relative,
    /// Two offset bytes, high first.
    LongRelative,
}

/// The one-operand operations (Figure 18).
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
    /// `TST`: test, setting `N` and `Z` and clearing `V`.
    Tst,
    /// `CLR`: clear.
    Clr,
}

/// The eight-bit operations that read an operand into an accumulator, or
/// compare against one.
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
    /// `LD`: load.
    Ld,
    /// `EOR`.
    Eor,
    /// `ADC`.
    Adc,
    /// `OR`: inclusive OR.
    Or,
    /// `ADD`.
    Add,
}

/// The sixteen-bit operations that read an operand and keep or compare it,
/// each on the one register its opcode names.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Alu16 {
    /// `SUBD`: subtract from `D`.
    SubD,
    /// `ADDD`: add to `D`.
    AddD,
    /// `CMPD`: compare with `D`.
    CmpD,
    /// `CMPX`: compare with `X`.
    CmpX,
    /// `CMPY`: compare with `Y`.
    CmpY,
    /// `CMPU`: compare with `U`.
    CmpU,
    /// `CMPS`: compare with `S`.
    CmpS,
}

impl Alu16 {
    /// The register the operation reads.
    pub(crate) const fn register(self) -> Reg16 {
        match self {
            Self::SubD | Self::AddD | Self::CmpD => Reg16::D,
            Self::CmpX => Reg16::X,
            Self::CmpY => Reg16::Y,
            Self::CmpU => Reg16::U,
            Self::CmpS => Reg16::S,
        }
    }
}

/// The sixteen branch tests (`MC6809E-031`), in opcode order from `$20`.
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

const CONDS: [Cond; 16] = [
    Cond::Always,
    Cond::Never,
    Cond::Hi,
    Cond::Ls,
    Cond::Cc,
    Cond::Cs,
    Cond::Ne,
    Cond::Eq,
    Cond::Vc,
    Cond::Vs,
    Cond::Pl,
    Cond::Mi,
    Cond::Ge,
    Cond::Lt,
    Cond::Gt,
    Cond::Le,
];

/// The inherent instructions that name no memory and take no postbyte.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Inherent {
    /// `NOP`.
    Nop,
    /// `SYNC`.
    Sync,
    /// `DAA`.
    Daa,
    /// `SEX`.
    Sex,
    /// `RTS`.
    Rts,
    /// `ABX`.
    Abx,
    /// `RTI`.
    Rti,
    /// `MUL`.
    Mul,
    /// `SWI`, `SWI2`, `SWI3`.
    Swi(Swi),
}

/// Which page of Table 9 a code is on: the one-byte codes, or those behind
/// the `$10` or `$11` prefix.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Page {
    /// The one-byte codes.
    One,
    /// The codes behind `$10`.
    Two,
    /// The codes behind `$11`.
    Three,
}

/// What an operation code does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    /// A one-operand operation on an accumulator or on memory.
    Unary(Unary, Target),
    /// An eight-bit operation reading an operand into an accumulator.
    Alu8(Alu8, Accumulator),
    /// `STA`, `STB`: a store of an accumulator.
    St8(Accumulator),
    /// `SUBD`, `ADDD`, `CMPD`, `CMPX`, `CMPY`, `CMPU`, `CMPS`.
    Alu16(Alu16),
    /// `LDD`, `LDX`, `LDY`, `LDU`, `LDS`.
    Ld16(Reg16),
    /// `STD`, `STX`, `STY`, `STU`, `STS`.
    St16(Reg16),
    /// `LEAX`, `LEAY`, `LEAU`, `LEAS`.
    Lea(LeaRegister),
    /// `JMP`.
    Jmp,
    /// `JSR`.
    Jsr,
    /// `BSR`, `LBSR`.
    Bsr,
    /// A relative branch on the condition, short or long.
    Branch(Cond),
    /// `PSHS`, `PSHU`.
    Push(Stack),
    /// `PULS`, `PULU`.
    Pull(Stack),
    /// `TFR`.
    Tfr,
    /// `EXG`.
    Exg,
    /// `ORCC`.
    Orcc,
    /// `ANDCC`.
    Andcc,
    /// `CWAI`.
    Cwai,
    /// An instruction of no operand that is not on a unary row.
    Inherent(Inherent),
    /// No entry in Table 9. The part runs it as `NOP` with one read at `PC`
    /// (`MC6809E-OPEN-01`).
    Undefined,
}

/// One decoded operation code: a row of Table 9.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub struct Decoded {
    /// What the operation does.
    pub kind: Kind,
    /// Where its operand is.
    pub(crate) addressing: Addressing,
}

const UNDEFINED: Decoded = Decoded {
    kind: Kind::Undefined,
    addressing: Addressing::Inherent,
};

const fn op(kind: Kind, addressing: Addressing) -> Option<Decoded> {
    Some(Decoded { kind, addressing })
}

/// The unary operation in column `lo` of rows `$0x`, `$4x`–`$7x`.
const fn unary(lo: u8) -> Option<Unary> {
    Some(match lo {
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

/// The mode of rows `$8x`–`$Fx` by their high nibble's low two bits.
const fn column_mode(hi: u8) -> Addressing {
    match hi & 3 {
        0 => Addressing::Immediate,
        1 => Addressing::Direct,
        2 => Addressing::Indexed,
        _ => Addressing::Extended,
    }
}

/// The operation at `code` on `page`. [`Kind::Undefined`] where Table 9 has
/// no entry. A prefix is not an operation: `decode(Page::One, 0x10)` is
/// [`Kind::Undefined`].
#[must_use]
pub const fn decode(page: Page, code: u8) -> Decoded {
    let row = match page {
        Page::One => page1(code),
        Page::Two => page2(code),
        Page::Three => page3(code),
    };
    match row {
        Some(d) => d,
        None => UNDEFINED,
    }
}

const fn page1(code: u8) -> Option<Decoded> {
    let hi = code >> 4;
    let lo = code & 0xf;
    match hi {
        0x0 | 0x6 | 0x7 => {
            let addressing = match hi {
                0x0 => Addressing::Direct,
                0x6 => Addressing::Indexed,
                _ => Addressing::Extended,
            };
            if lo == 0xe {
                return op(Kind::Jmp, addressing);
            }
            match unary(lo) {
                Some(u) => op(Kind::Unary(u, Target::Memory), addressing),
                None => None,
            }
        }
        0x4 | 0x5 => {
            let t = if hi == 4 { Target::A } else { Target::B };
            match unary(lo) {
                Some(u) => op(Kind::Unary(u, t), Addressing::Inherent),
                None => None,
            }
        }
        0x1 => match lo {
            0x2 => op(Kind::Inherent(Inherent::Nop), Addressing::Inherent),
            0x3 => op(Kind::Inherent(Inherent::Sync), Addressing::Inherent),
            0x6 => op(Kind::Branch(Cond::Always), Addressing::LongRelative),
            0x7 => op(Kind::Bsr, Addressing::LongRelative),
            0x9 => op(Kind::Inherent(Inherent::Daa), Addressing::Inherent),
            0xa => op(Kind::Orcc, Addressing::Immediate),
            0xc => op(Kind::Andcc, Addressing::Immediate),
            0xd => op(Kind::Inherent(Inherent::Sex), Addressing::Inherent),
            0xe => op(Kind::Exg, Addressing::Immediate),
            0xf => op(Kind::Tfr, Addressing::Immediate),
            _ => None,
        },
        0x2 => op(Kind::Branch(CONDS[lo as usize]), Addressing::Relative),
        0x3 => match lo {
            0x0 => op(Kind::Lea(LeaRegister::X), Addressing::Indexed),
            0x1 => op(Kind::Lea(LeaRegister::Y), Addressing::Indexed),
            0x2 => op(Kind::Lea(LeaRegister::S), Addressing::Indexed),
            0x3 => op(Kind::Lea(LeaRegister::U), Addressing::Indexed),
            0x4 => op(Kind::Push(Stack::S), Addressing::Immediate),
            0x5 => op(Kind::Pull(Stack::S), Addressing::Immediate),
            0x6 => op(Kind::Push(Stack::U), Addressing::Immediate),
            0x7 => op(Kind::Pull(Stack::U), Addressing::Immediate),
            0x9 => op(Kind::Inherent(Inherent::Rts), Addressing::Inherent),
            0xa => op(Kind::Inherent(Inherent::Abx), Addressing::Inherent),
            0xb => op(Kind::Inherent(Inherent::Rti), Addressing::Inherent),
            0xc => op(Kind::Cwai, Addressing::Immediate),
            0xd => op(Kind::Inherent(Inherent::Mul), Addressing::Inherent),
            0xf => op(
                Kind::Inherent(Inherent::Swi(Swi::Swi)),
                Addressing::Inherent,
            ),
            _ => None,
        },
        _ => {
            let addressing = column_mode(hi);
            let imm = matches!(addressing, Addressing::Immediate);
            let acc = if hi < 0xc {
                Accumulator::A
            } else {
                Accumulator::B
            };
            match lo {
                0x0 => op(Kind::Alu8(Alu8::Sub, acc), addressing),
                0x1 => op(Kind::Alu8(Alu8::Cmp, acc), addressing),
                0x2 => op(Kind::Alu8(Alu8::Sbc, acc), addressing),
                0x3 => {
                    let k = if hi < 0xc { Alu16::SubD } else { Alu16::AddD };
                    op(Kind::Alu16(k), addressing)
                }
                0x4 => op(Kind::Alu8(Alu8::And, acc), addressing),
                0x5 => op(Kind::Alu8(Alu8::Bit, acc), addressing),
                0x6 => op(Kind::Alu8(Alu8::Ld, acc), addressing),
                0x7 if !imm => op(Kind::St8(acc), addressing),
                0x8 => op(Kind::Alu8(Alu8::Eor, acc), addressing),
                0x9 => op(Kind::Alu8(Alu8::Adc, acc), addressing),
                0xa => op(Kind::Alu8(Alu8::Or, acc), addressing),
                0xb => op(Kind::Alu8(Alu8::Add, acc), addressing),
                0xc if hi < 0xc => op(Kind::Alu16(Alu16::CmpX), addressing),
                0xc => op(Kind::Ld16(Reg16::D), addressing),
                0xd if hi == 0x8 => op(Kind::Bsr, Addressing::Relative),
                0xd if hi < 0xc => op(Kind::Jsr, addressing),
                0xd if !imm => op(Kind::St16(Reg16::D), addressing),
                0xe if hi < 0xc => op(Kind::Ld16(Reg16::X), addressing),
                0xe => op(Kind::Ld16(Reg16::U), addressing),
                0xf if imm => None,
                0xf if hi < 0xc => op(Kind::St16(Reg16::X), addressing),
                0xf => op(Kind::St16(Reg16::U), addressing),
                _ => None,
            }
        }
    }
}

const fn page2(code: u8) -> Option<Decoded> {
    let hi = code >> 4;
    let lo = code & 0xf;
    match hi {
        0x2 if lo != 0 => op(Kind::Branch(CONDS[lo as usize]), Addressing::LongRelative),
        0x3 if lo == 0xf => op(
            Kind::Inherent(Inherent::Swi(Swi::Swi2)),
            Addressing::Inherent,
        ),
        0x8..=0xb => {
            let addressing = column_mode(hi);
            match lo {
                0x3 => op(Kind::Alu16(Alu16::CmpD), addressing),
                0xc => op(Kind::Alu16(Alu16::CmpY), addressing),
                0xe => op(Kind::Ld16(Reg16::Y), addressing),
                0xf if hi != 0x8 => op(Kind::St16(Reg16::Y), addressing),
                _ => None,
            }
        }
        0xc..=0xf => {
            let addressing = column_mode(hi);
            match lo {
                0xe => op(Kind::Ld16(Reg16::S), addressing),
                0xf if hi != 0xc => op(Kind::St16(Reg16::S), addressing),
                _ => None,
            }
        }
        _ => None,
    }
}

const fn page3(code: u8) -> Option<Decoded> {
    let hi = code >> 4;
    let lo = code & 0xf;
    match hi {
        0x3 if lo == 0xf => op(
            Kind::Inherent(Inherent::Swi(Swi::Swi3)),
            Addressing::Inherent,
        ),
        0x8..=0xb => {
            let addressing = column_mode(hi);
            match lo {
                0x3 => op(Kind::Alu16(Alu16::CmpU), addressing),
                0xc => op(Kind::Alu16(Alu16::CmpS), addressing),
                _ => None,
            }
        }
        _ => None,
    }
}
