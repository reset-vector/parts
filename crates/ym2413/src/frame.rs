//! The audio frame: 72 master clocks, 18 slots, one operator cell.
//!
//! Indices here are zero-based because Rust indexes from zero; the manual
//! numbers slots 1–18 and channels 1–9, so **manual slot N is index N − 1** and
//! the same for channels. Every doc comment below gives the manual's number
//! where it names one.

/// Which of a channel's two operator passes a slot is (`YM2413-032`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// The manual's "No. 1 slot", which handles the modulating wave and is the
    /// only one that can be put in feedback FM mode.
    Modulator,
    /// The manual's "No. 2 slot", the carrier.
    Carrier,
}

/// Master clock cycles in one sample: one audio frame of 18 slots (`YM2413-029`).
pub const CYCLES_PER_SAMPLE: usize = 72;
/// Operator passes in one audio frame (`YM2413-030`).
pub const SLOT_COUNT: usize = 18;
/// Master clock cycles each slot occupies (`YM2413-034`).
pub const CYCLES_PER_SLOT: usize = CYCLES_PER_SAMPLE / SLOT_COUNT;
/// FM channels (`YM2413-031`).
pub const CHANNEL_COUNT: usize = 9;

/// The channel a slot belongs to, zero-based (`YM2413-031`).
///
/// The manual's Table II-3 reads `1 2 3 1 2 3 4 5 6 4 5 6 7 8 9 7 8 9` across
/// slots 1–18: three channels' modulators, then those same three channels'
/// carriers, three times over.
///
/// # Panics
///
/// Panics if `slot` is not below [`SLOT_COUNT`].
#[must_use]
pub(crate) fn channel_of(slot: usize) -> usize {
    assert!(slot < SLOT_COUNT, "slot {slot} is outside the frame");
    (slot / 6) * 3 + (slot % 3)
}

/// Whether a slot is its channel's modulator or its carrier (`YM2413-032`).
///
/// # Panics
///
/// Panics if `slot` is not below [`SLOT_COUNT`].
#[must_use]
pub(crate) fn role_of(slot: usize) -> Role {
    assert!(slot < SLOT_COUNT, "slot {slot} is outside the frame");
    if slot % 6 < 3 {
        Role::Modulator
    } else {
        Role::Carrier
    }
}

/// The five percussion instruments rhythm mode sounds (`YM2413-070`).
#[allow(
    dead_code,
    reason = "the rhythm voices that read it are not modelled (YM2413-OPEN-13)"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RhythmSound {
    /// Bass drum.
    Bd,
    /// High hat.
    Hh,
    /// Tom-tom.
    Tom,
    /// Snare drum.
    Sd,
    /// Top cymbal.
    TopCym,
}

/// The percussion instrument a slot carries in rhythm mode, or `None` for a
/// melody slot (`YM2413-072`).
///
/// Table III-9 puts `BD` on manual slots 13 and 16, `HH` on 14, `TOM` on 15,
/// `SD` on 17 and `TOP-CYM` on 18.
///
/// # Panics
///
/// Panics if `slot` is not below [`SLOT_COUNT`].
#[must_use]
#[allow(
    dead_code,
    reason = "the rhythm voices that read it are not modelled (YM2413-OPEN-13)"
)]
pub(crate) fn rhythm_sound_of(slot: usize) -> Option<RhythmSound> {
    assert!(slot < SLOT_COUNT, "slot {slot} is outside the frame");
    match slot {
        12 | 15 => Some(RhythmSound::Bd),
        13 => Some(RhythmSound::Hh),
        14 => Some(RhythmSound::Tom),
        16 => Some(RhythmSound::Sd),
        17 => Some(RhythmSound::TopCym),
        _ => None,
    }
}

/// The `$20`–`$28` register a slot's channel data lives in (`YM2413-033`).
///
/// The manual gives this as its own row of Table II-3, worked for `$20`–`$28`:
/// the per-channel registers are indexed by channel, so both of a channel's
/// slots read the same one. It is `base` plus the channel, wrapping past
/// `$FF`.
///
/// # Panics
///
/// Panics if `slot` is not below [`SLOT_COUNT`].
#[must_use]
#[cfg(test)]
pub(crate) fn channel_register(base: u8, slot: usize) -> u8 {
    base.wrapping_add(channel_of(slot) as u8)
}

/// Where the part is inside the audio frame.
///
/// The counter is the master clock within the frame, `0..72`. A slot completes
/// on the last clock of its four (`YM2413-034`), so `advance` reports a slot at
/// most once every four calls and exactly 18 times a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Frame {
    clock: usize,
}

impl Frame {
    /// The frame at its start.
    pub(crate) const START: Self = Self { clock: 0 };

    /// The master clock within the frame, `0..72`.
    #[must_use]
    pub(crate) const fn clock_index(self) -> usize {
        self.clock
    }

    /// The slot currently executing, `0..18`.
    #[must_use]
    pub(crate) fn slot(self) -> usize {
        self.clock / CYCLES_PER_SLOT
    }

    /// Advance one master clock. Returns the slot that just completed, on the
    /// clock it completed, and `None` on the other three.
    pub(crate) fn advance(&mut self) -> Option<usize> {
        let slot = self.slot();
        self.clock = (self.clock + 1) % CYCLES_PER_SAMPLE;
        self.clock.is_multiple_of(CYCLES_PER_SLOT).then_some(slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    #[test]
    fn both_slots_of_a_channel_read_its_register() {
        let registers: Vec<u8> = (0..SLOT_COUNT).map(|s| channel_register(0x20, s)).collect();
        assert_eq!(
            registers,
            [
                0x20, 0x21, 0x22, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x23, 0x24, 0x25, 0x26, 0x27,
                0x28, 0x26, 0x27, 0x28
            ]
        );
    }

    #[test]
    fn rhythm_mode_puts_five_sounds_on_six_slots() {
        let sounds: Vec<Option<RhythmSound>> = (0..SLOT_COUNT).map(rhythm_sound_of).collect();
        assert!(sounds[..12].iter().all(Option::is_none), "melody slots");
        assert_eq!(
            sounds[12..],
            [
                Some(RhythmSound::Bd),
                Some(RhythmSound::Hh),
                Some(RhythmSound::Tom),
                Some(RhythmSound::Bd),
                Some(RhythmSound::Sd),
                Some(RhythmSound::TopCym),
            ]
        );
        assert_eq!(channel_of(12), channel_of(15));
        assert_eq!(role_of(12), Role::Modulator);
        assert_eq!(role_of(15), Role::Carrier);
    }
}
