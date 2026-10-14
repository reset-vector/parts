//! The Konami SCC — the `051649`, and the `052539` (the SCC+) beside it — at
//! its 256-byte register block and its summed output.
//!
//! The register map and the formats are Sean Young's two pages on the SCC at
//! msxnet.org; how a sample and a volume become a level, how the channels sum
//! and when a channel is silent are `KONAMI-SCC-015` to `KONAMI-SCC-019`. What
//! the crate chooses names its open item.
//!
//! **What the crate chooses**, each an open item: a period write taking
//! effect at the next step (`KONAMI-SCC-OPEN-04`); a read of the waveforms
//! has no effect, whether or not they are playing (`KONAMI-SCC-OPEN-06`); and
//! the `052539` takes the `051649`'s level law, sum and timing
//! (`KONAMI-SCC-OPEN-08`).
//!
//! # Examples
//!
//! ```
//! use konami_scc::{KonamiScc, Member, Select};
//!
//! let mut scc = KonamiScc::new(Member::K051649);
//! scc.write(Select::new(0x00), 0x7F); // channel 1's first sample
//! scc.write(Select::new(0x80), 0x20); // its period, TP = 32
//! scc.write(Select::new(0x8A), 0x0F); // its volume, the loudest
//! scc.write(Select::new(0x8F), 0x01); // channel 1 keyed on
//! assert_eq!(scc.audio().0, (0x7F * 15) >> 3);
//! scc.advance(33);
//! assert_eq!(scc.wave_position(0), 1);
//! ```
//!
//! # The shape
//!
//! - [`KonamiScc::write`] and [`KonamiScc::read`] take an offset within the block, the
//!   part's `A0`–`A7` as a [`Select`] (`KONAMI-SCC-005`). Which window of the machine reaches it —
//!   the cartridge's bank registers — is the mapper's and not here.
//! - [`KonamiScc::write_mode`] is the `052539`'s mode register (`KONAMI-SCC-020`); the
//!   `051649` has none and ignores it.
//! - [`KonamiScc::tick`] is one cycle of the part's clock (`KONAMI-SCC-004`), the MSX
//!   slot's 3,579,545 Hz; [`KonamiScc::advance`] is many.
//! - [`KonamiScc::audio`] is the five channels summed, from −1200 to
//!   +1190, 0 the silence (`KONAMI-SCC-016`); [`KonamiScc::channel_audio`] is one channel's.
//!
//! # What is not here
//!
//! The eleven-bit code at the pins (`KONAMI-SCC-OPEN-03`), the deformation register's
//! effect (`KONAMI-SCC-OPEN-01`), the mappers (`KONAMI-SCC-OPEN-07`), and the state after
//! power-up (`KONAMI-SCC-OPEN-02`; [`KonamiScc::new`] clears everything).

#![no_std]

use units::Audio;

/// The levels at `A7`–`A0`: which byte of the part's 256-byte block a
/// [`KonamiScc::write`] or [`KonamiScc::read`] reaches, so an offset and a
/// data byte cannot be swapped.
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

/// Which member of the family.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum Member {
    /// The `051649`: channels 4 and 5 share a waveform (`KONAMI-SCC-006`).
    K051649,
    /// The `052539`, the SCC+: channel 5 has its own (`KONAMI-SCC-024`).
    K052539,
}

/// The `052539`'s mode register's bits ([`KonamiScc::write_mode`]).
pub mod mode {
    /// Bit 5: the SCC+ sound mode, the `052539`'s own map (`KONAMI-SCC-020`).
    pub const K052539: u8 = 0x20;
}

/// Channels.
pub const CHANNEL_COUNT: usize = 5;

/// Samples in a waveform (`KONAMI-SCC-012`).
pub const POSITIONS_PER_WAVEFORM: usize = 32;

/// The largest magnitude [`KonamiScc::channel_audio`] returns: a channel at
/// −240 (`KONAMI-SCC-015`).
pub const CHANNEL_FULL_SCALE: i32 = 240;

/// The largest magnitude [`KonamiScc::audio`] returns: five channels at −240
/// (`KONAMI-SCC-016`).
pub const AUDIO_FULL_SCALE: i32 = 5 * CHANNEL_FULL_SCALE;

/// The highest `TP` that is silent: a channel at this `TP` or below sounds
/// nothing (`KONAMI-SCC-018`).
pub const LAST_SILENT_TP: u16 = 8;

/// One part.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KonamiScc {
    member: Member,
    wave: [[i8; POSITIONS_PER_WAVEFORM]; CHANNEL_COUNT],
    period: [u16; CHANNEL_COUNT],
    volume: [u8; CHANNEL_COUNT],
    key: u8,
    deformation: u8,
    mode: u8,
    /// Cycles left in the current step after this one.
    count: [u16; CHANNEL_COUNT],
    step: [u8; CHANNEL_COUNT],
}

impl KonamiScc {
    /// A part with every register, waveform and counter at 0, this crate's
    /// choice under `KONAMI-SCC-OPEN-02`; a host that needs a known state
    /// writes the registers.
    #[must_use]
    pub const fn new(member: Member) -> Self {
        Self {
            member,
            wave: [[0; POSITIONS_PER_WAVEFORM]; CHANNEL_COUNT],
            period: [0; CHANNEL_COUNT],
            volume: [0; CHANNEL_COUNT],
            key: 0,
            deformation: 0,
            mode: 0,
            count: [0; CHANNEL_COUNT],
            step: [0; CHANNEL_COUNT],
        }
    }

    /// Which member this is.
    #[must_use]
    pub const fn member(&self) -> Member {
        self.member
    }

    /// The `052539` is in SCC+ mode, its own map (`KONAMI-SCC-020`,
    /// `KONAMI-SCC-022`).
    #[must_use]
    pub const fn is_k052539_mode(&self) -> bool {
        matches!(self.member, Member::K052539) && self.mode & mode::K052539 != 0
    }

    /// Write the `052539`'s mode register, at `BFFEh` or `BFFFh` of the
    /// machine (`KONAMI-SCC-020`). Only bit 5 is the sound's; the rest are the
    /// mapper's and are kept but do nothing here. Ignored by the `051649`.
    pub fn write_mode(&mut self, d: u8) {
        if self.member == Member::K052539 {
            self.mode = d;
        }
    }

    /// Write `d` at `a` (the part's `A7`–`A0`) within the block, by the map of the
    /// part and, on the `052539`, of its mode (`KONAMI-SCC-006` to `KONAMI-SCC-010`,
    /// `KONAMI-SCC-021`, `KONAMI-SCC-022`).
    pub fn write(&mut self, a: Select, d: u8) {
        let a = a.levels();
        let o = usize::from(a);
        if self.is_k052539_mode() {
            match a {
                0x00..=0x9F => self.wave[o >> 5][o & 31] = d as i8,
                0xA0..=0xBF => self.control(a, d),
                0xC0..=0xDF => self.deformation = d,
                _ => {}
            }
            return;
        }
        match a {
            0x00..=0x7F => {
                self.wave[o >> 5][o & 31] = d as i8;
                if self.member == Member::K052539 && o >= 0x60 {
                    self.wave[4][o & 31] = d as i8;
                }
            }
            0x80..=0x9F => self.control(a, d),
            0xC0..=0xDF if self.member == Member::K052539 => self.deformation = d,
            0xE0..=0xFF if self.member == Member::K051649 => self.deformation = d,
            _ => {}
        }
    }

    /// The period, volume and key registers, sixteen bytes repeated twice
    /// (`KONAMI-SCC-007` to `KONAMI-SCC-010`).
    fn control(&mut self, a: u8, d: u8) {
        match a & 0x0F {
            r @ 0x0..=0x9 => {
                let ch = usize::from(r >> 1);
                self.period[ch] = if r & 1 == 0 {
                    self.period[ch] & 0xF00 | u16::from(d)
                } else {
                    self.period[ch] & 0x0FF | u16::from(d & 0x0F) << 8
                };
            }
            r @ 0xA..=0xE => self.volume[usize::from(r - 0xA)] = d & 0x0F,
            _ => self.key = d & 0x1F,
        }
    }

    /// Read `a` within the block: a waveform byte where the map has one,
    /// `FFh` everywhere else (`KONAMI-SCC-011`, `KONAMI-SCC-021`, `KONAMI-SCC-022`).
    /// The read changes nothing, the waveform it reads included
    /// (`KONAMI-SCC-OPEN-06`).
    #[must_use]
    pub fn read(&self, a: Select) -> u8 {
        let a = a.levels();
        let o = usize::from(a);
        let byte = match (self.is_k052539_mode(), a) {
            (true, 0x00..=0x9F) | (false, 0x00..=0x7F) => self.wave[o >> 5][o & 31],
            (false, 0xA0..=0xBF) if self.member == Member::K052539 => self.wave[4][o & 31],
            _ => return 0xFF,
        };
        byte as u8
    }

    /// The deformation register as last written; it changes nothing here
    /// (`KONAMI-SCC-OPEN-01`).
    #[must_use]
    pub const fn deformation(&self) -> u8 {
        self.deformation
    }

    /// Channel `ch`'s period register, the sheet's `TP`, 0–4095: each sample
    /// of the waveform lasts `TP` + 1 cycles (`KONAMI-SCC-007`,
    /// `KONAMI-SCC-013`).
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn tp(&self, ch: usize) -> u16 {
        self.period[ch]
    }

    /// Channel `ch`'s volume, rising with loudness: 0 is silent and 15 the
    /// loudest, each step one more eighth of the sample (`KONAMI-SCC-008`,
    /// `KONAMI-SCC-014`, `KONAMI-SCC-015`).
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn volume(&self, ch: usize) -> u8 {
        self.volume[ch]
    }

    /// Whether channel `ch` is keyed on: bit `ch` of the key register
    /// (`KONAMI-SCC-009`).
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn is_keyed(&self, ch: usize) -> bool {
        assert!(ch < CHANNEL_COUNT, "channel out of range");
        self.key >> ch & 1 != 0
    }

    /// The position within its waveform of the sample channel `ch` plays, 0–31
    /// (`KONAMI-SCC-012`).
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn wave_position(&self, ch: usize) -> usize {
        self.step[ch] as usize
    }

    /// The waveform channel `ch` plays: the `051649`'s channel 5 plays
    /// channel 4's (`KONAMI-SCC-006`).
    const fn source(&self, ch: usize) -> usize {
        if ch == 4 && matches!(self.member, Member::K051649) {
            3
        } else {
            ch
        }
    }

    /// One cycle of the clock. Each channel's step lasts its period + 1
    /// cycles, keyed or not (`KONAMI-SCC-013`, `KONAMI-SCC-019`); a new period is taken at
    /// the next step (`KONAMI-SCC-OPEN-04`).
    pub fn tick(&mut self) {
        for ch in 0..CHANNEL_COUNT {
            if self.count[ch] == 0 {
                self.count[ch] = self.period[ch];
                self.step[ch] = (self.step[ch] + 1) & 31;
            } else {
                self.count[ch] -= 1;
            }
        }
    }

    /// `n` cycles of the clock, as that many [`KonamiScc::tick`]s, for any `n`.
    pub fn advance(&mut self, n: usize) {
        let cycles = n;
        for ch in 0..CHANNEL_COUNT {
            let left = usize::from(self.count[ch]);
            if cycles <= left {
                self.count[ch] = (left - cycles) as u16;
                continue;
            }
            let p = usize::from(self.period[ch]) + 1;
            let rest = cycles - left - 1;
            let steps = 1 + (rest / p) % POSITIONS_PER_WAVEFORM;
            self.count[ch] = (p - 1 - rest % p) as u16;
            self.step[ch] = ((usize::from(self.step[ch]) + steps) & 31) as u8;
        }
    }

    /// Channel `ch`'s contribution: 0 while it is keyed off or its period is 8
    /// or less, otherwise its current sample times its volume shifted right
    /// three places with the sign kept, −240 to +238, within
    /// [`CHANNEL_FULL_SCALE`] (`KONAMI-SCC-015`, `KONAMI-SCC-017`,
    /// `KONAMI-SCC-018`).
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub fn channel_audio(&self, ch: usize) -> Audio<CHANNEL_FULL_SCALE> {
        assert!(ch < CHANNEL_COUNT, "channel out of range");
        if self.key >> ch & 1 == 0 || self.period[ch] <= LAST_SILENT_TP {
            return Audio(0);
        }
        let sample = i32::from(self.wave[self.source(ch)][usize::from(self.step[ch])]);
        Audio((sample * i32::from(self.volume[ch])) >> 3)
    }

    /// The five channels' [`KonamiScc::channel_audio`] summed, unclipped, as a
    /// signed sample at rest 0: one unit is one step of a channel's, from
    /// −1200 to +1190 (`KONAMI-SCC-016`). The part's own output stage, the
    /// eleven-bit code at its pins, is not modelled (`KONAMI-SCC-OPEN-03`).
    #[must_use]
    pub fn audio(&self) -> Audio<AUDIO_FULL_SCALE> {
        Audio((0..CHANNEL_COUNT).map(|ch| self.channel_audio(ch).0).sum())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `advance(usize::MAX)` overflows nothing and lands where ticking the
    /// same count modulo one whole cycle of every channel does: at `TP` 0
    /// a step is a cycle, at `TP` 1 two.
    #[test]
    fn advance_by_any_count_is_that_many_ticks() {
        let mut far = KonamiScc::new(Member::K052539);
        far.tick();
        let mut near = far.clone();
        far.advance(usize::MAX);
        near.advance(usize::MAX % POSITIONS_PER_WAVEFORM);
        assert_eq!(far, near);

        let mut slow = KonamiScc::new(Member::K051649);
        slow.write(Select::new(0x80), 0x01);
        let mut ticked = slow.clone();
        slow.advance(usize::MAX);
        for _ in 0..usize::MAX % (2 * POSITIONS_PER_WAVEFORM) {
            ticked.tick();
        }
        assert_eq!(slow, ticked);
    }
}
