//! The 8-bit ALU, the 16-bit addition, and the flag rules.
//!
//! Every function here is pure: it takes the operands and the incoming flags
//! and returns the result and the outgoing flags, with no machine cycle
//! anywhere near it.
//!
//! Two rules carry the weight. `Z80-032` is the default — `YF` and `XF` are
//! copies of bits 5 and 3 of the result — and most of this module's exceptions
//! break it. `Z80-084` supplies the ordinary documented effects.
//!
//! # Two more exceptions
//!
//! - **`CP`** takes `YF` and `XF` from the operand, not from the difference it
//!   discards (`Z80-089`).
//! - **`SCF` and `CCF`** take them from `A` or-ed with whatever of `F` the
//!   previous opcode did not compute (`Z80-093`).

use crate::regs::{FLAG_C, FLAG_H, FLAG_N, FLAG_PV, FLAG_S, FLAG_XF, FLAG_YF, FLAG_Z};

/// `S`, `Z`, `YF` and `XF` as a result sets them.
///
/// `Z80-039` puts `S` at bit 7, `Z80-038` sets `Z` on a zero result, and
/// `Z80-032` copies bits 5 and 3 into `YF` and `XF` — which is why masking the
/// result gives three of the four outright.
const fn szyx(result: u8) -> u8 {
    (result & (FLAG_S | FLAG_YF | FLAG_XF)) | if result == 0 { FLAG_Z } else { 0 }
}

/// `P/V` as even parity: set when the number of set bits is even (`Z80-035`).
const fn parity(value: u8) -> u8 {
    if value.count_ones().is_multiple_of(2) {
        FLAG_PV
    } else {
        0
    }
}

/// `ADD A,s` and `ADC A,s`.
///
/// `H` is the carry out of bit 3 (`Z80-037`), `P/V` is signed overflow
/// (`Z80-035`) — the operands agreeing in sign and the result disagreeing — and
/// `N` is reset because this is an addition (`Z80-034`).
pub(crate) const fn add(a: u8, operand: u8, carry_in: bool) -> (u8, u8) {
    let carry = carry_in as u16;
    let sum = a as u16 + operand as u16 + carry;
    let result = sum as u8;
    let half = (a & 0x0F) as u16 + (operand & 0x0F) as u16 + carry > 0x0F;
    let overflow = (a ^ result) & (operand ^ result) & 0x80 != 0;
    let flags = szyx(result)
        | if half { FLAG_H } else { 0 }
        | if overflow { FLAG_PV } else { 0 }
        | if sum > 0xFF { FLAG_C } else { 0 };
    (result, flags)
}

/// `SUB s`, `SBC A,s` and the comparison behind `CP s`.
///
/// `H` is the borrow into bit 4 and `C` the borrow out of bit 7 (`Z80-033`),
/// `P/V` is signed overflow — the operands differing in sign and the result
/// taking the operand's — and `N` is set (`Z80-034`).
pub(crate) const fn sub(a: u8, operand: u8, carry_in: bool) -> (u8, u8) {
    let carry = carry_in as u16;
    let difference = (a as u16).wrapping_sub(operand as u16).wrapping_sub(carry);
    let result = difference as u8;
    let half = ((a & 0x0F) as u16) < ((operand & 0x0F) as u16 + carry);
    let overflow = (a ^ operand) & (a ^ result) & 0x80 != 0;
    let flags = szyx(result)
        | FLAG_N
        | if half { FLAG_H } else { 0 }
        | if overflow { FLAG_PV } else { 0 }
        | if difference > 0xFF { FLAG_C } else { 0 };
    (result, flags)
}

/// `CP s`: `SUB` with the result thrown away.
///
/// `A` is unchanged and only the flags survive. `YF` and `XF` come from the
/// **operand**, not from the discarded difference (`Z80-089`) — the one place
/// `Z80-032`'s default is wrong about an ordinary ALU instruction.
pub(crate) const fn cp(a: u8, operand: u8) -> u8 {
    let (_, flags) = sub(a, operand, false);
    (flags & !(FLAG_YF | FLAG_XF)) | (operand & (FLAG_YF | FLAG_XF))
}

/// `AND s`: `H` set and `C` reset (`Z80-084`, `AND s` p. 158), `P/V` parity.
pub(crate) const fn and(a: u8, operand: u8) -> (u8, u8) {
    let result = a & operand;
    (result, szyx(result) | FLAG_H | parity(result))
}

/// `OR s`: `H` and `C` both reset, `P/V` parity.
pub(crate) const fn or(a: u8, operand: u8) -> (u8, u8) {
    let result = a | operand;
    (result, szyx(result) | parity(result))
}

/// `XOR s`: as `OR`.
pub(crate) const fn xor(a: u8, operand: u8) -> (u8, u8) {
    let result = a ^ operand;
    (result, szyx(result) | parity(result))
}

/// `INC r`.
///
/// `C` is the one flag an increment does not touch, which is what makes `INC`
/// usable inside a multi-byte addition. `P/V` marks the signed overflow at
/// `7Fh`, in the manual's own words (`Z80-084`, `INC r` p. 165).
pub(crate) const fn inc(value: u8, flags: u8) -> (u8, u8) {
    let result = value.wrapping_add(1);
    let new = szyx(result)
        | if value & 0x0F == 0x0F { FLAG_H } else { 0 }
        | if value == 0x7F { FLAG_PV } else { 0 };
    (result, new | (flags & FLAG_C))
}

/// `DEC r`, the mirror of [`inc`]: `C` untouched, `N` set, and `P/V` marking the
/// signed underflow at `80h`.
pub(crate) const fn dec(value: u8, flags: u8) -> (u8, u8) {
    let result = value.wrapping_sub(1);
    let new = szyx(result)
        | FLAG_N
        | if value & 0x0F == 0x00 { FLAG_H } else { 0 }
        | if value == 0x80 { FLAG_PV } else { 0 };
    (result, new | (flags & FLAG_C))
}

/// `ADD HL,ss`.
///
/// `Z80-057`: `S`, `Z` and `P/V` are unaffected, `H` is the carry out of bit 11,
/// and `N` is reset. `YF` and `XF` come from the high byte of the result, which
/// the part adds second (`Z80-056`).
pub(crate) const fn add16(target: u16, operand: u16, flags: u8) -> (u16, u8) {
    let sum = target as u32 + operand as u32;
    let result = sum as u16;
    let half = (target & 0x0FFF) + (operand & 0x0FFF) > 0x0FFF;
    let kept = flags & (FLAG_S | FLAG_Z | FLAG_PV);
    let new = ((result >> 8) as u8 & (FLAG_YF | FLAG_XF))
        | if half { FLAG_H } else { 0 }
        | if sum > 0xFFFF { FLAG_C } else { 0 };
    (result, kept | new)
}

/// `DAA`.
///
/// The correction is `Z80-045`'s nine-row table, `C` is `Z80-046`, `H` is
/// `Z80-047`, `P/V` is `Z80-088`, and the rest follows the result with `N`
/// carried through (`Z80-048`). The three tables are written out rather than
/// folded together, and each has its own surprise: `N` does not appear in the
/// carry table at all.
pub(crate) const fn daa(a: u8, flags: u8) -> (u8, u8) {
    let carry_in = flags & FLAG_C != 0;
    let half_in = flags & FLAG_H != 0;
    let subtracting = flags & FLAG_N != 0;
    let high = a >> 4;
    let low = a & 0x0F;

    #[expect(clippy::if_same_then_else, reason = "one branch per row of Z80-045")]
    let correction = if !carry_in && high <= 9 && !half_in && low <= 9 {
        0x00
    } else if !carry_in && high <= 9 && half_in && low <= 9 {
        0x06
    } else if !carry_in && high <= 8 && low >= 0x0A {
        0x06
    } else if !carry_in && high >= 0x0A && !half_in && low <= 9 {
        0x60
    } else if carry_in && !half_in && low <= 9 {
        0x60
    } else {
        0x66
    };

    let result = if subtracting {
        a.wrapping_sub(correction)
    } else {
        a.wrapping_add(correction)
    };

    let carry_out = carry_in || (high >= 9 && low >= 0x0A) || high >= 0x0A;
    let half_out = if subtracting {
        half_in && low <= 0x05
    } else {
        low >= 0x0A
    };

    let new = szyx(result)
        | parity(result)
        | (flags & FLAG_N)
        | if carry_out { FLAG_C } else { 0 }
        | if half_out { FLAG_H } else { 0 };
    (result, new)
}

/// `CPL`: `A` complemented, with `H` and `N` set and `YF`/`XF` from the result.
pub(crate) const fn cpl(a: u8, flags: u8) -> (u8, u8) {
    let result = !a;
    let kept = flags & (FLAG_S | FLAG_Z | FLAG_PV | FLAG_C);
    (
        result,
        kept | FLAG_H | FLAG_N | (result & (FLAG_YF | FLAG_XF)),
    )
}

/// `SCF`: carry set, `H` and `N` reset, and `S`, `Z` and `P/V` left alone
/// (`Z80-043`). `YF` and `XF` are `Z80-093`'s rule over `A`, `F` and `Q`.
pub(crate) const fn scf(a: u8, flags: u8, q: u8) -> u8 {
    (flags & (FLAG_S | FLAG_Z | FLAG_PV)) | undocumented_from_q(a, flags, q) | FLAG_C
}

/// `CCF`: carry inverted, the *previous* carry copied into `H`, `N` reset
/// (`Z80-044`). `YF` and `XF` as `SCF`.
pub(crate) const fn ccf(a: u8, flags: u8, q: u8) -> u8 {
    let carry_in = flags & FLAG_C;
    let kept = (flags & (FLAG_S | FLAG_Z | FLAG_PV)) | undocumented_from_q(a, flags, q);
    kept | (carry_in << 4) | (FLAG_C & !carry_in)
}

/// `Z80-093`: the `YF` and `XF` the two carry instructions produce, which is
/// `A` or-ed with whatever of `F` the previous opcode did *not* write.
///
/// `q` is zero when the previous opcode computed no flags, and otherwise equals
/// `flags`, so `flags ^ q` is either the whole of `F` or nothing.
const fn undocumented_from_q(a: u8, flags: u8, q: u8) -> u8 {
    (a | (flags ^ q)) & (FLAG_YF | FLAG_XF)
}

/// `RLCA`: `A` rotated left, bit 7 into both bit 0 and `C`.
///
/// The four accumulator rotates leave `S`, `Z` and `P/V` alone, reset `H` and
/// `N`, and take `YF` and `XF` from the result (`Z80-084`, `RLCA` p. 205, and
/// `Z80-032` for `YF` and `XF`).
pub(crate) const fn rlca(a: u8, flags: u8) -> (u8, u8) {
    (
        a.rotate_left(1),
        rotate_flags(a.rotate_left(1), flags, a & 0x80 != 0),
    )
}

/// `RRCA`: `A` rotated right, bit 0 into both bit 7 and `C`.
pub(crate) const fn rrca(a: u8, flags: u8) -> (u8, u8) {
    (
        a.rotate_right(1),
        rotate_flags(a.rotate_right(1), flags, a & 0x01 != 0),
    )
}

/// `RLA`: `A` rotated left through `C`.
pub(crate) const fn rla(a: u8, flags: u8) -> (u8, u8) {
    let result = (a << 1) | (flags & FLAG_C);
    (result, rotate_flags(result, flags, a & 0x80 != 0))
}

/// `RRA`: `A` rotated right through `C`.
pub(crate) const fn rra(a: u8, flags: u8) -> (u8, u8) {
    let result = (a >> 1) | ((flags & FLAG_C) << 7);
    (result, rotate_flags(result, flags, a & 1 != 0))
}

/// The flags common to the four accumulator rotates.
const fn rotate_flags(result: u8, flags: u8, carry: bool) -> u8 {
    (flags & (FLAG_S | FLAG_Z | FLAG_PV))
        | (result & (FLAG_YF | FLAG_XF))
        | if carry { FLAG_C } else { 0 }
}

/// The eight `CB` shifts and rotates, in the order the opcode's `y` field gives
/// them: `RLC RRC RL RR SLA SRA SLL SRL`.
///
/// Unlike the four accumulator rotates these set every flag from the result:
/// `S`, `Z`, `YF` and `XF` follow it, `P/V` is parity, `H` and `N` are reset,
/// and `C` takes the bit shifted out. `Z80-085` for the seven Zilog documents
/// and `Z80-086` for `SLL`, which differs from `SLA` only in the bit it shifts
/// in.
pub(crate) const fn shift(op: u8, value: u8, flags: u8) -> (u8, u8) {
    let carry_in = flags & FLAG_C;
    let (result, carry_out) = match op {
        0 => (value.rotate_left(1), value & 0x80),
        1 => (value.rotate_right(1), value & 0x01),
        2 => ((value << 1) | carry_in, value & 0x80),
        3 => ((value >> 1) | (carry_in << 7), value & 0x01),
        4 => (value << 1, value & 0x80),
        5 => ((value >> 1) | (value & 0x80), value & 0x01),
        6 => ((value << 1) | 1, value & 0x80),
        _ => (value >> 1, value & 0x01),
    };
    let new = szyx(result) | parity(result) | if carry_out != 0 { FLAG_C } else { 0 };
    (result, new)
}

/// `BIT b, r`.
///
/// `Z80-040` gives the rule as "the corresponding `AND` with a one-bit mask",
/// with `H` always set, `N` reset and `C` untouched, and `P/V` tracks `Z`
/// because a one-bit mask leaves either no bits set or one. `YF` and `XF` are
/// the exception: they come from the operand rather than from the masked result
/// (`Z80-091`), which is what the two memory forms do with an address.
pub(crate) const fn bit(index: u8, value: u8, flags: u8) -> u8 {
    let masked = value & (1 << index);
    (flags & FLAG_C)
        | FLAG_H
        | (masked & FLAG_S)
        | (value & (FLAG_YF | FLAG_XF))
        | if masked == 0 { FLAG_Z | FLAG_PV } else { 0 }
}

/// `BIT b` against memory, where `YF` and `XF` come from `high` instead.
///
/// For `(HL)` that byte is the high half of the internal register (`Z80-059`)
/// and for `(IX+d)` it is the high half of the computed address (`Z80-042`).
/// Every other flag is `Z80-040`'s, unchanged.
pub(crate) const fn bit_from(index: u8, value: u8, flags: u8, high: u8) -> u8 {
    let base = bit(index, value, flags) & !(FLAG_YF | FLAG_XF);
    base | (high & (FLAG_YF | FLAG_XF))
}

/// `ADC HL,ss`.
///
/// `Z80-056` gives the rule: `S`, `YF`, `H` and `XF` fall out of the high-byte
/// addition and `Z` out of the whole 16-bit result, because the part is 8 bits
/// wide and does the addition in two stages. `P/V` is signed overflow and `N` is
/// reset (`Z80-084`, `ADC HL, ss` p. 190).
pub(crate) const fn adc16(target: u16, operand: u16, flags: u8) -> (u16, u8) {
    let carry = (flags & FLAG_C) as u32;
    let sum = target as u32 + operand as u32 + carry;
    let result = sum as u16;
    let half = (target & 0x0FFF) as u32 + (operand & 0x0FFF) as u32 + carry > 0x0FFF;
    let overflow = (target ^ result) & (operand ^ result) & 0x8000 != 0;
    let new = ((result >> 8) as u8 & (FLAG_S | FLAG_YF | FLAG_XF))
        | if result == 0 { FLAG_Z } else { 0 }
        | if half { FLAG_H } else { 0 }
        | if overflow { FLAG_PV } else { 0 }
        | if sum > 0xFFFF { FLAG_C } else { 0 };
    (result, new)
}

/// `SBC HL,ss`, the mirror of [`adc16`] with `N` set.
pub(crate) const fn sbc16(target: u16, operand: u16, flags: u8) -> (u16, u8) {
    let carry = (flags & FLAG_C) as u32;
    let difference = (target as u32)
        .wrapping_sub(operand as u32)
        .wrapping_sub(carry);
    let result = difference as u16;
    let half = ((target & 0x0FFF) as u32) < ((operand & 0x0FFF) as u32 + carry);
    let overflow = (target ^ operand) & (target ^ result) & 0x8000 != 0;
    let new = ((result >> 8) as u8 & (FLAG_S | FLAG_YF | FLAG_XF))
        | FLAG_N
        | if result == 0 { FLAG_Z } else { 0 }
        | if half { FLAG_H } else { 0 }
        | if overflow { FLAG_PV } else { 0 }
        | if difference > 0xFFFF { FLAG_C } else { 0 };
    (result, new)
}

/// `NEG`: the accumulator subtracted from zero, with `SUB`'s flags.
pub(crate) const fn neg(a: u8) -> (u8, u8) {
    sub(0, a, false)
}

/// The flags `IN r,(C)` leaves (`Z80-058`).
///
/// `S` and `Z` from the byte read and `P/V` its parity, with `C` unchanged;
/// `H` and `N` are reset, and `YF` and `XF` follow `Z80-032`. `IN A,(n)` sets no
/// flag at all, and does not come here.
pub(crate) const fn in_flags(value: u8, flags: u8) -> u8 {
    (flags & FLAG_C) | szyx(value) | parity(value)
}

/// The flags after `LDI`, `LDD`, `LDIR` or `LDDR` (`Z80-049`, `Z80-036`).
///
/// The asymmetry is the thing to get right: `YF` is bit **1** of the transferred
/// byte plus `A`, not bit 5, while `XF` is bit 3 as usual. `H` and `N` are reset
/// (`Z80-084`, `LDI` p. 130) and `S`, `Z` and `C` come through unchanged.
pub(crate) const fn block_transfer_flags(byte: u8, a: u8, more: bool, flags: u8) -> u8 {
    let n = byte.wrapping_add(a);
    (flags & (FLAG_S | FLAG_Z | FLAG_C))
        | if n & 0x02 != 0 { FLAG_YF } else { 0 }
        | if n & 0x08 != 0 { FLAG_XF } else { 0 }
        | if more { FLAG_PV } else { 0 }
}

/// The flags after `CPI`, `CPD`, `CPIR` or `CPDR` (`Z80-050`, `Z80-036`).
///
/// `S`, `Z` and `H` come from the compare the instruction implies, and `YF` and
/// `XF` from bits 1 and 3 of `A − (HL) − H` — where that `H` is the one the
/// compare just produced, not the one on entry.
pub(crate) const fn block_compare_flags(a: u8, byte: u8, more: bool, flags: u8) -> u8 {
    let (difference, compared) = sub(a, byte, false);
    let borrowed = if compared & FLAG_H != 0 { 1 } else { 0 };
    let n = difference.wrapping_sub(borrowed);
    (flags & FLAG_C)
        | (compared & (FLAG_S | FLAG_Z | FLAG_H))
        | FLAG_N
        | if n & 0x02 != 0 { FLAG_YF } else { 0 }
        | if n & 0x08 != 0 { FLAG_XF } else { 0 }
        | if more { FLAG_PV } else { 0 }
}

/// The flags after any of the eight I/O block instructions.
///
/// `Z80-051` for `S`, `Z`, `YF`, `XF` and `N`; `Z80-052` for `H`, `C` and `P/V`.
/// `addend` is what `Z80-053` says to add to the transferred byte: `L` after `HL`
/// has moved for the output forms, and `(C ± 1) & 255` for the input ones. Every
/// flag is written, so none is carried in.
pub(crate) const fn block_io_flags(b_after: u8, byte: u8, addend: u8) -> u8 {
    let k = byte as u16 + addend as u16;
    let carried = k > 0xFF;
    szyx(b_after)
        | if byte & 0x80 != 0 { FLAG_N } else { 0 }
        | if carried { FLAG_H | FLAG_C } else { 0 }
        | parity(((k & 7) as u8) ^ b_after)
}

/// The correction a repeating I/O block instruction makes to `H` and `P/V` on an
/// iteration that goes round again (`Z80-095`).
///
/// `flags` is what [`block_io_flags`] just produced and `b_after` is `B` as the
/// decrement left it. `C` and `N` choose which neighbour of `B` the parity is
/// taken over, and whether the half-carry survives at all. `YF` and `XF` are not
/// touched here: they come from `PC` for every repeating form, which is
/// `Z80-094` and belongs to the repeat rather than to the I/O.
pub(crate) const fn block_io_repeat_flags(flags: u8, b_after: u8) -> u8 {
    let (half, counted) = if flags & FLAG_C == 0 {
        (0, b_after)
    } else if flags & FLAG_N != 0 {
        let half = if b_after & 0x0F == 0x00 { FLAG_H } else { 0 };
        (half, b_after.wrapping_sub(1))
    } else {
        let half = if b_after & 0x0F == 0x0F { FLAG_H } else { 0 };
        (half, b_after.wrapping_add(1))
    };
    let flipped = (flags ^ parity(counted & 7) ^ FLAG_PV) & FLAG_PV;
    (flags & !(FLAG_H | FLAG_PV)) | half | flipped
}

/// `RRD`: the low nibble of `(HL)` into `A`, `A`'s old low nibble into the high
/// nibble of `(HL)`, and `(HL)`'s old high nibble down into its low.
///
/// Returns the new accumulator, the byte to write back, and the flags — which
/// follow the accumulator with `H` and `N` reset and `C` unchanged (`Z80-085`,
/// `RRD` p. 240; `Z80-084` for the flags).
pub(crate) const fn rrd(a: u8, memory: u8, flags: u8) -> (u8, u8, u8) {
    let new_a = (a & 0xF0) | (memory & 0x0F);
    let new_memory = ((a & 0x0F) << 4) | (memory >> 4);
    (new_a, new_memory, nibble_rotate_flags(new_a, flags))
}

/// `RLD`, the other way about (`Z80-085`, `RLD` p. 238).
pub(crate) const fn rld(a: u8, memory: u8, flags: u8) -> (u8, u8, u8) {
    let new_a = (a & 0xF0) | (memory >> 4);
    let new_memory = ((memory & 0x0F) << 4) | (a & 0x0F);
    (new_a, new_memory, nibble_rotate_flags(new_a, flags))
}

/// The flags the two nibble rotates share.
const fn nibble_rotate_flags(a: u8, flags: u8) -> u8 {
    (flags & FLAG_C) | szyx(a) | parity(a)
}

#[cfg(test)]
mod tests {
    use super::{
        add, add16, and, block_io_repeat_flags, ccf, cp, daa, dec, inc, rlca, rra, scf, sub,
    };
    use crate::regs::{FLAG_C, FLAG_H, FLAG_N, FLAG_PV, FLAG_S, FLAG_XF, FLAG_YF, FLAG_Z};

    #[test]
    fn addition_carries_between_bits_three_and_four() {
        assert_eq!(add(0x0F, 0x01, false).1 & FLAG_H, FLAG_H);
        assert_eq!(add(0x0E, 0x01, false).1 & FLAG_H, 0);
    }

    #[test]
    fn addition_overflows_when_two_like_signs_make_the_other() {
        assert_eq!(add(0x7F, 0x01, false).1 & FLAG_PV, FLAG_PV);
        assert_eq!(add(0x7F, 0x01, false).1 & FLAG_C, 0);
        assert_eq!(add(0xFF, 0x01, false).1 & FLAG_C, FLAG_C);
        assert_eq!(add(0xFF, 0x01, false).1 & FLAG_PV, 0);
    }

    #[test]
    fn subtraction_sets_n_and_addition_resets_it() {
        assert_eq!(sub(0x10, 0x01, false).1 & FLAG_N, FLAG_N);
        assert_eq!(add(0x10, 0x01, false).1 & FLAG_N, 0);
    }

    #[test]
    fn the_undocumented_bits_copy_the_result() {
        let (result, flags) = add(0x00, 0x28, false);
        assert_eq!(result, 0x28);
        assert_eq!(flags & (FLAG_YF | FLAG_XF), FLAG_YF | FLAG_XF);
    }

    #[test]
    fn compare_leaves_the_accumulator_and_reports_the_difference() {
        assert_eq!(cp(0x10, 0x10) & FLAG_Z, FLAG_Z);
        assert_eq!(cp(0x10, 0x20) & FLAG_C, FLAG_C);
        assert_eq!(cp(0x20, 0x10) & FLAG_C, 0);
    }

    #[test]
    fn compare_takes_the_undocumented_bits_from_the_operand() {
        let both = FLAG_YF | FLAG_XF;
        assert_eq!(cp(0x28, 0x28) & both, both);
        assert_eq!(cp(0x28, 0x00) & both, 0);
    }

    #[test]
    fn the_logicals_set_h_only_for_and_and_reset_c_for_all() {
        assert_eq!(and(0xFF, 0x0F).1 & FLAG_H, FLAG_H);
        assert_eq!(super::or(0xF0, 0x0F).1 & FLAG_H, 0);
        assert_eq!(super::xor(0xF0, 0x0F).1 & FLAG_H, 0);
        for flags in [
            and(0xFF, 0xFF).1,
            super::or(0xFF, 0x00).1,
            super::xor(0x0F, 0xF0).1,
        ] {
            assert_eq!(flags & FLAG_C, 0);
        }
    }

    #[test]
    fn the_logicals_report_even_parity() {
        assert_eq!(and(0x03, 0xFF).1 & FLAG_PV, FLAG_PV);
        assert_eq!(and(0x01, 0xFF).1 & FLAG_PV, 0);
    }

    #[test]
    fn increment_and_decrement_leave_the_carry_alone() {
        assert_eq!(inc(0xFF, FLAG_C).1 & FLAG_C, FLAG_C);
        assert_eq!(inc(0xFF, 0).1 & FLAG_C, 0);
        assert_eq!(dec(0x00, FLAG_C).1 & FLAG_C, FLAG_C);
    }

    #[test]
    fn increment_overflows_at_seven_f_and_decrement_at_eight_zero() {
        assert_eq!(inc(0x7F, 0).1 & FLAG_PV, FLAG_PV);
        assert_eq!(inc(0x7E, 0).1 & FLAG_PV, 0);
        assert_eq!(dec(0x80, 0).1 & FLAG_PV, FLAG_PV);
        assert_eq!(dec(0x81, 0).1 & FLAG_PV, 0);
    }

    #[test]
    fn sixteen_bit_addition_leaves_sign_zero_and_overflow_alone() {
        let incoming = FLAG_S | FLAG_Z | FLAG_PV;
        let (result, flags) = add16(0x0000, 0x0000, incoming);
        assert_eq!(result, 0);
        assert_eq!(flags & (FLAG_S | FLAG_Z | FLAG_PV), incoming);
        assert_eq!(flags & FLAG_N, 0);
    }

    #[test]
    fn sixteen_bit_addition_half_carries_out_of_bit_eleven() {
        assert_eq!(add16(0x0FFF, 0x0001, 0).1 & FLAG_H, FLAG_H);
        assert_eq!(add16(0x0FFE, 0x0001, 0).1 & FLAG_H, 0);
        assert_eq!(add16(0xFFFF, 0x0001, 0).1 & FLAG_C, FLAG_C);
    }

    #[test]
    fn sixteen_bit_addition_takes_the_undocumented_bits_from_the_high_byte() {
        let (result, flags) = add16(0x2800, 0x0000, 0);
        assert_eq!(result, 0x2800);
        assert_eq!(flags & (FLAG_YF | FLAG_XF), FLAG_YF | FLAG_XF);
    }

    #[test]
    fn daa_corrects_an_addition_of_two_packed_digits() {
        let (sum, flags) = add(0x09, 0x08, false);
        assert_eq!(sum, 0x11);
        assert_eq!(daa(sum, flags).0, 0x17);
    }

    #[test]
    fn daa_corrects_a_subtraction() {
        let (difference, flags) = sub(0x17, 0x09, false);
        assert_eq!(difference, 0x0E);
        assert_eq!(daa(difference, flags).0, 0x08);
    }

    #[test]
    fn daa_sets_parity_from_its_result() {
        assert_eq!(daa(0x00, 0).1 & FLAG_PV, FLAG_PV, "00h has even parity");
        assert_eq!(daa(0x0B, 0).1 & FLAG_PV, FLAG_PV, "0Bh corrects to 11h");
        assert_eq!(daa(0x01, 0).1 & FLAG_PV, 0, "01h has odd parity");
    }

    #[test]
    fn daa_ignores_n_when_it_sets_the_carry() {
        let with_n = daa(0x9A, FLAG_N).1 & FLAG_C;
        let without_n = daa(0x9A, 0).1 & FLAG_C;
        assert_eq!(with_n, FLAG_C);
        assert_eq!(with_n, without_n);
    }

    #[test]
    fn daa_never_moves_n() {
        for flags in [0, FLAG_N, FLAG_N | FLAG_C | FLAG_H] {
            assert_eq!(daa(0x99, flags).1 & FLAG_N, flags & FLAG_N);
        }
    }

    #[test]
    fn daa_sets_the_half_carry_from_the_low_nibble_when_adding() {
        assert_eq!(daa(0x0A, 0).1 & FLAG_H, FLAG_H);
        assert_eq!(daa(0x09, 0).1 & FLAG_H, 0);
        assert_eq!(daa(0x0A, FLAG_N).1 & FLAG_H, 0, "and not when subtracting");
        assert_eq!(daa(0x05, FLAG_N | FLAG_H).1 & FLAG_H, FLAG_H);
    }

    #[test]
    fn scf_and_ccf_keep_sign_zero_and_overflow() {
        let incoming = FLAG_S | FLAG_Z | FLAG_PV;
        assert_eq!(
            scf(0, incoming, incoming) & (FLAG_S | FLAG_Z | FLAG_PV),
            incoming
        );
        assert_eq!(
            scf(0, incoming, incoming) & (FLAG_C | FLAG_H | FLAG_N),
            FLAG_C
        );
        assert_eq!(
            ccf(0, incoming, incoming) & (FLAG_S | FLAG_Z | FLAG_PV),
            incoming
        );
    }

    #[test]
    fn ccf_copies_the_previous_carry_into_the_half_carry() {
        assert_eq!(ccf(0, FLAG_C, FLAG_C) & FLAG_H, FLAG_H);
        assert_eq!(ccf(0, FLAG_C, FLAG_C) & FLAG_C, 0);
        assert_eq!(ccf(0, 0, 0) & FLAG_H, 0);
        assert_eq!(ccf(0, 0, 0) & FLAG_C, FLAG_C);
    }

    #[test]
    fn scf_and_ccf_take_the_undocumented_bits_from_the_accumulator_and_q() {
        let both = FLAG_YF | FLAG_XF;
        assert_eq!(scf(both, 0, 0) & both, both);
        assert_eq!(ccf(both, 0, 0) & both, both);
        assert_eq!(scf(0, both, both) & both, 0);
        assert_eq!(ccf(0, both, both) & both, 0);
        assert_eq!(scf(0, both, 0) & both, both);
        assert_eq!(ccf(0, both, 0) & both, both);
    }

    #[test]
    fn a_repeating_io_block_iteration_recomputes_the_half_carry_and_the_parity() {
        let carried = FLAG_C | FLAG_H | FLAG_PV;

        assert_eq!(block_io_repeat_flags(FLAG_PV, 0x07) & (FLAG_H | FLAG_PV), 0);
        assert_eq!(
            block_io_repeat_flags(FLAG_PV, 0x00) & (FLAG_H | FLAG_PV),
            FLAG_PV
        );

        assert_eq!(
            block_io_repeat_flags(carried | FLAG_N, 0x10) & FLAG_H,
            FLAG_H
        );
        assert_eq!(block_io_repeat_flags(carried | FLAG_N, 0x11) & FLAG_H, 0);
        assert_eq!(block_io_repeat_flags(carried | FLAG_N, 0x10) & FLAG_PV, 0);

        assert_eq!(block_io_repeat_flags(carried, 0x0F) & FLAG_H, FLAG_H);
        assert_eq!(block_io_repeat_flags(carried, 0x0E) & FLAG_H, 0);
        assert_eq!(block_io_repeat_flags(carried, 0x0F) & FLAG_PV, FLAG_PV);
    }

    #[test]
    fn the_accumulator_rotates_move_the_carry_and_nothing_else() {
        let incoming = FLAG_S | FLAG_Z | FLAG_PV | FLAG_H | FLAG_N;
        let (result, flags) = rlca(0x80, incoming);
        assert_eq!(result, 0x01);
        assert_eq!(flags & FLAG_C, FLAG_C);
        assert_eq!(
            flags & (FLAG_S | FLAG_Z | FLAG_PV),
            FLAG_S | FLAG_Z | FLAG_PV
        );
        assert_eq!(flags & (FLAG_H | FLAG_N), 0);
    }

    #[test]
    fn rotating_right_through_the_carry_brings_it_into_bit_seven() {
        let (result, flags) = rra(0x01, FLAG_C);
        assert_eq!(result, 0x80);
        assert_eq!(flags & FLAG_C, FLAG_C);
    }
}
