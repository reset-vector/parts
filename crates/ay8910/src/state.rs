//! A readout of the whole machine, and the way back in.
//!
//! State that cannot leave the crate cannot be saved by anything outside it,
//! and this crate is deliberately free of any machine that might want to. This
//! is a **readout, not a behaviour**: saving and restoring change nothing the
//! part does.
//!
//! The image is forty-two bytes, little-endian, with no header — a caller that
//! wants a magic and a version puts one around it.

use crate::{Ay8910, Level, MASKS};

/// Bytes in one [`Ay8910::save_state`] image.
pub const STATE_LEN: usize = 42;

/// The widest legal tone counter: the period is twelve bits (`AY8910-022`).
const TONE_COUNTER_MASK: u16 = 0x0FFF;
/// The widest legal noise counter: the period is five bits (`AY8910-027`).
const NOISE_COUNTER_MASK: u8 = 0x1F;
/// The envelope counter is sixteen bits and clears at the live period, so it
/// never reaches `u16::MAX` under a running part (`AY8910-032`).
const ENVELOPE_COUNTER_MAX: u16 = u16::MAX - 1;
/// The seventeen latches of the noise register (`AY8910-029`).
const SHIFT_REGISTER_MASK: u32 = 0x1FFFF;
/// No register is latched.
const NO_ADDRESS: u8 = 0xFF;

impl Ay8910 {
    /// Every bit of this part's state, as forty-two bytes.
    ///
    /// The register file, the latched address, the divide-by-eight divider's cycle, and
    /// the three tone generators, the noise generator and the envelope
    /// generator with their counters, divide-by-two flip-flops and levels, and
    /// the `/RESET` level the last [`Ay8910::step`] showed, which the clock
    /// verbs tick on.
    #[must_use]
    pub fn save_state(&self) -> [u8; STATE_LEN] {
        let mut image = [0u8; STATE_LEN];
        image[0..16].copy_from_slice(&self.registers);
        image[16] = self.address.unwrap_or(NO_ADDRESS);
        image[17] = self.divider_cycle;
        for (channel, tone) in self.tone.iter().enumerate() {
            let at = 18 + channel * 3;
            image[at..at + 2].copy_from_slice(&tone.counter.to_le_bytes());
            image[at + 2] = u8::from(tone.level.is_high());
        }
        image[27] = self.noise.counter;
        image[28] = u8::from(self.noise.divider.is_high());
        image[29..33].copy_from_slice(&self.noise.shift_register.to_le_bytes());
        image[33..35].copy_from_slice(&self.envelope.counter.to_le_bytes());
        image[35] = u8::from(self.envelope.test1.is_high());
        image[36] = self.envelope.shape;
        image[37] = self.envelope.shape_counter;
        image[38] = u8::from(self.envelope.attack);
        image[39] = u8::from(self.envelope.holding);
        image[40] = u8::from(self.envelope.counter_deferred);
        image[41] = u8::from(self.reset_n);
        image
    }

    /// Restore an image from [`Ay8910::save_state`].
    ///
    /// Every field is masked to the width the die gives it, so any forty-two
    /// bytes produce a machine that runs. The masking is lossless over every state
    /// `save_state` can produce: it only bounds counters and fields whose
    /// width is fixed by the part, and a byte pattern outside those widths was
    /// never written by this crate.
    pub fn restore_state(&mut self, image: &[u8; STATE_LEN]) {
        for (register, (byte, mask)) in self
            .registers
            .iter_mut()
            .zip(image[0..16].iter().zip(MASKS))
        {
            *register = byte & mask;
        }
        self.address = (image[16] < 16).then_some(image[16]);
        self.divider_cycle = image[17] & 7;
        for (channel, tone) in self.tone.iter_mut().enumerate() {
            let at = 18 + channel * 3;
            tone.counter = u16::from_le_bytes([image[at], image[at + 1]]) & TONE_COUNTER_MASK;
            tone.level = Level::from_bool(image[at + 2] != 0);
        }
        self.noise.counter = image[27] & NOISE_COUNTER_MASK;
        self.noise.divider = Level::from_bool(image[28] != 0);
        self.noise.shift_register =
            u32::from_le_bytes([image[29], image[30], image[31], image[32]]) & SHIFT_REGISTER_MASK;
        self.envelope.counter =
            u16::from_le_bytes([image[33], image[34]]).min(ENVELOPE_COUNTER_MAX);
        self.envelope.test1 = Level::from_bool(image[35] != 0);
        self.envelope.shape = image[36] & 0x0F;
        self.envelope.shape_counter = image[37] & 0x0F;
        self.envelope.attack = image[38] != 0;
        self.envelope.holding = image[39] != 0;
        self.envelope.counter_deferred = image[40] != 0;
        self.reset_n = image[41] != 0;
    }
}
