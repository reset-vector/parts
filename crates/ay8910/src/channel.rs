use crate::Level;

/// One output channel as the mixer and amplitude logic resolve it from the
/// live registers: the raw generator levels, the R7 enables, the mixer's Boolean level
/// `(tone ∨ ¬toneEnabled) ∧ (noise ∨ ¬noiseEnabled)`, the R8–R10 amplitude
/// selection, and the resulting four-bit D/A input code — the selected level
/// while the mixer level is High, zero while it is Low.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ChannelState {
    /// The channel's tone generator's square-wave level.
    pub tone_level: Level,
    /// The noise generator's level, which all three channels share.
    pub noise_level: Level,
    /// Whether R7 lets the tone into this channel's mixer (its tone bit Low).
    pub tone_enabled: bool,
    /// Whether R7 lets the noise into this channel's mixer (its noise bit Low).
    pub noise_enabled: bool,
    /// The channel's present bit: the mixer's level.
    pub level: Level,
    /// Whether R8–R10's bit 4 hands the channel the envelope's code in place of
    /// its fixed volume.
    pub envelope_selected: bool,
    /// R8–R10's low four bits, the fixed volume, in steps of one converter
    /// code: 0 to [`VOLUME_FULL_STEPS`], rising with loudness.
    pub volume_steps: u32,
    /// The four-bit converter code, 0..=15 ([`crate::dac::CODE_BIT_COUNT`]): 0
    /// while the mixer level is Low.
    pub code: u8,
}

impl Default for ChannelState {
    /// Every level Low, nothing enabled or selected, at code 0.
    fn default() -> Self {
        Self {
            tone_level: Level::Low,
            noise_level: Level::Low,
            tone_enabled: false,
            noise_enabled: false,
            level: Level::Low,
            envelope_selected: false,
            volume_steps: 0,
            code: 0,
        }
    }
}

/// The top of [`ChannelState::volume_steps`]: the fixed volume's four bits all set.
pub const VOLUME_FULL_STEPS: u32 = 15;

impl ChannelState {
    pub(crate) fn resolve(
        channel: usize,
        mixer: u8,
        amplitude: u8,
        tone_level: Level,
        noise_level: Level,
        envelope_code: u8,
    ) -> Self {
        let tone_enabled = mixer & (1 << channel) == 0;
        let noise_enabled = mixer & (8 << channel) == 0;
        let mixer_level =
            (tone_level.is_high() || !tone_enabled) && (noise_level.is_high() || !noise_enabled);
        let envelope_selected = amplitude & 0x10 != 0;
        let volume = amplitude & 0x0F;
        let selected = if envelope_selected {
            envelope_code
        } else {
            volume
        };
        Self {
            tone_level,
            noise_level,
            tone_enabled,
            noise_enabled,
            level: Level::from_bool(mixer_level),
            envelope_selected,
            volume_steps: u32::from(volume),
            code: if mixer_level { selected } else { 0 },
        }
    }
}
