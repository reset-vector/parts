//! The Sega PCM (`315-5218`), at its window, its ROM reads and its output.
//!
//! # Examples
//!
//! ```
//! use segapcm::{SegaPcm, Select, control, reg};
//!
//! let mut pcm = SegaPcm::new();
//! // Channel 0 at full volume left, silent right, playing from 0x1000.
//! pcm.write(Select::new(reg::VOLUME_LEFT), 0x7F);
//! pcm.write(Select::new(reg::ADDRESS_UPPER), 0x10);
//! pcm.write(Select::new(reg::END), 0x10);
//! pcm.write(Select::new(reg::INCREMENT), 0x80);
//! for c in 1..16 {
//!     pcm.write(Select::new(8 * c + reg::CONTROL), control::STOPPED);
//! }
//! // A ROM of 0x90 everywhere: 16 above the midpoint.
//! let [left, right] = pcm.sample(&mut |_| 0x90);
//! assert_eq!((left.0, right.0), (16 * 127, 0));
//! assert_eq!(pcm.audio(), [left, right]);
//! ```
//!
//! # The shape
//!
//! - [`SegaPcm::write`] and [`SegaPcm::read`] are the sound CPU's accesses to
//!   the 256-byte window, its offset a [`Select`]. How a board mirrors it in the CPU's map is the
//!   board's.
//! - [`SegaPcm::sample`] is one output sample, 128 cycles of the part's clock
//!   (`SEGAPCM-006`). It reads ROM through the [`Rom`] it is given, once for
//!   each playing channel, and returns the stereo sum; [`SegaPcm::audio`]
//!   reads the last one back.
//!
//! The ROM read gets the channel's control byte with bits 0–1 cleared and
//! its 16-bit address. Which ROM that reaches is the board's decode
//! (`SEGAPCM-SIB-01` to `-03`).
//!
//! **What the crate chooses**, each an open item: the window and every
//! fraction at 0 in a new part (`SEGAPCM-OPEN-02`); and the 256-byte window as
//! the whole of the part's RAM (`SEGAPCM-OPEN-05`).
//!
//! # What is not here
//!
//! `WAIT` (`SEGAPCM-OPEN-01`), any timing inside a sample (`SEGAPCM-OPEN-04`),
//! and any width or clipping of the sum (`SEGAPCM-OPEN-03`).

#![no_std]

use units::Audio;

/// The levels at `A7`–`A0`: which byte of the 256-byte window a
/// [`SegaPcm::write`] or [`SegaPcm::read`] reaches, so an offset and a data
/// byte cannot be swapped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Select(u8);

impl Select {
    /// The levels `levels` at `A7`–`A0`, bit 0 `A0`.
    #[must_use]
    pub const fn new(levels: u8) -> Self {
        Self(levels)
    }

    /// The levels at `A7`–`A0`, bit 0 `A0`.
    #[must_use]
    pub const fn levels(self) -> u8 {
        self.0
    }
}

/// Channels in the part.
pub const CHANNEL_COUNT: usize = 16;

/// Cycles of the part's clock per output sample (`SEGAPCM-006`).
pub const CYCLES_PER_SAMPLE: usize = 128;

/// The largest magnitude [`SegaPcm::audio`] returns on a side: sixteen
/// channels of −128 at volume 127.
pub const AUDIO_FULL_SCALE: i32 = 16 * 128 * 127;

/// Offsets within a channel's eight bytes in the window's low half.
pub mod reg {
    /// Left volume, seven bits (`SEGAPCM-011`).
    pub const VOLUME_LEFT: u8 = 2;
    /// Right volume, seven bits (`SEGAPCM-011`).
    pub const VOLUME_RIGHT: u8 = 3;
    /// Loop address, low byte (`SEGAPCM-009`).
    pub const LOOP_LOWER: u8 = 4;
    /// Loop address, high byte (`SEGAPCM-009`).
    pub const LOOP_UPPER: u8 = 5;
    /// The last page played (`SEGAPCM-009`).
    pub const END: u8 = 6;
    /// The phase increment, in 256ths of a byte a sample (`SEGAPCM-007`).
    pub const INCREMENT: u8 = 7;
    /// Sample address, low byte, in the high half (`SEGAPCM-002`).
    pub const ADDRESS_LOWER: u8 = 0x84;
    /// Sample address, high byte, in the high half (`SEGAPCM-002`).
    pub const ADDRESS_UPPER: u8 = 0x85;
    /// Control, in the high half: bit 0 stops the channel, bit 1 makes it a
    /// one-shot, bits 2–7 go to the ROM read (`SEGAPCM-003` to `-005`).
    pub const CONTROL: u8 = 0x86;
}

/// The bits of a channel's control byte ([`reg::CONTROL`]).
pub mod control {
    /// Bit 0: the channel is stopped (`SEGAPCM-003`).
    pub const STOPPED: u8 = 0x01;

    /// Bit 1: the channel stops at its end instead of looping (`SEGAPCM-004`).
    pub const NO_LOOP: u8 = 0x02;
}

/// The ROM the part reads samples from: `read(address)` is the byte the board
/// puts on the part's data lines. Bits 0–15 of `address` are the channel's
/// sixteen-bit address and bits 16–23 its control byte with bits 0–1
/// cleared, which pass to the read beside it (`SEGAPCM-005`); bits 24–31 are
/// 0. Which ROM that reaches is the board's decode.
pub trait Rom {
    /// The byte at `address`.
    #[must_use]
    fn read(&mut self, address: u32) -> u8;
}

impl<F: FnMut(u32) -> u8> Rom for F {
    fn read(&mut self, address: u32) -> u8 {
        self(address)
    }
}

/// The part.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SegaPcm {
    ram: [u8; 256],
    /// Each channel's fraction below its address, not in the window
    /// (`SEGAPCM-008`).
    frac: [u8; CHANNEL_COUNT],
    /// What the last [`SegaPcm::sample`] returned.
    out: [i32; 2],
}

impl Default for SegaPcm {
    fn default() -> Self {
        Self::new()
    }
}

impl SegaPcm {
    /// A part with the window and every fraction at 0, this crate's choice
    /// under `SEGAPCM-OPEN-02`; a host that needs a known state writes the
    /// window.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ram: [0; 256],
            frac: [0; CHANNEL_COUNT],
            out: [0; 2],
        }
    }

    /// The sound CPU writes `d` at window offset `a`.
    pub fn write(&mut self, a: Select, d: u8) {
        self.ram[usize::from(a.levels())] = d;
    }

    /// The sound CPU reads window offset `a` (`SEGAPCM-001`).
    #[must_use]
    pub const fn read(&self, a: Select) -> u8 {
        self.ram[a.levels() as usize]
    }

    /// Channel `c`'s byte at `off` within its eight, `off` in either half.
    fn reg(&self, c: usize, off: u8) -> u8 {
        self.ram[8 * c + usize::from(off & 0x7) + usize::from(off & 0x80)]
    }

    fn set_reg(&mut self, c: usize, off: u8, v: u8) {
        self.ram[8 * c + usize::from(off & 0x7) + usize::from(off & 0x80)] = v;
    }

    /// One output sample, reading each playing channel's byte from `rom`:
    /// `[left, right]`, the sum over the sixteen channels (`SEGAPCM-012`).
    ///
    /// Each side runs from −260 096 to +258 064: sixteen channels of a sample
    /// byte less 128, −128 to 127, times a volume of 0 to 127.
    #[must_use]
    pub fn sample<R: Rom + ?Sized>(&mut self, rom: &mut R) -> [Audio<AUDIO_FULL_SCALE>; 2] {
        let mut out = [0; 2];
        for c in 0..CHANNEL_COUNT {
            let control = self.reg(c, reg::CONTROL);
            if control & control::STOPPED != 0 {
                continue;
            }
            let addr = u16::from_le_bytes([
                self.reg(c, reg::ADDRESS_LOWER),
                self.reg(c, reg::ADDRESS_UPPER),
            ]);
            let bank = control & !(control::STOPPED | control::NO_LOOP);
            let b = i32::from(rom.read(u32::from(bank) << 16 | u32::from(addr))) - 0x80;
            out[0] += b * i32::from(self.reg(c, reg::VOLUME_LEFT) & 0x7F);
            out[1] += b * i32::from(self.reg(c, reg::VOLUME_RIGHT) & 0x7F);

            let mut pos = ((u32::from(addr) << 8) | u32::from(self.frac[c]))
                + u32::from(self.reg(c, reg::INCREMENT));
            pos &= 0x00FF_FFFF;
            if (pos >> 16) as u8 == self.reg(c, reg::END).wrapping_add(1) {
                if control & control::NO_LOOP != 0 {
                    self.set_reg(c, reg::CONTROL, control | control::STOPPED);
                } else {
                    let lp = u16::from_le_bytes([
                        self.reg(c, reg::LOOP_LOWER),
                        self.reg(c, reg::LOOP_UPPER),
                    ]);
                    pos = u32::from(lp) << 8;
                }
            }
            let [lo, hi] = ((pos >> 8) as u16).to_le_bytes();
            self.set_reg(c, reg::ADDRESS_LOWER, lo);
            self.set_reg(c, reg::ADDRESS_UPPER, hi);
            self.frac[c] = pos as u8;
        }
        self.out = out;
        self.audio()
    }

    /// The last output sample [`SegaPcm::sample`] returned, `[left, right]`,
    /// signed, 0 at rest and from [`SegaPcm::new`]: −[`AUDIO_FULL_SCALE`] to
    /// +258 064 a side.
    #[must_use]
    pub const fn audio(&self) -> [Audio<AUDIO_FULL_SCALE>; 2] {
        [Audio(self.out[0]), Audio(self.out[1])]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SEGAPCM-OPEN-02`: a new part is silent whatever the ROM holds.
    #[test]
    fn a_new_part_is_silent() {
        let mut p = SegaPcm::new();
        for _ in 0..1000 {
            assert_eq!(p.sample(&mut |_| 0xFF), [Audio(0); 2]);
            assert_eq!(p.audio(), [Audio(0); 2]);
        }
    }
}
