//! One byte in, one instruction out.
//!
//! `I8080-049`: the manual's Summary of Processor Instructions assigns 244 of
//! the 256 codes. [`decode`] is total — it answers for all 256 — and decodes
//! the other twelve to [`Kind::Undefined`], so a caller can count them without
//! this module having to keep a second list.
use core::num::NonZeroUsize;

/// One of the eight three-bit register selectors.
///
/// `M` is not a register: it selects the memory byte at `H,L`, and the
/// sequencer is what turns that into a machine cycle.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Reg {
    /// Register `B`, `000`.
    B,
    /// Register `C`, `001`.
    C,
    /// Register `D`, `010`.
    D,
    /// Register `E`, `011`.
    E,
    /// Register `H`, `100`.
    H,
    /// Register `L`, `101`.
    L,
    /// The memory byte at `H,L`, `110`.
    M,
    /// The accumulator, `111`.
    A,
}

impl Reg {
    /// The `SSS` or `DDD` field's value, `000` for `B` through `111` for `A`.
    #[must_use]
    pub(crate) const fn from_bits(bits: u8) -> Self {
        match bits & 7 {
            0 => Self::B,
            1 => Self::C,
            2 => Self::D,
            3 => Self::E,
            4 => Self::H,
            5 => Self::L,
            6 => Self::M,
            _ => Self::A,
        }
    }
}

/// One of the four `RP` selectors. Which register `11` names depends on the
/// instruction: the stack pointer everywhere except `PUSH` and `POP`, where
/// `I8080-069` says it is the processor status word instead.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Pair {
    /// `B,C`, `00`.
    Bc,
    /// `D,E`, `01`.
    De,
    /// `H,L`, `10`.
    Hl,
    /// The stack pointer, or the accumulator and flags for `PUSH` and `POP`.
    SpPsw,
}

/// `MOV`'s two operands, which are never both `M`: that code is `HLT`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Mov {
    ddd: Reg,
    sss: Reg,
}

impl Mov {
    /// `DDD`: the register, or the memory byte at `H,L`, written.
    #[must_use]
    pub const fn ddd(self) -> Reg {
        self.ddd
    }

    /// `SSS`: the register, or the memory byte at `H,L`, read.
    #[must_use]
    pub const fn sss(self) -> Reg {
        self.sss
    }
}

/// The pair `LDAX` and `STAX` address memory through: the two their `RP`
/// field can name.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum LdaxPair {
    /// `B,C`, `00`.
    Bc,
    /// `D,E`, `01`.
    De,
}

impl LdaxPair {
    /// The pair bit 4 of the opcode selects.
    const fn from_bits(bits: u8) -> Self {
        if bits & 1 == 0 { Self::Bc } else { Self::De }
    }
}

impl From<LdaxPair> for Pair {
    fn from(pair: LdaxPair) -> Self {
        match pair {
            LdaxPair::Bc => Self::Bc,
            LdaxPair::De => Self::De,
        }
    }
}

impl Pair {
    /// The `RP` field's value.
    #[must_use]
    pub(crate) const fn from_bits(bits: u8) -> Self {
        match bits & 3 {
            0 => Self::Bc,
            1 => Self::De,
            2 => Self::Hl,
            _ => Self::SpPsw,
        }
    }
}

/// The eight conditions a conditional transfer may test (`I8080-064`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Cond {
    /// Not zero, `Z = 0`.
    Nz,
    /// Zero, `Z = 1`.
    Z,
    /// No carry, `CY = 0`.
    Nc,
    /// Carry, `CY = 1`.
    C,
    /// Parity odd, `P = 0`.
    Po,
    /// Parity even, `P = 1`.
    Pe,
    /// Plus, `S = 0`.
    P,
    /// Minus, `S = 1`.
    M,
}

impl Cond {
    /// The `CCC` field's value, `000` for `NZ` through `111` for `M`.
    #[must_use]
    pub(crate) const fn from_bits(bits: u8) -> Self {
        match bits & 7 {
            0 => Self::Nz,
            1 => Self::Z,
            2 => Self::Nc,
            3 => Self::C,
            4 => Self::Po,
            5 => Self::Pe,
            6 => Self::P,
            _ => Self::M,
        }
    }
}

/// The eight accumulator operations of `I8080-054`, `I8080-059` and
/// `I8080-061`, which share one encoding across register, memory and immediate
/// operands.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Alu8 {
    /// `ADD`, `000`: add.
    Add,
    /// `ADC`, `001`: add with carry.
    Adc,
    /// `SUB`, `010`: subtract.
    Sub,
    /// `SBB`, `011`: subtract with borrow.
    Sbb,
    /// `ANA`, `100`: logical and.
    Ana,
    /// `XRA`, `101`: exclusive or.
    Xra,
    /// `ORA`, `110`: inclusive or.
    Ora,
    /// `CMP`, `111`: compare, a subtraction that keeps only the flags.
    Cmp,
}

impl Alu8 {
    /// The `ALU` field's value, `000` for `ADD` through `111` for `CMP`.
    #[must_use]
    pub(crate) const fn from_bits(bits: u8) -> Self {
        match bits & 7 {
            0 => Self::Add,
            1 => Self::Adc,
            2 => Self::Sub,
            3 => Self::Sbb,
            4 => Self::Ana,
            5 => Self::Xra,
            6 => Self::Ora,
            _ => Self::Cmp,
        }
    }
}

/// What one opcode says to do: the instruction's mnemonic, with the
/// register, pair, condition or operation its code selects.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    /// An instruction of no operand that no register, pair or condition
    /// field selects.
    Inherent(Inherent),
    /// `MOV`.
    Mov(Mov),
    /// `MVI r`: the byte after the opcode into `r`.
    Mvi(Reg),
    /// `LXI rp`: the two bytes after the opcode into `rp`.
    Lxi(Pair),
    /// `LDA`: the accumulator from the address after the opcode.
    Lda,
    /// `STA`: the accumulator to the address after the opcode.
    Sta,
    /// `LHLD`: `L` and `H` from the address after the opcode.
    Lhld,
    /// `SHLD`: `L` and `H` to the address after the opcode.
    Shld,
    /// `LDAX`: the accumulator from the address in `B,C` or `D,E`.
    Ldax(LdaxPair),
    /// `STAX`: the accumulator to the address in `B,C` or `D,E`.
    Stax(LdaxPair),
    /// One of the eight accumulator operations against a register or memory.
    Alu(Alu8, Reg),
    /// The same eight against an immediate byte.
    AluImmediate(Alu8),
    /// `INR r`: increment.
    Inr(Reg),
    /// `DCR r`: decrement.
    Dcr(Reg),
    /// `INX rp`: increment a pair.
    Inx(Pair),
    /// `DCX rp`: decrement a pair.
    Dcx(Pair),
    /// `DAD rp`: `rp` added to `H,L`.
    Dad(Pair),
    /// `JMP`.
    Jmp,
    /// `Jcond`: jump if the condition holds.
    Jcond(Cond),
    /// `CALL`.
    Call,
    /// `Ccond`: call if the condition holds.
    Ccond(Cond),
    /// `Rcond`: return if the condition holds.
    Rcond(Cond),
    /// `RST n`, carrying `n` rather than the address, because `I8080-032` is
    /// about the multiplication.
    Rst(Restart),
    /// `PUSH rp`.
    Push(Pair),
    /// `POP rp`.
    Pop(Pair),
    /// `IN`: the accumulator from the port after the opcode.
    In,
    /// `OUT`: the accumulator to the port after the opcode.
    Out,
    /// One of the twelve codes the Summary of Processor Instructions leaves
    /// out (`I8080-049`). **What the part does with one is this crate's
    /// choice under `I8080-OPEN-01`**: `08h`, `10h`, `18h`, `20h`, `28h`,
    /// `30h` and `38h` run as `NOP`; `CBh` as `JMP`; `D9h` as `RET`; `DDh`,
    /// `EDh` and `FDh` as `CALL`.
    Undefined,
}

/// The instructions of no operand: each its own code, applied to nothing a
/// field selects.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Inherent {
    /// `NOP`.
    Nop,
    /// `XCHG`: `H,L` and `D,E` swapped.
    Xchg,
    /// `DAA`: decimal adjust the accumulator.
    Daa,
    /// `RLC`: rotate left.
    Rlc,
    /// `RRC`: rotate right.
    Rrc,
    /// `RAL`: rotate left through carry.
    Ral,
    /// `RAR`: rotate right through carry.
    Rar,
    /// `CMA`: complement the accumulator.
    Cma,
    /// `STC`: set carry.
    Stc,
    /// `CMC`: complement carry.
    Cmc,
    /// `RET`.
    Ret,
    /// `PCHL`: `H,L` into the program counter.
    Pchl,
    /// `XTHL`: `H,L` and the top of the stack swapped.
    Xthl,
    /// `SPHL`: `H,L` into the stack pointer.
    Sphl,
    /// `EI`: enable interrupts.
    Ei,
    /// `DI`: disable interrupts.
    Di,
    /// `HLT`.
    Hlt,
}

/// The `NNN` field of `RST n`: which of the eight restarts.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Restart {
    /// `RST 0`.
    Zero,
    /// `RST 1`.
    One,
    /// `RST 2`.
    Two,
    /// `RST 3`.
    Three,
    /// `RST 4`.
    Four,
    /// `RST 5`.
    Five,
    /// `RST 6`.
    Six,
    /// `RST 7`.
    Seven,
}

impl Restart {
    /// The `NNN` field's value.
    pub(crate) const fn n(self) -> u8 {
        self as u8
    }

    /// The restart whose `NNN` field is `bits`.
    const fn from_bits(bits: u8) -> Self {
        match bits & 7 {
            0 => Self::Zero,
            1 => Self::One,
            2 => Self::Two,
            3 => Self::Three,
            4 => Self::Four,
            5 => Self::Five,
            6 => Self::Six,
            _ => Self::Seven,
        }
    }
}

/// One opcode, read: the decoder's row.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub struct Decoded {
    /// What the part does.
    pub kind: Kind,
    /// The instruction's length in bytes: one, two or three (`I8080-050`),
    /// and for [`Kind::Undefined`] the length of the instruction the part
    /// runs in its place.
    pub len: NonZeroUsize,
}

/// Read one opcode.
///
/// Total over all 256 bytes; the twelve the manual never names are
/// [`Kind::Undefined`].
#[must_use]
pub const fn decode(code: u8) -> Decoded {
    let run = runs(code);
    if documented(code) {
        run
    } else {
        Decoded {
            kind: Kind::Undefined,
            len: run.len,
        }
    }
}

/// Whether the Summary of Processor Instructions assigns `code`.
const fn documented(code: u8) -> bool {
    !matches!(
        code,
        0x08 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 | 0xcb | 0xd9 | 0xdd | 0xed | 0xfd
    )
}

/// What the part runs for `code`: [`decode`]'s row, and for an undocumented
/// code the one [`Kind::Undefined`] names, which is `I8080-OPEN-01`.
pub(crate) const fn runs(code: u8) -> Decoded {
    let ddd = (code >> 3) & 7;
    let sss = code & 7;
    let rp = (code >> 4) & 3;

    let (kind, len) = match code {
        0x00 | 0x08 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 => (Kind::Inherent(Inherent::Nop), 1),
        0x01 | 0x11 | 0x21 | 0x31 => (Kind::Lxi(Pair::from_bits(rp)), 3),
        0x02 | 0x12 => (Kind::Stax(LdaxPair::from_bits(rp)), 1),
        0x22 => (Kind::Shld, 3),
        0x32 => (Kind::Sta, 3),
        0x0a | 0x1a => (Kind::Ldax(LdaxPair::from_bits(rp)), 1),
        0x2a => (Kind::Lhld, 3),
        0x3a => (Kind::Lda, 3),
        0x03 | 0x13 | 0x23 | 0x33 => (Kind::Inx(Pair::from_bits(rp)), 1),
        0x0b | 0x1b | 0x2b | 0x3b => (Kind::Dcx(Pair::from_bits(rp)), 1),
        0x09 | 0x19 | 0x29 | 0x39 => (Kind::Dad(Pair::from_bits(rp)), 1),
        0x07 => (Kind::Inherent(Inherent::Rlc), 1),
        0x0f => (Kind::Inherent(Inherent::Rrc), 1),
        0x17 => (Kind::Inherent(Inherent::Ral), 1),
        0x1f => (Kind::Inherent(Inherent::Rar), 1),
        0x27 => (Kind::Inherent(Inherent::Daa), 1),
        0x2f => (Kind::Inherent(Inherent::Cma), 1),
        0x37 => (Kind::Inherent(Inherent::Stc), 1),
        0x3f => (Kind::Inherent(Inherent::Cmc), 1),
        0x76 => (Kind::Inherent(Inherent::Hlt), 1),

        0x40..=0x7f => (
            Kind::Mov(Mov {
                ddd: Reg::from_bits(ddd),
                sss: Reg::from_bits(sss),
            }),
            1,
        ),

        0x80..=0xbf => (Kind::Alu(Alu8::from_bits(ddd), Reg::from_bits(sss)), 1),

        0xc6 | 0xce | 0xd6 | 0xde | 0xe6 | 0xee | 0xf6 | 0xfe => {
            (Kind::AluImmediate(Alu8::from_bits(ddd)), 2)
        }
        0xc3 | 0xcb => (Kind::Jmp, 3),
        0xcd | 0xdd | 0xed | 0xfd => (Kind::Call, 3),
        0xc9 | 0xd9 => (Kind::Inherent(Inherent::Ret), 1),
        0xe9 => (Kind::Inherent(Inherent::Pchl), 1),
        0xf9 => (Kind::Inherent(Inherent::Sphl), 1),
        0xe3 => (Kind::Inherent(Inherent::Xthl), 1),
        0xeb => (Kind::Inherent(Inherent::Xchg), 1),
        0xdb => (Kind::In, 2),
        0xd3 => (Kind::Out, 2),
        0xfb => (Kind::Inherent(Inherent::Ei), 1),
        0xf3 => (Kind::Inherent(Inherent::Di), 1),
        0xc5 | 0xd5 | 0xe5 | 0xf5 => (Kind::Push(Pair::from_bits(rp)), 1),
        0xc1 | 0xd1 | 0xe1 | 0xf1 => (Kind::Pop(Pair::from_bits(rp)), 1),
        0x00..=0x3f => match code & 7 {
            4 => (Kind::Inr(Reg::from_bits(ddd)), 1),
            5 => (Kind::Dcr(Reg::from_bits(ddd)), 1),
            _ => (Kind::Mvi(Reg::from_bits(ddd)), 2),
        },

        _ => match code & 7 {
            0 => (Kind::Rcond(Cond::from_bits(ddd)), 1),
            2 => (Kind::Jcond(Cond::from_bits(ddd)), 3),
            7 => (Kind::Rst(Restart::from_bits(ddd)), 1),
            _ => (Kind::Ccond(Cond::from_bits(ddd)), 3),
        },
    };

    Decoded {
        kind,
        len: NonZeroUsize::new(len).expect("every row is one, two or three bytes"),
    }
}
