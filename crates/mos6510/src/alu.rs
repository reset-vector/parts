//! The arithmetic, the flags, and the two instructions that read the decimal
//! flag.
//!
//! Everything here is integer.

/// Negative.
pub const FLAG_N: u8 = 0x80;
/// Overflow.
pub const FLAG_V: u8 = 0x40;
/// Bit 5 of the status register, which no instruction clears (`MOS6510-023`).
pub const FLAG_U: u8 = 0x20;
/// Break. Not a register bit at all — it is what the part pushes, and what it
/// pushes says whether the push came from `BRK` (`MOS6510-078`).
pub const FLAG_B: u8 = 0x10;
/// Decimal.
pub const FLAG_D: u8 = 0x08;
/// Interrupt disable.
pub const FLAG_I: u8 = 0x04;
/// Zero.
pub const FLAG_Z: u8 = 0x02;
/// Carry.
pub const FLAG_C: u8 = 0x01;

/// `p` with the `N` and `Z` flags set from `value` (`MOS6510-024`).
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

/// `ADC` in binary mode (`MOS6510-051`).
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

/// `ADC` in decimal mode (`MOS6510-052`).
///
/// The shape is Groepaz, *NMOS 6510 Unintended Opcodes — no more secrets*
/// v1.0, §"Unintended decimal mode" — ADC, p. 74's pseudocode: the low nibble is
/// fixed, `N` and `V` are taken from the half-fixed value, `Z` comes from the
/// binary sum, and only then is the high nibble fixed.
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

/// `SBC` in binary mode (`MOS6510-053`).
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

/// `SBC` in decimal mode (`MOS6510-054`).
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

/// `ADC`, taking the decimal flag from `p` (`MOS6510-050`).
#[must_use]
pub(crate) const fn adc(a: u8, m: u8, p: u8) -> Out {
    if p & FLAG_D != 0 {
        adc_decimal(a, m, p)
    } else {
        adc_binary(a, m, p)
    }
}

/// `SBC`, taking the decimal flag from `p` (`MOS6510-050`).
#[must_use]
pub(crate) const fn sbc(a: u8, m: u8, p: u8) -> Out {
    if p & FLAG_D != 0 {
        sbc_decimal(a, m, p)
    } else {
        sbc_binary(a, m, p)
    }
}

/// `CMP`, `CPX` and `CPY` (`MOS6510-036`): a subtraction that keeps no result.
#[must_use]
pub(crate) const fn compare(r: u8, m: u8, p: u8) -> u8 {
    let value = r.wrapping_sub(m);
    set(nz(p, value), FLAG_C, r >= m)
}

/// `ARR` in binary mode (`MOS6510-064`).
///
/// The rotate is an ordinary `ROR`; the flags are the `ADC` circuitry's, so
/// `C` is bit 7 of the `AND` result and `V` is bits 7 and 6 of it exclusive-or'd.
#[must_use]
pub(crate) const fn arr_binary(a: u8, imm: u8, p: u8) -> Out {
    let and = a & imm;
    let value = (and >> 1) | ((p & FLAG_C) << 7);
    let mut p = nz(p, value);
    p = set(p, FLAG_C, and & 0x80 != 0);
    p = set(p, FLAG_V, ((and >> 7) ^ (and >> 6)) & 1 != 0);
    Out { value, p }
}

/// `ARR` in decimal mode (`MOS6510-065`).
///
/// The shape is Groepaz, *NMOS 6510 Unintended Opcodes — no more secrets*
/// v1.0, §"Unintended decimal mode" — ARR, p. 78's pseudocode.
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

    /// *NMOS 6510 Unintended Opcodes — no more secrets* v1.0
    /// §"Unintended decimal mode" p. 71 gives this one worked example outright.
    #[test]
    fn the_documents_worked_decimal_example() {
        let out = adc(0x80, 0x80, FLAG_D | FLAG_U);
        assert_eq!(out.value, 0x60);
        assert_eq!(out.p & FLAG_C, FLAG_C);
        assert_eq!(out.p & FLAG_Z, FLAG_Z, "Z is the binary sum's, and it is 0");
    }

    /// The "invalid BCD" list on p. 72, every row of it.
    #[test]
    fn invalid_bcd_nibbles() {
        for (a, m, want) in [
            (0x00u8, 0x1fu8, 0x25u8),
            (0x10, 0x1f, 0x35),
            (0x05, 0x1f, 0x2a),
            (0x0f, 0x0a, 0x1f),
            (0x0f, 0x0b, 0x10),
        ] {
            assert_eq!(adc(a, m, FLAG_D | FLAG_U).value, want, "{a:02x} + {m:02x}");
        }
    }

    /// p. 76: "In decimal mode, there is a similar wraparound effect:
    /// $00 - $01 = $99, and the carry is clear."
    #[test]
    fn decimal_subtraction_wraps_at_ninety_nine() {
        let out = sbc(0x00, 0x01, FLAG_D | FLAG_U | FLAG_C);
        assert_eq!(out.value, 0x99);
        assert_eq!(out.p & FLAG_C, 0);
    }
}
