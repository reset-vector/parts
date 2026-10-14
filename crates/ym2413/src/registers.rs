//! The register file: 64 addresses, of which the manual's map defines 37.
//!
//! Storage is the whole 64-address space so that an address write is always
//! representable, but only the addresses the map defines are written and only
//! the bits it defines are kept (`YM2413-015`). The part has no readback path
//! for these bits, so masking is not observable at the pins; it is here because
//! an unmasked bit would otherwise reach the machine as if it meant something.

/// The addresses the manual's register map (II-7) defines, and the bits each
/// keeps. Every other address stores nothing.
///
/// `$03` drops `D5`: the map gives that address `KSL` in `D7`–`D6`, `DC` in
/// `D4`, `DM` in `D3` and `FB` in `D2`–`D0`, and names nothing for `D5`.
/// `$0E` and `$20`–`$28` keep six bits each for the same reason.
#[must_use]
pub(crate) fn mask(address: u8) -> Option<u8> {
    match address {
        0x00..=0x02 | 0x04..=0x07 | 0x0F | 0x10..=0x18 | 0x30..=0x38 => Some(0xFF),
        0x03 => Some(0b1101_1111),
        0x0E => Some(0b0011_1111),
        0x20..=0x28 => Some(0b0011_1111),
        _ => None,
    }
}

/// The eight-address user tone register, `$00`–`$07` (`YM2413-016`).
pub const USER_TONE: core::ops::RangeInclusive<u8> = 0x00..=0x07;
/// Rhythm control, `$0E` (`YM2413-021`).
pub const RHYTHM: u8 = 0x0E;
/// LSI test, `$0F` (`YM2413-022`).
pub const TEST: u8 = 0x0F;
/// F-Number low eight bits, `$10`–`$18`, one per channel (`YM2413-023`).
pub const FNUM_LOWER: u8 = 0x10;
/// Sustain, key, block and F-Number bit 8, `$20`–`$28` (`YM2413-024`).
pub const KEY_BLOCK: u8 = 0x20;
/// Instrument selection and volume, `$30`–`$38` (`YM2413-025`).
pub const INST_VOL: u8 = 0x30;

/// The whole 64-address space, with the map's bits kept and the rest dropped.
///
/// A per-channel reader takes `channel` below 9; the part's public readers
/// check it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Registers {
    cells: [u8; 64],
}

impl Default for Registers {
    fn default() -> Self {
        Self::CLEARED
    }
}

impl Registers {
    /// Every register cleared to zero, which is what `/IC` low does
    /// (`YM2413-011`).
    pub(crate) const CLEARED: Self = Self { cells: [0; 64] };

    /// Write `value` to `address`, keeping only the bits the map defines.
    /// Returns whether the address is one the map defines.
    ///
    /// An address the map does not define is dropped, and so is one at `$40`
    /// or above, which this crate does not fold onto the map because
    /// `YM2413-005` leaves the decode width unstated; such a write is
    /// `YM2413-OPEN-02`.
    pub(crate) fn write(&mut self, address: u8, value: u8) -> bool {
        match mask(address) {
            Some(mask) => {
                self.cells[address as usize] = value & mask;
                true
            }
            None => false,
        }
    }

    /// The stored byte at `address`, masked as it was written, and zero above
    /// the 64-address space, where nothing is stored (`YM2413-OPEN-02`).
    #[must_use]
    pub(crate) fn get(&self, address: u8) -> u8 {
        self.cells.get(address as usize).copied().unwrap_or(0)
    }

    /// The user tone register's eight bytes, `$00`–`$07`.
    #[must_use]
    pub(crate) fn user_tone(&self) -> [u8; 8] {
        let mut out = [0; 8];
        for (address, slot) in out.iter_mut().enumerate() {
            *slot = self.cells[address];
        }
        out
    }

    /// Whether rhythm mode is selected — `$0E` `D5` (`YM2413-069`).
    #[must_use]
    pub(crate) fn is_rhythm_mode(&self) -> bool {
        self.get(RHYTHM) & 0b0010_0000 != 0
    }

    /// The five rhythm instrument on/off bits, `$0E` `D4`–`D0` in the manual's
    /// order `BD SD TOM TOP-CY HH` (`YM2413-070`).
    #[must_use]
    pub(crate) fn rhythm_keys(&self) -> u8 {
        self.get(RHYTHM) & 0b0001_1111
    }

    /// A channel's nine-bit F-Number: eight bits from `$1n` and bit 8 from
    /// `$2n` `D0` (`YM2413-038`).
    #[must_use]
    pub(crate) fn fnum(&self, channel: usize) -> u16 {
        let low = u16::from(self.get(FNUM_LOWER.wrapping_add(channel as u8)));
        let high = u16::from(self.get(KEY_BLOCK.wrapping_add(channel as u8)) & 0b0000_0001);
        high << 8 | low
    }

    /// A channel's three-bit block, `$2n` `D3`–`D1` (`YM2413-039`).
    #[must_use]
    pub(crate) fn block(&self, channel: usize) -> u8 {
        (self.get(KEY_BLOCK.wrapping_add(channel as u8)) >> 1) & 0b0000_0111
    }

    /// A channel's key bit, `$2n` `D4` (`YM2413-053`).
    #[must_use]
    pub(crate) fn key_on(&self, channel: usize) -> bool {
        self.get(KEY_BLOCK.wrapping_add(channel as u8)) & 0b0001_0000 != 0
    }

    /// A channel's sustain bit, `$2n` `D5` (`YM2413-052`).
    #[must_use]
    pub(crate) fn sustain_on(&self, channel: usize) -> bool {
        self.get(KEY_BLOCK.wrapping_add(channel as u8)) & 0b0010_0000 != 0
    }

    /// A channel's instrument selection, `$3n` `D7`–`D4` (`YM2413-026`).
    #[must_use]
    pub(crate) fn instrument(&self, channel: usize) -> u8 {
        self.get(INST_VOL.wrapping_add(channel as u8)) >> 4
    }

    /// A channel's four-bit volume, `$3n` `D3`–`D0` (`YM2413-027`).
    #[must_use]
    pub(crate) fn volume(&self, channel: usize) -> u8 {
        self.get(INST_VOL.wrapping_add(channel as u8)) & 0x0F
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    #[test]
    fn the_map_defines_37_addresses_and_their_bits() {
        let defined: Vec<u8> = (0..64u8).filter(|a| mask(*a).is_some()).collect();
        let expected: Vec<u8> = USER_TONE
            .chain([RHYTHM, TEST])
            .chain(FNUM_LOWER..=FNUM_LOWER + 8)
            .chain(KEY_BLOCK..=KEY_BLOCK + 8)
            .chain(INST_VOL..=INST_VOL + 8)
            .collect();
        assert_eq!(defined, expected);
        assert_eq!(defined.len(), 37);

        assert_eq!(mask(0x03), Some(0b1101_1111));
        assert_eq!(mask(RHYTHM), Some(0b0011_1111));
        assert_eq!(mask(KEY_BLOCK), Some(0b0011_1111));
        assert_eq!(mask(TEST), Some(0xFF));
    }
}
