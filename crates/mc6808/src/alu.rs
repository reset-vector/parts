//! The condition codes of Tables 3 to 6 and their twelve notes
//! (`MC6808-015`, `MC6808-018` to `MC6808-022`, `MC6808-025`).

use crate::decode::{Alu8, Cond, Unary};

/// Carry from bit 7.
pub const C: u8 = 1 << 0;
/// Two's complement overflow.
pub const V: u8 = 1 << 1;
/// Zero.
pub const Z: u8 = 1 << 2;
/// Negative.
pub const N: u8 = 1 << 3;
/// Interrupt mask.
pub const I: u8 = 1 << 4;
/// Half carry from bit 3.
pub const H: u8 = 1 << 5;
/// Bits 6 and 7, which read as ones (`MC6808-015`).
pub(crate) const ONES: u8 = 0xc0;

const fn put(ccr: u8, bit: u8, on: bool) -> u8 {
    if on { ccr | bit } else { ccr & !bit }
}

/// `N` and `Z` from an eight-bit result.
#[must_use]
pub(crate) const fn nz(ccr: u8, r: u8) -> u8 {
    put(put(ccr, N, r & 0x80 != 0), Z, r == 0)
}

/// `N` from bit 15 and `Z` from all sixteen bits (note 9).
#[must_use]
pub(crate) const fn nz16(ccr: u8, r: u16) -> u8 {
    put(put(ccr, N, r & 0x8000 != 0), Z, r == 0)
}

/// `a + m + carry`, setting `H`, `N`, `Z`, `V`, `C`.
#[must_use]
pub(crate) const fn add(ccr: u8, a: u8, m: u8, carry: bool) -> (u8, u8) {
    let c = carry as u16;
    let wide = a as u16 + m as u16 + c;
    let r = wide as u8;
    let half = (a & 0x0f) as u16 + (m & 0x0f) as u16 + c > 0x0f;
    let over = !(a ^ m) & (a ^ r) & 0x80 != 0;
    let ccr = put(put(put(nz(ccr, r), H, half), V, over), C, wide > 0xff);
    (r, ccr)
}

/// `a - m - borrow`, setting `N`, `Z`, `V`, `C` and leaving `H`.
#[must_use]
pub(crate) const fn sub(ccr: u8, a: u8, m: u8, borrow: bool) -> (u8, u8) {
    let b = borrow as u16;
    let r = a.wrapping_sub(m).wrapping_sub(b as u8);
    let over = (a ^ m) & (a ^ r) & 0x80 != 0;
    let carry = (a as u16) < m as u16 + b;
    (r, put(put(nz(ccr, r), V, over), C, carry))
}

/// A Table 3 two-operand operation: the accumulator's new value, or `None`
/// where it is left alone, and the new CCR.
#[must_use]
pub(crate) const fn alu8(op: Alu8, ccr: u8, a: u8, m: u8) -> (Option<u8>, u8) {
    let carry = ccr & C != 0;
    match op {
        Alu8::Add => {
            let (r, f) = add(ccr, a, m, false);
            (Some(r), f)
        }
        Alu8::Adc => {
            let (r, f) = add(ccr, a, m, carry);
            (Some(r), f)
        }
        Alu8::Sub => {
            let (r, f) = sub(ccr, a, m, false);
            (Some(r), f)
        }
        Alu8::Sbc => {
            let (r, f) = sub(ccr, a, m, carry);
            (Some(r), f)
        }
        Alu8::Cmp => (None, sub(ccr, a, m, false).1),
        Alu8::And => (Some(a & m), logic(ccr, a & m)),
        Alu8::Bit => (None, logic(ccr, a & m)),
        Alu8::Eor => (Some(a ^ m), logic(ccr, a ^ m)),
        Alu8::Or => (Some(a | m), logic(ccr, a | m)),
        Alu8::Ld => (Some(m), logic(ccr, m)),
    }
}

/// `N` and `Z` from the result, `V` cleared, `C` left: `AND`, `BIT`, `EOR`,
/// `ORA`, `LDA`, `STA`, `TAB`, `TBA`.
#[must_use]
pub(crate) const fn logic(ccr: u8, r: u8) -> u8 {
    nz(ccr, r) & !V
}

/// A unary operation on `m`: the result and the new CCR. `TST` returns its
/// operand unchanged.
#[must_use]
pub(crate) const fn unary(op: Unary, ccr: u8, m: u8) -> (u8, u8) {
    let carry = ccr & C != 0;
    const fn shifted(ccr: u8, r: u8, c: bool) -> (u8, u8) {
        let f = put(nz(ccr, r), C, c);
        (r, put(f, V, (f & N != 0) != c))
    }
    match op {
        Unary::Neg => {
            let r = 0u8.wrapping_sub(m);
            (r, put(put(nz(ccr, r), V, r == 0x80), C, r != 0))
        }
        Unary::Com => {
            let r = !m;
            (r, (nz(ccr, r) & !V) | C)
        }
        Unary::Lsr => shifted(ccr, m >> 1, m & 1 != 0),
        Unary::Ror => shifted(ccr, (m >> 1) | ((carry as u8) << 7), m & 1 != 0),
        Unary::Asr => shifted(ccr, (m >> 1) | (m & 0x80), m & 1 != 0),
        Unary::Asl => shifted(ccr, m << 1, m & 0x80 != 0),
        Unary::Rol => shifted(ccr, (m << 1) | carry as u8, m & 0x80 != 0),
        Unary::Dec => {
            let r = m.wrapping_sub(1);
            (r, put(nz(ccr, r), V, m == 0x80))
        }
        Unary::Inc => {
            let r = m.wrapping_add(1);
            (r, put(nz(ccr, r), V, m == 0x7f))
        }
        Unary::Tst => (m, nz(ccr, m) & !(V | C)),
        Unary::Clr => (0, (ccr & !(N | V | C)) | Z),
    }
}

/// `CPX`: `Z` from all sixteen bits; `N` and `V` from the subtraction of the
/// most significant bytes (notes 7 and 8, `MC6808-OPEN-10`); `C` left.
#[must_use]
pub(crate) const fn cpx(ccr: u8, x: u16, m: u16) -> u8 {
    let xh = (x >> 8) as u8;
    let mh = (m >> 8) as u8;
    let r = xh.wrapping_sub(mh);
    let over = (xh ^ mh) & (xh ^ r) & 0x80 != 0;
    put(put(put(ccr, N, r & 0x80 != 0), Z, x == m), V, over)
}

/// `DAA` (`MC6808-021`). The correction is `MC6808-OPEN-08`'s: `$06` when
/// `H` is set or the low digit exceeds nine, `$60` when `C` is set, the high
/// digit exceeds nine, or it is nine with a low digit above nine. `C` is set
/// with the `$60` and never cleared (note 3); `V` is left.
#[must_use]
pub(crate) const fn daa(ccr: u8, a: u8) -> (u8, u8) {
    let lo = a & 0x0f;
    let hi = a >> 4;
    let carry = ccr & C != 0;
    let mut add = 0u8;
    if ccr & H != 0 || lo > 9 {
        add |= 0x06;
    }
    let high = carry || hi > 9 || (hi > 8 && lo > 9);
    if high {
        add |= 0x60;
    }
    let r = a.wrapping_add(add);
    (r, put(nz(ccr, r), C, carry || high))
}

/// Whether a branch is taken (Table 5).
#[must_use]
pub(crate) const fn taken(cond: Cond, ccr: u8) -> bool {
    let c = ccr & C != 0;
    let v = ccr & V != 0;
    let z = ccr & Z != 0;
    let n = ccr & N != 0;
    match cond {
        Cond::Always => true,
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
        Cond::Gt => !(z || n != v),
        Cond::Le => z || n != v,
    }
}
