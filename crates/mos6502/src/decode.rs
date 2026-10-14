//! The opcode table: all 256 of them, the 151 the manufacturer prints and the
//! 105 it marks `Future Expansion`.
//!
//! The table is written out in full rather than derived from bit fields
//! (`MOS6502-OPEN-04`).
//!
//! # The 105 are not the manufacturer's, and the table says so twice
//!
//! `MOS6502-033`: Appendix D of MOS Technology's *MCS6500 Microcomputer Family
//! Programming Manual* (6500-50A) prints `Future Expansion` against each of
//! them. What they do is `MOS6502-062` to `MOS6502-070`, **except** for the
//! twelve that stop the part and the two that mix in a constant, which are
//! this crate's choice under `MOS6502-OPEN-01` and `MOS6502-OPEN-02`.

/// A register an operation applies to.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Reg {
    /// The accumulator.
    A,
    /// Index X.
    X,
    /// Index Y.
    Y,
}

/// An operation on a register and the byte the operand brings
/// ([`Kind::Alu8`]), each on the one register its opcode names: the
/// accumulator unless the variant names an index.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Alu8 {
    /// `ADC`: add with carry.
    Adc,
    /// `AND`.
    And,
    /// `BIT`: test bits against the accumulator.
    Bit,
    /// `CMP`: compare with the accumulator.
    Cmp,
    /// `CPX`: compare with index X.
    CmpX,
    /// `CPY`: compare with index Y.
    CmpY,
    /// `EOR`: exclusive OR.
    Eor,
    /// `LDA`: load the accumulator.
    Ld,
    /// `LDX`: load index X.
    LdX,
    /// `LDY`: load index Y.
    LdY,
    /// `ORA`: OR.
    Ora,
    /// `SBC`: subtract with borrow.
    Sbc,
}

/// A read-modify-write operation and where it applies ([`Kind::Unary`]):
/// the shifts on the accumulator or memory, the steps on an index or memory.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Unary {
    /// `ASL A`: shift the accumulator left one bit.
    AslA,
    /// `ASL`: shift the byte at the effective address left one bit.
    AslMemory,
    /// `LSR A`: shift the accumulator right one bit.
    LsrA,
    /// `LSR`: shift the byte at the effective address right one bit.
    LsrMemory,
    /// `ROL A`: rotate the accumulator one bit left.
    RolA,
    /// `ROL`: rotate the byte at the effective address one bit left.
    RolMemory,
    /// `ROR A`: rotate the accumulator one bit right.
    RorA,
    /// `ROR`: rotate the byte at the effective address one bit right.
    RorMemory,
    /// `INX`: increment index X by one.
    IncX,
    /// `INY`: increment index Y by one.
    IncY,
    /// `INC`: increment the byte at the effective address by one.
    IncMemory,
    /// `DEX`: decrement index X by one.
    DecX,
    /// `DEY`: decrement index Y by one.
    DecY,
    /// `DEC`: decrement the byte at the effective address by one.
    DecMemory,
}

/// A branch's condition ([`Kind::Branch`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Cond {
    /// `BCC`: carry clear.
    Cc,
    /// `BCS`: carry set.
    Cs,
    /// `BEQ`: result zero.
    Eq,
    /// `BNE`: result not zero.
    Ne,
    /// `BMI`: result minus.
    Mi,
    /// `BPL`: result plus.
    Pl,
    /// `BVC`: overflow clear.
    Vc,
    /// `BVS`: overflow set.
    Vs,
}

/// A register [`Inherent::Push`] and [`Inherent::Pull`] move through the
/// stack.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Stacked {
    /// The accumulator: `PHA`, `PLA`.
    A,
    /// The processor status: `PHP`, `PLP`.
    P,
}

/// An instruction of no operand ([`Kind::Inherent`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Inherent {
    /// `BRK`: force break.
    Brk,
    /// `CLC`: clear the carry flag.
    Clc,
    /// `CLD`: clear decimal mode.
    Cld,
    /// `CLI`: clear the interrupt disable bit.
    Cli,
    /// `CLV`: clear the overflow flag.
    Clv,
    /// `SEC`: set the carry flag.
    Sec,
    /// `SED`: set decimal mode.
    Sed,
    /// `SEI`: set the interrupt disable bit.
    Sei,
    /// `NOP`: no operation, whatever it reads.
    Nop,
    /// `PHA`, `PHP`: push a register on the stack.
    Push(Stacked),
    /// `PLA`, `PLP`: pull a register from the stack.
    Pull(Stacked),
    /// `RTI`: return from interrupt.
    Rti,
    /// `RTS`: return from subroutine.
    Rts,
    /// `TAX`: transfer the accumulator to index X.
    Tax,
    /// `TAY`: transfer the accumulator to index Y.
    Tay,
    /// `TSX`: transfer the stack pointer to index X.
    Tsx,
    /// `TXA`: transfer index X to the accumulator.
    Txa,
    /// `TXS`: transfer index X to the stack pointer.
    Txs,
    /// `TYA`: transfer index Y to the accumulator.
    Tya,
}

/// What an instruction does, once its operand is in hand.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    /// `ADC`, `AND`, `BIT`, `CMP`, `CPX`, `CPY`, `EOR`, `LDA`, `LDX`, `LDY`,
    /// `ORA`, `SBC`: an operation on a register and the operand.
    Alu8(Alu8),
    /// `STA`, `STX`, `STY`: store a register.
    St(Reg),
    /// `ASL`, `LSR`, `ROL`, `ROR` on the accumulator or memory, `INC`, `DEC`
    /// on memory and `INX`, `INY`, `DEX`, `DEY`.
    Unary(Unary),
    /// The eight conditional branches.
    Branch(Cond),
    /// `JMP`: jump to a new location.
    Jmp,
    /// `JSR`: jump to a new location, saving the return address.
    Jsr,
    /// The instructions of no operand not above, `NOP` among them.
    Inherent(Inherent),
    /// `SLO`: `ASL` memory, then `ORA` the result.
    Slo,
    /// `RLA`: `ROL` memory, then `AND` the result.
    Rla,
    /// `SRE`: `LSR` memory, then `EOR` the result.
    Sre,
    /// `RRA`: `ROR` memory, then `ADC` the result.
    Rra,
    /// `SAX`: store the accumulator AND index X.
    Sax,
    /// `LAX`: load the accumulator and index X from memory.
    Lax,
    /// `DCP`: `DEC` memory, then `CMP` the result.
    Dcp,
    /// `ISC`: `INC` memory, then `SBC` the result.
    Isc,
    /// `ANC`: `AND` an immediate byte, carry from bit 7.
    Anc,
    /// `ALR`: `AND` an immediate byte, then `LSR` the accumulator.
    Alr,
    /// `ARR`: `AND` an immediate byte, then `ROR` the accumulator.
    Arr,
    /// `SBX`: index X becomes the accumulator AND X, less an immediate byte.
    Sbx,
    /// `LAS`: memory AND the stack pointer, into the accumulator, X and S.
    Las,
    /// `SHA`: store the accumulator AND X AND the address's high byte plus one.
    Sha,
    /// `SHX`: store index X AND the address's high byte plus one.
    Shx,
    /// `SHY`: store index Y AND the address's high byte plus one.
    Shy,
    /// `TAS`: S becomes the accumulator AND X, stored as `SHA` stores.
    Tas,
    /// One of the twelve codes the sheet leaves empty that this crate stops
    /// the part on until `RES`, which is the crate's choice
    /// (`MOS6502-OPEN-01`): [`crate::Mos6502::is_jammed`] then holds.
    Undefined,
    /// `ANE`: the accumulator becomes a constant ORed in, AND X AND an immediate byte.
    Ane,
    /// `LXA`: the accumulator and X become a constant ORed in, AND an immediate byte.
    Lxa,
}

/// How the operand address is formed: the sequencer's, read every cycle,
/// and not the API, where `Decoded` carries its kind and length only.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum Addressing {
    /// Two cycles, the second a discarded fetch from `PC + 1` (`MOS6502-040`).
    Implied,
    /// As [`Addressing::Implied`], but the operand is the accumulator.
    Accumulator,
    /// The operand byte follows the opcode.
    Immediate,
    /// Zero page.
    ZeroPage,
    /// Zero page, X indexed, wrapping inside the page (`MOS6502-044`).
    ZeroPageX,
    /// Zero page, Y indexed, wrapping inside the page (`MOS6502-044`).
    ZeroPageY,
    /// Absolute.
    Absolute,
    /// Absolute, X indexed.
    AbsoluteX,
    /// Absolute, Y indexed.
    AbsoluteY,
    /// Zero page X indexed indirect, `(zp,x)`.
    IndirectX,
    /// Zero page indirect Y indexed, `(zp),y`.
    IndirectY,
    /// Relative — the branches.
    Relative,
    /// `JMP abs`.
    JmpAbsolute,
    /// `JMP (abs)`, which the manufacturer calls Absolute Indirect.
    Indirect,
    /// `BRK`, and the three hardware sequences with it.
    Brk,
    /// `JSR abs`.
    Jsr,
    /// `RTS`.
    Rts,
    /// `RTI`.
    Rti,
    /// `PHA`, `PHP`.
    Push,
    /// `PLA`, `PLP`.
    Pull,
    /// A [`Kind::Undefined`] code, which stops the part: this crate's choice
    /// under `MOS6502-OPEN-01`.
    Undefined,
}

/// What the instruction does to the byte at the effective address: the
/// sequencer's, as [`Addressing`] is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum Access {
    /// No effective address: the mode carries its own cycles.
    None,
    /// One read, and the operation runs on what came back.
    Read,
    /// One write, and no read of the target.
    Write,
    /// Read, write the byte back unchanged, then write the new one
    /// (`MOS6502-047`).
    Modify,
}

/// One row of the table.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub struct Decoded {
    /// The operation.
    pub kind: Kind,
    /// The instruction's length in bytes, opcode included: one for `BRK`,
    /// whose return address skips the byte after it.
    pub len: NonZeroUsize,
    /// The addressing mode.
    pub(crate) addressing: Addressing,
    /// What it does at the effective address.
    pub(crate) access: Access,
}

const fn d(kind: Kind, addressing: Addressing, access: Access) -> Decoded {
    let len = match addressing {
        Addressing::Implied
        | Addressing::Accumulator
        | Addressing::Brk
        | Addressing::Rts
        | Addressing::Rti
        | Addressing::Push
        | Addressing::Pull
        | Addressing::Undefined => 1,
        Addressing::Immediate
        | Addressing::ZeroPage
        | Addressing::ZeroPageX
        | Addressing::ZeroPageY
        | Addressing::IndirectX
        | Addressing::IndirectY
        | Addressing::Relative => 2,
        Addressing::Absolute
        | Addressing::AbsoluteX
        | Addressing::AbsoluteY
        | Addressing::JmpAbsolute
        | Addressing::Indirect
        | Addressing::Jsr => 3,
    };
    Decoded {
        kind,
        len: NonZeroUsize::new(len).expect("every mode is one, two or three bytes"),
        addressing,
        access,
    }
}

use Access::{Modify, None as NoAccess, Read, Write};
use Addressing::{
    Absolute, AbsoluteX, AbsoluteY, Accumulator, Brk, Immediate, Implied, Indirect, IndirectX,
    IndirectY, JmpAbsolute, Jsr, Pull, Push, Relative, Rti, Rts, ZeroPage, ZeroPageX, ZeroPageY,
};
use Kind as O;
use core::num::NonZeroUsize;

/// The whole instruction set, indexed by opcode.
pub(crate) const TABLE: [Decoded; 256] = {
    let mut t = [d(O::Undefined, Addressing::Undefined, NoAccess); 256];

    t[0x00] = d(O::Inherent(Inherent::Brk), Brk, NoAccess);
    t[0x01] = d(O::Alu8(Alu8::Ora), IndirectX, Read);
    t[0x03] = d(O::Slo, IndirectX, Modify);
    t[0x04] = d(O::Inherent(Inherent::Nop), ZeroPage, Read);
    t[0x05] = d(O::Alu8(Alu8::Ora), ZeroPage, Read);
    t[0x06] = d(O::Unary(Unary::AslMemory), ZeroPage, Modify);
    t[0x07] = d(O::Slo, ZeroPage, Modify);
    t[0x08] = d(O::Inherent(Inherent::Push(Stacked::P)), Push, NoAccess);
    t[0x09] = d(O::Alu8(Alu8::Ora), Immediate, Read);
    t[0x0a] = d(O::Unary(Unary::AslA), Accumulator, NoAccess);
    t[0x0b] = d(O::Anc, Immediate, Read);
    t[0x0c] = d(O::Inherent(Inherent::Nop), Absolute, Read);
    t[0x0d] = d(O::Alu8(Alu8::Ora), Absolute, Read);
    t[0x0e] = d(O::Unary(Unary::AslMemory), Absolute, Modify);
    t[0x0f] = d(O::Slo, Absolute, Modify);
    t[0x10] = d(O::Branch(Cond::Pl), Relative, NoAccess);
    t[0x11] = d(O::Alu8(Alu8::Ora), IndirectY, Read);
    t[0x13] = d(O::Slo, IndirectY, Modify);
    t[0x14] = d(O::Inherent(Inherent::Nop), ZeroPageX, Read);
    t[0x15] = d(O::Alu8(Alu8::Ora), ZeroPageX, Read);
    t[0x16] = d(O::Unary(Unary::AslMemory), ZeroPageX, Modify);
    t[0x17] = d(O::Slo, ZeroPageX, Modify);
    t[0x18] = d(O::Inherent(Inherent::Clc), Implied, NoAccess);
    t[0x19] = d(O::Alu8(Alu8::Ora), AbsoluteY, Read);
    t[0x1a] = d(O::Inherent(Inherent::Nop), Implied, NoAccess);
    t[0x1b] = d(O::Slo, AbsoluteY, Modify);
    t[0x1c] = d(O::Inherent(Inherent::Nop), AbsoluteX, Read);
    t[0x1d] = d(O::Alu8(Alu8::Ora), AbsoluteX, Read);
    t[0x1e] = d(O::Unary(Unary::AslMemory), AbsoluteX, Modify);
    t[0x1f] = d(O::Slo, AbsoluteX, Modify);
    t[0x20] = d(O::Jsr, Jsr, NoAccess);
    t[0x21] = d(O::Alu8(Alu8::And), IndirectX, Read);
    t[0x23] = d(O::Rla, IndirectX, Modify);
    t[0x24] = d(O::Alu8(Alu8::Bit), ZeroPage, Read);
    t[0x25] = d(O::Alu8(Alu8::And), ZeroPage, Read);
    t[0x26] = d(O::Unary(Unary::RolMemory), ZeroPage, Modify);
    t[0x27] = d(O::Rla, ZeroPage, Modify);
    t[0x28] = d(O::Inherent(Inherent::Pull(Stacked::P)), Pull, NoAccess);
    t[0x29] = d(O::Alu8(Alu8::And), Immediate, Read);
    t[0x2a] = d(O::Unary(Unary::RolA), Accumulator, NoAccess);
    t[0x2b] = d(O::Anc, Immediate, Read);
    t[0x2c] = d(O::Alu8(Alu8::Bit), Absolute, Read);
    t[0x2d] = d(O::Alu8(Alu8::And), Absolute, Read);
    t[0x2e] = d(O::Unary(Unary::RolMemory), Absolute, Modify);
    t[0x2f] = d(O::Rla, Absolute, Modify);
    t[0x30] = d(O::Branch(Cond::Mi), Relative, NoAccess);
    t[0x31] = d(O::Alu8(Alu8::And), IndirectY, Read);
    t[0x33] = d(O::Rla, IndirectY, Modify);
    t[0x34] = d(O::Inherent(Inherent::Nop), ZeroPageX, Read);
    t[0x35] = d(O::Alu8(Alu8::And), ZeroPageX, Read);
    t[0x36] = d(O::Unary(Unary::RolMemory), ZeroPageX, Modify);
    t[0x37] = d(O::Rla, ZeroPageX, Modify);
    t[0x38] = d(O::Inherent(Inherent::Sec), Implied, NoAccess);
    t[0x39] = d(O::Alu8(Alu8::And), AbsoluteY, Read);
    t[0x3a] = d(O::Inherent(Inherent::Nop), Implied, NoAccess);
    t[0x3b] = d(O::Rla, AbsoluteY, Modify);
    t[0x3c] = d(O::Inherent(Inherent::Nop), AbsoluteX, Read);
    t[0x3d] = d(O::Alu8(Alu8::And), AbsoluteX, Read);
    t[0x3e] = d(O::Unary(Unary::RolMemory), AbsoluteX, Modify);
    t[0x3f] = d(O::Rla, AbsoluteX, Modify);
    t[0x40] = d(O::Inherent(Inherent::Rti), Rti, NoAccess);
    t[0x41] = d(O::Alu8(Alu8::Eor), IndirectX, Read);
    t[0x43] = d(O::Sre, IndirectX, Modify);
    t[0x44] = d(O::Inherent(Inherent::Nop), ZeroPage, Read);
    t[0x45] = d(O::Alu8(Alu8::Eor), ZeroPage, Read);
    t[0x46] = d(O::Unary(Unary::LsrMemory), ZeroPage, Modify);
    t[0x47] = d(O::Sre, ZeroPage, Modify);
    t[0x48] = d(O::Inherent(Inherent::Push(Stacked::A)), Push, NoAccess);
    t[0x49] = d(O::Alu8(Alu8::Eor), Immediate, Read);
    t[0x4a] = d(O::Unary(Unary::LsrA), Accumulator, NoAccess);
    t[0x4b] = d(O::Alr, Immediate, Read);
    t[0x4c] = d(O::Jmp, JmpAbsolute, NoAccess);
    t[0x4d] = d(O::Alu8(Alu8::Eor), Absolute, Read);
    t[0x4e] = d(O::Unary(Unary::LsrMemory), Absolute, Modify);
    t[0x4f] = d(O::Sre, Absolute, Modify);
    t[0x50] = d(O::Branch(Cond::Vc), Relative, NoAccess);
    t[0x51] = d(O::Alu8(Alu8::Eor), IndirectY, Read);
    t[0x53] = d(O::Sre, IndirectY, Modify);
    t[0x54] = d(O::Inherent(Inherent::Nop), ZeroPageX, Read);
    t[0x55] = d(O::Alu8(Alu8::Eor), ZeroPageX, Read);
    t[0x56] = d(O::Unary(Unary::LsrMemory), ZeroPageX, Modify);
    t[0x57] = d(O::Sre, ZeroPageX, Modify);
    t[0x58] = d(O::Inherent(Inherent::Cli), Implied, NoAccess);
    t[0x59] = d(O::Alu8(Alu8::Eor), AbsoluteY, Read);
    t[0x5a] = d(O::Inherent(Inherent::Nop), Implied, NoAccess);
    t[0x5b] = d(O::Sre, AbsoluteY, Modify);
    t[0x5c] = d(O::Inherent(Inherent::Nop), AbsoluteX, Read);
    t[0x5d] = d(O::Alu8(Alu8::Eor), AbsoluteX, Read);
    t[0x5e] = d(O::Unary(Unary::LsrMemory), AbsoluteX, Modify);
    t[0x5f] = d(O::Sre, AbsoluteX, Modify);
    t[0x60] = d(O::Inherent(Inherent::Rts), Rts, NoAccess);
    t[0x61] = d(O::Alu8(Alu8::Adc), IndirectX, Read);
    t[0x63] = d(O::Rra, IndirectX, Modify);
    t[0x64] = d(O::Inherent(Inherent::Nop), ZeroPage, Read);
    t[0x65] = d(O::Alu8(Alu8::Adc), ZeroPage, Read);
    t[0x66] = d(O::Unary(Unary::RorMemory), ZeroPage, Modify);
    t[0x67] = d(O::Rra, ZeroPage, Modify);
    t[0x68] = d(O::Inherent(Inherent::Pull(Stacked::A)), Pull, NoAccess);
    t[0x69] = d(O::Alu8(Alu8::Adc), Immediate, Read);
    t[0x6a] = d(O::Unary(Unary::RorA), Accumulator, NoAccess);
    t[0x6b] = d(O::Arr, Immediate, Read);
    t[0x6c] = d(O::Jmp, Indirect, NoAccess);
    t[0x6d] = d(O::Alu8(Alu8::Adc), Absolute, Read);
    t[0x6e] = d(O::Unary(Unary::RorMemory), Absolute, Modify);
    t[0x6f] = d(O::Rra, Absolute, Modify);
    t[0x70] = d(O::Branch(Cond::Vs), Relative, NoAccess);
    t[0x71] = d(O::Alu8(Alu8::Adc), IndirectY, Read);
    t[0x73] = d(O::Rra, IndirectY, Modify);
    t[0x74] = d(O::Inherent(Inherent::Nop), ZeroPageX, Read);
    t[0x75] = d(O::Alu8(Alu8::Adc), ZeroPageX, Read);
    t[0x76] = d(O::Unary(Unary::RorMemory), ZeroPageX, Modify);
    t[0x77] = d(O::Rra, ZeroPageX, Modify);
    t[0x78] = d(O::Inherent(Inherent::Sei), Implied, NoAccess);
    t[0x79] = d(O::Alu8(Alu8::Adc), AbsoluteY, Read);
    t[0x7a] = d(O::Inherent(Inherent::Nop), Implied, NoAccess);
    t[0x7b] = d(O::Rra, AbsoluteY, Modify);
    t[0x7c] = d(O::Inherent(Inherent::Nop), AbsoluteX, Read);
    t[0x7d] = d(O::Alu8(Alu8::Adc), AbsoluteX, Read);
    t[0x7e] = d(O::Unary(Unary::RorMemory), AbsoluteX, Modify);
    t[0x7f] = d(O::Rra, AbsoluteX, Modify);
    t[0x80] = d(O::Inherent(Inherent::Nop), Immediate, Read);
    t[0x81] = d(O::St(Reg::A), IndirectX, Write);
    t[0x82] = d(O::Inherent(Inherent::Nop), Immediate, Read);
    t[0x83] = d(O::Sax, IndirectX, Write);
    t[0x84] = d(O::St(Reg::Y), ZeroPage, Write);
    t[0x85] = d(O::St(Reg::A), ZeroPage, Write);
    t[0x86] = d(O::St(Reg::X), ZeroPage, Write);
    t[0x87] = d(O::Sax, ZeroPage, Write);
    t[0x88] = d(O::Unary(Unary::DecY), Implied, NoAccess);
    t[0x89] = d(O::Inherent(Inherent::Nop), Immediate, Read);
    t[0x8a] = d(O::Inherent(Inherent::Txa), Implied, NoAccess);
    t[0x8b] = d(O::Ane, Immediate, Read);
    t[0x8c] = d(O::St(Reg::Y), Absolute, Write);
    t[0x8d] = d(O::St(Reg::A), Absolute, Write);
    t[0x8e] = d(O::St(Reg::X), Absolute, Write);
    t[0x8f] = d(O::Sax, Absolute, Write);
    t[0x90] = d(O::Branch(Cond::Cc), Relative, NoAccess);
    t[0x91] = d(O::St(Reg::A), IndirectY, Write);
    t[0x93] = d(O::Sha, IndirectY, Write);
    t[0x94] = d(O::St(Reg::Y), ZeroPageX, Write);
    t[0x95] = d(O::St(Reg::A), ZeroPageX, Write);
    t[0x96] = d(O::St(Reg::X), ZeroPageY, Write);
    t[0x97] = d(O::Sax, ZeroPageY, Write);
    t[0x98] = d(O::Inherent(Inherent::Tya), Implied, NoAccess);
    t[0x99] = d(O::St(Reg::A), AbsoluteY, Write);
    t[0x9a] = d(O::Inherent(Inherent::Txs), Implied, NoAccess);
    t[0x9b] = d(O::Tas, AbsoluteY, Write);
    t[0x9c] = d(O::Shy, AbsoluteX, Write);
    t[0x9d] = d(O::St(Reg::A), AbsoluteX, Write);
    t[0x9e] = d(O::Shx, AbsoluteY, Write);
    t[0x9f] = d(O::Sha, AbsoluteY, Write);
    t[0xa0] = d(O::Alu8(Alu8::LdY), Immediate, Read);
    t[0xa1] = d(O::Alu8(Alu8::Ld), IndirectX, Read);
    t[0xa2] = d(O::Alu8(Alu8::LdX), Immediate, Read);
    t[0xa3] = d(O::Lax, IndirectX, Read);
    t[0xa4] = d(O::Alu8(Alu8::LdY), ZeroPage, Read);
    t[0xa5] = d(O::Alu8(Alu8::Ld), ZeroPage, Read);
    t[0xa6] = d(O::Alu8(Alu8::LdX), ZeroPage, Read);
    t[0xa7] = d(O::Lax, ZeroPage, Read);
    t[0xa8] = d(O::Inherent(Inherent::Tay), Implied, NoAccess);
    t[0xa9] = d(O::Alu8(Alu8::Ld), Immediate, Read);
    t[0xaa] = d(O::Inherent(Inherent::Tax), Implied, NoAccess);
    t[0xab] = d(O::Lxa, Immediate, Read);
    t[0xac] = d(O::Alu8(Alu8::LdY), Absolute, Read);
    t[0xad] = d(O::Alu8(Alu8::Ld), Absolute, Read);
    t[0xae] = d(O::Alu8(Alu8::LdX), Absolute, Read);
    t[0xaf] = d(O::Lax, Absolute, Read);
    t[0xb0] = d(O::Branch(Cond::Cs), Relative, NoAccess);
    t[0xb1] = d(O::Alu8(Alu8::Ld), IndirectY, Read);
    t[0xb3] = d(O::Lax, IndirectY, Read);
    t[0xb4] = d(O::Alu8(Alu8::LdY), ZeroPageX, Read);
    t[0xb5] = d(O::Alu8(Alu8::Ld), ZeroPageX, Read);
    t[0xb6] = d(O::Alu8(Alu8::LdX), ZeroPageY, Read);
    t[0xb7] = d(O::Lax, ZeroPageY, Read);
    t[0xb8] = d(O::Inherent(Inherent::Clv), Implied, NoAccess);
    t[0xb9] = d(O::Alu8(Alu8::Ld), AbsoluteY, Read);
    t[0xba] = d(O::Inherent(Inherent::Tsx), Implied, NoAccess);
    t[0xbb] = d(O::Las, AbsoluteY, Read);
    t[0xbc] = d(O::Alu8(Alu8::LdY), AbsoluteX, Read);
    t[0xbd] = d(O::Alu8(Alu8::Ld), AbsoluteX, Read);
    t[0xbe] = d(O::Alu8(Alu8::LdX), AbsoluteY, Read);
    t[0xbf] = d(O::Lax, AbsoluteY, Read);
    t[0xc0] = d(O::Alu8(Alu8::CmpY), Immediate, Read);
    t[0xc1] = d(O::Alu8(Alu8::Cmp), IndirectX, Read);
    t[0xc2] = d(O::Inherent(Inherent::Nop), Immediate, Read);
    t[0xc3] = d(O::Dcp, IndirectX, Modify);
    t[0xc4] = d(O::Alu8(Alu8::CmpY), ZeroPage, Read);
    t[0xc5] = d(O::Alu8(Alu8::Cmp), ZeroPage, Read);
    t[0xc6] = d(O::Unary(Unary::DecMemory), ZeroPage, Modify);
    t[0xc7] = d(O::Dcp, ZeroPage, Modify);
    t[0xc8] = d(O::Unary(Unary::IncY), Implied, NoAccess);
    t[0xc9] = d(O::Alu8(Alu8::Cmp), Immediate, Read);
    t[0xca] = d(O::Unary(Unary::DecX), Implied, NoAccess);
    t[0xcb] = d(O::Sbx, Immediate, Read);
    t[0xcc] = d(O::Alu8(Alu8::CmpY), Absolute, Read);
    t[0xcd] = d(O::Alu8(Alu8::Cmp), Absolute, Read);
    t[0xce] = d(O::Unary(Unary::DecMemory), Absolute, Modify);
    t[0xcf] = d(O::Dcp, Absolute, Modify);
    t[0xd0] = d(O::Branch(Cond::Ne), Relative, NoAccess);
    t[0xd1] = d(O::Alu8(Alu8::Cmp), IndirectY, Read);
    t[0xd3] = d(O::Dcp, IndirectY, Modify);
    t[0xd4] = d(O::Inherent(Inherent::Nop), ZeroPageX, Read);
    t[0xd5] = d(O::Alu8(Alu8::Cmp), ZeroPageX, Read);
    t[0xd6] = d(O::Unary(Unary::DecMemory), ZeroPageX, Modify);
    t[0xd7] = d(O::Dcp, ZeroPageX, Modify);
    t[0xd8] = d(O::Inherent(Inherent::Cld), Implied, NoAccess);
    t[0xd9] = d(O::Alu8(Alu8::Cmp), AbsoluteY, Read);
    t[0xda] = d(O::Inherent(Inherent::Nop), Implied, NoAccess);
    t[0xdb] = d(O::Dcp, AbsoluteY, Modify);
    t[0xdc] = d(O::Inherent(Inherent::Nop), AbsoluteX, Read);
    t[0xdd] = d(O::Alu8(Alu8::Cmp), AbsoluteX, Read);
    t[0xde] = d(O::Unary(Unary::DecMemory), AbsoluteX, Modify);
    t[0xdf] = d(O::Dcp, AbsoluteX, Modify);
    t[0xe0] = d(O::Alu8(Alu8::CmpX), Immediate, Read);
    t[0xe1] = d(O::Alu8(Alu8::Sbc), IndirectX, Read);
    t[0xe2] = d(O::Inherent(Inherent::Nop), Immediate, Read);
    t[0xe3] = d(O::Isc, IndirectX, Modify);
    t[0xe4] = d(O::Alu8(Alu8::CmpX), ZeroPage, Read);
    t[0xe5] = d(O::Alu8(Alu8::Sbc), ZeroPage, Read);
    t[0xe6] = d(O::Unary(Unary::IncMemory), ZeroPage, Modify);
    t[0xe7] = d(O::Isc, ZeroPage, Modify);
    t[0xe8] = d(O::Unary(Unary::IncX), Implied, NoAccess);
    t[0xe9] = d(O::Alu8(Alu8::Sbc), Immediate, Read);
    t[0xea] = d(O::Inherent(Inherent::Nop), Implied, NoAccess);
    t[0xeb] = d(O::Alu8(Alu8::Sbc), Immediate, Read);
    t[0xec] = d(O::Alu8(Alu8::CmpX), Absolute, Read);
    t[0xed] = d(O::Alu8(Alu8::Sbc), Absolute, Read);
    t[0xee] = d(O::Unary(Unary::IncMemory), Absolute, Modify);
    t[0xef] = d(O::Isc, Absolute, Modify);
    t[0xf0] = d(O::Branch(Cond::Eq), Relative, NoAccess);
    t[0xf1] = d(O::Alu8(Alu8::Sbc), IndirectY, Read);
    t[0xf3] = d(O::Isc, IndirectY, Modify);
    t[0xf4] = d(O::Inherent(Inherent::Nop), ZeroPageX, Read);
    t[0xf5] = d(O::Alu8(Alu8::Sbc), ZeroPageX, Read);
    t[0xf6] = d(O::Unary(Unary::IncMemory), ZeroPageX, Modify);
    t[0xf7] = d(O::Isc, ZeroPageX, Modify);
    t[0xf8] = d(O::Inherent(Inherent::Sed), Implied, NoAccess);
    t[0xf9] = d(O::Alu8(Alu8::Sbc), AbsoluteY, Read);
    t[0xfa] = d(O::Inherent(Inherent::Nop), Implied, NoAccess);
    t[0xfb] = d(O::Isc, AbsoluteY, Modify);
    t[0xfc] = d(O::Inherent(Inherent::Nop), AbsoluteX, Read);
    t[0xfd] = d(O::Alu8(Alu8::Sbc), AbsoluteX, Read);
    t[0xfe] = d(O::Unary(Unary::IncMemory), AbsoluteX, Modify);
    t[0xff] = d(O::Isc, AbsoluteX, Modify);

    t
};

/// The opcode's row of the table of all 256.
#[must_use]
pub const fn decode(opcode: u8) -> Decoded {
    TABLE[opcode as usize]
}

/// The twenty opcodes outside the manufacturer's table that this crate does not
/// spell `NOP` or `SBC` — used by [`documented`].
#[cfg(test)]
pub(crate) const OUTSIDE_THE_TABLE: [Kind; 20] = [
    Kind::Slo,
    Kind::Rla,
    Kind::Sre,
    Kind::Rra,
    Kind::Sax,
    Kind::Lax,
    Kind::Dcp,
    Kind::Isc,
    Kind::Anc,
    Kind::Alr,
    Kind::Arr,
    Kind::Sbx,
    Kind::Las,
    Kind::Sha,
    Kind::Shx,
    Kind::Shy,
    Kind::Tas,
    Kind::Undefined,
    Kind::Ane,
    Kind::Lxa,
];

/// How many of the 256 opcodes this table spells as the manufacturer's.
///
/// `$EA` is Appendix D's only `NOP` and `$E9` its only `SBC #Oper`, so every
/// other row carrying either mnemonic is one of the 105.
#[cfg(test)]
#[must_use]
pub(crate) fn documented() -> usize {
    (0usize..256)
        .filter(|&i| {
            let row = TABLE[i];
            !OUTSIDE_THE_TABLE.contains(&row.kind)
                && !(row.kind == Kind::Inherent(Inherent::Nop) && i != 0xea)
                && i != 0xeb
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `MOS6502-OPEN-01`: which opcodes stop the part is this crate's choice.
    #[test]
    fn the_twelve_this_crate_stops_on_are_the_twelve_the_pin_excludes() {
        let stops: std::vec::Vec<usize> = (0..256)
            .filter(|&i| TABLE[i].addressing == Addressing::Undefined)
            .collect();
        assert_eq!(
            stops,
            [
                0x02, 0x12, 0x22, 0x32, 0x42, 0x52, 0x62, 0x72, 0x92, 0xb2, 0xd2, 0xf2
            ]
        );
    }

    /// `MOS6502-033` and `MOS6502-034`: Appendix D prints 151 opcodes and marks
    /// the other 105 `Future Expansion`, and the 151 carry 56 mnemonics.
    #[test]
    fn one_hundred_and_fifty_one_opcodes_are_the_manufacturers_and_the_rest_are_not() {
        assert_eq!(documented(), 151);
        assert_eq!(256 - documented(), 105);
    }
}
