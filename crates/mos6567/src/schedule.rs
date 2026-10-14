//! The raster line of the 6567R8, cycle by cycle.
//!
//! `MOS6567-048` gives the line's length and X coordinate, `MOS6567-049` what
//! the part fetches in each half-cycle, and `MOS6567-050` where `BA` falls.

/// Cycles per raster line on the 6567R8 (`MOS6567-004`, `MOS6567-048`).
pub const CYCLES_PER_LINE: usize = 65;

/// The number of a line's last cycle; the first is 1.
pub(crate) const LAST_CYCLE: u8 = CYCLES_PER_LINE as u8;

/// Dots per cycle (`MOS6567-005`).
pub const DOTS_PER_CYCLE: usize = 8;

/// Dots per raster line (`MOS6567-004`).
pub const DOTS_PER_LINE: usize = CYCLES_PER_LINE * DOTS_PER_CYCLE;

/// The X coordinate at the first dot of cycle `cycle`, `1..=65`.
///
/// `$19C` at cycle 1 and eight more per cycle (`MOS6567-048`). The R8's line
/// is eight dots longer than its nine-bit coordinate, and where the surplus
/// falls is `MOS6567-OPEN-05`: this crate repeats `$18C`–`$193` over cycles
/// 63 and 64. A `cycle` outside `1..=65` continues the count modulo `$200`.
#[must_use]
pub(crate) const fn x_at(cycle: u8) -> u16 {
    match cycle {
        63 | 64 => 0x18c,
        65 => 0x194,
        n => 0x19c_u16.wrapping_add((n as u16).wrapping_sub(1).wrapping_mul(8)) & 0x1ff,
    }
}

/// The cycle in which sprite `k`'s pointer is fetched, in the first phase
/// (`MOS6567-049`): 60, 62, 64, 1, 3, 5, 7, 9.
#[must_use]
pub(crate) const fn pointer_cycle(k: u8) -> u8 {
    match k {
        0 => 60,
        1 => 62,
        2 => 64,
        n => 2 * n - 5,
    }
}

/// The cycle `n` cycles after `cycle`, wrapping at the line's end.
#[must_use]
pub(crate) const fn after(cycle: u8, n: u8) -> u8 {
    (cycle - 1 + n) % LAST_CYCLE + 1
}

/// The cycle `n` cycles before `cycle`, wrapping at the line's start.
#[must_use]
pub(crate) const fn before(cycle: u8, n: u8) -> u8 {
    (cycle + LAST_CYCLE - 1 - n % LAST_CYCLE) % LAST_CYCLE + 1
}

/// The first refresh cycle (`MOS6567-049`); five follow in sequence.
pub(crate) const REFRESH_FIRST: u8 = 11;
/// The last refresh cycle.
pub(crate) const REFRESH_LAST: u8 = 15;
/// The first cycle whose first phase is a graphics fetch (`MOS6567-049`).
pub(crate) const GRAPHICS_FIRST: u8 = 16;
/// The last cycle whose first phase is a graphics fetch.
pub(crate) const GRAPHICS_LAST: u8 = 55;
/// The first cycle whose second phase is a matrix fetch on a bad line
/// (`MOS6567-049`, `MOS6567-052`).
pub(crate) const MATRIX_FIRST: u8 = 15;
/// The last cycle whose second phase is a matrix fetch on a bad line.
pub(crate) const MATRIX_LAST: u8 = 54;
/// The line cycles `BA` is held low on a bad line, as `(min, max)`, both
/// included: it falls at the first, three before the first matrix fetch
/// (`MOS6567-050`), and is held through the last matrix fetch.
pub const BAD_LINE_BA_RANGE_CYCLES: (usize, usize) = (12, 54);
/// The cycle `VC` is loaded from `VCBASE` (`MOS6567-052`).
pub(crate) const VC_LOAD: u8 = 14;
/// The cycle the row counter is checked for 7 (`MOS6567-052`).
pub(crate) const RC_CHECK: u8 = 58;
/// The cycle a sprite's `MCBASE` takes its first increment (`MOS6567-057`).
pub(crate) const MCBASE_FIRST: u8 = 15;
/// The cycle a sprite's `MCBASE` takes its second increment and is checked
/// for 63 (`MOS6567-057`).
pub(crate) const MCBASE_SECOND: u8 = 16;
/// The cycle a sprite's expansion flip-flop is inverted and its Y compare is
/// first made (`MOS6567-057`, `MOS6567-OPEN-04`).
pub(crate) const SPRITE_COMPARE_FIRST: u8 = 57;
/// The second Y-compare cycle (`MOS6567-OPEN-04`).
pub(crate) const SPRITE_COMPARE_SECOND: u8 = 58;
/// The cycle every sprite's `MC` is loaded from `MCBASE` and its display is
/// switched on: sprite 0's pointer cycle (`MOS6567-OPEN-04`).
pub(crate) const SPRITE_LOAD: u8 = 60;
/// The cycles before its pointer cycle a sprite's `BA` falls
/// (`MOS6567-010`, `MOS6567-050`).
pub(crate) const BA_LEAD: u8 = 3;
