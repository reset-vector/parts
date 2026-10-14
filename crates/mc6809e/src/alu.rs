//! The condition codes of Figure 18 (`MC6809E-024` to `MC6809E-029`, `MC6809E-046`).
//!
//! Where Figure 18 prints a half carry as undefined (note 8) the crate leaves
//! `H` as it was (`MC6809E-OPEN-05`).

use crate::decode::{Alu8, Cond, Unary};

/// Carry, or borrow.
pub const C: u8 = 1 << 0;
/// Two's complement overflow.
pub const V: u8 = 1 << 1;
/// Zero.
pub const Z: u8 = 1 << 2;
/// Negative.
pub const N: u8 = 1 << 3;
/// `IRQ` mask.
pub const I: u8 = 1 << 4;
/// Half carry from bit 3.
pub const H: u8 = 1 << 5;
/// `FIRQ` mask.
pub const F: u8 = 1 << 6;
/// The entire state was stacked.
pub const E: u8 = 1 << 7;

const fn put(cc: u8, bit: u8, on: bool) -> u8 {
    if on { cc | bit } else { cc & !bit }
}

/// `N` and `Z` from an eight-bit result.
#[must_use]
pub(crate) const fn nz(cc: u8, r: u8) -> u8 {
    put(put(cc, N, r & 0x80 != 0), Z, r == 0)
}

/// `N` and `Z` from a sixteen-bit result.
#[must_use]
pub(crate) const fn nz16(cc: u8, r: u16) -> u8 {
    put(put(cc, N, r & 0x8000 != 0), Z, r == 0)
}

/// `N`, `Z` from the result and `V` cleared: loads, stores, logic, `TST`.
#[must_use]
pub(crate) const fn logic(cc: u8, r: u8) -> u8 {
    nz(cc, r) & !V
}

/// As [`logic`], sixteen bits.
#[must_use]
pub(crate) const fn logic16(cc: u8, r: u16) -> u8 {
    nz16(cc, r) & !V
}

/// `a + m + carry`, setting `H`, `N`, `Z`, `V`, `C`.
#[must_use]
pub(crate) const fn add(cc: u8, a: u8, m: u8, carry: bool) -> (u8, u8) {
    let c = carry as u16;
    let wide = a as u16 + m as u16 + c;
    let r = wide as u8;
    let half = (a & 0x0f) as u16 + (m & 0x0f) as u16 + c > 0x0f;
    let over = !(a ^ m) & (a ^ r) & 0x80 != 0;
    (
        r,
        put(put(put(nz(cc, r), H, half), V, over), C, wide > 0xff),
    )
}

/// `a - m - borrow`, setting `N`, `Z`, `V`, `C`; `H` left.
#[must_use]
pub(crate) const fn sub(cc: u8, a: u8, m: u8, borrow: bool) -> (u8, u8) {
    let b = borrow as u16;
    let r = a.wrapping_sub(m).wrapping_sub(b as u8);
    let over = (a ^ m) & (a ^ r) & 0x80 != 0;
    (
        r,
        put(put(nz(cc, r), V, over), C, (a as u16) < m as u16 + b),
    )
}

/// `a + m`, sixteen bits, setting `N`, `Z`, `V`, `C`.
#[must_use]
pub(crate) const fn add16(cc: u8, a: u16, m: u16) -> (u16, u8) {
    let wide = a as u32 + m as u32;
    let r = wide as u16;
    let over = !(a ^ m) & (a ^ r) & 0x8000 != 0;
    (r, put(put(nz16(cc, r), V, over), C, wide > 0xffff))
}

/// `a - m`, sixteen bits, setting `N`, `Z`, `V`, `C`.
#[must_use]
pub(crate) const fn sub16(cc: u8, a: u16, m: u16) -> (u16, u8) {
    let r = a.wrapping_sub(m);
    let over = (a ^ m) & (a ^ r) & 0x8000 != 0;
    (r, put(put(nz16(cc, r), V, over), C, a < m))
}

/// A two-operand eight-bit operation: the accumulator's new value, or `None`
/// where it is left alone (`CMP`, `BIT`), and the new CC.
#[must_use]
pub(crate) const fn alu8(cc: u8, op: Alu8, a: u8, m: u8) -> (Option<u8>, u8) {
    let carry = cc & C != 0;
    match op {
        Alu8::Sub => {
            let (r, cc) = sub(cc, a, m, false);
            (Some(r), cc)
        }
        Alu8::Cmp => (None, sub(cc, a, m, false).1),
        Alu8::Sbc => {
            let (r, cc) = sub(cc, a, m, carry);
            (Some(r), cc)
        }
        Alu8::And => (Some(a & m), logic(cc, a & m)),
        Alu8::Bit => (None, logic(cc, a & m)),
        Alu8::Ld => (Some(m), logic(cc, m)),
        Alu8::Eor => (Some(a ^ m), logic(cc, a ^ m)),
        Alu8::Adc => {
            let (r, cc) = add(cc, a, m, carry);
            (Some(r), cc)
        }
        Alu8::Or => (Some(a | m), logic(cc, a | m)),
        Alu8::Add => {
            let (r, cc) = add(cc, a, m, false);
            (Some(r), cc)
        }
    }
}

/// A one-operand operation: the new value, or `None` for `TST`, and the
/// new CC (`MC6809E-025` to `MC6809E-027`).
#[must_use]
pub(crate) const fn unary(cc: u8, op: Unary, m: u8) -> (Option<u8>, u8) {
    let carry = cc & C != 0;
    match op {
        Unary::Neg => {
            let r = 0u8.wrapping_sub(m);
            let cc = put(put(nz(cc, r), V, m == 0x80), C, m != 0);
            (Some(r), cc)
        }
        Unary::Com => (Some(!m), logic(cc, !m) | C),
        Unary::Lsr => {
            let r = m >> 1;
            (Some(r), put(nz(cc, r), C, m & 1 != 0))
        }
        Unary::Ror => {
            let r = (m >> 1) | ((carry as u8) << 7);
            (Some(r), put(nz(cc, r), C, m & 1 != 0))
        }
        Unary::Asr => {
            let r = (m >> 1) | (m & 0x80);
            (Some(r), put(nz(cc, r), C, m & 1 != 0))
        }
        Unary::Asl => {
            let r = m << 1;
            let cc = put(
                put(nz(cc, r), C, m & 0x80 != 0),
                V,
                (m ^ (m << 1)) & 0x80 != 0,
            );
            (Some(r), cc)
        }
        Unary::Rol => {
            let r = (m << 1) | carry as u8;
            let cc = put(
                put(nz(cc, r), C, m & 0x80 != 0),
                V,
                (m ^ (m << 1)) & 0x80 != 0,
            );
            (Some(r), cc)
        }
        Unary::Dec => {
            let r = m.wrapping_sub(1);
            (Some(r), put(nz(cc, r), V, m == 0x80))
        }
        Unary::Inc => {
            let r = m.wrapping_add(1);
            (Some(r), put(nz(cc, r), V, m == 0x7f))
        }
        Unary::Tst => (None, logic(cc, m)),
        Unary::Clr => (Some(0), (cc & !(N | V | C)) | Z),
    }
}

/// `DAA`: the corrected `A` and CC. The correction rule is this crate's
/// choice under `MC6809E-OPEN-10`. `V` is cleared as Figure 18 prints.
#[must_use]
pub(crate) const fn daa(cc: u8, a: u8) -> (u8, u8) {
    let lo = a & 0x0f;
    let hi = a >> 4;
    let mut fix = 0u8;
    if cc & H != 0 || lo > 9 {
        fix |= 0x06;
    }
    let carry = cc & C != 0 || hi > 9 || (hi > 8 && lo > 9);
    if carry {
        fix |= 0x60;
    }
    let r = a.wrapping_add(fix);
    (r, put(nz(cc, r) & !V, C, carry))
}

/// `MUL`: `D` and the CC, `Z` from `D` and `C` from bit 7 (note 9).
#[must_use]
pub(crate) const fn mul(cc: u8, a: u8, b: u8) -> (u16, u8) {
    let d = a as u16 * b as u16;
    (d, put(put(cc, Z, d == 0), C, d & 0x80 != 0))
}

/// Whether a branch on `cond` is taken under `cc`.
#[must_use]
pub(crate) const fn taken(cc: u8, cond: Cond) -> bool {
    let c = cc & C != 0;
    let v = cc & V != 0;
    let z = cc & Z != 0;
    let n = cc & N != 0;
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
