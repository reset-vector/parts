//! The register file — the part's whole visible state.
//!
//! The layout is flat and named rather than indexed.

use crate::Level;

/// Sign: a copy of bit 7 of the result (`Z80-039`).
pub const FLAG_S: u8 = 1 << 7;
/// Zero: set when the result is zero (`Z80-038`).
pub const FLAG_Z: u8 = 1 << 6;
/// The undocumented bit 5 (`Z80-031`).
pub const FLAG_YF: u8 = 1 << 5;
/// Half carry: the carry between bits 3 and 4 (`Z80-037`).
pub const FLAG_H: u8 = 1 << 4;
/// The undocumented bit 3 (`Z80-031`).
pub const FLAG_XF: u8 = 1 << 3;
/// Parity or overflow, depending on the instruction class (`Z80-035`).
pub const FLAG_PV: u8 = 1 << 2;
/// Add or subtract, kept for `DAA` (`Z80-034`).
pub const FLAG_N: u8 = 1 << 1;
/// Carry (`Z80-033`).
pub const FLAG_C: u8 = 1 << 0;

/// The opcode field value that names `(HL)` rather than a register, and under a
/// `DD` or `FD` prefix names `(IX+d)` or `(IY+d)`.
pub(crate) const REG_MEM: u8 = 6;

/// How the part answers a maskable interrupt, as `IM 0`, `IM 1` and `IM 2` select
/// it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum InterruptMode {
    /// Mode 0: the part executes the instruction the device places on the data
    /// bus (`Z80-073`). A reset selects it (`Z80-012`).
    #[default]
    Im0,
    /// Mode 1: a restart to `0038h` whatever is on the bus (`Z80-074`).
    Im1,
    /// Mode 2: an indirect call through the table at `I` × 256 plus the
    /// device's byte (`Z80-075`).
    Im2,
}

impl InterruptMode {
    /// The mode's number, as the instruction names it.
    pub(crate) const fn number(self) -> u8 {
        match self {
            Self::Im0 => 0,
            Self::Im1 => 1,
            Self::Im2 => 2,
        }
    }

    /// The mode a number names, or `None` for one the part has not got.
    pub(crate) const fn from_number(number: u8) -> Option<Self> {
        match number {
            0 => Some(Self::Im0),
            1 => Some(Self::Im1),
            2 => Some(Self::Im2),
            _ => None,
        }
    }
}

/// The programmer-visible registers, plus `WZ`, the interrupt state and the
/// latches that carry an instruction's or a pin's effect past a boundary.
///
/// `Z80-017` counts 208 bits: eighteen 8-bit registers — two banks of `A`, `F`,
/// `B`, `C`, `D`, `E`, `H` and `L`, plus `I` and `R` — and four 16-bit ones.
/// `WZ` is outside that count.
///
/// `#[non_exhaustive]`, since it gains a field when a round trip needs a new
/// latch: code outside this crate starts from [`Registers::default`] or
/// [`Z80::registers`](crate::Z80::registers) and
/// sets the fields it wants.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub struct Registers {
    /// Accumulator.
    pub a: u8,
    /// Flags, `S Z YF H XF P/V N C` from bit 7 down (`Z80-031`).
    pub f: u8,
    /// General-purpose `B`.
    pub b: u8,
    /// General-purpose `C`.
    pub c: u8,
    /// General-purpose `D`.
    pub d: u8,
    /// General-purpose `E`.
    pub e: u8,
    /// General-purpose `H`.
    pub h: u8,
    /// General-purpose `L`.
    pub l: u8,

    /// Alternate `A`, reachable only through `EX AF, AF'` (`Z80-026`).
    pub a_alt: u8,
    /// Alternate `F`.
    pub f_alt: u8,
    /// Alternate `B`, reachable only through `EXX` (`Z80-026`).
    pub b_alt: u8,
    /// Alternate `C`.
    pub c_alt: u8,
    /// Alternate `D`.
    pub d_alt: u8,
    /// Alternate `E`.
    pub e_alt: u8,
    /// Alternate `H`.
    pub h_alt: u8,
    /// Alternate `L`.
    pub l_alt: u8,

    /// Interrupt vector page, driven on the upper address bits during refresh
    /// (`Z80-018`) and used to build the mode 2 service address (`Z80-075`).
    pub i: u8,
    /// Memory refresh counter. Only its low seven bits are incremented
    /// (`Z80-019`); bit 7 moves only under `LD R,A` (`Z80-020`).
    pub r: u8,

    /// Index register `X`, also addressable as two halves (`Z80-027`).
    pub ix: u16,
    /// Index register `Y`, also addressable as two halves (`Z80-027`).
    pub iy: u16,
    /// Stack pointer.
    pub sp: u16,
    /// Program counter.
    pub pc: u16,

    /// The internal address latch called `WZ`, or `MEMPTR`. Not
    /// programmer-visible, and not part of `Z80-017`'s count.
    ///
    /// [`Registers::default`] zeroes it, which is this crate's choice.
    pub wz: u16,

    /// `Q`: the flags as the last *computation* of them left the register, or
    /// zero where the last opcode fetched computed none (`Z80-093`).
    ///
    /// The one thing that reads it is `SCF` and `CCF`, and the one thing that
    /// clears it is an opcode fetch — a `DD`, `FD`, `CB` or `ED` prefix
    /// included, which is why the prefixed forms of those two behave unlike the
    /// bare ones. Every computation of the flags writes it with `F`, and nothing
    /// else does: a wholesale load of `F` by `POP AF` or `EX AF,AF'` writes the field
    /// directly and leaves this alone.
    pub q: u8,

    /// Interrupt enable flip-flop 1: whether a maskable interrupt is accepted
    /// (`Z80-070`).
    pub iff1: bool,
    /// Interrupt enable flip-flop 2: `IFF1`'s keeper across an `NMI`
    /// (`Z80-067`, `Z80-069`).
    pub iff2: bool,
    /// Interrupt mode (`Z80-073`, `Z80-074`, `Z80-075`).
    pub im: InterruptMode,

    /// The part has executed a `HALT` and re-fetches it, driving `HALT`, until
    /// an interrupt or a reset (`Z80-079`, `Z80-080`); `pc` stands on the
    /// `HALT` until then (`Z80-082`).
    pub halted: bool,
    /// `NMI`'s level at the last tick, against which the next
    /// falling edge is found (`Z80-066`). A line held asserted across a
    /// boundary is not taken again.
    pub nmi_n: Level,
    /// An `NMI` edge has been latched (`Z80-066`) and not yet accepted: the
    /// next boundary accepts it in place of the opcode fetch, `INT`
    /// notwithstanding. Latched rather than sampled, so an edge during a bus
    /// acknowledge is kept (`Z80-009`).
    pub nmi_pending: bool,
    /// The request besides `NMI` latched and not yet served, if any: a bus
    /// request excludes a maskable interrupt (`Z80-009`).
    pub pending: Option<Pending>,
}

/// A request latched and not yet served, held in [`Registers::pending`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Pending {
    /// `INT` was asserted at the sample point that ended the instruction just
    /// ended, with `IFF1` set, no `EI` before it and no bus request
    /// outstanding (`Z80-070`, `Z80-009`): the next boundary accepts it in
    /// place of the opcode fetch, unless [`Registers::nmi_pending`] outranks
    /// it.
    Int,
    /// `BUSREQ` was asserted at the sample point that ended the last machine
    /// cycle, so the buses are released before the next one begins
    /// (`Z80-008`), and a maskable interrupt is not accepted (`Z80-009`).
    Busreq,
}

impl Registers {
    /// The power-on state, the same as [`Registers::default`].
    #[deprecated(since = "18.2.0", note = "use `Registers::default()`")]
    #[must_use]
    pub const fn new() -> Self {
        Self::power_on()
    }

    /// [`Registers::default`], for the crate's `const` constructors.
    pub(crate) const fn power_on() -> Self {
        Self {
            a: 0xFF,
            f: 0xFF,
            b: 0xFF,
            c: 0xFF,
            d: 0xFF,
            e: 0xFF,
            h: 0xFF,
            l: 0xFF,
            a_alt: 0xFF,
            f_alt: 0xFF,
            b_alt: 0xFF,
            c_alt: 0xFF,
            d_alt: 0xFF,
            e_alt: 0xFF,
            h_alt: 0xFF,
            l_alt: 0xFF,
            i: 0x00,
            r: 0x00,
            ix: 0xFFFF,
            iy: 0xFFFF,
            sp: 0xFFFF,
            pc: crate::vector::RESET,
            wz: 0x0000,
            q: 0x00,
            iff1: false,
            iff2: false,
            im: InterruptMode::Im0,
            halted: false,
            nmi_n: Level::High,
            nmi_pending: false,
            pending: None,
        }
    }

    /// Applies a reset (`Z80-012`).
    ///
    /// These registers and no others: what `AF`, `SP` and the general set hold
    /// afterwards is `Z80-025`'s, and is not touched here.
    pub(crate) const fn reset(&mut self) {
        self.pc = crate::vector::RESET;
        self.i = 0;
        self.r = 0;
        self.iff1 = false;
        self.iff2 = false;
        self.im = InterruptMode::Im0;
    }

    /// Sets the flags, and with them `Q`.
    ///
    /// Every *computation* of the flags goes through here, which is what makes
    /// [`Registers::q`] mean what `Z80-093` needs it to mean.
    pub(crate) const fn set_flags(&mut self, value: u8) {
        self.f = value;
        self.q = value;
    }

    /// The address driven during T3-T4 of an opcode fetch.
    ///
    /// `I` on the upper eight bits (`Z80-018`) and `R` as it stands *before* the
    /// fetch increments it (`Z80-029`) on the lower eight. That order is this
    /// crate's choice under `Z80-OPEN-05`, and `R`'s bit 7 on `A7` its choice
    /// under `Z80-OPEN-04`.
    #[must_use]
    pub(crate) const fn refresh_address(&self) -> u16 {
        ((self.i as u16) << 8) | self.r as u16
    }

    /// Increments `R` the way an M1 cycle does.
    ///
    /// Only the low seven bits take part (`Z80-019`), so 127 wraps to 0 rather
    /// than to 128, and bit 7 comes through untouched (`Z80-020`).
    pub(crate) const fn inc_r(&mut self) {
        self.r = (self.r & 0x80) | (self.r.wrapping_add(1) & 0x7F);
    }

    /// Exchanges `BC`, `DE` and `HL` with their alternates (`Z80-026`).
    ///
    /// Independent of [`Registers::ex_af`], and touching neither `IX`, `IY`,
    /// `I`, `R` nor `SP`.
    pub(crate) const fn exx(&mut self) {
        (self.b, self.b_alt) = (self.b_alt, self.b);
        (self.c, self.c_alt) = (self.c_alt, self.c);
        (self.d, self.d_alt) = (self.d_alt, self.d);
        (self.e, self.e_alt) = (self.e_alt, self.e);
        (self.h, self.h_alt) = (self.h_alt, self.h);
        (self.l, self.l_alt) = (self.l_alt, self.l);
    }

    /// Exchanges `AF` with `AF'`, and nothing else (`Z80-026`).
    pub(crate) const fn ex_af(&mut self) {
        (self.a, self.a_alt) = (self.a_alt, self.a);
        (self.f, self.f_alt) = (self.f_alt, self.f);
    }

    /// The register an opcode's three-bit field selects, in the encoding the
    /// opcode uses: `B C D E H L (HL) A`.
    ///
    /// # Panics
    ///
    /// On [`REG_MEM`], which names memory and not a register. Decode routes that
    /// value to a memory cycle and never here.
    #[must_use]
    pub(crate) const fn reg(&self, index: u8) -> u8 {
        match index {
            0 => self.b,
            1 => self.c,
            2 => self.d,
            3 => self.e,
            4 => self.h,
            5 => self.l,
            7 => self.a,
            _ => panic!("opcode register field 6 is (HL), which is memory"),
        }
    }

    /// Writes the register an opcode's three-bit field selects.
    ///
    /// # Panics
    ///
    /// On [`REG_MEM`], for the reason [`Registers::reg`] gives.
    pub(crate) const fn set_reg(&mut self, index: u8, value: u8) {
        match index {
            0 => self.b = value,
            1 => self.c = value,
            2 => self.d = value,
            3 => self.e = value,
            4 => self.h = value,
            5 => self.l = value,
            7 => self.a = value,
            _ => panic!("opcode register field 6 is (HL), which is memory"),
        }
    }

    /// The pair an opcode's two-bit field selects: `BC DE HL SP`.
    #[must_use]
    pub(crate) const fn pair(&self, index: u8) -> u16 {
        match index {
            0 => self.bc(),
            1 => self.de(),
            2 => self.hl(),
            _ => self.sp,
        }
    }

    /// Writes the pair an opcode's two-bit field selects.
    pub(crate) const fn set_pair(&mut self, index: u8, value: u16) {
        match index {
            0 => self.set_bc(value),
            1 => self.set_de(value),
            2 => self.set_hl(value),
            _ => self.sp = value,
        }
    }

    /// The pair the stack instructions' two-bit field selects: `BC DE HL AF`.
    #[must_use]
    pub(crate) const fn stack_pair(&self, index: u8) -> u16 {
        match index {
            0 => self.bc(),
            1 => self.de(),
            2 => self.hl(),
            _ => self.af(),
        }
    }

    /// Writes the pair the stack instructions' two-bit field selects.
    pub(crate) const fn set_stack_pair(&mut self, index: u8, value: u16) {
        match index {
            0 => self.set_bc(value),
            1 => self.set_de(value),
            2 => self.set_hl(value),
            _ => self.set_af(value),
        }
    }
}

/// Builds the pair writers the instructions use.
macro_rules! set_pairs {
    ($($set:ident = $hi:ident : $lo:ident, $doc:literal;)*) => {
        impl Registers {
            $(
                #[doc = $doc]
                pub(crate) const fn $set(&mut self, value: u16) {
                    self.$hi = (value >> 8) as u8;
                    self.$lo = value as u8;
                }
            )*
        }
    };
}

/// The register pairs.
impl Registers {
    /// `A` in the high byte, `F` in the low — the order `PUSH AF` uses.
    #[must_use]
    pub const fn af(&self) -> u16 {
        ((self.a as u16) << 8) | self.f as u16
    }

    /// `B` and `C` as one 16-bit register.
    #[must_use]
    pub const fn bc(&self) -> u16 {
        ((self.b as u16) << 8) | self.c as u16
    }

    /// `D` and `E` as one 16-bit register.
    #[must_use]
    pub const fn de(&self) -> u16 {
        ((self.d as u16) << 8) | self.e as u16
    }

    /// `H` and `L` as one 16-bit register.
    #[must_use]
    pub const fn hl(&self) -> u16 {
        ((self.h as u16) << 8) | self.l as u16
    }

    /// The alternate `AF`.
    #[must_use]
    pub const fn af_alt(&self) -> u16 {
        ((self.a_alt as u16) << 8) | self.f_alt as u16
    }

    /// The alternate `BC`.
    #[must_use]
    pub const fn bc_alt(&self) -> u16 {
        ((self.b_alt as u16) << 8) | self.c_alt as u16
    }

    /// The alternate `DE`.
    #[must_use]
    pub const fn de_alt(&self) -> u16 {
        ((self.d_alt as u16) << 8) | self.e_alt as u16
    }

    /// The alternate `HL`.
    #[must_use]
    pub const fn hl_alt(&self) -> u16 {
        ((self.h_alt as u16) << 8) | self.l_alt as u16
    }
}

set_pairs! {
    set_af = a : f, "Writes `AF`.";
    set_bc = b : c, "Writes `BC`.";
    set_de = d : e, "Writes `DE`.";
    set_hl = h : l, "Writes `HL`.";
}

/// The 8-bit halves of `IX` and `IY` (`Z80-027`).
impl Registers {
    /// The high half of `IX`.
    #[must_use]
    pub const fn ixh(&self) -> u8 {
        (self.ix >> 8) as u8
    }

    /// The low half of `IX`.
    #[must_use]
    pub const fn ixl(&self) -> u8 {
        self.ix as u8
    }

    /// The high half of `IY`.
    #[must_use]
    pub const fn iyh(&self) -> u8 {
        (self.iy >> 8) as u8
    }

    /// The low half of `IY`.
    #[must_use]
    pub const fn iyl(&self) -> u8 {
        self.iy as u8
    }

    /// Writes the high half of `IX`.
    pub(crate) const fn set_ixh(&mut self, value: u8) {
        self.ix = ((value as u16) << 8) | (self.ix & 0x00FF);
    }

    /// Writes the low half of `IX`.
    pub(crate) const fn set_ixl(&mut self, value: u8) {
        self.ix = (self.ix & 0xFF00) | value as u16;
    }

    /// Writes the high half of `IY`.
    pub(crate) const fn set_iyh(&mut self, value: u8) {
        self.iy = ((value as u16) << 8) | (self.iy & 0x00FF);
    }

    /// Writes the low half of `IY`.
    pub(crate) const fn set_iyl(&mut self, value: u8) {
        self.iy = (self.iy & 0xFF00) | value as u16;
    }
}

impl Default for Registers {
    /// The power-on state (`Z80-025`).
    ///
    /// `AF`, `SP` and the rest of the general set, both banks of it and `IX`
    /// and `IY`, hold all ones. `PC`, `I`, `R`, the two flip-flops and the
    /// mode come from `Z80-012` instead, and are the registers a reset is
    /// specified to clear.
    fn default() -> Self {
        Self::power_on()
    }
}

#[cfg(test)]
mod tests {
    use super::{InterruptMode, Registers};

    #[test]
    fn r_increments_in_seven_bits() {
        let mut regs = Registers::power_on();
        regs.r = 0x7F;
        regs.inc_r();
        assert_eq!(regs.r, 0x00);
    }

    #[test]
    fn r_carries_bit_seven_through_the_increment() {
        let mut regs = Registers::power_on();
        regs.r = 0x80;
        regs.inc_r();
        assert_eq!(regs.r, 0x81);
        regs.r = 0xFF;
        regs.inc_r();
        assert_eq!(regs.r, 0x80, "bit 7 stays set across the seven-bit wrap");
    }

    #[test]
    fn refresh_address_is_i_over_the_pre_increment_r() {
        let mut regs = Registers::power_on();
        regs.i = 0x4B;
        regs.r = 0x07;
        assert_eq!(regs.refresh_address(), 0x4B07);
        regs.inc_r();
        assert_eq!(
            regs.refresh_address(),
            0x4B08,
            "and moves on for the next fetch"
        );
    }

    #[test]
    fn reset_clears_only_what_it_is_specified_to() {
        let mut regs = Registers::power_on();
        regs.pc = 0x1234;
        regs.i = 0x55;
        regs.r = 0x66;
        regs.iff1 = true;
        regs.iff2 = true;
        regs.im = InterruptMode::Im2;
        regs.set_bc(0x1111);
        regs.reset();

        assert_eq!(regs.pc, 0);
        assert_eq!(regs.i, 0);
        assert_eq!(regs.r, 0);
        assert!(!regs.iff1);
        assert!(!regs.iff2);
        assert_eq!(regs.im, InterruptMode::Im0);
        assert_eq!(regs.bc(), 0x1111);
        assert_eq!(regs.sp, 0xFFFF);
        assert_eq!(regs.af(), 0xFFFF);
    }

    #[test]
    fn the_two_exchanges_are_independent() {
        let mut regs = Registers::power_on();
        regs.set_af(0x0102);
        (regs.a_alt, regs.f_alt) = (0x03, 0x04);
        regs.set_bc(0x1122);
        regs.set_de(0x3344);
        regs.set_hl(0x5566);
        (regs.b_alt, regs.c_alt) = (0xAA, 0xBB);
        (regs.d_alt, regs.e_alt) = (0xCC, 0xDD);
        (regs.h_alt, regs.l_alt) = (0xEE, 0xFF);
        regs.ix = 0x9999;
        regs.sp = 0x8888;

        regs.exx();
        assert_eq!((regs.bc(), regs.de(), regs.hl()), (0xAABB, 0xCCDD, 0xEEFF));
        assert_eq!(
            (regs.bc_alt(), regs.de_alt(), regs.hl_alt()),
            (0x1122, 0x3344, 0x5566)
        );
        assert_eq!(regs.af(), 0x0102, "EXX leaves AF alone");
        assert_eq!(
            (regs.ix, regs.sp),
            (0x9999, 0x8888),
            "and IX and SP with it"
        );

        regs.ex_af();
        assert_eq!((regs.af(), regs.af_alt()), (0x0304, 0x0102));
        assert_eq!(regs.bc(), 0xAABB, "EX AF, AF' leaves BC where EXX put it");
    }

    #[test]
    fn index_registers_split_into_halves() {
        let mut regs = Registers::power_on();
        regs.ix = 0x1234;
        assert_eq!((regs.ixh(), regs.ixl()), (0x12, 0x34));
        regs.set_ixh(0xAB);
        regs.set_ixl(0xCD);
        assert_eq!(regs.ix, 0xABCD);

        regs.iy = 0x5678;
        assert_eq!((regs.iyh(), regs.iyl()), (0x56, 0x78));
        regs.set_iyh(0xEF);
        assert_eq!(regs.iy, 0xEF78);
    }
}
