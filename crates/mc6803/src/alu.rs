//! The condition codes, as Appendix A's Boolean formulae write them.
//!
//! `MC6803-013` names the bits, and `MC6803-018` to `MC6803-023` give each
//! operation its effect on them. Every function takes the flags in and hands
//! them back changed only where its claim says.

/// Carry-borrow, bit 0.
pub const C: u8 = 1 << 0;
/// Two's complement overflow, bit 1.
pub const V: u8 = 1 << 1;
/// Zero, bit 2.
pub const Z: u8 = 1 << 2;
/// Negative, bit 3.
pub const N: u8 = 1 << 3;
/// Interrupt mask, bit 4.
pub const I: u8 = 1 << 4;
/// Half carry from bit 3, bit 5.
pub const H: u8 = 1 << 5;
/// Bits 6 and 7, which read as 1 (`MC6803-024`).
pub(crate) const UPPER: u8 = 0xc0;

const fn set(ccr: u8, mask: u8, on: bool) -> u8 {
    if on { ccr | mask } else { ccr & !mask }
}

/// `N` and `Z` from an eight-bit result.
#[must_use]
pub(crate) const fn nz8(ccr: u8, r: u8) -> u8 {
    let ccr = set(ccr, N, r & 0x80 != 0);
    set(ccr, Z, r == 0)
}

/// `N` and `Z` from a sixteen-bit result.
#[must_use]
pub(crate) const fn nz16(ccr: u8, r: u16) -> u8 {
    let ccr = set(ccr, N, r & 0x8000 != 0);
    set(ccr, Z, r == 0)
}

/// `N`, `Z` and `V` cleared: the loads, stores, transfers and logic group
/// (`MC6803-018`, `MC6803-021`).
#[must_use]
pub(crate) const fn nzv0_8(ccr: u8, r: u8) -> u8 {
    nz8(ccr, r) & !V
}

/// The sixteen-bit form of [`nzv0_8`].
#[must_use]
pub(crate) const fn nzv0_16(ccr: u8, r: u16) -> u8 {
    nz16(ccr, r) & !V
}

/// `ADD`, `ADC`, `ABA`: `H`, `N`, `Z`, `V`, `C` (`MC6803-018`).
#[must_use]
pub(crate) const fn add8(ccr: u8, x: u8, m: u8, carry_in: bool) -> (u8, u8) {
    let c = carry_in as u16;
    let sum = x as u16 + m as u16 + c;
    let r = sum as u8;
    let ccr = set(ccr, H, ((x & 0x0f) + (m & 0x0f) + c as u8) & 0x10 != 0);
    let ccr = set(ccr, V, ((x ^ r) & (m ^ r)) & 0x80 != 0);
    let ccr = set(ccr, C, sum & 0x100 != 0);
    (nz8(ccr, r), r)
}

/// `SUB`, `SBC`, `SBA`, `CMP`, `CBA`: `N`, `Z`, `V`, `C`, `H` untouched
/// (`MC6803-018`).
#[must_use]
pub(crate) const fn sub8(ccr: u8, x: u8, m: u8, borrow_in: bool) -> (u8, u8) {
    let b = borrow_in as u16;
    let diff = (x as u16).wrapping_sub(m as u16).wrapping_sub(b);
    let r = diff as u8;
    let ccr = set(ccr, V, ((x ^ m) & (x ^ r)) & 0x80 != 0);
    let ccr = set(ccr, C, diff & 0x100 != 0);
    (nz8(ccr, r), r)
}

/// `ADDD`: `N`, `Z`, `V`, `C` on sixteen bits, `H` untouched (`MC6803-021`).
#[must_use]
pub(crate) const fn add16(ccr: u8, d: u16, m: u16) -> (u8, u16) {
    let sum = d as u32 + m as u32;
    let r = sum as u16;
    let ccr = set(ccr, V, ((d ^ r) & (m ^ r)) & 0x8000 != 0);
    let ccr = set(ccr, C, sum & 0x1_0000 != 0);
    (nz16(ccr, r), r)
}

/// `SUBD` and `CPX`: `N`, `Z`, `V`, `C` on sixteen bits (`MC6803-021`).
#[must_use]
pub(crate) const fn sub16(ccr: u8, d: u16, m: u16) -> (u8, u16) {
    let diff = (d as u32).wrapping_sub(m as u32);
    let r = diff as u16;
    let ccr = set(ccr, V, ((d ^ m) & (d ^ r)) & 0x8000 != 0);
    let ccr = set(ccr, C, diff & 0x1_0000 != 0);
    (nz16(ccr, r), r)
}

/// The shifts and rotates: `N`, `Z`, `V` = `N` ⊕ `C`, `C` the bit shifted
/// out (`MC6803-019`).
const fn shifted(ccr: u8, r: u8, out: bool) -> u8 {
    let ccr = set(nz8(ccr, r), C, out);
    set(ccr, V, (ccr & N != 0) != (ccr & C != 0))
}

/// `ASL`/`LSL`.
#[must_use]
pub(crate) const fn asl(ccr: u8, m: u8) -> (u8, u8) {
    let r = m << 1;
    (shifted(ccr, r, m & 0x80 != 0), r)
}

/// `ASR`: bit 7 held.
#[must_use]
pub(crate) const fn asr(ccr: u8, m: u8) -> (u8, u8) {
    let r = (m >> 1) | (m & 0x80);
    (shifted(ccr, r, m & 1 != 0), r)
}

/// `LSR`: bit 7 cleared, so `N` is always clear.
#[must_use]
pub(crate) const fn lsr(ccr: u8, m: u8) -> (u8, u8) {
    let r = m >> 1;
    (shifted(ccr, r, m & 1 != 0), r)
}

/// `ROL`: bit 0 from `C`.
#[must_use]
pub(crate) const fn rol(ccr: u8, m: u8) -> (u8, u8) {
    let r = (m << 1) | (ccr & C);
    (shifted(ccr, r, m & 0x80 != 0), r)
}

/// `ROR`: bit 7 from `C`.
#[must_use]
pub(crate) const fn ror(ccr: u8, m: u8) -> (u8, u8) {
    let r = (m >> 1) | ((ccr & C) << 7);
    (shifted(ccr, r, m & 1 != 0), r)
}

/// `ASLD`: sixteen bits, `C` from bit 15 (`MC6803-019`).
#[must_use]
pub(crate) const fn asld(ccr: u8, d: u16) -> (u8, u16) {
    let r = d << 1;
    let ccr = set(nz16(ccr, r), C, d & 0x8000 != 0);
    let ccr = set(ccr, V, (ccr & N != 0) != (ccr & C != 0));
    (ccr, r)
}

/// `LSRD`: sixteen bits, `C` from bit 0, `N` clear (`MC6803-019`).
#[must_use]
pub(crate) const fn lsrd(ccr: u8, d: u16) -> (u8, u16) {
    let r = d >> 1;
    let ccr = set(nz16(ccr, r), C, d & 1 != 0);
    let ccr = set(ccr, V, (ccr & N != 0) != (ccr & C != 0));
    (ccr, r)
}

/// `NEG`: `V` iff the operand was `$80`, `C` unless it was `$00`
/// (`MC6803-020`).
#[must_use]
pub(crate) const fn neg(ccr: u8, m: u8) -> (u8, u8) {
    let r = 0u8.wrapping_sub(m);
    let ccr = set(nz8(ccr, r), V, r == 0x80);
    (set(ccr, C, r != 0), r)
}

/// `COM`: `V` clear, `C` set (`MC6803-020`).
#[must_use]
pub(crate) const fn com(ccr: u8, m: u8) -> (u8, u8) {
    let r = !m;
    ((nz8(ccr, r) & !V) | C, r)
}

/// `INC`: `V` iff the operand was `$7F`, `C` untouched (`MC6803-020`).
#[must_use]
pub(crate) const fn inc(ccr: u8, m: u8) -> (u8, u8) {
    let r = m.wrapping_add(1);
    (set(nz8(ccr, r), V, m == 0x7f), r)
}

/// `DEC`: `V` iff the operand was `$80`, `C` untouched (`MC6803-020`).
#[must_use]
pub(crate) const fn dec(ccr: u8, m: u8) -> (u8, u8) {
    let r = m.wrapping_sub(1);
    (set(nz8(ccr, r), V, m == 0x80), r)
}

/// `TST`: `N`, `Z`; `V` and `C` clear (`MC6803-020`).
#[must_use]
pub(crate) const fn tst(ccr: u8, m: u8) -> u8 {
    nz8(ccr, m) & !(V | C)
}

/// `CLR`: `Z` set, `N`, `V`, `C` clear (`MC6803-020`).
#[must_use]
pub(crate) const fn clr(ccr: u8) -> u8 {
    (ccr & !(N | V | C)) | Z
}

/// `DAA`, by the nine rows of Appendix A's table (`MC6803-023`). `V` is
/// left as it was, which is `MC6803-OPEN-03`.
#[must_use]
pub(crate) const fn daa(ccr: u8, a: u8) -> (u8, u8) {
    let upper = a >> 4;
    let lower = a & 0x0f;
    let carry = ccr & C != 0;
    let half = ccr & H != 0;
    let (add, carry_out) = if !carry {
        if !half {
            if upper <= 9 && lower <= 9 {
                (0x00, false)
            } else if upper <= 8 && lower >= 0xa {
                (0x06, false)
            } else if upper >= 0xa && lower <= 9 {
                (0x60, true)
            } else {
                (0x66, true)
            }
        } else if upper <= 9 && lower <= 3 {
            (0x06, false)
        } else {
            (0x66, true)
        }
    } else if !half {
        if upper <= 2 && lower <= 9 {
            (0x60, true)
        } else {
            (0x66, true)
        }
    } else {
        (0x66, true)
    };
    let r = a.wrapping_add(add);
    (set(nz8(ccr, r), C, carry_out), r)
}

/// `MUL`: `C` is bit 7 of the low byte of the product, nothing else moves
/// (`MC6803-022`).
#[must_use]
pub(crate) const fn mul(ccr: u8, a: u8, b: u8) -> (u8, u16) {
    let r = a as u16 * b as u16;
    (set(ccr, C, r & 0x80 != 0), r)
}

/// Whether a branch condition holds (`MC6803-026`).
#[must_use]
pub(crate) const fn branch(ccr: u8, cond: crate::decode::Cond) -> bool {
    use crate::decode::Cond;
    let c = ccr & C != 0;
    let v = ccr & V != 0;
    let z = ccr & Z != 0;
    let n = ccr & N != 0;
    match cond {
        Cond::Always => true,
        Cond::Never => false,
        Cond::Hi => !(c || z),
        Cond::Ls => c || z,
        Cond::Cc => !c,
        Cond::Cs => c,
        Cond::Ne => !z,
        Cond::Eq => z,
        Cond::Vc => !v,
        Cond::Vs => v,
        Cond::Pl => !n,
        Cond::Mi => n,
        Cond::Ge => n == v,
        Cond::Lt => n != v,
        Cond::Gt => !z && n == v,
        Cond::Le => z || n != v,
    }
}
