//! The arithmetic and logic unit, and the five-bit flag register beside it.
//!
//! **The flag byte here is the one `PUSH PSW` writes** (`I8080-047`),
//! constants and all, so that the round trip through the stack is the identity
//! and nothing has to reassemble it.
//!
//! The auxiliary carry after an `AND` is [`and`]'s rule (`I8080-075`).

/// Sign: bit 7 of the result (`I8080-043`).
pub const FLAG_S: u8 = 1 << 7;
/// Zero: the result was zero (`I8080-043`).
pub const FLAG_Z: u8 = 1 << 6;
/// Auxiliary carry: a carry out of bit 3 into bit 4 (`I8080-046`).
pub const FLAG_AC: u8 = 1 << 4;
/// Parity: **even** parity, the modulo 2 sum of the bits being zero
/// (`I8080-044`). This bit is parity on every instruction that writes it and
/// is never an overflow, which is where the Z80 next door diverges.
pub const FLAG_P: u8 = 1 << 2;
/// Carry, or a borrow after a subtraction or comparison (`I8080-045`).
pub const FLAG_CY: u8 = 1 << 0;

/// Bit 1 of the flag byte, which belongs to no flag and always reads 1; bits 3
/// and 5 always read 0 (`I8080-047`).
pub const FLAG_ONE: u8 = 1 << 1;

/// The bits a flag actually occupies. Of the rest, bit 1 is [`FLAG_ONE`]
/// and bits 3 and 5 read 0.
pub const FLAG_MASK: u8 = FLAG_S | FLAG_Z | FLAG_AC | FLAG_P | FLAG_CY;

/// The flag byte with the five flags cleared and the three constants in place.
pub(crate) const FLAGS_CLEAR: u8 = FLAG_ONE;

/// Force the three bits that belong to no flag, whatever was written over
/// them. `POP PSW` takes only the five (`I8080-048`), so a byte arriving from
/// the stack passes through here.
#[must_use]
pub(crate) const fn fix_constant_bits(flags: u8) -> u8 {
    (flags & FLAG_MASK) | FLAG_ONE
}

/// Sign, zero and parity from a result byte, under the standard rules of
/// `I8080-043` and `I8080-044`.
#[must_use]
pub(crate) const fn szp(value: u8) -> u8 {
    let mut f = FLAG_ONE;
    if value & 0x80 != 0 {
        f |= FLAG_S;
    }
    if value == 0 {
        f |= FLAG_Z;
    }
    if value.count_ones().is_multiple_of(2) {
        f |= FLAG_P;
    }
    f
}

/// Eight-bit addition with a carry in, setting all five flags
/// (`I8080-054`).
///
/// Auxiliary carry is the carry out of bit 3 into bit 4 of the addition
/// actually performed (`I8080-046`).
#[must_use]
pub(crate) const fn add(a: u8, b: u8, carry_in: bool) -> (u8, u8) {
    let cin = carry_in as u16;
    let wide = a as u16 + b as u16 + cin;
    let result = wide as u8;
    let mut f = szp(result);
    if wide > 0xff {
        f |= FLAG_CY;
    }
    if (a & 0x0f) + (b & 0x0f) + cin as u8 > 0x0f {
        f |= FLAG_AC;
    }
    (result, f)
}

/// Eight-bit subtraction with a borrow in, setting all five flags.
///
/// `I8080-045` says every subtraction is performed in two's complement and
/// sets carry **to indicate a borrow**, so the flag is the complement of the
/// carry out of `a + !b + !borrow`. The auxiliary carry is read off that same
/// addition (`I8080-046`).
#[must_use]
pub(crate) const fn sub(a: u8, b: u8, borrow_in: bool) -> (u8, u8) {
    let (result, f) = add(a, !b, !borrow_in);
    (result, f ^ FLAG_CY)
}

/// `ANA` and `ANI` (`I8080-059`, `I8080-060`, `I8080-075`).
///
/// **The auxiliary carry is the OR of bit 3 of the two operands**
/// (`I8080-075`).
#[must_use]
pub(crate) const fn and(a: u8, b: u8) -> (u8, u8) {
    let result = a & b;
    let mut f = szp(result);
    if (a | b) & 0x08 != 0 {
        f |= FLAG_AC;
    }
    (result, f)
}

/// `XRA` and `XRI` (`I8080-059`, `I8080-060`), which clear carry and auxiliary
/// carry both.
#[must_use]
pub(crate) const fn xor(a: u8, b: u8) -> (u8, u8) {
    let result = a ^ b;
    (result, szp(result))
}

/// `ORA` and `ORI` (`I8080-059`, `I8080-060`), on the same terms as [`xor`].
#[must_use]
pub(crate) const fn or(a: u8, b: u8) -> (u8, u8) {
    let result = a | b;
    (result, szp(result))
}

/// `INR` — every flag except carry (`I8080-055`).
#[must_use]
pub(crate) const fn inr(value: u8, carry: bool) -> (u8, u8) {
    let (result, f) = add(value, 1, false);
    let mut f = f & !FLAG_CY;
    if carry {
        f |= FLAG_CY;
    }
    (result, f)
}

/// `DCR` — every flag except carry (`I8080-055`).
///
/// The decrement is an addition of `0xff`, so the auxiliary carry falls out of
/// `I8080-046` the same way it does for [`sub`].
#[must_use]
pub(crate) const fn dcr(value: u8, carry: bool) -> (u8, u8) {
    let (result, f) = add(value, 0xff, false);
    let mut f = f & !FLAG_CY;
    if carry {
        f |= FLAG_CY;
    }
    (result, f)
}

/// `CMP` and `CPI` — the flags of a subtraction whose result is discarded
/// (`I8080-061`).
#[must_use]
pub(crate) const fn cmp(a: u8, b: u8) -> u8 {
    let (_, f) = sub(a, b, false);
    f
}

/// `DAA` (`I8080-076`), as one addition of a correction whose two
/// halves are decided from the accumulator **before** either is applied.
///
/// Both halves of the correction are decided from the original value, so
/// `DAA` of `FAh` gives `60h` (`I8080-076`).
///
/// Carry is sticky: set by the high correction, never cleared. Auxiliary carry
/// is the carry out of bit 3 of the addition actually performed.
#[must_use]
pub(crate) const fn daa(a: u8, flags: u8) -> (u8, u8) {
    let carry_in = flags & FLAG_CY != 0;
    let aux_in = flags & FLAG_AC != 0;

    let mut correction = 0;
    if (a & 0x0f) > 9 || aux_in {
        correction |= 0x06;
    }
    let high = a > 0x99 || carry_in;
    if high {
        correction |= 0x60;
    }

    let (value, f) = add(a, correction, false);
    let mut f = f & !FLAG_CY;
    if high || carry_in {
        f |= FLAG_CY;
    }
    (value, f)
}

/// `RLC` — the bit shifted out reaches both the low end and carry
/// (`I8080-062`).
#[must_use]
pub(crate) const fn rlc(a: u8, flags: u8) -> (u8, u8) {
    let out = a >> 7;
    ((a << 1) | out, keep_but_carry(flags, out != 0))
}

/// `RRC` — the mirror of [`rlc`].
#[must_use]
pub(crate) const fn rrc(a: u8, flags: u8) -> (u8, u8) {
    let out = a & 1;
    ((a >> 1) | (out << 7), keep_but_carry(flags, out != 0))
}

/// `RAL` — rotate left through carry (`I8080-062`).
#[must_use]
pub(crate) const fn ral(a: u8, flags: u8) -> (u8, u8) {
    let carry_in = (flags & FLAG_CY != 0) as u8;
    ((a << 1) | carry_in, keep_but_carry(flags, a & 0x80 != 0))
}

/// `RAR` — rotate right through carry (`I8080-062`).
#[must_use]
pub(crate) const fn rar(a: u8, flags: u8) -> (u8, u8) {
    let carry_in = (flags & FLAG_CY != 0) as u8;
    (
        (a >> 1) | (carry_in << 7),
        keep_but_carry(flags, a & 1 != 0),
    )
}

/// The four rotates affect only carry, so the other four flags survive
/// unchanged (`I8080-062`).
const fn keep_but_carry(flags: u8, carry: bool) -> u8 {
    let kept = flags & !FLAG_CY;
    if carry { kept | FLAG_CY } else { kept }
}

/// `DAD` — sixteen-bit addition affecting carry alone (`I8080-057`).
#[must_use]
pub(crate) const fn dad(hl: u16, rp: u16, flags: u8) -> (u16, u8) {
    let wide = hl as u32 + rp as u32;
    (wide as u16, keep_but_carry(flags, wide > 0xffff))
}
