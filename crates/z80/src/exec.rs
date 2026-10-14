//! Decode, and the instruction bodies.
//!
//! An instruction is a short sequence of machine cycles. Decode turns an opcode
//! into a [`Kind`]; [`Z80::body`] is re-entered once per completed cycle, does
//! the register work that cycle's result allows, and names the next cycle. The
//! step number is where in the sequence the instruction stands, so the whole
//! instruction set is one `match` over `(kind, step)` and no instruction holds
//! state of its own beyond two operand latches.
//!
//! # Cycle counts
//!
//! `Z80-083`: the manual's per-instruction T-state list. It folds an
//! instruction's internal T-states into the machine cycle they follow, so
//! `PUSH qq` is `(5, 3, 3)` — a five-T opcode fetch and two writes. This module
//! emits those as the bus cycle plus a trailing [`BusOp::Internal`], because the
//! sequencer fixes a cycle's length when it starts and the opcode is not decoded
//! until the fetch has ended. **The T-state totals are identical**, and the
//! extra T-states fall at the cycle's end, after the bus access (`Z80-103`).
//! `BUSREQ` and `WAIT` are sampled per cycle, so
//! splitting one cycle in two also gives them one more sample point than the
//! manual's grouping does.
//!
//! # `WZ`
//!
//! What `WZ` holds after each family that writes it is `Z80-096` to
//! `Z80-102`. An instruction not named there leaves the register alone.

use crate::Z80;
use crate::alu;
use crate::regs::{
    FLAG_C, FLAG_PV, FLAG_S, FLAG_XF, FLAG_YF, FLAG_Z, InterruptMode, Pending, REG_MEM,
};
use crate::seq::BusOp;

/// Where the part stands in the instruction it is executing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum Stage {
    /// An opcode fetch is in flight; the byte it samples is the opcode. A `DD`
    /// or `FD` seen here sets the index and returns to this same stage, which is
    /// why a run of them costs one M1 each (`Z80-021`, `Z80-022`).
    Opcode,
    /// The opcode fetch after a `CB`. Its own M1, so `R` moves twice.
    CbOpcode,
    /// The displacement read of a `DD CB` or `FD CB` instruction.
    IndexedCbDisplacement,
    /// The opcode read of a `DD CB` or `FD CB` instruction.
    ///
    /// A read and not a fetch: four opcode bytes, two M1 cycles, and `R`
    /// incremented twice (`Z80-021`).
    IndexedCbOpcode,
    /// The two internal T-states that lengthen a `DD CB` opcode read to the five
    /// the manual gives it.
    IndexedCbSettle { kind: Kind },
    /// The five internal T-states between an indexed instruction's displacement
    /// read and its memory access.
    DisplacementSettle { kind: Kind },
    /// The opcode fetch after an `ED`.
    EdOpcode,
    /// The displacement read of an ordinary `DD` or `FD` instruction that names
    /// memory.
    Displacement { kind: Kind },
    /// `kind` is executing, and `step` of its machine cycles have completed.
    Body { kind: Kind, step: u8 },
}

/// Which index register a `DD` or `FD` prefix puts in `HL`'s place.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum Index {
    /// No prefix: `HL`, `H`, `L` and `(HL)` mean themselves.
    None,
    /// `DD`: `IX`, `IXH`, `IXL` and `(IX+d)`.
    Ix,
    /// `FD`: `IY`, `IYH`, `IYL` and `(IY+d)`.
    Iy,
}

/// What an opcode means, after the regularity of the encoding is taken out of it.
///
/// The Z80's opcode byte splits into `x` (bits 7-6), `y` (5-3) and `z` (2-0),
/// with `y` further splitting into `p` (bits 5-4) and `q` (bit 3); most of the
/// instruction set is one shape per `(x, z)` with `y` naming a register, a
/// condition or a vector. These variants are those shapes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum Kind {
    /// `NOP`.
    Nop,
    /// `LD r, r'`.
    LdRegReg { dst: u8, src: u8 },
    /// `LD r, n`.
    LdRegImm(u8),
    /// `LD r, (HL)`.
    LdRegMem(u8),
    /// `LD (HL), r`.
    LdMemReg(u8),
    /// `LD (HL), n`.
    LdMemImm,
    /// `LD dd, nn`.
    LdPairImm(u8),
    /// `LD (BC), A` and `LD (DE), A`.
    LdMemPairA(u8),
    /// `LD A, (BC)` and `LD A, (DE)`.
    LdAMemPair(u8),
    /// `LD (nn), HL`.
    LdNnHl,
    /// `LD HL, (nn)`.
    LdHlNn,
    /// `LD (nn), A`.
    LdNnA,
    /// `LD A, (nn)`.
    LdANn,
    /// `LD SP, HL`.
    LdSpHl,
    /// `INC ss`, which touches no flag.
    IncPair(u8),
    /// `DEC ss`, which touches no flag.
    DecPair(u8),
    /// `EX AF, AF'`.
    ExAf,
    /// `EXX`.
    Exx,
    /// `EX DE, HL`.
    ExDeHl,
    /// `EX (SP), HL`.
    ExSpHl,
    /// `PUSH qq`.
    Push(u8),
    /// `POP qq`.
    Pop(u8),
    /// `JP nn`.
    JpNn,
    /// `JP cc, nn`.
    JpCc(u8),
    /// `JP (HL)`.
    JpHl,
    /// `JR e`.
    JrD,
    /// `JR cc, e`, where `cc` is one of the first four conditions only.
    JrCc(u8),
    /// `DJNZ e`.
    Djnz,
    /// `CALL nn`.
    CallNn,
    /// `CALL cc, nn`.
    CallCc(u8),
    /// `RET`.
    Ret,
    /// `RET cc`.
    RetCc(u8),
    /// `RST p`.
    Rst(u8),
    /// `ADD`, `ADC`, `SUB`, `SBC`, `AND`, `XOR`, `OR` or `CP` against a register
    /// or `(HL)`, with `op` the operation's index in that order.
    Alu { op: u8, src: u8 },
    /// The same eight against an immediate byte.
    AluImm(u8),
    /// `INC r`, or `INC (HL)` when the field names memory.
    IncReg(u8),
    /// `DEC r`, or `DEC (HL)`.
    DecReg(u8),
    /// `ADD HL, ss`.
    AddHlPair(u8),
    /// `RLCA`, `RRCA`, `RLA` and `RRA`, by that index.
    RotateA(u8),
    /// `DAA`.
    Daa,
    /// `CPL`.
    Cpl,
    /// `SCF`.
    Scf,
    /// `CCF`.
    Ccf,
    /// `RLC`, `RRC`, `RL`, `RR`, `SLA`, `SRA`, the undocumented shift, or `SRL`,
    /// with `op` the operation's index in that order.
    Shift { op: u8, target: u8 },
    /// `BIT b, r` and its memory forms.
    Bit { bit: u8, target: u8 },
    /// `RES b, r`.
    Res { bit: u8, target: u8 },
    /// `SET b, r`.
    Set { bit: u8, target: u8 },
    /// `IN r, (C)`; a field of six is the form that sets flags and keeps
    /// nothing.
    InC(u8),
    /// `OUT (C), r`; a field of six is the form that outputs zero.
    OutC(u8),
    /// `ADC HL, ss`.
    AdcHlPair(u8),
    /// `SBC HL, ss`.
    SbcHlPair(u8),
    /// `LD (nn), dd`.
    LdNnPair(u8),
    /// `LD dd, (nn)`.
    LdPairNn(u8),
    /// `NEG`.
    Neg,
    /// `RETN`, and `RETI` with it: `Z80-069` says every `ED` return behaves
    /// alike.
    RetInterrupt,
    /// `IM 0`, `IM 1` or `IM 2`.
    SetInterruptMode(InterruptMode),
    /// `LD I, A` and `LD R, A`, by whether it is `R`.
    LdSpecialA(bool),
    /// `LD A, I` and `LD A, R`, by whether it is `R`.
    LdASpecial(bool),
    /// `RRD` and `RLD`, by whether it is `RLD`.
    NibbleRotate(bool),
    /// `LDI`, `LDD`, `LDIR` and `LDDR`.
    BlockTransfer { up: bool, repeat: bool },
    /// `CPI`, `CPD`, `CPIR` and `CPDR`.
    BlockCompare { up: bool, repeat: bool },
    /// `INI`, `IND`, `INIR` and `INDR`.
    BlockIn { up: bool, repeat: bool },
    /// `OUTI`, `OUTD`, `OTIR` and `OTDR`.
    BlockOut { up: bool, repeat: bool },
    /// `IN A, (n)`, which sets no flag at all.
    InAImm,
    /// `OUT (n), A`.
    OutImmA,
    /// `HALT`.
    Halt,
    /// `DI`.
    Di,
    /// `EI`.
    Ei,
    /// Accepting a non-maskable interrupt (`Z80-067`, `Z80-068`).
    AcceptNmi,
    /// Accepting a maskable interrupt (`Z80-071` to `Z80-075`).
    AcceptInt,
    /// An opcode with no operation behind it.
    ///
    /// Executed as a four-T-state opcode fetch and nothing else. The 178
    /// undefined `ED` opcodes are the only ones that reach it, and [`decode_ed`]
    /// is where what they cost is read. The four prefix bytes decode to it in
    /// the unprefixed table and never arrive, the step taking them before the
    /// decode.
    Undefined,
}

/// Reads an opcode as one of the shapes in [`Kind`].
///
/// The `(x, z)` split is the encoding's own: `x` chooses the broad family, `z`
/// the operation within it, and `y` — or `p` and `q`, its halves — the operands.
pub(crate) const fn decode(op: u8) -> Kind {
    let x = op >> 6;
    let y = (op >> 3) & 7;
    let z = op & 7;
    let p = y >> 1;
    let q = y & 1;

    match (x, z) {
        (0, 0) => match y {
            0 => Kind::Nop,
            1 => Kind::ExAf,
            2 => Kind::Djnz,
            3 => Kind::JrD,
            _ => Kind::JrCc(y - 4),
        },
        (0, 1) if q == 0 => Kind::LdPairImm(p),
        (0, 1) => Kind::AddHlPair(p),
        (0, 2) => match (q, p) {
            (0, 0 | 1) => Kind::LdMemPairA(p),
            (0, 2) => Kind::LdNnHl,
            (0, _) => Kind::LdNnA,
            (_, 0 | 1) => Kind::LdAMemPair(p),
            (_, 2) => Kind::LdHlNn,
            _ => Kind::LdANn,
        },
        (0, 3) if q == 0 => Kind::IncPair(p),
        (0, 3) => Kind::DecPair(p),
        (0, 4) => Kind::IncReg(y),
        (0, 5) => Kind::DecReg(y),
        (0, 6) if y == REG_MEM => Kind::LdMemImm,
        (0, 6) => Kind::LdRegImm(y),
        (0, _) => match y {
            0..=3 => Kind::RotateA(y),
            4 => Kind::Daa,
            5 => Kind::Cpl,
            6 => Kind::Scf,
            _ => Kind::Ccf,
        },
        (1, _) if y == REG_MEM && z == REG_MEM => Kind::Halt,
        (1, _) if z == REG_MEM => Kind::LdRegMem(y),
        (1, _) if y == REG_MEM => Kind::LdMemReg(z),
        (1, _) => Kind::LdRegReg { dst: y, src: z },
        (2, _) => Kind::Alu { op: y, src: z },
        (3, 0) => Kind::RetCc(y),
        (3, 1) if q == 0 => Kind::Pop(p),
        (3, 1) => match p {
            0 => Kind::Ret,
            1 => Kind::Exx,
            2 => Kind::JpHl,
            _ => Kind::LdSpHl,
        },
        (3, 2) => Kind::JpCc(y),
        (3, 3) => match y {
            0 => Kind::JpNn,
            2 => Kind::OutImmA,
            3 => Kind::InAImm,
            4 => Kind::ExSpHl,
            5 => Kind::ExDeHl,
            6 => Kind::Di,
            7 => Kind::Ei,
            _ => Kind::Undefined,
        },
        (3, 4) => Kind::CallCc(y),
        (3, 5) if q == 0 => Kind::Push(p),
        (3, 5) if p == 0 => Kind::CallNn,
        (3, 6) => Kind::AluImm(y),
        (3, 7) => Kind::Rst(y),
        _ => Kind::Undefined,
    }
}

/// Reads an `ED`-prefixed opcode.
///
/// An opcode this does not name falls through to [`Kind::Undefined`], which costs
/// 8 T (4, 4) and does nothing — exactly what Young records for the unlisted
/// `ED` opcodes: "no effect (same behaviour as 2 NOP instructions)".
pub(crate) const fn decode_ed(op: u8) -> Kind {
    let x = op >> 6;
    let y = (op >> 3) & 7;
    let z = op & 7;
    let p = y >> 1;
    let q = y & 1;

    match (x, z) {
        (1, 0) => Kind::InC(y),
        (1, 1) => Kind::OutC(y),
        (1, 2) if q == 0 => Kind::SbcHlPair(p),
        (1, 2) => Kind::AdcHlPair(p),
        (1, 3) if q == 0 => Kind::LdNnPair(p),
        (1, 3) => Kind::LdPairNn(p),
        (1, 4) => Kind::Neg,
        (1, 5) => Kind::RetInterrupt,
        (1, 6) => Kind::SetInterruptMode(match y & 3 {
            2 => InterruptMode::Im1,
            3 => InterruptMode::Im2,
            _ => InterruptMode::Im0,
        }),
        (1, _) => match y {
            0 | 1 => Kind::LdSpecialA(y == 1),
            2 | 3 => Kind::LdASpecial(y == 3),
            4 | 5 => Kind::NibbleRotate(y == 5),
            _ => Kind::Undefined,
        },
        (2, 0..=3) if y >= 4 => {
            let up = y & 1 == 0;
            let repeat = y >= 6;
            match z {
                0 => Kind::BlockTransfer { up, repeat },
                1 => Kind::BlockCompare { up, repeat },
                2 => Kind::BlockIn { up, repeat },
                _ => Kind::BlockOut { up, repeat },
            }
        }
        _ => Kind::Undefined,
    }
}

/// Reads a `CB`-prefixed opcode. Its `x` field chooses among the four families
/// and `z` names the register, `(HL)` at six.
pub(crate) const fn decode_cb(op: u8) -> Kind {
    let y = (op >> 3) & 7;
    let z = op & 7;
    match op >> 6 {
        0 => Kind::Shift { op: y, target: z },
        1 => Kind::Bit { bit: y, target: z },
        2 => Kind::Res { bit: y, target: z },
        _ => Kind::Set { bit: y, target: z },
    }
}

impl Kind {
    /// The register field a `CB` instruction's low three bits name, if this is
    /// one.
    const fn cb_target(self) -> Option<u8> {
        match self {
            Self::Shift { target, .. }
            | Self::Bit { target, .. }
            | Self::Res { target, .. }
            | Self::Set { target, .. } => Some(target),
            _ => None,
        }
    }

    /// Whether the instruction names `(HL)`, and so needs a displacement when a
    /// `DD` or `FD` prefix is in force.
    const fn names_memory(self) -> bool {
        match self {
            Self::LdRegMem(_) | Self::LdMemReg(_) | Self::LdMemImm => true,
            Self::IncReg(target) | Self::DecReg(target) => target == REG_MEM,
            Self::Alu { src, .. } => src == REG_MEM,
            _ => false,
        }
    }
}

/// Whether condition `cc` holds, in the encoding the manual's condition table
/// gives: `NZ Z NC C PO PE P M`.
///
/// The high two bits choose the flag and the low bit whether it must be set.
const fn condition(f: u8, cc: u8) -> bool {
    let flag = match cc >> 1 {
        0 => FLAG_Z,
        1 => FLAG_C,
        2 => FLAG_PV,
        _ => FLAG_S,
    };
    (f & flag != 0) == (cc & 1 != 0)
}

/// The two bytes [`Kind::encode`] writes: which table decoded it, and the byte.
pub(crate) const KIND_BYTES: usize = 2;

/// The four bytes [`Stage::encode`] writes.
pub(crate) const STAGE_BYTES: usize = KIND_BYTES + 2;

/// The table id [`Kind::encode`] writes when it finds no preimage.
///
/// [`Kind::decode`] rejects it, so a state image can never carry a silently
/// wrong instruction. `every_kind_round_trips` asserts the value never occurs.
const KIND_UNREPRESENTABLE: u8 = 0xFF;

impl Kind {
    /// The instruction as bytes, for `Z80::save_state`.
    ///
    /// Every `Kind` a [`Stage`] can hold came out of [`decode`], [`decode_cb`]
    /// or [`decode_ed`], or is one of the two the interrupt boundary builds, so
    /// searching those tables for a preimage is total. Searching rather than
    /// matching keeps the encoding from having to be revised variant by variant
    /// as the decode tables grow.
    pub(crate) fn encode(self) -> [u8; KIND_BYTES] {
        match self {
            Self::AcceptNmi => return [3, 0],
            Self::AcceptInt => return [4, 0],
            _ => {}
        }
        for byte in 0..=u8::MAX {
            if decode(byte) == self {
                return [0, byte];
            }
        }
        for byte in 0..=u8::MAX {
            if decode_cb(byte) == self {
                return [1, byte];
            }
        }
        for byte in 0..=u8::MAX {
            if decode_ed(byte) == self {
                return [2, byte];
            }
        }
        [KIND_UNREPRESENTABLE, 0]
    }

    /// The inverse of [`Kind::encode`]; `None` for a table id no version wrote.
    pub(crate) const fn decode(bytes: [u8; KIND_BYTES]) -> Option<Self> {
        let [table, byte] = bytes;
        Some(match table {
            0 => decode(byte),
            1 => decode_cb(byte),
            2 => decode_ed(byte),
            3 => Self::AcceptNmi,
            4 => Self::AcceptInt,
            _ => return None,
        })
    }
}

impl Index {
    /// The prefix in force as one byte.
    pub(crate) const fn encode(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Ix => 1,
            Self::Iy => 2,
        }
    }

    /// The inverse of [`Index::encode`].
    pub(crate) const fn decode(byte: u8) -> Option<Self> {
        Some(match byte {
            0 => Self::None,
            1 => Self::Ix,
            2 => Self::Iy,
            _ => return None,
        })
    }
}

impl Stage {
    /// Where the part stands in its instruction, as bytes.
    pub(crate) fn encode(self) -> [u8; STAGE_BYTES] {
        let (tag, kind, step) = match self {
            Self::Opcode => (0, None, 0),
            Self::CbOpcode => (1, None, 0),
            Self::IndexedCbDisplacement => (2, None, 0),
            Self::IndexedCbOpcode => (3, None, 0),
            Self::EdOpcode => (4, None, 0),
            Self::IndexedCbSettle { kind } => (5, Some(kind), 0),
            Self::DisplacementSettle { kind } => (6, Some(kind), 0),
            Self::Displacement { kind } => (7, Some(kind), 0),
            Self::Body { kind, step } => (8, Some(kind), step),
        };
        let [table, byte] = match kind {
            Some(kind) => kind.encode(),
            None => [0, 0],
        };
        [tag, table, byte, step]
    }

    /// The inverse of [`Stage::encode`].
    pub(crate) const fn decode(bytes: [u8; STAGE_BYTES]) -> Option<Self> {
        let [tag, table, byte, step] = bytes;
        let kind = match tag {
            0..=4 => Kind::Nop,
            _ => match Kind::decode([table, byte]) {
                Some(kind) => kind,
                None => return None,
            },
        };
        Some(match tag {
            0 => Self::Opcode,
            1 => Self::CbOpcode,
            2 => Self::IndexedCbDisplacement,
            3 => Self::IndexedCbOpcode,
            4 => Self::EdOpcode,
            5 => Self::IndexedCbSettle { kind },
            6 => Self::DisplacementSettle { kind },
            7 => Self::Displacement { kind },
            8 => Self::Body { kind, step },
            _ => return None,
        })
    }
}

impl Z80 {
    /// Decides what the part does next, now that a machine cycle has ended and
    /// `byte` is whatever it sampled.
    pub(crate) fn next_cycle(&mut self, byte: u8) -> BusOp {
        match self.stage {
            Stage::Opcode => {
                self.consume_fetch();
                if self.regs.halted {
                    return self.body(Kind::Halt, 0, 0);
                }
                match byte {
                    0xDD | 0xFD => {
                        self.index = if byte == 0xDD { Index::Ix } else { Index::Iy };
                        self.opcode_fetch()
                    }
                    0xCB if self.index == Index::None => {
                        self.stage = Stage::CbOpcode;
                        self.opcode_fetch()
                    }
                    0xCB => {
                        self.stage = Stage::IndexedCbDisplacement;
                        self.read_immediate()
                    }
                    0xED => {
                        self.index = Index::None;
                        self.stage = Stage::EdOpcode;
                        self.opcode_fetch()
                    }
                    _ => self.begin(decode(byte)),
                }
            }
            Stage::CbOpcode => {
                self.consume_fetch();
                self.body(decode_cb(byte), 0, byte)
            }
            Stage::EdOpcode => {
                self.consume_fetch();
                self.body(decode_ed(byte), 0, byte)
            }
            Stage::IndexedCbDisplacement => {
                self.set_indexed_address(byte);
                self.stage = Stage::IndexedCbOpcode;
                self.read_immediate()
            }
            Stage::IndexedCbOpcode => {
                self.stage = Stage::IndexedCbSettle {
                    kind: decode_cb(byte),
                };
                BusOp::Internal(2)
            }
            Stage::IndexedCbSettle { kind } => self.body(kind, 0, byte),
            Stage::Displacement { kind } => {
                self.set_indexed_address(byte);
                if matches!(kind, Kind::LdMemImm) {
                    self.body(kind, 0, byte)
                } else {
                    self.stage = Stage::DisplacementSettle { kind };
                    BusOp::Internal(5)
                }
            }
            Stage::DisplacementSettle { kind } => self.body(kind, 0, byte),
            Stage::Body { kind, step } => self.body(kind, step, byte),
        }
    }

    /// Accounts for an opcode fetch that has just ended.
    ///
    /// `PC` moves past the byte it placed on the bus and `R` past the value it
    /// drove as the refresh address (`Z80-019`, `Z80-029`). `LD A,R` therefore
    /// sees `R` after its own fetch's increment, which is `Z80-028`.
    fn consume_fetch(&mut self) {
        self.regs.pc = self.regs.pc.wrapping_add(1);
        self.regs.inc_r();
        self.q_previous = core::mem::take(&mut self.regs.q);
    }

    /// Starts `kind`, first reading a displacement if a prefix is in force and
    /// the instruction names memory.
    fn begin(&mut self, kind: Kind) -> BusOp {
        if self.index != Index::None && kind.names_memory() {
            self.stage = Stage::Displacement { kind };
            self.read_immediate()
        } else {
            self.body(kind, 0, 0)
        }
    }

    /// Latches `IX+d` or `IY+d` from a displacement byte.
    fn set_indexed_address(&mut self, displacement: u8) {
        #[expect(
            clippy::cast_possible_wrap,
            reason = "the displacement is two's complement"
        )]
        let offset = displacement as i8;
        let base = match self.index {
            Index::Ix => self.regs.ix,
            Index::Iy => self.regs.iy,
            Index::None => self.regs.hl(),
        };
        self.indexed = base.wrapping_add_signed(i16::from(offset));
        self.regs.wz = self.indexed;
    }

    /// Runs `kind` from `step`, with `byte` the result of the cycle just ended.
    ///
    /// A `DD` or `FD` prefix reaches this through the substituting accessors —
    /// [`Z80::reg_field`], [`Z80::hl_value`], [`Z80::memory_address`] — rather
    /// than through arms of its own, which is the whole reason a prefix costs
    /// only its own four T-states on top of the instruction it modifies.
    #[expect(
        clippy::too_many_lines,
        reason = "one arm per instruction shape; splitting it would only move the \
                  match somewhere else and break the reading against the manual's \
                  T-state lists"
    )]
    fn body(&mut self, kind: Kind, step: u8, byte: u8) -> BusOp {
        match (kind, step) {
            (Kind::Nop | Kind::Undefined, _) => self.finish(),

            (Kind::LdRegReg { dst, src }, _) => {
                let value = self.reg_field(src, true);
                self.set_reg_field(dst, true, value);
                self.finish()
            }

            (Kind::LdRegImm(_), 0) => {
                let op = self.read_immediate();
                self.next(kind, op)
            }
            (Kind::LdRegImm(dst), _) => {
                self.set_reg_field(dst, true, byte);
                self.finish()
            }

            (Kind::LdRegMem(_), 0) => self.next(kind, BusOp::MemRead(self.memory_address())),
            (Kind::LdRegMem(dst), _) => {
                self.regs.set_reg(dst, byte);
                self.finish()
            }

            (Kind::LdMemReg(src), 0) => {
                let value = self.regs.reg(src);
                self.next(kind, BusOp::MemWrite(self.memory_address(), value))
            }
            (Kind::LdMemReg(_), _) => self.finish(),

            (Kind::LdMemImm, 0) => {
                let op = self.read_immediate();
                self.next(kind, op)
            }
            (Kind::LdMemImm, 1) => {
                self.lo = byte;
                if self.index == Index::None {
                    self.next(kind, BusOp::MemWrite(self.regs.hl(), byte))
                } else {
                    self.next(kind, BusOp::Internal(2))
                }
            }
            (Kind::LdMemImm, 2) if self.index != Index::None => {
                self.next(kind, BusOp::MemWrite(self.indexed, self.lo))
            }
            (Kind::LdMemImm, _) => self.finish(),

            (Kind::LdPairImm(_), 0) => {
                let op = self.read_immediate();
                self.next(kind, op)
            }
            (Kind::LdPairImm(_), 1) => {
                self.lo = byte;
                let op = self.read_immediate();
                self.next(kind, op)
            }
            (Kind::LdPairImm(pair), _) => {
                self.set_pair_field(pair, u16::from_le_bytes([self.lo, byte]));
                self.finish()
            }

            (Kind::LdMemPairA(pair), 0) => {
                let address = self.regs.pair(pair);
                self.write_wz_split(address);
                self.next(kind, BusOp::MemWrite(address, self.regs.a))
            }
            (Kind::LdMemPairA(_), _) => self.finish(),

            (Kind::LdAMemPair(pair), 0) => {
                let address = self.regs.pair(pair);
                self.regs.wz = address.wrapping_add(1);
                self.next(kind, BusOp::MemRead(address))
            }
            (Kind::LdAMemPair(_), _) => {
                self.regs.a = byte;
                self.finish()
            }

            (Kind::LdNnA | Kind::LdANn | Kind::LdNnHl | Kind::LdHlNn, 0) => {
                let op = self.read_immediate();
                self.next(kind, op)
            }
            (Kind::LdNnA | Kind::LdANn | Kind::LdNnHl | Kind::LdHlNn, 1) => {
                self.lo = byte;
                let op = self.read_immediate();
                self.next(kind, op)
            }
            (Kind::LdNnA, 2) => {
                self.hi = byte;
                let address = self.operand_address();
                self.write_wz_split(address);
                self.next(kind, BusOp::MemWrite(address, self.regs.a))
            }
            (Kind::LdNnA, _) => self.finish(),
            (Kind::LdANn, 2) => {
                self.hi = byte;
                let address = self.operand_address();
                self.regs.wz = address.wrapping_add(1);
                self.next(kind, BusOp::MemRead(address))
            }
            (Kind::LdANn, _) => {
                self.regs.a = byte;
                self.finish()
            }
            (Kind::LdNnHl, 2) => {
                self.hi = byte;
                let address = self.operand_address();
                self.regs.wz = address.wrapping_add(1);
                let low = self.hl_value() as u8;
                self.next(kind, BusOp::MemWrite(address, low))
            }
            (Kind::LdNnHl, 3) => {
                let address = self.operand_address().wrapping_add(1);
                let high = (self.hl_value() >> 8) as u8;
                self.next(kind, BusOp::MemWrite(address, high))
            }
            (Kind::LdNnHl, _) => self.finish(),
            (Kind::LdHlNn, 2) => {
                self.hi = byte;
                self.next(kind, BusOp::MemRead(self.operand_address()))
            }
            (Kind::LdHlNn, 3) => {
                let address = self.operand_address().wrapping_add(1);
                self.regs.wz = address;
                self.lo = byte;
                self.next(kind, BusOp::MemRead(address))
            }
            (Kind::LdHlNn, _) => {
                self.set_hl_value(u16::from_le_bytes([self.lo, byte]));
                self.finish()
            }

            (Kind::LdSpHl, 0) => {
                self.regs.sp = self.hl_value();
                self.next(kind, BusOp::Internal(2))
            }
            (Kind::IncPair(pair), 0) => {
                self.set_pair_field(pair, self.pair_field(pair).wrapping_add(1));
                self.next(kind, BusOp::Internal(2))
            }
            (Kind::DecPair(pair), 0) => {
                self.set_pair_field(pair, self.pair_field(pair).wrapping_sub(1));
                self.next(kind, BusOp::Internal(2))
            }
            (Kind::LdSpHl | Kind::IncPair(_) | Kind::DecPair(_), _) => self.finish(),

            (Kind::ExAf, _) => {
                self.regs.ex_af();
                self.finish()
            }
            (Kind::Exx, _) => {
                self.regs.exx();
                self.finish()
            }
            (Kind::ExDeHl, _) => {
                let de = self.regs.de();
                self.regs.set_de(self.regs.hl());
                self.regs.set_hl(de);
                self.finish()
            }

            (Kind::ExSpHl, 0) => self.next(kind, BusOp::MemRead(self.regs.sp)),
            (Kind::ExSpHl, 1) => {
                self.lo = byte;
                let address = self.regs.sp.wrapping_add(1);
                self.next(kind, BusOp::MemRead(address))
            }
            (Kind::ExSpHl, 2) => {
                self.hi = byte;
                self.next(kind, BusOp::Internal(1))
            }
            (Kind::ExSpHl, 3) => {
                let address = self.regs.sp.wrapping_add(1);
                let high = (self.hl_value() >> 8) as u8;
                self.next(kind, BusOp::MemWrite(address, high))
            }
            (Kind::ExSpHl, 4) => {
                let low = self.hl_value() as u8;
                self.next(kind, BusOp::MemWrite(self.regs.sp, low))
            }
            (Kind::ExSpHl, 5) => self.next(kind, BusOp::Internal(2)),
            (Kind::ExSpHl, _) => {
                let value = u16::from_le_bytes([self.lo, self.hi]);
                self.regs.wz = value;
                self.set_hl_value(value);
                self.finish()
            }

            (Kind::Push(_), 0) => self.next(kind, BusOp::Internal(1)),
            (Kind::Push(pair), 1) => {
                let value = self.stack_pair_field(pair);
                let op = self.push_byte((value >> 8) as u8);
                self.next(kind, op)
            }
            (Kind::Push(pair), 2) => {
                let value = self.stack_pair_field(pair);
                let op = self.push_byte(value as u8);
                self.next(kind, op)
            }
            (Kind::Push(_), _) => self.finish(),

            (Kind::Pop(_), 0) => {
                let op = self.pop_byte();
                self.next(kind, op)
            }
            (Kind::Pop(_), 1) => {
                self.lo = byte;
                let op = self.pop_byte();
                self.next(kind, op)
            }
            (Kind::Pop(pair), _) => {
                self.set_stack_pair_field(pair, u16::from_le_bytes([self.lo, byte]));
                self.finish()
            }

            (Kind::JpNn | Kind::JpCc(_), 0) => {
                let op = self.read_immediate();
                self.next(kind, op)
            }
            (Kind::JpNn | Kind::JpCc(_), 1) => {
                self.lo = byte;
                let op = self.read_immediate();
                self.next(kind, op)
            }
            (Kind::JpNn, _) => {
                let target = u16::from_le_bytes([self.lo, byte]);
                self.regs.wz = target;
                self.regs.pc = target;
                self.finish()
            }
            (Kind::JpCc(cc), _) => {
                let target = u16::from_le_bytes([self.lo, byte]);
                self.regs.wz = target;
                if condition(self.regs.f, cc) {
                    self.regs.pc = target;
                }
                self.finish()
            }

            (Kind::JpHl, _) => {
                self.regs.pc = self.hl_value();
                self.finish()
            }

            (Kind::JrD | Kind::JrCc(_), 0) => {
                let op = self.read_immediate();
                self.next(kind, op)
            }
            (Kind::JrD, 1) => {
                self.lo = byte;
                self.next(kind, BusOp::Internal(5))
            }
            (Kind::JrCc(cc), 1) => {
                self.lo = byte;
                if condition(self.regs.f, cc) {
                    self.next(kind, BusOp::Internal(5))
                } else {
                    self.finish()
                }
            }
            (Kind::JrD | Kind::JrCc(_), _) => {
                self.jump_relative();
                self.finish()
            }

            (Kind::Djnz, 0) => {
                self.regs.b = self.regs.b.wrapping_sub(1);
                self.next(kind, BusOp::Internal(1))
            }
            (Kind::Djnz, 1) => {
                let op = self.read_immediate();
                self.next(kind, op)
            }
            (Kind::Djnz, 2) => {
                self.lo = byte;
                if self.regs.b == 0 {
                    self.finish()
                } else {
                    self.next(kind, BusOp::Internal(5))
                }
            }
            (Kind::Djnz, _) => {
                self.jump_relative();
                self.finish()
            }

            (Kind::CallNn | Kind::CallCc(_), 0) => {
                let op = self.read_immediate();
                self.next(kind, op)
            }
            (Kind::CallNn | Kind::CallCc(_), 1) => {
                self.lo = byte;
                let op = self.read_immediate();
                self.next(kind, op)
            }
            (Kind::CallNn, 2) => {
                self.hi = byte;
                self.regs.wz = self.operand_address();
                self.next(kind, BusOp::Internal(1))
            }
            (Kind::CallCc(cc), 2) => {
                self.hi = byte;
                self.regs.wz = self.operand_address();
                if condition(self.regs.f, cc) {
                    self.next(kind, BusOp::Internal(1))
                } else {
                    self.finish()
                }
            }
            (Kind::CallNn | Kind::CallCc(_), 3) => {
                let op = self.push_byte((self.regs.pc >> 8) as u8);
                self.next(kind, op)
            }
            (Kind::CallNn | Kind::CallCc(_), 4) => {
                let op = self.push_byte(self.regs.pc as u8);
                self.next(kind, op)
            }
            (Kind::CallNn | Kind::CallCc(_), _) => {
                self.regs.pc = self.operand_address();
                self.finish()
            }

            (Kind::Ret, 0) => {
                let op = self.pop_byte();
                self.next(kind, op)
            }
            (Kind::RetCc(_), 0) => self.next(kind, BusOp::Internal(1)),
            (Kind::RetCc(cc), 1) => {
                if condition(self.regs.f, cc) {
                    let op = self.pop_byte();
                    self.next(kind, op)
                } else {
                    self.finish()
                }
            }
            (Kind::Ret, 1) | (Kind::RetCc(_), 2) => {
                self.lo = byte;
                let op = self.pop_byte();
                self.next(kind, op)
            }
            (Kind::Ret | Kind::RetCc(_), _) => {
                let target = u16::from_le_bytes([self.lo, byte]);
                self.regs.wz = target;
                self.regs.pc = target;
                self.finish()
            }

            (Kind::Alu { src, .. }, 0) if src == REG_MEM => {
                self.next(kind, BusOp::MemRead(self.memory_address()))
            }
            (Kind::AluImm(_), 0) => {
                let op = self.read_immediate();
                self.next(kind, op)
            }
            (Kind::Alu { op, src }, step) => {
                let operand = if step == 0 {
                    self.reg_field(src, true)
                } else {
                    byte
                };
                self.apply_alu(op, operand);
                self.finish()
            }
            (Kind::AluImm(op), _) => {
                self.apply_alu(op, byte);
                self.finish()
            }

            (Kind::IncReg(target) | Kind::DecReg(target), 0) if target == REG_MEM => {
                self.next(kind, BusOp::MemRead(self.memory_address()))
            }
            (Kind::IncReg(target), 0) => {
                let (result, flags) = alu::inc(self.reg_field(target, true), self.regs.f);
                self.set_reg_field(target, true, result);
                self.regs.set_flags(flags);
                self.finish()
            }
            (Kind::DecReg(target), 0) => {
                let (result, flags) = alu::dec(self.reg_field(target, true), self.regs.f);
                self.set_reg_field(target, true, result);
                self.regs.set_flags(flags);
                self.finish()
            }
            (Kind::IncReg(_) | Kind::DecReg(_), 1) => {
                let (result, flags) = if matches!(kind, Kind::IncReg(_)) {
                    alu::inc(byte, self.regs.f)
                } else {
                    alu::dec(byte, self.regs.f)
                };
                self.lo = result;
                self.regs.set_flags(flags);
                self.next(kind, BusOp::Internal(1))
            }
            (Kind::IncReg(_) | Kind::DecReg(_), 2) => {
                self.next(kind, BusOp::MemWrite(self.memory_address(), self.lo))
            }
            (Kind::IncReg(_) | Kind::DecReg(_), _) => self.finish(),

            (Kind::AddHlPair(pair), 0) => {
                self.regs.wz = self.hl_value().wrapping_add(1);
                let (result, flags) =
                    alu::add16(self.hl_value(), self.pair_field(pair), self.regs.f);
                self.set_hl_value(result);
                self.regs.set_flags(flags);
                self.next(kind, BusOp::Internal(4))
            }
            (Kind::AddHlPair(_), 1) => self.next(kind, BusOp::Internal(3)),
            (Kind::AddHlPair(_), _) => self.finish(),

            (Kind::RotateA(which), _) => {
                let (result, flags) = match which {
                    0 => alu::rlca(self.regs.a, self.regs.f),
                    1 => alu::rrca(self.regs.a, self.regs.f),
                    2 => alu::rla(self.regs.a, self.regs.f),
                    _ => alu::rra(self.regs.a, self.regs.f),
                };
                self.regs.a = result;
                self.regs.set_flags(flags);
                self.finish()
            }
            (Kind::Daa, _) => {
                let (result, flags) = alu::daa(self.regs.a, self.regs.f);
                self.regs.a = result;
                self.regs.set_flags(flags);
                self.finish()
            }
            (Kind::Cpl, _) => {
                let (result, flags) = alu::cpl(self.regs.a, self.regs.f);
                self.regs.a = result;
                self.regs.set_flags(flags);
                self.finish()
            }
            (Kind::Scf, _) => {
                self.regs
                    .set_flags(alu::scf(self.regs.a, self.regs.f, self.q_previous));
                self.finish()
            }
            (Kind::Ccf, _) => {
                self.regs
                    .set_flags(alu::ccf(self.regs.a, self.regs.f, self.q_previous));
                self.finish()
            }

            (Kind::Shift { .. } | Kind::Bit { .. } | Kind::Res { .. } | Kind::Set { .. }, 0)
                if self.cb_reaches_memory(kind) =>
            {
                self.next(kind, BusOp::MemRead(self.memory_address()))
            }
            (Kind::Shift { op, target }, 0) => {
                let (result, flags) = alu::shift(op, self.regs.reg(target), self.regs.f);
                self.regs.set_reg(target, result);
                self.regs.set_flags(flags);
                self.finish()
            }
            (Kind::Bit { bit, target }, 0) => {
                self.regs
                    .set_flags(alu::bit(bit, self.regs.reg(target), self.regs.f));
                self.finish()
            }
            (Kind::Res { bit, target }, 0) => {
                self.regs
                    .set_reg(target, self.regs.reg(target) & !(1 << bit));
                self.finish()
            }
            (Kind::Set { bit, target }, 0) => {
                self.regs
                    .set_reg(target, self.regs.reg(target) | (1 << bit));
                self.finish()
            }
            (Kind::Bit { bit, .. }, 1) => {
                let source = self.bit_source();
                self.regs
                    .set_flags(alu::bit_from(bit, byte, self.regs.f, source));
                self.next(kind, BusOp::Internal(1))
            }
            (Kind::Shift { op, .. }, 1) => {
                let (result, flags) = alu::shift(op, byte, self.regs.f);
                self.lo = result;
                self.regs.set_flags(flags);
                self.next(kind, BusOp::Internal(1))
            }
            (Kind::Res { bit, .. }, 1) => {
                self.lo = byte & !(1 << bit);
                self.next(kind, BusOp::Internal(1))
            }
            (Kind::Set { bit, .. }, 1) => {
                self.lo = byte | (1 << bit);
                self.next(kind, BusOp::Internal(1))
            }
            (Kind::Bit { .. }, _) => self.finish(),
            (
                Kind::Shift { target, .. } | Kind::Res { target, .. } | Kind::Set { target, .. },
                2,
            ) => {
                if self.index != Index::None && target != REG_MEM {
                    self.regs.set_reg(target, self.lo);
                }
                self.next(kind, BusOp::MemWrite(self.memory_address(), self.lo))
            }
            (Kind::Shift { .. } | Kind::Res { .. } | Kind::Set { .. }, _) => self.finish(),

            (Kind::InC(_), 0) => {
                let port = self.regs.bc();
                self.regs.wz = port.wrapping_add(1);
                self.next(kind, BusOp::IoRead(port))
            }
            (Kind::InC(target), _) => {
                self.regs.set_flags(alu::in_flags(byte, self.regs.f));
                if target != REG_MEM {
                    self.regs.set_reg(target, byte);
                }
                self.finish()
            }
            (Kind::OutC(source), 0) => {
                let value = if source == REG_MEM {
                    0
                } else {
                    self.regs.reg(source)
                };
                let port = self.regs.bc();
                self.regs.wz = port.wrapping_add(1);
                self.next(kind, BusOp::IoWrite(port, value))
            }
            (Kind::OutC(_), _) => self.finish(),

            (Kind::AdcHlPair(pair) | Kind::SbcHlPair(pair), 0) => {
                self.regs.wz = self.regs.hl().wrapping_add(1);
                let (result, flags) = if matches!(kind, Kind::AdcHlPair(_)) {
                    alu::adc16(self.regs.hl(), self.regs.pair(pair), self.regs.f)
                } else {
                    alu::sbc16(self.regs.hl(), self.regs.pair(pair), self.regs.f)
                };
                self.regs.set_hl(result);
                self.regs.set_flags(flags);
                self.next(kind, BusOp::Internal(4))
            }
            (Kind::AdcHlPair(_) | Kind::SbcHlPair(_), 1) => self.next(kind, BusOp::Internal(3)),
            (Kind::AdcHlPair(_) | Kind::SbcHlPair(_), _) => self.finish(),

            (Kind::LdNnPair(_) | Kind::LdPairNn(_), 0) => {
                let op = self.read_immediate();
                self.next(kind, op)
            }
            (Kind::LdNnPair(_) | Kind::LdPairNn(_), 1) => {
                self.lo = byte;
                let op = self.read_immediate();
                self.next(kind, op)
            }
            (Kind::LdNnPair(pair), 2) => {
                self.hi = byte;
                let address = self.operand_address();
                self.regs.wz = address.wrapping_add(1);
                let low = self.regs.pair(pair) as u8;
                self.next(kind, BusOp::MemWrite(address, low))
            }
            (Kind::LdNnPair(pair), 3) => {
                let address = self.operand_address().wrapping_add(1);
                let high = (self.regs.pair(pair) >> 8) as u8;
                self.next(kind, BusOp::MemWrite(address, high))
            }
            (Kind::LdNnPair(_), _) => self.finish(),
            (Kind::LdPairNn(_), 2) => {
                self.hi = byte;
                self.next(kind, BusOp::MemRead(self.operand_address()))
            }
            (Kind::LdPairNn(_), 3) => {
                let address = self.operand_address().wrapping_add(1);
                self.regs.wz = address;
                self.lo = byte;
                self.next(kind, BusOp::MemRead(address))
            }
            (Kind::LdPairNn(pair), _) => {
                self.regs
                    .set_pair(pair, u16::from_le_bytes([self.lo, byte]));
                self.finish()
            }

            (Kind::Neg, _) => {
                let (result, flags) = alu::neg(self.regs.a);
                self.regs.a = result;
                self.regs.set_flags(flags);
                self.finish()
            }
            (Kind::SetInterruptMode(mode), _) => {
                self.regs.im = mode;
                self.finish()
            }

            (Kind::RetInterrupt, 0) => {
                let op = self.pop_byte();
                self.next(kind, op)
            }
            (Kind::RetInterrupt, 1) => {
                self.lo = byte;
                let op = self.pop_byte();
                self.next(kind, op)
            }
            (Kind::RetInterrupt, _) => {
                let target = u16::from_le_bytes([self.lo, byte]);
                self.regs.wz = target;
                self.regs.pc = target;
                self.regs.iff1 = self.regs.iff2;
                self.finish()
            }

            (Kind::LdSpecialA(is_r), 0) => {
                if is_r {
                    self.regs.r = self.regs.a;
                } else {
                    self.regs.i = self.regs.a;
                }
                self.next(kind, BusOp::Internal(1))
            }
            (Kind::LdASpecial(is_r), 0) => {
                let value = if is_r { self.regs.r } else { self.regs.i };
                self.regs.a = value;
                self.regs.set_flags(
                    (self.regs.f & FLAG_C)
                        | (value & (FLAG_S | FLAG_YF | FLAG_XF))
                        | if value == 0 { FLAG_Z } else { 0 }
                        | if self.regs.iff2 { FLAG_PV } else { 0 },
                );
                self.next(kind, BusOp::Internal(1))
            }
            (Kind::LdSpecialA(_) | Kind::LdASpecial(_), _) => self.finish(),

            (Kind::NibbleRotate(_), 0) => {
                self.regs.wz = self.regs.hl().wrapping_add(1);
                self.next(kind, BusOp::MemRead(self.regs.hl()))
            }
            (Kind::NibbleRotate(is_rld), 1) => {
                let (a, memory, flags) = if is_rld {
                    alu::rld(self.regs.a, byte, self.regs.f)
                } else {
                    alu::rrd(self.regs.a, byte, self.regs.f)
                };
                self.regs.a = a;
                self.regs.set_flags(flags);
                self.lo = memory;
                self.next(kind, BusOp::Internal(4))
            }
            (Kind::NibbleRotate(_), 2) => self.next(kind, BusOp::MemWrite(self.regs.hl(), self.lo)),
            (Kind::NibbleRotate(_), _) => self.finish(),

            (Kind::BlockTransfer { .. }, 0) => self.next(kind, BusOp::MemRead(self.regs.hl())),
            (Kind::BlockTransfer { .. }, 1) => {
                self.lo = byte;
                self.next(kind, BusOp::MemWrite(self.regs.de(), byte))
            }
            (Kind::BlockTransfer { up, repeat }, 2) => {
                let step = if up { 1 } else { 1_u16.wrapping_neg() };
                self.regs.set_hl(self.regs.hl().wrapping_add(step));
                self.regs.set_de(self.regs.de().wrapping_add(step));
                self.regs.set_bc(self.regs.bc().wrapping_sub(1));
                let more = self.regs.bc() != 0;
                self.regs.set_flags(alu::block_transfer_flags(
                    self.lo,
                    self.regs.a,
                    more,
                    self.regs.f,
                ));
                self.again = repeat && more;
                self.next(kind, BusOp::Internal(2))
            }
            (Kind::BlockTransfer { .. }, 3) => self.repeat_block(kind),
            (Kind::BlockTransfer { .. }, _) => self.finish(),

            (Kind::BlockCompare { .. }, 0) => self.next(kind, BusOp::MemRead(self.regs.hl())),
            (Kind::BlockCompare { up, repeat }, 1) => {
                let step = if up { 1 } else { 1_u16.wrapping_neg() };
                self.regs.wz = self.regs.wz.wrapping_add(step);
                self.regs.set_hl(self.regs.hl().wrapping_add(step));
                self.regs.set_bc(self.regs.bc().wrapping_sub(1));
                let more = self.regs.bc() != 0;
                self.regs.set_flags(alu::block_compare_flags(
                    self.regs.a,
                    byte,
                    more,
                    self.regs.f,
                ));
                let found = self.regs.f & FLAG_Z != 0;
                self.again = repeat && more && !found;
                self.next(kind, BusOp::Internal(5))
            }
            (Kind::BlockCompare { .. }, 2) => self.repeat_block(kind),
            (Kind::BlockCompare { .. }, _) => self.finish(),

            (Kind::BlockIn { .. }, 0) => self.next(kind, BusOp::Internal(1)),
            (Kind::BlockIn { up, .. }, 1) => {
                let port = self.regs.bc();
                self.regs.wz = if up {
                    port.wrapping_add(1)
                } else {
                    port.wrapping_sub(1)
                };
                self.next(kind, BusOp::IoRead(port))
            }
            (Kind::BlockIn { .. }, 2) => {
                self.lo = byte;
                self.regs.b = self.regs.b.wrapping_sub(1);
                self.next(kind, BusOp::MemWrite(self.regs.hl(), byte))
            }
            (Kind::BlockIn { up, repeat }, 3) => {
                let step = if up { 1 } else { 1_u16.wrapping_neg() };
                self.regs.set_hl(self.regs.hl().wrapping_add(step));
                let addend = if up {
                    self.regs.c.wrapping_add(1)
                } else {
                    self.regs.c.wrapping_sub(1)
                };
                self.regs
                    .set_flags(alu::block_io_flags(self.regs.b, self.lo, addend));
                self.again = repeat && self.regs.b != 0;
                if self.again {
                    self.regs
                        .set_flags(alu::block_io_repeat_flags(self.regs.f, self.regs.b));
                }
                self.repeat_block(kind)
            }
            (Kind::BlockIn { .. }, _) => self.finish(),

            (Kind::BlockOut { .. }, 0) => self.next(kind, BusOp::Internal(1)),
            (Kind::BlockOut { .. }, 1) => self.next(kind, BusOp::MemRead(self.regs.hl())),
            (Kind::BlockOut { up, .. }, 2) => {
                self.lo = byte;
                self.regs.b = self.regs.b.wrapping_sub(1);
                let port = self.regs.bc();
                self.regs.wz = if up {
                    port.wrapping_add(1)
                } else {
                    port.wrapping_sub(1)
                };
                self.next(kind, BusOp::IoWrite(port, byte))
            }
            (Kind::BlockOut { up, repeat }, 3) => {
                let step = if up { 1 } else { 1_u16.wrapping_neg() };
                self.regs.set_hl(self.regs.hl().wrapping_add(step));
                self.regs
                    .set_flags(alu::block_io_flags(self.regs.b, self.lo, self.regs.l));
                self.again = repeat && self.regs.b != 0;
                if self.again {
                    self.regs
                        .set_flags(alu::block_io_repeat_flags(self.regs.f, self.regs.b));
                }
                self.repeat_block(kind)
            }
            (Kind::BlockOut { .. }, _) => self.finish(),

            (Kind::InAImm | Kind::OutImmA, 0) => {
                let op = self.read_immediate();
                self.next(kind, op)
            }
            (Kind::InAImm, 1) => {
                let port = (u16::from(self.regs.a) << 8) | u16::from(byte);
                self.regs.wz = port.wrapping_add(1);
                self.next(kind, BusOp::IoRead(port))
            }
            (Kind::InAImm, _) => {
                self.regs.a = byte;
                self.finish()
            }
            (Kind::OutImmA, 1) => {
                let port = (u16::from(self.regs.a) << 8) | u16::from(byte);
                self.write_wz_split(port);
                self.next(kind, BusOp::IoWrite(port, self.regs.a))
            }
            (Kind::OutImmA, _) => self.finish(),

            (Kind::Halt, _) => {
                self.regs.halted = true;
                self.regs.pc = self.regs.pc.wrapping_sub(1);
                self.finish()
            }

            (Kind::Di, _) => {
                self.regs.iff1 = false;
                self.regs.iff2 = false;
                self.finish_skipping_int()
            }
            (Kind::Ei, _) => {
                self.regs.iff1 = true;
                self.regs.iff2 = true;
                self.finish_skipping_int()
            }

            (Kind::AcceptNmi, 0) => {
                let refresh = self.regs.refresh_address();
                self.regs.inc_r();
                let pc = self.regs.pc;
                self.next(kind, BusOp::Fetch { pc, refresh })
            }
            (Kind::AcceptNmi, 1) => self.next(kind, BusOp::Internal(1)),
            (Kind::AcceptNmi, 2) => {
                let op = self.push_byte((self.regs.pc >> 8) as u8);
                self.next(kind, op)
            }
            (Kind::AcceptNmi, 3) => {
                let op = self.push_byte(self.regs.pc as u8);
                self.next(kind, op)
            }
            (Kind::AcceptNmi, _) => {
                self.regs.pc = 0x0066;
                self.finish()
            }

            (Kind::AcceptInt, 0) => {
                self.regs.inc_r();
                let pc = self.regs.pc;
                self.next(kind, BusOp::IntAck(pc))
            }
            (Kind::AcceptInt, 1) => {
                self.hi = byte;
                self.next(kind, BusOp::Internal(2))
            }
            (Kind::AcceptInt, 2) => {
                let op = self.push_byte((self.regs.pc >> 8) as u8);
                self.next(kind, op)
            }
            (Kind::AcceptInt, 3) => {
                let op = self.push_byte(self.regs.pc as u8);
                self.next(kind, op)
            }
            (Kind::AcceptInt, 4) => match self.regs.im {
                InterruptMode::Im2 => {
                    let vector = (u16::from(self.regs.i) << 8) | u16::from(self.hi);
                    self.next(kind, BusOp::MemRead(vector))
                }
                InterruptMode::Im1 => {
                    self.regs.pc = 0x0038;
                    self.finish()
                }
                InterruptMode::Im0 => {
                    if let Kind::Rst(target) = decode(self.hi) {
                        self.regs.pc = u16::from(target) * 8;
                    }
                    self.finish()
                }
            },
            (Kind::AcceptInt, 5) => {
                self.lo = byte;
                let vector = (u16::from(self.regs.i) << 8) | u16::from(self.hi);
                self.next(kind, BusOp::MemRead(vector.wrapping_add(1)))
            }
            (Kind::AcceptInt, _) => {
                self.regs.pc = u16::from_le_bytes([self.lo, byte]);
                self.finish()
            }

            (Kind::Rst(_), 0) => self.next(kind, BusOp::Internal(1)),
            (Kind::Rst(_), 1) => {
                let op = self.push_byte((self.regs.pc >> 8) as u8);
                self.next(kind, op)
            }
            (Kind::Rst(_), 2) => {
                let op = self.push_byte(self.regs.pc as u8);
                self.next(kind, op)
            }
            (Kind::Rst(vector), _) => {
                self.regs.pc = u16::from(vector) * 8;
                self.regs.wz = self.regs.pc;
                self.finish()
            }
        }
    }

    /// Steps out of a `HALT` on the way into an interrupt routine.
    ///
    /// `Z80-082`: `PC` is incremented past the `HALT` before the routine is
    /// called, so the return lands on the instruction after it rather than back
    /// in the halt.
    fn leave_halt(&mut self) {
        if self.regs.halted {
            self.regs.halted = false;
            self.regs.pc = self.regs.pc.wrapping_add(1);
        }
    }

    /// Ends a block instruction, or sets it up to run again.
    ///
    /// `Z80-054`: a repeat is not a loop inside the instruction but `PC` stepped
    /// back over the two opcode bytes, so the whole thing is fetched again —
    /// which is why `Z80-024` counts two refreshes per iteration and why an
    /// interrupt can be accepted in between. The five extra T-states are what
    /// take every repeating form from 16 to 21.
    /// `Z80-094`: an iteration that repeats replaces `YF` and `XF` with the two
    /// bits of the high half of the `PC` it has just stepped back — which is the
    /// instruction's own address, and is why the rule is invisible on the last
    /// iteration of every repeating form.
    fn repeat_block(&mut self, kind: Kind) -> BusOp {
        if core::mem::take(&mut self.again) {
            self.regs.pc = self.regs.pc.wrapping_sub(2);
            self.regs.wz = self.regs.pc.wrapping_add(1);
            let [_, high] = self.regs.pc.to_le_bytes();
            self.regs
                .set_flags((self.regs.f & !(FLAG_YF | FLAG_XF)) | (high & (FLAG_YF | FLAG_XF)));
            self.next(kind, BusOp::Internal(5))
        } else {
            self.finish()
        }
    }

    /// Whether a `CB` instruction reaches memory: either its own field names
    /// `(HL)`, or a `DD`/`FD` prefix has made every form indexed (`Z80-087`).
    fn cb_reaches_memory(&self, kind: Kind) -> bool {
        self.index != Index::None || kind.cb_target() == Some(REG_MEM)
    }

    /// Applies one of the eight accumulator operations, in the order the opcode's
    /// `y` field gives them: `ADD ADC SUB SBC AND XOR OR CP`.
    ///
    /// `CP` is the odd one: it keeps the flags and throws the difference away.
    fn apply_alu(&mut self, op: u8, operand: u8) {
        let a = self.regs.a;
        let carry = self.regs.f & FLAG_C != 0;
        let (result, flags) = match op {
            0 => alu::add(a, operand, false),
            1 => alu::add(a, operand, carry),
            2 => alu::sub(a, operand, false),
            3 => alu::sub(a, operand, carry),
            4 => alu::and(a, operand),
            5 => alu::xor(a, operand),
            6 => alu::or(a, operand),
            _ => (a, alu::cp(a, operand)),
        };
        self.regs.a = result;
        self.regs.set_flags(flags);
    }

    /// Reads the 8-bit register an opcode field names.
    ///
    /// Under a `DD` or `FD` prefix, `H` and `L` become the index register's
    /// halves (`Z80-027`) — but only where `substitute` says so, because an
    /// instruction that also names `(IX+d)` keeps its register operand as the
    /// real `H` or `L`. `LD H,(IX+d)` loads the real `H`.
    fn reg_field(&self, field: u8, substitute: bool) -> u8 {
        match (field, self.index) {
            (4, Index::Ix) if substitute => self.regs.ixh(),
            (5, Index::Ix) if substitute => self.regs.ixl(),
            (4, Index::Iy) if substitute => self.regs.iyh(),
            (5, Index::Iy) if substitute => self.regs.iyl(),
            _ => self.regs.reg(field),
        }
    }

    /// Writes the 8-bit register an opcode field names, on [`Z80::reg_field`]'s
    /// terms.
    fn set_reg_field(&mut self, field: u8, substitute: bool, value: u8) {
        match (field, self.index) {
            (4, Index::Ix) if substitute => self.regs.set_ixh(value),
            (5, Index::Ix) if substitute => self.regs.set_ixl(value),
            (4, Index::Iy) if substitute => self.regs.set_iyh(value),
            (5, Index::Iy) if substitute => self.regs.set_iyl(value),
            _ => self.regs.set_reg(field, value),
        }
    }

    /// `HL`, or the index register standing in for it.
    const fn hl_value(&self) -> u16 {
        match self.index {
            Index::None => self.regs.hl(),
            Index::Ix => self.regs.ix,
            Index::Iy => self.regs.iy,
        }
    }

    /// Writes `HL`, or the index register standing in for it.
    const fn set_hl_value(&mut self, value: u16) {
        match self.index {
            Index::None => self.regs.set_hl(value),
            Index::Ix => self.regs.ix = value,
            Index::Iy => self.regs.iy = value,
        }
    }

    /// The pair an opcode's two-bit field names, with the index register in
    /// `HL`'s place under a prefix.
    const fn pair_field(&self, field: u8) -> u16 {
        if field == 2 {
            self.hl_value()
        } else {
            self.regs.pair(field)
        }
    }

    /// Writes the pair an opcode's two-bit field names.
    const fn set_pair_field(&mut self, field: u8, value: u16) {
        if field == 2 {
            self.set_hl_value(value);
        } else {
            self.regs.set_pair(field, value);
        }
    }

    /// The stack instructions' pair, with the index register in `HL`'s place.
    const fn stack_pair_field(&self, field: u8) -> u16 {
        if field == 2 {
            self.hl_value()
        } else {
            self.regs.stack_pair(field)
        }
    }

    /// Writes the stack instructions' pair.
    const fn set_stack_pair_field(&mut self, field: u8, value: u16) {
        if field == 2 {
            self.set_hl_value(value);
        } else {
            self.regs.set_stack_pair(field, value);
        }
    }

    /// The address an instruction's `(HL)` operand resolves to: `HL`, or the
    /// `IX+d` a displacement has already been read for.
    const fn memory_address(&self) -> u16 {
        if matches!(self.index, Index::None) {
            self.regs.hl()
        } else {
            self.indexed
        }
    }

    /// The byte `BIT` against memory takes `YF` and `XF` from.
    ///
    /// The high half of the computed address for the indexed forms (`Z80-042`),
    /// and of the internal register for `(HL)` (`Z80-059`).
    const fn bit_source(&self) -> u8 {
        if matches!(self.index, Index::None) {
            (self.regs.wz >> 8) as u8
        } else {
            (self.indexed >> 8) as u8
        }
    }

    /// Ends the instruction and starts the next opcode fetch.
    fn finish(&mut self) -> BusOp {
        self.end_instruction(false)
    }

    /// [`Z80::finish`] for `EI` and `DI`, whose end is not a boundary at which
    /// a maskable interrupt is accepted (`Z80-076`). The skip is decided here,
    /// in the tick that ends the instruction, so no delay outlives it.
    fn finish_skipping_int(&mut self) -> BusOp {
        self.end_instruction(true)
    }

    /// Ends the instruction and starts the next opcode fetch, or accepts an
    /// interrupt in its place.
    fn end_instruction(&mut self, delayed: bool) -> BusOp {
        self.index = Index::None;

        if self.regs.pending != Some(Pending::Busreq) {
            self.regs.pending =
                (!delayed && self.int_line && self.regs.iff1).then_some(Pending::Int);
        }
        self.stage = Stage::Opcode;
        let fetch = BusOp::Fetch {
            pc: self.regs.pc,
            refresh: self.regs.refresh_address(),
        };
        if self.stop_at_boundary {
            return fetch;
        }
        self.after_boundary(fetch)
    }

    /// Accepts the latched `NMI` edge at the boundary just reached.
    fn accept_nmi(&mut self) -> BusOp {
        self.regs.nmi_pending = false;
        if self.regs.pending == Some(Pending::Int) {
            self.regs.pending = None;
        }
        self.regs.iff1 = false;
        self.leave_halt();
        self.stage = Stage::Opcode;
        self.body(Kind::AcceptNmi, 0, 0)
    }

    /// Accepts `INT` at the boundary just reached.
    fn accept_int(&mut self) -> BusOp {
        self.regs.pending = None;
        self.regs.iff1 = false;
        self.regs.iff2 = false;
        self.leave_halt();
        self.stage = Stage::Opcode;
        self.body(Kind::AcceptInt, 0, 0)
    }

    /// Begins what follows a boundary that ended in `fetch`: the latched `NMI`
    /// edge, accepted in its place, which `Z80-066` says cannot be masked and
    /// takes priority over `INT`; or `INT`; or the fetch.
    pub(crate) fn after_boundary(&mut self, fetch: BusOp) -> BusOp {
        if self.regs.nmi_pending {
            self.accept_nmi()
        } else if self.regs.pending == Some(Pending::Int) {
            self.accept_int()
        } else {
            fetch
        }
    }

    /// Stays in `kind`, one step further on, and runs `op`.
    fn next(&mut self, kind: Kind, op: BusOp) -> BusOp {
        let step = match self.stage {
            Stage::Body { step, .. } => step + 1,
            _ => 1,
        };
        self.stage = Stage::Body { kind, step };
        op
    }

    /// Reads the byte `PC` points at and steps `PC` over it.
    fn read_immediate(&mut self) -> BusOp {
        let address = self.regs.pc;
        self.regs.pc = address.wrapping_add(1);
        BusOp::MemRead(address)
    }

    /// The two operand bytes gathered so far, low byte first.
    /// `Z80-097`: what a store through an address leaves in the internal
    /// register — `A` over the low half of the address one past the byte
    /// written. The high half of the register never sees the address at all,
    /// which is the whole oddity of the rule.
    fn write_wz_split(&mut self, address: u16) {
        let low = address.wrapping_add(1) & 0x00FF;
        self.regs.wz = (u16::from(self.regs.a) << 8) | low;
    }

    const fn operand_address(&self) -> u16 {
        u16::from_le_bytes([self.lo, self.hi])
    }

    /// Decrements `SP` and writes `byte` to the address it then holds.
    fn push_byte(&mut self, byte: u8) -> BusOp {
        self.regs.sp = self.regs.sp.wrapping_sub(1);
        BusOp::MemWrite(self.regs.sp, byte)
    }

    /// Reads the byte `SP` points at and steps `SP` over it.
    fn pop_byte(&mut self) -> BusOp {
        let address = self.regs.sp;
        self.regs.sp = address.wrapping_add(1);
        BusOp::MemRead(address)
    }

    /// Adds the displacement latched in `lo` to `PC`, as `JR` and `DJNZ` do.
    ///
    /// `Z80-062`: the internal register's high byte is the jump target's, which
    /// is satisfied by giving it the whole target.
    fn jump_relative(&mut self) {
        #[expect(
            clippy::cast_possible_wrap,
            reason = "the displacement is two's complement"
        )]
        let displacement = self.lo as i8;
        self.regs.pc = self.regs.pc.wrapping_add_signed(i16::from(displacement));
        self.regs.wz = self.regs.pc;
    }
}
