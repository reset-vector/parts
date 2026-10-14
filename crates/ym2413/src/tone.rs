//! A tone's eight bytes, and the per-slot view of them.
//!
//! The user tone register holds one tone as eight bytes (`YM2413-016`); the
//! Instruments ROM holds nineteen more in the same shape (`YM2413-081`). **This
//! crate holds none of the ROM's** (`YM2413-OPEN-13`). So a channel that
//! selects an instrument other than 0 resolves to no tone at all, and the
//! machine says so rather than substituting numbers.

use crate::frame::Role;

/// One slot's half of a tone: the fields that address the modulator or the
/// carrier rather than the channel.
///
/// `#[non_exhaustive]`: the part hands one back, [`crate::Ym2413::tone`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct SlotTone {
    /// Amplitude modulation switch, `D7` of `$00`/`$01` (`YM2413-043`).
    /// Decoded and not applied: setting it changes no output.
    pub am: bool,
    /// Vibrato switch, `D6` of `$00`/`$01` (`YM2413-041`).
    pub vib: bool,
    /// Sustained tone rather than percussive, `D5` (`YM2413-054`). Decoded and
    /// not applied: both kinds run one envelope law, so setting it changes no
    /// output (`YM2413-OPEN-16`).
    pub sustained: bool,
    /// Key scale of RATE switch, `D4` (`YM2413-051`).
    pub ksr: bool,
    /// Multiple, `D3`–`D0` (`YM2413-040`).
    pub mul: u8,
    /// Level key scale, two bits (`YM2413-065`).
    pub ksl: u8,
    /// Attack rate, four bits (`YM2413-056`).
    pub ar: u8,
    /// Decay rate, four bits (`YM2413-056`).
    pub dr: u8,
    /// Sustain level, four bits (`YM2413-055`).
    pub sl: u8,
    /// Release rate, four bits (`YM2413-056`).
    pub rr: u8,
    /// Half-wave rectification: `DM` for the modulator, `DC` for the carrier
    /// (`YM2413-061`, `YM2413-062`).
    pub half_wave: bool,
}

/// A whole tone, as the eight bytes of `$00`–`$07` lay it out.
///
/// `#[non_exhaustive]`: the part hands one back, [`crate::Ym2413::tone`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Tone {
    /// The modulator's half: the even addresses' fields and `DM`.
    pub modulator: SlotTone,
    /// The carrier's half: the odd addresses' fields and `DC`.
    pub carrier: SlotTone,
    /// Modulator total level `TL`, six bits — the modulation index control, and the
    /// only level field that is not the channel's volume (`YM2413-063`,
    /// `YM2413-064`).
    pub tl: u8,
    /// Feedback constant, three bits, the modulator's alone (`YM2413-059`,
    /// `YM2413-060`).
    pub feedback: u8,
}

impl Tone {
    /// Read a tone out of the eight bytes `$00`–`$07` (`YM2413-017` through
    /// `YM2413-020`).
    ///
    /// Even addresses are the modulator's and odd ones the carrier's, except
    /// `$02`'s `TL` and `$03`'s `DC`, `DM` and `FB`, which the map places on one
    /// side only.
    #[must_use]
    pub(crate) fn from_bytes(bytes: [u8; 8]) -> Self {
        let half = |byte: u8, ksl: u8, rates: u8, levels: u8, half_wave: bool| SlotTone {
            am: byte & 0b1000_0000 != 0,
            vib: byte & 0b0100_0000 != 0,
            sustained: byte & 0b0010_0000 != 0,
            ksr: byte & 0b0001_0000 != 0,
            mul: byte & 0x0F,
            ksl,
            ar: rates >> 4,
            dr: rates & 0x0F,
            sl: levels >> 4,
            rr: levels & 0x0F,
            half_wave,
        };
        Self {
            modulator: half(
                bytes[0],
                bytes[2] >> 6,
                bytes[4],
                bytes[6],
                bytes[3] & 0b0000_1000 != 0,
            ),
            carrier: half(
                bytes[1],
                bytes[3] >> 6,
                bytes[5],
                bytes[7],
                bytes[3] & 0b0001_0000 != 0,
            ),
            tl: bytes[2] & 0x3F,
            feedback: bytes[3] & 0b0000_0111,
        }
    }

    /// The half of this tone a slot's role selects.
    #[must_use]
    pub fn slot(&self, role: Role) -> SlotTone {
        match role {
            Role::Modulator => self.modulator,
            Role::Carrier => self.carrier,
        }
    }
}
