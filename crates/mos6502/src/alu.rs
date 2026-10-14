//! The arithmetic, the flags, and the two instructions that read the decimal
//! flag.
//!
//! Everything here is integer.
//!
//! # Binary and decimal
//!
//! The **binary** arithmetic is `MOS6502-039`: the manufacturer's own
//! instruction pages give `ADC` as `A + M + C` and `SBC` as `A - M - C̄`, and
//! name the four flags each of them touches. The **decimal** arithmetic is
//! `MOS6502-054` — the adder corrects the binary result as it goes — with
//! `MOS6502-060` and `MOS6502-061` for what `N`, `V` and `Z` hold after a
//! decimal add and what happens to a nibble that is not a decimal digit.

/// Negative.
pub const FLAG_N: u8 = 0x80;
/// Overflow.
pub const FLAG_V: u8 = 0x40;
/// Bit 5 of the status register. `MOS6502-029`: the manufacturer calls it
/// unused and guarantees nothing about it; `MOS6502-056` is why this crate
/// keeps it set.
pub const FLAG_U: u8 = 0x20;
/// Break. Not a register bit — it is what the part pushes, and what it pushes
/// says whether the push came from `BRK` (`MOS6502-028`, `MOS6502-057`).
pub const FLAG_B: u8 = 0x10;
/// Decimal.
pub const FLAG_D: u8 = 0x08;
/// Interrupt disable.
pub const FLAG_I: u8 = 0x04;
/// Zero.
pub const FLAG_Z: u8 = 0x02;
/// Carry.
pub const FLAG_C: u8 = 0x01;

/// `p` with the `N` and `Z` flags set from `value` (`MOS6502-027`).
#[must_use]
pub(crate) const fn nz(p: u8, value: u8) -> u8 {
    let mut p = p & !(FLAG_N | FLAG_Z);
    if value == 0 {
        p |= FLAG_Z;
    }
    p |= value & FLAG_N;
    p
}

/// `p` with `flag` set to `on`.
#[must_use]
pub(crate) const fn set(p: u8, flag: u8, on: bool) -> u8 {
    if on { p | flag } else { p & !flag }
}

/// The result of an operation that produces a byte and new flags.
pub(crate) struct Out {
    /// The byte.
    pub(crate) value: u8,
    /// The status register after it.
    pub(crate) p: u8,
}

/// `ADC` in binary mode (`MOS6502-039`).
#[must_use]
pub(crate) const fn adc_binary(a: u8, m: u8, p: u8) -> Out {
    let carry = (p & FLAG_C) as u16;
    let sum = a as u16 + m as u16 + carry;
    let value = sum as u8;
    let overflow = (!(a ^ m) & (a ^ value) & 0x80) != 0;
    let mut p = nz(p, value);
    p = set(p, FLAG_C, sum > 0xff);
    p = set(p, FLAG_V, overflow);
    Out { value, p }
}

/// `ADC` in decimal mode (`MOS6502-060`).
///
/// The adder corrects as it goes (`MOS6502-054`): the low nibble is fixed, `N`
/// and `V` are taken from the half-fixed value, `Z` comes from the binary sum,
/// and only then is the high nibble fixed.
#[must_use]
pub(crate) const fn adc_decimal(a: u8, m: u8, p: u8) -> Out {
    let carry = (p & FLAG_C) as u16;
    let mut tmp = (a as u16 & 0x0f) + (m as u16 & 0x0f) + carry;
    if tmp > 9 {
        tmp += 6;
    }
    tmp = if tmp <= 15 {
        (tmp & 0x0f) + (a as u16 & 0xf0) + (m as u16 & 0xf0)
    } else {
        (tmp & 0x0f) + (a as u16 & 0xf0) + (m as u16 & 0xf0) + 0x10
    };

    let binary = (a as u16 + m as u16 + carry) as u8;
    let mut p = set(p, FLAG_Z, binary == 0);
    p = set(p, FLAG_N, tmp & 0x80 != 0);
    p = set(
        p,
        FLAG_V,
        ((a as u16 ^ tmp) & 0x80) != 0 && ((a ^ m) & 0x80) == 0,
    );

    if (tmp & 0x1f0) > 0x90 {
        tmp += 0x60;
    }
    p = set(p, FLAG_C, (tmp & 0xff0) > 0xf0);
    Out {
        value: tmp as u8,
        p,
    }
}

/// `SBC` in binary mode (`MOS6502-039`).
///
/// The manufacturer's own operation line is `A - M - C̄`, with the note that
/// `C̄` is the borrow, so a set carry means no borrow.
#[must_use]
pub(crate) const fn sbc_binary(a: u8, m: u8, p: u8) -> Out {
    let borrow = 1 - (p & FLAG_C) as u16;
    let diff = (a as u16).wrapping_sub(m as u16).wrapping_sub(borrow);
    let value = diff as u8;
    let overflow = ((a ^ value) & (a ^ m) & 0x80) != 0;
    let mut p = nz(p, value);
    p = set(p, FLAG_C, diff < 0x100);
    p = set(p, FLAG_V, overflow);
    Out { value, p }
}

/// `SBC` in decimal mode (`MOS6502-061`).
///
/// Every flag is the binary subtraction's; only the byte differs.
#[must_use]
pub(crate) const fn sbc_decimal(a: u8, m: u8, p: u8) -> Out {
    let binary = sbc_binary(a, m, p);
    let borrow = 1 - (p & FLAG_C) as i32;

    let lo = (a as i32 & 0x0f) - (m as i32 & 0x0f) - borrow;
    let mut tmp = if lo & 0x10 != 0 {
        ((lo - 6) & 0x0f) | ((a as i32 & 0xf0) - (m as i32 & 0xf0) - 0x10)
    } else {
        (lo & 0x0f) | ((a as i32 & 0xf0) - (m as i32 & 0xf0))
    };
    if tmp & 0x100 != 0 {
        tmp -= 0x60;
    }
    Out {
        value: tmp as u8,
        p: binary.p,
    }
}

/// `ADC`, taking the decimal flag from `p` (`MOS6502-030`).
#[must_use]
pub(crate) const fn adc(a: u8, m: u8, p: u8) -> Out {
    if p & FLAG_D != 0 {
        adc_decimal(a, m, p)
    } else {
        adc_binary(a, m, p)
    }
}

/// `SBC`, taking the decimal flag from `p` (`MOS6502-030`).
#[must_use]
pub(crate) const fn sbc(a: u8, m: u8, p: u8) -> Out {
    if p & FLAG_D != 0 {
        sbc_decimal(a, m, p)
    } else {
        sbc_binary(a, m, p)
    }
}

/// `CMP`, `CPX` and `CPY` (`MOS6502-058`): a subtraction that keeps no result.
#[must_use]
pub(crate) const fn compare(r: u8, m: u8, p: u8) -> u8 {
    let value = r.wrapping_sub(m);
    set(nz(p, value), FLAG_C, r >= m)
}

/// `ARR` in binary mode (`MOS6502-066`).
///
/// The rotate is an ordinary `ROR`; the flags are the `ADC` circuitry's, so
/// `C` is bit 7 of the `AND` result and `V` is bits 7 and 6 of it
/// exclusive-or'd.
#[must_use]
pub(crate) const fn arr_binary(a: u8, imm: u8, p: u8) -> Out {
    let and = a & imm;
    let value = (and >> 1) | ((p & FLAG_C) << 7);
    let mut p = nz(p, value);
    p = set(p, FLAG_C, and & 0x80 != 0);
    p = set(p, FLAG_V, ((and >> 7) ^ (and >> 6)) & 1 != 0);
    Out { value, p }
}

/// `ARR` in decimal mode (`MOS6502-066`).
///
/// An opcode outside the manufacturer's table, run in a mode the manufacturer
/// describes only for `ADC` and `SBC`.
#[must_use]
pub(crate) const fn arr_decimal(a: u8, imm: u8, p: u8) -> Out {
    let tmp = a & imm;
    let mut tmp2 = (tmp >> 1) | ((p & FLAG_C) << 7);

    let mut p = set(p, FLAG_N, p & FLAG_C != 0);
    p = set(p, FLAG_Z, tmp2 == 0);
    p = set(p, FLAG_V, ((tmp2 ^ tmp) & 0x40) != 0);

    if ((tmp as u16 & 0x0f) + (tmp as u16 & 0x01)) > 0x05 {
        tmp2 = (tmp2 & 0xf0) | (tmp2.wrapping_add(0x06) & 0x0f);
    }
    if ((tmp as u16 & 0xf0) + (tmp as u16 & 0x10)) > 0x50 {
        tmp2 = (tmp2 & 0x0f) | (tmp2.wrapping_add(0x60) & 0xf0);
        p |= FLAG_C;
    } else {
        p &= !FLAG_C;
    }
    Out { value: tmp2, p }
}

/// `ARR`, taking the decimal flag from `p`.
#[must_use]
pub(crate) const fn arr(a: u8, imm: u8, p: u8) -> Out {
    if p & FLAG_D != 0 {
        arr_decimal(a, imm, p)
    } else {
        arr_binary(a, imm, p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `MOS6502-039`: the manufacturer's own operation line for `ADC` is
    /// `A + M + C`, and `C` is a ninth bit out of the top.
    #[test]
    fn binary_add_carries_out_of_bit_seven() {
        let out = adc(0xff, 0x01, FLAG_U);
        assert_eq!(out.value, 0x00);
        assert_eq!(out.p & FLAG_C, FLAG_C);
        assert_eq!(out.p & FLAG_Z, FLAG_Z);
    }

    /// `MOS6502-039`: `SBC` is `A - M - C̄`, and the note on the same page says
    /// `C̄` is the borrow — so a clear carry costs one.
    #[test]
    fn binary_subtract_borrows_when_the_carry_is_clear() {
        assert_eq!(sbc(0x10, 0x01, FLAG_U | FLAG_C).value, 0x0f);
        assert_eq!(sbc(0x10, 0x01, FLAG_U).value, 0x0e);
    }

    /// `MOS6502-054`: the adder corrects the binary result as it goes, which
    /// is the manufacturer's own worked example on p. 13.
    #[test]
    fn the_documents_worked_decimal_example() {
        let out = adc(0x79, 0x14, FLAG_D | FLAG_U);
        assert_eq!(out.value, 0x93);
    }

    /// `MOS6502-061`: in decimal mode the byte wraps at ninety-nine and the
    /// flags stay the binary subtraction's.
    #[test]
    fn decimal_subtraction_wraps_at_ninety_nine() {
        let out = sbc(0x00, 0x01, FLAG_D | FLAG_U | FLAG_C);
        assert_eq!(out.value, 0x99);
        assert_eq!(out.p & FLAG_C, 0);
    }
}
