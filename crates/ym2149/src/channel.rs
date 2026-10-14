use crate::Level;

/// One output channel as the mixer and level control resolve it from the
/// live registers.
///
/// The mixer level is `(tone ∨ ¬toneEnabled) ∧ (noise ∨ ¬noiseEnabled)`
/// (`YM2149-023`). `code` is the five-bit level handed to the channel's
/// converter: while the mixer level is High, the envelope's `E4`..`E0` under
/// `M = 1` or `2L + 1` for fixed level `L` under `M = 0` (`YM2149-024`,
/// `YM2149-026`, `YM2149-027`); while it is Low, level 0 of the selected mode — code 0
/// under the envelope, code 1 under a fixed level (`YM2149-028`).
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
    /// Whether R8–R10's `M` bit hands the channel the envelope's code in place
    /// of its fixed level.
    pub envelope_selected: bool,
    /// R8–R10's low four bits, the fixed volume `L`, 0 to [`VOLUME_FULL_STEPS`]
    /// rising with loudness in steps of two converter codes: a High mixer
    /// level hands the converter code `2L + 1`.
    pub volume_steps: u32,
    /// The five-bit converter code, 0..=31 ([`crate::dac::CODE_BIT_COUNT`]).
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
        let code = match (envelope_selected, mixer_level) {
            (true, true) => envelope_code,
            (true, false) => 0,
            (false, true) => 2 * volume + 1,
            (false, false) => 1,
        };
        Self {
            tone_level,
            noise_level,
            tone_enabled,
            noise_enabled,
            level: Level::from_bool(mixer_level),
            envelope_selected,
            volume_steps: u32::from(volume),
            code,
        }
    }
}
