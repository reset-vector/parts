//! The square-wave half: three tone generators, the noise generator, the
//! envelope generator and each channel's mixer and level.
//!
//! **Every generator advances on one step, once every `4 × m` cycles of
//! `φM`** for the SSG divider `m` of 4, 2 or 1 (`YM2203-032`): a tone
//! toggles every `TP` steps, the noise register shifts every `2 × NP`, the
//! envelope moves one of its thirty-two levels every `EP`, and a period of
//! zero is one (`YM2203-031`). Each counter's direction and phase out of
//! initial clear are this crate's choice under `YM2203-OPEN-17`.

/// One tone generator: a 12-bit counter and its square-wave level. Reaching
/// `max(1, period)` toggles the level (`YM2203-032`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct ToneState {
    pub counter: u16,
    pub level: bool,
}

impl ToneState {
    fn step(&mut self, period: u16) {
        self.counter += 1;
        if self.counter >= period.max(1) {
            self.counter = 0;
            self.level = !self.level;
        }
    }
}

/// The noise generator: a 5-bit counter, a divide-by-two and a 17-stage
/// register shifting once every `2 × max(1, NP)` steps (`YM2203-033`). The
/// register shifts towards bit 16 and takes `bit 16 XOR bit 13` into bit 0,
/// and the level is bit 16, so the level's sequence obeys
/// `out[n] = out[n−14] XOR out[n−17]` (`YM2203-033`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct NoiseState {
    pub counter: u8,
    pub phase: bool,
    /// The seventeen stages, bit 16 highest; 1 out of initial clear.
    pub shift_register: u32,
}

impl Default for NoiseState {
    fn default() -> Self {
        Self {
            counter: 0,
            phase: false,
            shift_register: 1,
        }
    }
}

impl NoiseState {
    /// The noise level the mixer sees.
    #[must_use]
    pub(crate) fn level(self) -> bool {
        self.shift_register & 1 << 16 != 0
    }

    fn step(&mut self, period: u8) {
        self.counter += 1;
        if self.counter >= period.max(1) {
            self.counter = 0;
            self.phase = !self.phase;
            if !self.phase {
                let feedback = ((self.shift_register >> 16) ^ (self.shift_register >> 13)) & 1;
                self.shift_register = (self.shift_register << 1 | feedback) & 0x1_FFFF;
            }
        }
    }
}

/// The envelope generator: a 16-bit counter that moves a five-bit shape
/// counter once every `max(1, EP)` steps, thirty-two to a cycle
/// (`YM2203-034`), and the shape's polarity and hold flags. A write to
/// `0DH` restarts it at step 31 with Attack as its polarity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct EnvelopeState {
    pub counter: u16,
    /// `0DH`'s low four bits as latched by the last trigger.
    pub shape: u8,
    /// The shape counter, counting 31 down to 0.
    pub step: u8,
    /// Output polarity: the level is `step` inverted while set.
    pub attack: bool,
    pub holding: bool,
}

impl Default for EnvelopeState {
    fn default() -> Self {
        Self::triggered(0)
    }
}

impl EnvelopeState {
    const HOLD: u8 = 1;
    const ALTERNATE: u8 = 2;
    const ATTACK: u8 = 4;
    const CONTINUE: u8 = 8;

    /// The five-bit envelope level.
    #[must_use]
    pub(crate) fn level(self) -> u8 {
        if self.attack {
            self.step ^ 0x1F
        } else {
            self.step
        }
    }

    pub(crate) fn triggered(shape: u8) -> Self {
        let shape = shape & 0x0F;
        Self {
            counter: 0,
            shape,
            step: 31,
            attack: shape & Self::ATTACK != 0,
            holding: false,
        }
    }

    fn step(&mut self, period: u16) {
        self.counter += 1;
        if self.counter >= period.max(1) {
            self.counter = 0;
            self.advance();
        }
    }

    fn advance(&mut self) {
        if self.holding {
            return;
        }
        if self.step > 0 {
            self.step -= 1;
            return;
        }
        let alternate = self.shape & Self::ALTERNATE != 0;
        if self.shape & Self::CONTINUE == 0 {
            self.attack = false;
            self.holding = true;
        } else if self.shape & Self::HOLD != 0 {
            self.holding = true;
            self.attack ^= alternate;
        } else {
            self.step = 31;
            self.attack ^= alternate;
        }
    }
}

/// The three generators' state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Ssg {
    pub tone: [ToneState; 3],
    pub noise: NoiseState,
    pub envelope: EnvelopeState,
}

impl Ssg {
    /// One step of every generator under the SSG registers `00H`–`0DH`.
    pub(crate) fn step(&mut self, regs: &[u8]) {
        for ch in 0..3 {
            let tp = u16::from(regs[2 * ch]) | u16::from(regs[2 * ch + 1] & 0x0F) << 8;
            self.tone[ch].step(tp);
        }
        self.noise.step(regs[6] & 0x1F);
        self.envelope
            .step(u16::from(regs[0x0B]) | u16::from(regs[0x0C]) << 8);
    }

    /// A channel's five-bit converter code under the mixer `07H` and its
    /// amplitude register (`YM2203-035`, `YM2203-036`). The gate is
    /// `(tone ∨ ¬toneEnabled) ∧ (noise ∨ ¬noiseEnabled)`; while it is open
    /// the code is the envelope's level under `M = 1` or `2L + 1` for fixed
    /// level `L`; while it is closed, code 0 under the envelope and 1 under a
    /// fixed level.
    #[must_use]
    pub(crate) fn code(&self, ch: usize, mixer: u8, amplitude: u8) -> u8 {
        let tone_enabled = mixer & (1 << ch) == 0;
        let noise_enabled = mixer & (8 << ch) == 0;
        let gate = (self.tone[ch].level || !tone_enabled) && (self.noise.level() || !noise_enabled);
        let fixed = amplitude & 0x0F;
        match (amplitude & 0x10 != 0, gate) {
            (true, true) => self.envelope.level(),
            (true, false) => 0,
            (false, true) => 2 * fixed + 1,
            (false, false) => 1,
        }
    }
}

/// The level each of the thirty-two codes sounds at, the bottom code at 0
/// and the top at 4094, one entry per step of the envelope rising one code at
/// a time (`YM2203-035`). The curve is this crate's choice under
/// `YM2203-OPEN-16`, not the part's converter.
#[allow(dead_code, reason = "the YM2203-035 curve, which only its test reads")]
pub(crate) const CODE_LEVELS: [u16; 32] = [
    0, 8, 19, 35, 44, 55, 65, 76, 92, 110, 127, 146, 175, 209, 241, 278, 333, 399, 463, 536, 644,
    770, 894, 1034, 1250, 1501, 1756, 2039, 2491, 2994, 3532, 4094,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_levels() {
        const FIXED_LEVELS: [u16; 16] = [
            0, 27, 47, 68, 102, 138, 201, 270, 391, 529, 763, 1028, 1496, 2035, 2992, 4094,
        ];
        for level in 0..16u8 {
            let code = 2 * level + 1;
            let code_level = CODE_LEVELS[code as usize];
            assert!(
                code_level.abs_diff(FIXED_LEVELS[level as usize]) <= 8,
                "level {level}"
            );
        }
        assert!(CODE_LEVELS.windows(2).all(|w| w[0] < w[1]));
    }
}
