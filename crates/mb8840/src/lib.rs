//! The Fujitsu MB8842, MB8843 and MB8844 four-bit microcomputers, at their
//! pins.
//!
//! Every item below names the claim it implements. The program is the
//! consumer's: a masked part is this core with its ROM filled — 1K on the
//! `MB8843` and `MB8844`, 2K on the `MB8842` — and the crate carries no ROM
//! content.
//!
//! # Examples
//!
//! ```
//! use mb8840::{Inputs, Mb8840, Member};
//!
//! let mut rom = [0u8; 1024];
//! rom[..2].copy_from_slice(&[0x95, 0x02]); // LI 5 : OUTP
//! let mut mcu = Mb8840::with_image(Member::Mb8843, &rom)?;
//! mcu.step(Inputs::idle());
//! let out = mcu.step(Inputs::idle());
//! assert_eq!(out.p, Some(5), "MB8840-014");
//! # Ok::<(), mb8840::ImageError>(())
//! ```
//!
//! # The shape
//!
//! The part is clocked, and one [`Mb8840::step`] is one instruction cycle —
//! six periods of the clock at `EX` (`MB8840-004`), which
//! [`EX_CYCLES_PER_MACHINE_CYCLE`] states for a host counting a faster clock.
//! Each step samples the input pins once, as [`Inputs`], runs the timer and
//! the serial port for one cycle, then either takes a pending interrupt,
//! finishes a two-cycle instruction, or executes one instruction. What the
//! part holds its output lines at afterwards is [`Mb8840::outputs`].
//!
//! A pin's level is a [`Level`], and a 1 bit in a port word is High. `IRQ`,
//! `TC` and `SC` are the pins the sheet draws with a bar, named `_n` here, so
//! their events are High-to-Low edges between two
//! samples (`MB8840-018`, `MB8840-020`, `MB8840-021`). An edge shorter than a
//! cycle is not seen, which is within the sheet's own input limits.
//!
//! # What this crate chooses
//!
//! Fifteen behaviours here are this crate's choice. The three that matter
//! most:
//!
//! - the status flag is the complement of the last test, and a conditional
//!   branch sets it back to 1 (`MB8840-OPEN-03`);
//! - an interrupt saves the counter and the three flags, clears its request,
//!   leaves the enable bits alone, enters with the status flag set and costs
//!   one cycle (`MB8840-OPEN-02`);
//! - a rotate leaves the status flag at 1, whatever bit it shifted into `CF`
//!   (`MB8840-OPEN-15`).
//!
//! The rest:
//!
//! - the counter is eleven bits and wraps at 2048, and a word the image does
//!   not populate fetches as `00`, a `NOP` (`MB8840-OPEN-01`);
//! - in internal-clock mode the timer counts once per instruction cycle
//!   (`MB8840-OPEN-04`);
//! - the serial flag `SF` sets on every fourth shift, and shifting goes on
//!   while the port is enabled (`MB8840-OPEN-05`);
//! - `IN`, `TSTR` and `TSTD` read the pin, the AND of the latch and the level
//!   the host presents (`MB8840-OPEN-06`);
//! - on the 64-nibble members `M(X,Y)` takes the low two bits of `X`, so pages
//!   4 to 7 alias pages 0 to 3 (`MB8840-OPEN-08`);
//! - `CF` is 1 on a borrow, and `DAA` and `DAS` keep a `CF` that was set and
//!   set it when the adjustment carries (`MB8840-OPEN-09`);
//! - `JPA` takes its page from the low five bits of its second byte
//!   (`MB8840-OPEN-10`);
//! - `LXI`, `LYI`, `LS` and `STS` set `ZF` on the value loaded or stored
//!   (`MB8840-OPEN-11`);
//! - the timer and serial interrupts enter at word 2 like the external one,
//!   ordered external, timer, serial, one entry per instruction boundary
//!   (`MB8840-OPEN-12`);
//! - `RSTC` and `SETC` leave the status flag alone (`MB8840-OPEN-13`);
//! - reset clears `AC`, `X`, `Y`, `SB`, the timer, `CF`, `ZF`, the enable
//!   bits, the requests and the stacks and sets the status flag; the RAM keeps
//!   its contents and is zero when the part is built (`MB8840-OPEN-14`);
//! - the `O` port, below (`MB8840-OPEN-07`).
//!
//! **The `O` port is the sheet's dual 4-bit option only** (`MB8840-015`,
//! `MB8840-OPEN-07`). A part whose mask carries the 8-bit PLA option gets the
//! wrong `O` port from this crate and nothing here can tell.
#![no_std]

use core::num::NonZeroUsize;

/// Periods of the clock at `EX` per instruction cycle, without the prescaler
/// (`MB8840-004`).
pub const EX_CYCLES_PER_MACHINE_CYCLE: usize = 6;

/// Words of program ROM in the `MB8843` and `MB8844` (`MB8840-001`); the
/// `MB8842`'s is [`Member::rom_words`].
pub const ROM_WORDS: usize = 1024;

/// Nibbles of RAM in the `MB8843` and `MB8844` (`MB8840-001`); the `MB8842`'s
/// is [`Member::ram_nibbles`].
pub const RAM_NIBBLES: usize = 64;

/// Words of program space, the ROM the counter can address: thirty-two pages
/// of sixty-four (`MB8840-011`), the most any member populates
/// (`MB8840-058`). A member's own ROM is [`Member::rom_words`].
pub const ROM_SPACE_WORDS: usize = 2048;

/// Nibbles of RAM space, the RAM `X` and `Y` can address: eight pages of
/// sixteen reached by `LXI`'s 0 to 7 (`MB8840-010`, `MB8840-032`), the most
/// any member populates (`MB8840-058`). A member's own RAM is
/// [`Member::ram_nibbles`].
pub const RAM_SPACE_NIBBLES: usize = 128;

/// The words the part enters at. Both are fixed entry points, not vectors
/// read from the ROM.
pub mod vector {
    /// The word releasing `RESET` starts at, word 0 of page 0 (`MB8840-006`).
    pub const RESET: u16 = 0x000;
    /// The word an interrupt enters at (`MB8840-024`).
    pub const INTERRUPT: u16 = 0x002;
}

/// The three members this crate models (`MB8840-002`, `MB8840-003`,
/// `MB8840-059`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum Member {
    /// 28 pins as the `MB8844`'s, with 2K of ROM and 128 nibbles of RAM.
    Mb8842,
    /// 42 pins: `P` port, serial port, sixteen `R` lines.
    Mb8843,
    /// 28 pins: no `P` port, no serial port, eleven `R` lines.
    Mb8844,
}

impl Member {
    /// The `R` lines this member has, as a mask over `R15`–`R0`.
    #[must_use]
    pub const fn r_mask(self) -> u16 {
        match self {
            Self::Mb8843 => 0xFFFF,
            Self::Mb8842 | Self::Mb8844 => 0x07FF,
        }
    }

    /// Words of program ROM in the mask (`MB8840-001`, `MB8840-058`).
    #[must_use]
    pub const fn rom_words(self) -> usize {
        match self {
            Self::Mb8842 => ROM_SPACE_WORDS,
            Self::Mb8843 | Self::Mb8844 => ROM_WORDS,
        }
    }

    /// Nibbles of RAM (`MB8840-001`, `MB8840-058`).
    #[must_use]
    pub const fn ram_nibbles(self) -> usize {
        match self {
            Self::Mb8842 => RAM_SPACE_NIBBLES,
            Self::Mb8843 | Self::Mb8844 => RAM_NIBBLES,
        }
    }

    /// The bits of `X` that pick a RAM page: all three on the `MB8842`, the
    /// low two on the 64-nibble members, whose pages alias
    /// (`MB8840-OPEN-08`).
    const fn page_mask(self) -> u8 {
        match self {
            Self::Mb8842 => 7,
            Self::Mb8843 | Self::Mb8844 => 3,
        }
    }

    /// Whether the member has the `P` port and executes `OUTP` (`MB8840-005`).
    #[must_use]
    pub const fn has_p_port(self) -> bool {
        matches!(self, Self::Mb8843)
    }

    /// Whether the member has the serial port (`MB8840-003`, `MB8840-059`).
    #[must_use]
    pub const fn has_serial_port(self) -> bool {
        matches!(self, Self::Mb8843)
    }

    /// Whether `TC` is `R9`'s pin and `IRQ` is `R10`'s (`MB8840-003`,
    /// `MB8840-059`).
    const fn has_shared_tc_and_irq_pins(self) -> bool {
        matches!(self, Self::Mb8842 | Self::Mb8844)
    }
}

/// An image handed to [`Mb8840::with_image`] or [`Mb8840::at_boundary`] that
/// the part refuses.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum ImageError {
    /// The image is `len` words long and the member's ROM is `expected`
    /// ([`Member::rom_words`]).
    Len {
        /// The length it had.
        len: usize,
        /// The member's ROM length.
        expected: usize,
    },
}

impl core::fmt::Display for ImageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Len { len, expected } => {
                write!(f, "an image of {len} words for a ROM of {expected} words")
            }
        }
    }
}

impl core::error::Error for ImageError {}

/// Why [`Mb8840::at_boundary`] refused its image or its registers.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum BoundaryError {
    /// The image, as [`Mb8840::with_image`] refuses it.
    Image(ImageError),
    /// [`Registers::p`] does not match the member: `Some` where it has no `P`
    /// port, or `None` where it has one.
    PPort {
        /// Whether the member has the port ([`Member::has_p_port`]).
        has_p_port: bool,
    },
}

impl From<ImageError> for BoundaryError {
    fn from(error: ImageError) -> Self {
        Self::Image(error)
    }
}

impl core::fmt::Display for BoundaryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Image(_) => f.write_str("an image the part refuses"),
            Self::PPort { has_p_port: true } => {
                f.write_str("registers with no P latch for a member with a P port")
            }
            Self::PPort { has_p_port: false } => {
                f.write_str("registers with a P latch for a member with no P port")
            }
        }
    }
}

impl core::error::Error for BoundaryError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Image(error) => Some(error),
            Self::PPort { .. } => None,
        }
    }
}

/// The levels a host holds the input pins at during one cycle.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `K3`–`K0`, low nibble (`MB8840-013`).
    pub k: u8,
    /// The level the outside world presents on `R15`–`R0`. A line the part
    /// holds low reads low whatever this says (`MB8840-OPEN-06`). On the
    /// 28-pin members bit 9 is also `TC` and bit 10 also `IRQ`, each at the
    /// pin's level with the part's own latch.
    pub r: u16,
    /// `IRQ`, active low. Ignored on the 28-pin members, where `IRQ` is
    /// bit 10 of [`Self::r`].
    pub irq_n: Level,
    /// `TC`, active low. Ignored on the 28-pin members, where `TC` is bit 9
    /// of [`Self::r`].
    pub tc_n: Level,
    /// `SI`. Ignored on the 28-pin members.
    pub si: Level,
    /// `SC` as an input, active low. Ignored on the 28-pin members.
    pub sc_n: Level,
    /// `RESET`, active low: held Low, the part stays in reset; the step that
    /// sees it High again runs from word 0 (`MB8840-006`).
    pub reset_n: Level,
}

impl Inputs {
    /// Every input pulled up and nothing asserted.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            k: 0xF,
            r: 0xFFFF,
            irq_n: Level::High,
            tc_n: Level::High,
            si: Level::High,
            sc_n: Level::High,
            reset_n: Level::High,
        }
    }
}

impl Default for Inputs {
    /// [`Inputs::idle`].
    fn default() -> Self {
        Self::idle()
    }
}

/// What the part holds its output lines at.
///
/// The `SC/TO` pin appears here in neither of its output roles. The part may
/// drive it as the timing output `TO` ([`enable::TIMING_OUTPUT`]) or as `SC`
/// in internal-clock serial mode ([`enable::SERIAL_INTERNAL`]); this crate
/// does not model the pin as an output and this struct will not grow a field
/// for it. A host reads which role the program has enabled from
/// [`Registers::enabled`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `O7`–`O0` (`MB8840-015`).
    pub o: u8,
    /// `P3`–`P0`, low nibble (`MB8840-014`); `None` on the 28-pin members,
    /// which have no `P` port ([`Member::has_p_port`]).
    pub p: Option<u8>,
    /// The `R` latches, `R15`–`R0`: a 0 is a line the part holds low, a 1 a
    /// line it releases (`MB8840-016`, `MB8840-017`). The 28-pin members have
    /// `R10`–`R0` only, and bits 15–11 read 1 there.
    pub r: u16,
    /// `SO`, the serial buffer's LSB (`MB8840-021`); `None` on the 28-pin
    /// members, which have no serial port ([`Member::has_serial_port`]).
    pub so: Option<Level>,
}

/// The `EN`/`DIS` operand bits (`MB8840-022`).
pub mod enable {
    /// `b0` — the serial buffer full/empty interrupt.
    pub const SERIAL_INTERRUPT: u8 = 0x01;
    /// `b1` — the timer/counter overflow interrupt.
    pub const TIMER_INTERRUPT: u8 = 0x02;
    /// `b2` — the external interrupt.
    pub const EXTERNAL_INTERRUPT: u8 = 0x04;
    /// `b3` — the synchronous timing output `TO`.
    pub const TIMING_OUTPUT: u8 = 0x08;
    /// `b4` — the serial port on the external clock `SC`.
    pub const SERIAL_EXTERNAL: u8 = 0x10;
    /// `b5` — the serial port on the internal clock.
    pub const SERIAL_INTERNAL: u8 = 0x20;
    /// `b6` — the counter on the external clock `TC`.
    pub const COUNTER_EXTERNAL: u8 = 0x40;
    /// `b7` — the timer on the internal clock.
    pub const TIMER_INTERNAL: u8 = 0x80;
}

/// An instruction of Table 5, as [`decode`] reads it off a first byte
/// (`MB8840-056`). The operand of a two-byte form is the second byte, which
/// the decoder does not see.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    /// `NOP`: nothing.
    Nop,
    /// `OUTO`: `A` to the `O` port's half `CF` picks.
    Outo,
    /// `OUTP`: `A` to the `P` port.
    Outp,
    /// `OUT`: `A` to the `R` port group `Y` picks.
    Out,
    /// `TAY`: `A` to `Y`.
    Tay,
    /// `TATH`: `A` to the timer's high nibble.
    Tath,
    /// `TATL`: `A` to the timer's low nibble.
    Tatl,
    /// `TAS`: `A` to the serial buffer.
    Tas,
    /// `ICY`: increment `Y`.
    Icy,
    /// `ICM`: increment `M(X,Y)`.
    Icm,
    /// `STIC`: `A` to `M(X,Y)`, then increment `Y`.
    Stic,
    /// `X`: exchange `A` and `M(X,Y)`.
    X,
    /// `ROL`: rotate `A` left through `CF`.
    Rol,
    /// `L`: `M(X,Y)` to `A`.
    L,
    /// `ADC`: add `M(X,Y)` and `CF` to `A`.
    Adc,
    /// `AND`: `A` and `M(X,Y)`.
    And,
    /// `DAA`: decimal adjust `A` after an addition.
    Daa,
    /// `DAS`: decimal adjust `A` after a subtraction.
    Das,
    /// `INK`: the `K` port to `A`.
    Ink,
    /// `IN`: the `R` port group `Y` picks to `A`.
    In,
    /// `TYA`: `Y` to `A`.
    Tya,
    /// `TTHA`: the timer's high nibble to `A`.
    Ttha,
    /// `TTLA`: the timer's low nibble to `A`.
    Ttla,
    /// `TSA`: the serial buffer to `A`.
    Tsa,
    /// `DCY`: decrement `Y`.
    Dcy,
    /// `DCM`: decrement `M(X,Y)`.
    Dcm,
    /// `STDC`: `A` to `M(X,Y)`, then decrement `Y`.
    Stdc,
    /// `XX`: exchange `A` and `X`.
    Xx,
    /// `ROR`: rotate `A` right through `CF`.
    Ror,
    /// `ST`: `A` to `M(X,Y)`.
    St,
    /// `SBC`: `M(X,Y)` minus `A` minus `CF` to `A`.
    Sbc,
    /// `OR`: `A` or `M(X,Y)`.
    Or,
    /// `SETR`: set the `R` line `Y` picks.
    Setr,
    /// `SETC`: set `CF`.
    Setc,
    /// `RSTR`: clear the `R` line `Y` picks.
    Rstr,
    /// `RSTC`: clear `CF`.
    Rstc,
    /// `TSTR`: test the `R` line `Y` picks.
    Tstr,
    /// `TSTI`: test the `IRQ` line.
    Tsti,
    /// `TSTV`: test and clear the timer overflow flag.
    Tstv,
    /// `TSTS`: test and clear the serial flag.
    Tsts,
    /// `TSTC`: test `CF`.
    Tstc,
    /// `TSTZ`: test `ZF`.
    Tstz,
    /// `STS`: the serial buffer to `M(X,Y)`.
    Sts,
    /// `LS`: `M(X,Y)` to the serial buffer.
    Ls,
    /// `RTS`: return from a subroutine.
    Rts,
    /// `NEG`: negate `A`.
    Neg,
    /// `C`: compare `A` with `M(X,Y)`.
    C,
    /// `EOR`: `A` exclusive-or `M(X,Y)`.
    Eor,
    /// `SBIT bp`: set bit `bp` of `M(X,Y)`.
    Sbit(Bit),
    /// `RBIT bp`: clear bit `bp` of `M(X,Y)`.
    Rbit(Bit),
    /// `TBIT bp`: test bit `bp` of `M(X,Y)`.
    Tbit(Bit),
    /// `RTI`: return from an interrupt, restoring the flags.
    Rti,
    /// `JPA addr`, two bytes.
    Jpa,
    /// `EN imm`, two bytes.
    En,
    /// `DIS imm`, two bytes.
    Dis,
    /// `SETD d`: set the `R` line `d`, `R0`–`R3`.
    Setd(Bit),
    /// `RSTD d`: clear the `R` line `d`, `R0`–`R3`.
    Rstd(Bit),
    /// `TSTD d`: test the `R` line `8 + d`, `R8`–`R11`.
    Tstd(Bit),
    /// `TBA bp`: test bit `bp` of `A`.
    Tba(Bit),
    /// `XD D`: exchange `A` with `M(0,D)`, `D` 0 to 3.
    Xd(XdCell),
    /// `XYD D`: exchange `Y` with `M(0,D)`, `D` 4 to 7.
    Xyd(XydCell),
    /// `LXI imm`: the RAM page to `X`.
    Lxi(RamPage),
    /// `CALL addr`, two bytes; the page the first names.
    Call(Page),
    /// `JPL addr`, two bytes; the page the first names.
    Jpl(Page),
    /// `AI imm`: add the immediate to `A` — `ICA` at 1, `DCA` at 15.
    Ai(Nibble),
    /// `LYI imm`: the immediate to `Y`.
    Lyi(Nibble),
    /// `LI imm`: the immediate to `A` — `CLA` at 0.
    Li(Nibble),
    /// `CYI imm`: compare `Y` with the immediate.
    Cyi(Nibble),
    /// `CI imm`: compare `A` with the immediate.
    Ci(Nibble),
    /// `JMP addr`: a word within the page of the word after it.
    Jmp(PageWord),
}

/// A four-bit immediate, the low nibble of an `AI`, `LYI`, `LI`, `CYI` or
/// `CI` code (`MB8840-056`).
pub use signal::{Level, Nibble};

/// A word within a 64-word page, the low six bits of a `JMP` code
/// (`MB8840-056`), which replace the low six bits of the counter.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub struct PageWord(u8);

impl PageWord {
    /// The word `bits`' low six bits name.
    #[must_use]
    pub const fn new(bits: u8) -> Self {
        Self(bits & 0x3F)
    }

    /// Its place in the page, 0 to 63.
    #[must_use]
    pub const fn value(self) -> u8 {
        self.0
    }
}

/// The RAM page of sixteen nibbles an `LXI` field names, the value it loads
/// into `X` (`MB8840-010`, `MB8840-032`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum RamPage {
    /// `X` = 0.
    Zero,
    /// `X` = 1.
    One,
    /// `X` = 2.
    Two,
    /// `X` = 3.
    Three,
    /// `X` = 4.
    Four,
    /// `X` = 5.
    Five,
    /// `X` = 6.
    Six,
    /// `X` = 7.
    Seven,
}

impl RamPage {
    /// The page whose number is `bits`' low three bits.
    const fn from_bits(bits: u8) -> Self {
        match bits & 7 {
            0 => Self::Zero,
            1 => Self::One,
            2 => Self::Two,
            3 => Self::Three,
            4 => Self::Four,
            5 => Self::Five,
            6 => Self::Six,
            _ => Self::Seven,
        }
    }
}

/// The bit an instruction's `bp` or `d` field names, the low two bits of its
/// code (`MB8840-056`): of a nibble, or of a group of four `R` lines.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Bit {
    /// Bit 0.
    Zero,
    /// Bit 1.
    One,
    /// Bit 2.
    Two,
    /// Bit 3.
    Three,
}

impl Bit {
    /// The bit whose number is `bits`' low two bits.
    const fn from_bits(bits: u8) -> Self {
        match bits & 3 {
            0 => Self::Zero,
            1 => Self::One,
            2 => Self::Two,
            _ => Self::Three,
        }
    }
}

/// The RAM cell `M(0,D)` an `XD` field `D` names, `X` = 0 and `Y` = `D`
/// (`MB8840-031`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum XdCell {
    /// `D` = 0.
    Zero,
    /// `D` = 1.
    One,
    /// `D` = 2.
    Two,
    /// `D` = 3.
    Three,
}

impl XdCell {
    /// The cell whose `D` is `bits`' low two bits.
    const fn from_bits(bits: u8) -> Self {
        match bits & 3 {
            0 => Self::Zero,
            1 => Self::One,
            2 => Self::Two,
            _ => Self::Three,
        }
    }
}

/// The RAM cell `M(0,D)` an `XYD` field `D` names, `X` = 0 and `Y` = `D`
/// (`MB8840-031`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum XydCell {
    /// `D` = 4.
    Four = 4,
    /// `D` = 5.
    Five,
    /// `D` = 6.
    Six,
    /// `D` = 7.
    Seven,
}

impl XydCell {
    /// The cell whose `D` is 4 plus `bits`' low two bits.
    const fn from_bits(bits: u8) -> Self {
        match bits & 3 {
            0 => Self::Four,
            1 => Self::Five,
            2 => Self::Six,
            _ => Self::Seven,
        }
    }
}

/// The 256-word page a `CALL` or `JPL` field names: the high three bits of
/// the address, the second byte giving the low eight (`MB8840-056`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Page {
    /// `000h`–`0FFh`.
    Zero,
    /// `100h`–`1FFh`.
    One,
    /// `200h`–`2FFh`.
    Two,
    /// `300h`–`3FFh`.
    Three,
    /// `400h`–`4FFh`.
    Four,
    /// `500h`–`5FFh`.
    Five,
    /// `600h`–`6FFh`.
    Six,
    /// `700h`–`7FFh`.
    Seven,
}

impl Page {
    /// The page whose number is `bits`' low three bits.
    const fn from_bits(bits: u8) -> Self {
        match bits & 7 {
            0 => Self::Zero,
            1 => Self::One,
            2 => Self::Two,
            3 => Self::Three,
            4 => Self::Four,
            5 => Self::Five,
            6 => Self::Six,
            _ => Self::Seven,
        }
    }

    /// The page's first address.
    const fn base(self) -> u16 {
        (self as u16) << 8
    }
}

/// One row of Table 5, as [`decode`] reads it off a first byte
/// (`MB8840-056`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub struct Decoded {
    /// The instruction.
    pub kind: Kind,
    /// Its length in bytes, one or two, which is also the cycles it takes
    /// (`MB8840-004`).
    pub len: NonZeroUsize,
}

/// Table 6, the instruction code map (`MB8840-056`).
#[must_use]
pub const fn decode(code: u8) -> Decoded {
    let kind = match code {
        0x00 => Kind::Nop,
        0x01 => Kind::Outo,
        0x02 => Kind::Outp,
        0x03 => Kind::Out,
        0x04 => Kind::Tay,
        0x05 => Kind::Tath,
        0x06 => Kind::Tatl,
        0x07 => Kind::Tas,
        0x08 => Kind::Icy,
        0x09 => Kind::Icm,
        0x0A => Kind::Stic,
        0x0B => Kind::X,
        0x0C => Kind::Rol,
        0x0D => Kind::L,
        0x0E => Kind::Adc,
        0x0F => Kind::And,
        0x10 => Kind::Daa,
        0x11 => Kind::Das,
        0x12 => Kind::Ink,
        0x13 => Kind::In,
        0x14 => Kind::Tya,
        0x15 => Kind::Ttha,
        0x16 => Kind::Ttla,
        0x17 => Kind::Tsa,
        0x18 => Kind::Dcy,
        0x19 => Kind::Dcm,
        0x1A => Kind::Stdc,
        0x1B => Kind::Xx,
        0x1C => Kind::Ror,
        0x1D => Kind::St,
        0x1E => Kind::Sbc,
        0x1F => Kind::Or,
        0x20 => Kind::Setr,
        0x21 => Kind::Setc,
        0x22 => Kind::Rstr,
        0x23 => Kind::Rstc,
        0x24 => Kind::Tstr,
        0x25 => Kind::Tsti,
        0x26 => Kind::Tstv,
        0x27 => Kind::Tsts,
        0x28 => Kind::Tstc,
        0x29 => Kind::Tstz,
        0x2A => Kind::Sts,
        0x2B => Kind::Ls,
        0x2C => Kind::Rts,
        0x2D => Kind::Neg,
        0x2E => Kind::C,
        0x2F => Kind::Eor,
        0x30..=0x33 => Kind::Sbit(Bit::from_bits(code)),
        0x34..=0x37 => Kind::Rbit(Bit::from_bits(code)),
        0x38..=0x3B => Kind::Tbit(Bit::from_bits(code)),
        0x3C => Kind::Rti,
        0x3D => Kind::Jpa,
        0x3E => Kind::En,
        0x3F => Kind::Dis,
        0x40..=0x43 => Kind::Setd(Bit::from_bits(code)),
        0x44..=0x47 => Kind::Rstd(Bit::from_bits(code)),
        0x48..=0x4B => Kind::Tstd(Bit::from_bits(code)),
        0x4C..=0x4F => Kind::Tba(Bit::from_bits(code)),
        0x50..=0x53 => Kind::Xd(XdCell::from_bits(code)),
        0x54..=0x57 => Kind::Xyd(XydCell::from_bits(code)),
        0x58..=0x5F => Kind::Lxi(RamPage::from_bits(code)),
        0x60..=0x67 => Kind::Call(Page::from_bits(code)),
        0x68..=0x6F => Kind::Jpl(Page::from_bits(code)),
        0x70..=0x7F => Kind::Ai(Nibble::new(code)),
        0x80..=0x8F => Kind::Lyi(Nibble::new(code)),
        0x90..=0x9F => Kind::Li(Nibble::new(code)),
        0xA0..=0xAF => Kind::Cyi(Nibble::new(code)),
        0xB0..=0xBF => Kind::Ci(Nibble::new(code)),
        0xC0..=0xFF => Kind::Jmp(PageWord::new(code)),
    };
    let len = match kind {
        Kind::Jpa | Kind::En | Kind::Dis | Kind::Call(_) | Kind::Jpl(_) => {
            NonZeroUsize::MIN.saturating_add(1)
        }
        _ => NonZeroUsize::MIN,
    };
    Decoded { kind, len }
}

/// One level of the return stack: the address a `CALL` or an interrupt
/// pushed, and the three flags an interrupt saves beside it, which `RTI`
/// restores and `RTS` ignores (`MB8840-009`, `MB8840-OPEN-02`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub struct StackLevel {
    /// The return address, eleven bits.
    pub pc: u16,
    /// `CF` as an interrupt saved it.
    pub cf: bool,
    /// `ZF` as an interrupt saved it.
    pub zf: bool,
    /// `ST` as an interrupt saved it.
    pub st: bool,
}

/// The programmer-visible registers, for [`Mb8840::registers`] and
/// [`Mb8840::at_boundary`] (`MB8840-009`). The three flags are separate
/// flip-flops, which an interrupt saves (`MB8840-OPEN-02`). The return stack,
/// the output latches, the serial bit count, the interrupt requests, the RAM
/// and the levels the three edge-triggered pins were last sampled at are here
/// too, so a part rebuilt from them runs on as the original would.
///
/// `Default` is the `MB8843`'s reset state, what [`Mb8840::with_image`]
/// reports for it, and a 28-pin member's but for `p`, which it reads `None`.
///
/// `#[non_exhaustive]`, since it gains a field when a round trip needs a new
/// latch: a caller builds one from [`Registers::default`] or
/// [`Mb8840::registers`] and sets the fields it wants.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub struct Registers {
    /// The program counter, eleven bits; bits 15–11 are ignored.
    pub pc: u16,
    /// `AC`, low nibble.
    pub ac: u8,
    /// `X`, low nibble: `LXI` loads 0 to 7, `XX` any value of `AC`, and the
    /// RAM page is its low bits (`MB8840-010`, `MB8840-OPEN-08`).
    pub x: u8,
    /// `Y`, low nibble.
    pub y: u8,
    /// `CF`.
    pub cf: bool,
    /// `ZF`.
    pub zf: bool,
    /// `ST`, the status flag.
    pub st: bool,
    /// The serial buffer `SB`, low nibble.
    pub sb: u8,
    /// The serial buffer full/empty flag `SF`.
    pub sf: bool,
    /// The timer/counter, `TH` above `TL`.
    pub counter: u8,
    /// The timer/counter overflow flag `VF`.
    pub vf: bool,
    /// The enable bits as `EN` and `DIS` have left them, [`enable`]
    /// (`MB8840-022`).
    pub enabled: u8,
    /// The four-level return stack, oldest first (`MB8840-009`).
    pub stack: [StackLevel; 4],
    /// How many levels of [`Self::stack`] are in use, 0 to 4.
    pub sp: u8,
    /// Bits shifted into `SB` since it last filled, 0 to 3 (`MB8840-021`).
    pub shift_count: u8,
    /// The external interrupt request flag (`MB8840-018`).
    pub external_pending: bool,
    /// The timer/counter overflow interrupt request (`MB8840-020`).
    pub timer_pending: bool,
    /// The serial buffer interrupt request (`MB8840-021`).
    pub serial_pending: bool,
    /// The `O` latch, `O7`–`O0` (`MB8840-015`).
    pub o: u8,
    /// The `P` latch, low nibble (`MB8840-014`); `None` on the 28-pin
    /// members, which have no `P` port ([`Member::has_p_port`]).
    pub p: Option<u8>,
    /// The `R` latches, `R15`–`R0`; a 0 holds the line low (`MB8840-016`).
    /// Bits 15–11 read 1 on the 28-pin members, which have `R10`–`R0` only.
    pub r: u16,
    /// The RAM, nibble `M(X,Y)` at index `X × 16 + Y` (`MB8840-010`); a
    /// member with less uses the first [`Member::ram_nibbles`].
    pub ram: [u8; RAM_SPACE_NIBBLES],
    /// `IRQ` at the last cycle, against which its next falling edge is found
    /// (`MB8840-018`); `TSTI` tests it.
    pub irq_n: Level,
    /// `TC` at the last cycle (`MB8840-020`).
    pub tc_n: Level,
    /// `SC` at the last cycle (`MB8840-021`); High on the 28-pin members.
    pub sc_n: Level,
}

/// The state `RESET` leaves (`MB8840-006`, `MB8840-007`, `MB8840-008`): `ST`
/// set, every output latch High, every resource disabled, the RAM zero and
/// the input pins High. `p` is `Some(0xF)`, the `MB8843`'s, so a caller
/// building a 28-pin member's registers sets it to `None`.
impl Default for Registers {
    fn default() -> Self {
        Self {
            pc: vector::RESET,
            ac: 0,
            x: 0,
            y: 0,
            cf: false,
            zf: false,
            st: true,
            sb: 0,
            sf: false,
            counter: 0,
            vf: false,
            enabled: 0,
            stack: [StackLevel::default(); 4],
            sp: 0,
            shift_count: 0,
            external_pending: false,
            timer_pending: false,
            serial_pending: false,
            o: 0xFF,
            p: Some(0xF),
            r: 0xFFFF,
            ram: [0; RAM_SPACE_NIBBLES],
            irq_n: Level::High,
            tc_n: Level::High,
            sc_n: Level::High,
        }
    }
}

/// The three flags, as an interrupt saves them (`MB8840-OPEN-02`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
struct Flags {
    cf: bool,
    zf: bool,
    st: bool,
}

/// One part, with its ROM.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Mb8840 {
    member: Member,
    rom: [u8; ROM_SPACE_WORDS],
    ram: [u8; RAM_SPACE_NIBBLES],
    pc: u16,
    ac: u8,
    x: u8,
    y: u8,
    flags: Flags,
    stack: [u16; 4],
    flag_stack: [Flags; 4],
    sp: u8,
    sb: u8,
    shift_count: u8,
    sf: bool,
    timer: u8,
    vf: bool,
    enabled: u8,
    external_pending: bool,
    timer_pending: bool,
    serial_pending: bool,
    irq_low: bool,
    tc_low: bool,
    sc_low: bool,
    o_latch: u8,
    p_latch: u8,
    r_latch: u16,
    stall: u8,
    in_reset: bool,
}

impl Mb8840 {
    /// A part of `member` with `image` in its mask, which must be exactly
    /// [`Member::rom_words`] long, and `RESET` just released (`MB8840-006`).
    /// The RAM is zero (`MB8840-OPEN-14`).
    ///
    /// # Errors
    ///
    /// [`ImageError`] when `image` is any other length: a 2,048-word image
    /// for a 1K member, or a 1,024-word one for the `MB8842`, is a mislabelled
    /// dump and not a part.
    ///
    /// # Examples
    ///
    /// ```
    /// use mb8840::{Mb8840, Member, ROM_WORDS, Registers};
    ///
    /// let part = Mb8840::with_image(Member::Mb8843, &[0; ROM_WORDS])?;
    /// assert_eq!(part.registers(), Registers::default());
    /// assert!(Mb8840::with_image(Member::Mb8842, &[0; ROM_WORDS]).is_err());
    /// # Ok::<(), mb8840::ImageError>(())
    /// ```
    pub fn with_image(member: Member, image: &[u8]) -> Result<Self, ImageError> {
        if image.len() != member.rom_words() {
            return Err(ImageError::Len {
                len: image.len(),
                expected: member.rom_words(),
            });
        }
        let mut space = [0; ROM_SPACE_WORDS];
        space[..image.len()].copy_from_slice(image);
        Ok(Self::from_space(member, space))
    }

    fn from_space(member: Member, rom: [u8; ROM_SPACE_WORDS]) -> Self {
        let mut part = Self {
            member,
            rom,
            ram: [0; RAM_SPACE_NIBBLES],
            pc: 0,
            ac: 0,
            x: 0,
            y: 0,
            flags: Flags::default(),
            stack: [0; 4],
            flag_stack: [Flags::default(); 4],
            sp: 0,
            sb: 0,
            shift_count: 0,
            sf: false,
            timer: 0,
            vf: false,
            enabled: 0,
            external_pending: false,
            timer_pending: false,
            serial_pending: false,
            irq_low: false,
            tc_low: false,
            sc_low: false,
            o_latch: 0xFF,
            p_latch: 0xF,
            r_latch: 0xFFFF,
            stall: 0,
            in_reset: false,
        };
        part.reset();
        part
    }

    /// A part of `member` with `image` in its mask, as [`Self::with_image`]
    /// takes it, standing at an instruction boundary with `regs` loaded, RAM
    /// included. The image is an argument because the part's program comes
    /// from it. Nothing else is held between instructions, so
    /// `at_boundary(member, image, cpu.registers())` at any boundary rebuilds
    /// a part that runs on as `cpu` does.
    ///
    /// # Errors
    ///
    /// [`BoundaryError::Image`] when `image` is not [`Member::rom_words`]
    /// long, and [`BoundaryError::PPort`] when `regs.p` is `Some` on a member
    /// with no `P` port or `None` on one with it.
    ///
    /// # Examples
    ///
    /// ```
    /// use mb8840::{Mb8840, Member, ROM_WORDS, Registers};
    ///
    /// let image = [0; ROM_WORDS];
    /// let mut regs = Registers::default();
    /// regs.pc = 0x123;
    /// regs.p = None; // the MB8844 has no `P` port
    /// let part = Mb8840::at_boundary(Member::Mb8844, &image, regs)?;
    /// assert_eq!(part.registers().pc, 0x123);
    /// assert!(Mb8840::at_boundary(Member::Mb8843, &image, regs).is_err());
    /// # Ok::<(), mb8840::BoundaryError>(())
    /// ```
    pub fn at_boundary(
        member: Member,
        image: &[u8],
        regs: Registers,
    ) -> Result<Self, BoundaryError> {
        let mut part = Self::with_image(member, image)?;
        part.pc = regs.pc & 0x7FF;
        part.ac = regs.ac & 0xF;
        part.x = regs.x & 0xF;
        part.y = regs.y & 0xF;
        part.flags = Flags {
            cf: regs.cf,
            zf: regs.zf,
            st: regs.st,
        };
        part.sb = regs.sb & 0xF;
        part.sf = regs.sf;
        part.timer = regs.counter;
        part.vf = regs.vf;
        part.enabled = regs.enabled;
        for (level, saved) in regs.stack.iter().enumerate() {
            part.stack[level] = saved.pc & 0x7FF;
            part.flag_stack[level] = Flags {
                cf: saved.cf,
                zf: saved.zf,
                st: saved.st,
            };
        }
        part.sp = regs.sp.min(4);
        part.shift_count = regs.shift_count & 3;
        part.external_pending = regs.external_pending;
        part.timer_pending = regs.timer_pending;
        part.serial_pending = regs.serial_pending;
        part.o_latch = regs.o;
        part.p_latch = match (member.has_p_port(), regs.p) {
            (true, Some(p)) => p & 0xF,
            (false, None) => 0xF,
            (has_p_port, _) => return Err(BoundaryError::PPort { has_p_port }),
        };
        part.r_latch = regs.r | !member.r_mask();
        for (nibble, &value) in part.ram.iter_mut().zip(&regs.ram) {
            *nibble = value & 0xF;
        }
        part.irq_low = regs.irq_n.is_low();
        part.tc_low = regs.tc_n.is_low();
        part.sc_low = member.has_serial_port() && regs.sc_n.is_low();
        Ok(part)
    }

    /// The registers as they stand.
    #[must_use]
    pub const fn registers(&self) -> Registers {
        let mut stack = [StackLevel {
            pc: 0,
            cf: false,
            zf: false,
            st: false,
        }; 4];
        let mut level = 0;
        while level < 4 {
            let flags = self.flag_stack[level];
            stack[level] = StackLevel {
                pc: self.stack[level],
                cf: flags.cf,
                zf: flags.zf,
                st: flags.st,
            };
            level += 1;
        }
        Registers {
            pc: self.pc,
            ac: self.ac,
            x: self.x,
            y: self.y,
            cf: self.flags.cf,
            zf: self.flags.zf,
            st: self.flags.st,
            sb: self.sb,
            sf: self.sf,
            counter: self.timer,
            vf: self.vf,
            enabled: self.enabled,
            stack,
            sp: self.sp,
            shift_count: self.shift_count,
            external_pending: self.external_pending,
            timer_pending: self.timer_pending,
            serial_pending: self.serial_pending,
            o: self.o_latch,
            p: if self.member.has_p_port() {
                Some(self.p_latch)
            } else {
                None
            },
            r: self.r_latch,
            ram: self.ram,
            irq_n: Level::from_bool(!self.irq_low),
            tc_n: Level::from_bool(!self.tc_low),
            sc_n: Level::from_bool(!self.sc_low),
        }
    }

    /// `RESET` held the part at the last step (`MB8840-006`). A part built by
    /// [`Self::with_image`] stands with `RESET` just released.
    #[must_use]
    pub const fn is_in_reset(&self) -> bool {
        self.in_reset
    }

    /// What `RESET` Low does, and keeps doing while it is held: execution
    /// restarts at word 0 of page 0, every output line goes high, every
    /// resource is disabled (`MB8840-006`, `MB8840-007`, `MB8840-008`,
    /// `MB8840-OPEN-14`). The RAM is left as it was.
    fn reset(&mut self) {
        self.pc = vector::RESET;
        self.ac = 0;
        self.x = 0;
        self.y = 0;
        self.flags = Flags {
            cf: false,
            zf: false,
            st: true,
        };
        self.stack = [0; 4];
        self.flag_stack = [Flags::default(); 4];
        self.sp = 0;
        self.sb = 0;
        self.shift_count = 0;
        self.sf = false;
        self.timer = 0;
        self.vf = false;
        self.enabled = 0;
        self.external_pending = false;
        self.timer_pending = false;
        self.serial_pending = false;
        self.irq_low = false;
        self.tc_low = false;
        self.sc_low = false;
        self.o_latch = 0xFF;
        self.p_latch = 0xF;
        self.r_latch = 0xFFFF;
        self.stall = 0;
    }

    /// Which member this is.
    #[must_use]
    pub const fn member(&self) -> Member {
        self.member
    }

    /// The output lines as the part holds them now.
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        Outputs {
            o: self.o_latch,
            p: if self.member.has_p_port() {
                Some(self.p_latch)
            } else {
                None
            },
            r: self.r_latch | !self.member.r_mask(),
            so: if self.member.has_serial_port() {
                Some(Level::from_bool(self.sb & 1 != 0))
            } else {
                None
            },
        }
    }

    /// The word at `addr` of program space; `00` above the member's ROM
    /// (`MB8840-OPEN-01`).
    #[must_use]
    pub const fn rom(&self, addr: u16) -> u8 {
        let addr = (addr & 0x7FF) as usize;
        if addr < self.member.rom_words() {
            self.rom[addr]
        } else {
            0
        }
    }

    /// One instruction cycle, and the output lines as it leaves them
    /// ([`Self::outputs`]).
    /// With `RESET` Low the step does what [`Inputs::reset_n`] says and
    /// nothing else.
    pub fn step(&mut self, inputs: Inputs) -> Outputs {
        if inputs.reset_n.is_low() {
            self.reset();
            self.in_reset = true;
            return self.outputs();
        }
        self.in_reset = false;
        self.sample(&inputs);
        if self.stall > 0 {
            self.stall -= 1;
        } else if !self.take_interrupt() {
            self.execute(&inputs);
        }
        self.outputs()
    }

    /// The next [`Self::step`] begins an instruction or takes an interrupt: no
    /// two-cycle instruction is half done, and `RESET` did not hold the part
    /// at the last step.
    #[must_use]
    pub const fn is_at_instruction_boundary(&self) -> bool {
        self.stall == 0 && !self.in_reset
    }

    /// The pin edges and the two clocked resources, once per cycle.
    fn sample(&mut self, inputs: &Inputs) {
        let (irq_low, tc_low) = if self.member.has_shared_tc_and_irq_pins() {
            (!self.r_line(inputs, 10), !self.r_line(inputs, 9))
        } else {
            (inputs.irq_n.is_low(), inputs.tc_n.is_low())
        };
        if irq_low && !self.irq_low && self.enabled & enable::EXTERNAL_INTERRUPT != 0 {
            self.external_pending = true;
        }
        self.irq_low = irq_low;

        if tc_low && !self.tc_low && self.enabled & enable::COUNTER_EXTERNAL != 0 {
            self.count();
        }
        self.tc_low = tc_low;
        if self.enabled & enable::TIMER_INTERNAL != 0 {
            self.count();
        }

        if self.member.has_serial_port() {
            let sc_low = inputs.sc_n.is_low();
            if sc_low && !self.sc_low && self.enabled & enable::SERIAL_EXTERNAL != 0 {
                self.shift(inputs.si.is_high());
            }
            self.sc_low = sc_low;
            if self.enabled & enable::SERIAL_INTERNAL != 0 {
                self.shift(inputs.si.is_high());
            }
        }
    }

    /// One count of the timer/counter (`MB8840-020`, `MB8840-OPEN-04`).
    fn count(&mut self) {
        let (next, overflow) = self.timer.overflowing_add(1);
        self.timer = next;
        if overflow {
            self.vf = true;
            if self.enabled & enable::TIMER_INTERRUPT != 0 {
                self.timer_pending = true;
            }
        }
    }

    /// One shift of the serial buffer (`MB8840-021`, `MB8840-OPEN-05`).
    fn shift(&mut self, si: bool) {
        self.sb = (self.sb >> 1) | (u8::from(si) << 3);
        self.shift_count += 1;
        if self.shift_count == 4 {
            self.shift_count = 0;
            self.sf = true;
            if self.enabled & enable::SERIAL_INTERRUPT != 0 {
                self.serial_pending = true;
            }
        }
    }

    /// Enter the interrupt routine if a request stands (`MB8840-024`,
    /// `MB8840-OPEN-02`, `MB8840-OPEN-12`).
    fn take_interrupt(&mut self) -> bool {
        if self.external_pending {
            self.external_pending = false;
        } else if self.timer_pending {
            self.timer_pending = false;
        } else if self.serial_pending {
            self.serial_pending = false;
        } else {
            return false;
        }
        let level = self.push(self.pc);
        self.flag_stack[level] = self.flags;
        self.flags.st = true;
        self.pc = vector::INTERRUPT;
        true
    }

    /// Push a return address; the level it went to. On a full stack the
    /// oldest level is lost and the rest still return newest first
    /// (`MB8840-OPEN-02`).
    fn push(&mut self, addr: u16) -> usize {
        if self.sp == 4 {
            self.stack.copy_within(1.., 0);
            self.flag_stack.copy_within(1.., 0);
            self.sp = 3;
        }
        let level = usize::from(self.sp);
        self.stack[level] = addr;
        self.sp += 1;
        level
    }

    fn pop(&mut self) -> (u16, Flags) {
        self.sp = self.sp.saturating_sub(1);
        let level = usize::from(self.sp & 3);
        (self.stack[level], self.flag_stack[level])
    }

    /// The nibble at `M(X,Y)` (`MB8840-010`, `MB8840-OPEN-08`).
    const fn mem_index(&self) -> usize {
        ((self.x & self.member.page_mask()) as usize) << 4 | (self.y & 0xF) as usize
    }

    fn mem(&self) -> u8 {
        self.ram[self.mem_index()]
    }

    fn set_mem(&mut self, value: u8) {
        let i = self.mem_index();
        self.ram[i] = value & 0xF;
    }

    /// `ZF` by a value (`MB8840-026` and the like).
    fn set_zf(&mut self, value: u8) {
        self.flags.zf = value & 0xF == 0;
    }

    /// The pin level of `R` line `line` (`MB8840-017`, `MB8840-OPEN-06`).
    fn r_line(&self, inputs: &Inputs, line: u8) -> bool {
        let mask = 1u16 << (line & 15);
        (self.r_latch & (inputs.r | !self.member.r_mask()) & mask) != 0
    }

    fn set_r_line(&mut self, line: u8, high: bool) {
        let mask = 1u16 << (line & 15);
        if high {
            self.r_latch |= mask;
        } else {
            self.r_latch &= !mask;
        }
        self.r_latch |= !self.member.r_mask();
    }

    /// The four pins of `R` port `port` (`MB8840-016`).
    fn r_port(&self, inputs: &Inputs, port: u8) -> u8 {
        let shift = 4 * (port & 3);
        let pins = self.r_latch & (inputs.r | !self.member.r_mask());
        ((pins >> shift) & 0xF) as u8
    }

    fn set_r_port(&mut self, port: u8, value: u8) {
        let shift = 4 * (port & 3);
        self.r_latch = (self.r_latch & !(0xF << shift)) | (u16::from(value & 0xF) << shift);
        self.r_latch |= !self.member.r_mask();
    }

    /// `AC ← a + b + carry_in`, with `ZF`, `CF` and `ST` as an add sets them
    /// (`MB8840-033`, `MB8840-034`).
    fn add(&mut self, a: u8, b: u8, carry_in: bool) {
        let sum = a + b + u8::from(carry_in);
        self.ac = sum & 0xF;
        self.flags.cf = sum > 0xF;
        self.flags.zf = self.ac == 0;
        self.flags.st = sum <= 0xF;
    }

    /// `a − b − borrow_in`, returning the four-bit result and the borrow
    /// (`MB8840-036`, `MB8840-042`, `MB8840-OPEN-09`).
    const fn sub(a: u8, b: u8, borrow_in: bool) -> (u8, bool) {
        let diff = (a as i16) - (b as i16) - (borrow_in as i16);
        ((diff & 0xF) as u8, diff < 0)
    }

    /// A compare: the flags of `a − b` with the result discarded and `ST`
    /// cleared on zero (`MB8840-036`).
    fn compare(&mut self, a: u8, b: u8) {
        let (result, borrow) = Self::sub(a, b, false);
        self.flags.cf = borrow;
        self.flags.zf = result == 0;
        self.flags.st = result != 0;
    }

    /// `DAA` and `DAS` (`MB8840-037`, `MB8840-OPEN-09`).
    fn adjust(&mut self, amount: u8) {
        if self.ac > 9 || self.flags.cf {
            let sum = self.ac + amount;
            self.ac = sum & 0xF;
            let carry = sum > 0xF;
            self.flags.cf |= carry;
            self.flags.st = !carry;
        } else {
            self.flags.st = true;
        }
    }

    /// A test whose zero clears `ST` (`MB8840-043`, `MB8840-047`,
    /// `MB8840-054`, `MB8840-OPEN-03`): `ST ← !bit`.
    fn test(&mut self, bit: bool) {
        self.flags.st = !bit;
    }

    /// Fetch the operand of a two-byte instruction and charge its second cycle.
    fn operand(&mut self) -> u8 {
        let byte = self.rom(self.pc);
        self.pc = (self.pc + 1) & 0x7FF;
        self.stall = 1;
        byte
    }

    /// A conditional branch's gate: taken on `ST` = 1, and `ST` is 1 after
    /// (`MB8840-048`, `MB8840-049`, `MB8840-051`, `MB8840-OPEN-03`).
    fn gate(&mut self) -> bool {
        let taken = self.flags.st;
        self.flags.st = true;
        taken
    }

    /// One instruction (`MB8840-025` to `MB8840-040`, `MB8840-042` to `MB8840-057`).
    #[allow(clippy::too_many_lines)]
    fn execute(&mut self, inputs: &Inputs) {
        let at = self.pc;
        let code = self.rom(at);
        self.pc = (at + 1) & 0x7FF;
        match decode(code).kind {
            Kind::Nop => {}
            Kind::Outo => {
                if self.flags.cf {
                    self.o_latch = (self.o_latch & 0x0F) | (self.ac << 4);
                } else {
                    self.o_latch = (self.o_latch & 0xF0) | self.ac;
                }
            }
            Kind::Outp => {
                if self.member.has_p_port() {
                    self.p_latch = self.ac;
                }
            }
            Kind::Out => self.set_r_port(self.y, self.ac),
            Kind::Tay => self.y = self.ac,
            Kind::Tath => self.timer = (self.timer & 0x0F) | (self.ac << 4),
            Kind::Tatl => self.timer = (self.timer & 0xF0) | self.ac,
            Kind::Tas => self.sb = self.ac,
            Kind::Icy => {
                let sum = self.y + 1;
                self.y = sum & 0xF;
                self.flags.zf = self.y == 0;
                self.flags.st = sum <= 0xF;
            }
            Kind::Icm => {
                let sum = self.mem() + 1;
                self.set_mem(sum);
                self.flags.zf = sum & 0xF == 0;
                self.flags.st = sum <= 0xF;
            }
            Kind::Stic => {
                self.set_mem(self.ac);
                let sum = self.y + 1;
                self.y = sum & 0xF;
                self.flags.zf = self.y == 0;
                self.flags.st = sum <= 0xF;
            }
            Kind::X => {
                let m = self.mem();
                self.set_mem(self.ac);
                self.ac = m;
                self.set_zf(self.ac);
            }
            Kind::Rol => {
                let out = self.ac & 8 != 0;
                self.ac = ((self.ac << 1) & 0xF) | u8::from(self.flags.cf);
                self.flags.cf = out;
                self.flags.zf = self.ac == 0;
                self.flags.st = true;
            }
            Kind::L => {
                self.ac = self.mem();
                self.set_zf(self.ac);
            }
            Kind::Adc => self.add(self.ac, self.mem(), self.flags.cf),
            Kind::And => {
                self.ac &= self.mem();
                self.set_zf(self.ac);
                self.flags.st = self.ac != 0;
            }
            Kind::Daa => self.adjust(6),
            Kind::Das => self.adjust(10),
            Kind::Ink => {
                self.ac = inputs.k & 0xF;
                self.set_zf(self.ac);
            }
            Kind::In => {
                self.ac = self.r_port(inputs, self.y);
                self.set_zf(self.ac);
            }
            Kind::Tya => {
                self.ac = self.y;
                self.set_zf(self.ac);
            }
            Kind::Ttha => {
                self.ac = self.timer >> 4;
                self.set_zf(self.ac);
            }
            Kind::Ttla => {
                self.ac = self.timer & 0xF;
                self.set_zf(self.ac);
            }
            Kind::Tsa => {
                self.ac = self.sb;
                self.set_zf(self.ac);
            }
            Kind::Dcy => {
                let (result, borrow) = Self::sub(self.y, 1, false);
                self.y = result;
                self.flags.zf = result == 0;
                self.flags.st = !borrow;
            }
            Kind::Dcm => {
                let (result, borrow) = Self::sub(self.mem(), 1, false);
                self.set_mem(result);
                self.flags.zf = result == 0;
                self.flags.st = !borrow;
            }
            Kind::Stdc => {
                self.set_mem(self.ac);
                let (result, borrow) = Self::sub(self.y, 1, false);
                self.y = result;
                self.flags.zf = result == 0;
                self.flags.st = !borrow;
            }
            Kind::Xx => {
                core::mem::swap(&mut self.ac, &mut self.x);
                self.set_zf(self.ac);
            }
            Kind::Ror => {
                let out = self.ac & 1 != 0;
                self.ac = (self.ac >> 1) | (u8::from(self.flags.cf) << 3);
                self.flags.cf = out;
                self.flags.zf = self.ac == 0;
                self.flags.st = true;
            }
            Kind::St => self.set_mem(self.ac),
            Kind::Sbc => {
                let (result, borrow) = Self::sub(self.mem(), self.ac, self.flags.cf);
                self.ac = result;
                self.flags.cf = borrow;
                self.flags.zf = result == 0;
                self.flags.st = !borrow;
            }
            Kind::Or => {
                self.ac |= self.mem();
                self.set_zf(self.ac);
                self.flags.st = self.ac != 0;
            }
            Kind::Setr => self.set_r_line(self.y, true),
            Kind::Setc => self.flags.cf = true,
            Kind::Rstr => self.set_r_line(self.y, false),
            Kind::Rstc => self.flags.cf = false,
            Kind::Tstr => {
                let line = self.r_line(inputs, self.y);
                self.test(line);
            }
            Kind::Tsti => self.test(self.irq_low),
            Kind::Tstv => {
                self.test(self.vf);
                self.vf = false;
            }
            Kind::Tsts => {
                self.test(self.sf);
                self.sf = false;
            }
            Kind::Tstc => self.test(self.flags.cf),
            Kind::Tstz => self.test(self.flags.zf),
            Kind::Sts => {
                self.set_mem(self.sb);
                self.set_zf(self.sb);
            }
            Kind::Ls => {
                self.sb = self.mem();
                self.set_zf(self.sb);
            }
            Kind::Rts => {
                let (addr, _) = self.pop();
                self.pc = addr;
            }
            Kind::Neg => {
                self.ac = self.ac.wrapping_neg() & 0xF;
                self.flags.st = self.ac != 0;
            }
            Kind::C => self.compare(self.mem(), self.ac),
            Kind::Eor => {
                self.ac ^= self.mem();
                self.set_zf(self.ac);
                self.flags.st = self.ac != 0;
            }
            Kind::Sbit(bp) => {
                let m = self.mem() | (1 << bp as u8);
                self.set_mem(m);
            }
            Kind::Rbit(bp) => {
                let m = self.mem() & !(1 << bp as u8);
                self.set_mem(m);
            }
            Kind::Tbit(bp) => {
                let bit = self.mem() & (1 << bp as u8) != 0;
                self.test(bit);
            }
            Kind::Rti => {
                let (addr, flags) = self.pop();
                self.pc = addr;
                self.flags = flags;
            }
            Kind::Jpa => {
                let page = u16::from(self.operand() & 0x1F);
                self.pc = (page << 6) | (u16::from(self.ac) << 2);
            }
            Kind::En => {
                let bits = self.operand();
                self.enabled |= bits;
            }
            Kind::Dis => {
                let bits = self.operand();
                self.enabled &= !bits;
            }
            Kind::Setd(d) => self.set_r_line(d as u8, true),
            Kind::Rstd(d) => self.set_r_line(d as u8, false),
            Kind::Tstd(d) => {
                let line = self.r_line(inputs, 8 + d as u8);
                self.test(line);
            }
            Kind::Tba(bp) => {
                let bit = self.ac & (1 << bp as u8) != 0;
                self.test(bit);
            }
            Kind::Xd(d) => {
                let i = d as usize;
                core::mem::swap(&mut self.ac, &mut self.ram[i]);
                self.set_zf(self.ac);
            }
            Kind::Xyd(d) => {
                let i = d as usize;
                core::mem::swap(&mut self.y, &mut self.ram[i]);
                self.set_zf(self.y);
            }
            Kind::Lxi(page) => {
                self.x = page as u8;
                self.set_zf(self.x);
            }
            Kind::Call(page) => {
                let low = self.operand();
                let addr = page.base() | u16::from(low);
                if self.gate() {
                    let _ = self.push(self.pc);
                    self.pc = addr;
                }
            }
            Kind::Jpl(page) => {
                let low = self.operand();
                let addr = page.base() | u16::from(low);
                if self.gate() {
                    self.pc = addr;
                }
            }
            Kind::Ai(imm) => self.add(self.ac, imm.value(), false),
            Kind::Lyi(imm) => {
                self.y = imm.value();
                self.set_zf(self.y);
            }
            Kind::Li(imm) => {
                self.ac = imm.value();
                self.set_zf(self.ac);
            }
            Kind::Cyi(imm) => self.compare(imm.value(), self.y),
            Kind::Ci(imm) => self.compare(imm.value(), self.ac),
            Kind::Jmp(addr) => {
                if self.gate() {
                    self.pc = (self.pc & 0x7C0) | u16::from(addr.value());
                }
            }
        }
    }
}
