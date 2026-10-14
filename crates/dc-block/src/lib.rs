//! A DC-blocking stage for any part's audio output, so that its DC level and its
//! power-on step do not reach a speaker.
//!
//! A part here stops at its pins. A level such as the 6581's `AUDIO OUT`, which
//! rests near 6 V, is that level with the DC included, and played as it stands
//! it is a full-scale step the moment playback starts. On a board, a coupling
//! capacitor removes it. [`DcBlock`] does that job for a caller with no board:
//! it represents no board's capacitor, and its cutoff and its fade-in are chosen
//! for listening safety, not for fidelity.
//!
//! Three things keep the power-on step off the output:
//!
//! - **The first sample sets the DC level.** A part that rests away from 0 from
//!   power-on reads 0 out of the first sample, and stays there while it rests.
//! - **A one-pole high-pass at [`CUTOFF_HZ`]** follows a DC level that moves
//!   afterwards: a part that settles onto a new rest, or a program that holds a
//!   channel at a steady level.
//! - **A fade-in over [`FADE_PS`]** scales the output up linearly from
//!   silence, so a step soon after power-on, which a high-pass alone passes at
//!   full height and only then decays, arrives already shrunk.
//!
//! A step after the fade-in is music and passes as a high-pass passes it.
//!
//! The stage is integer-only and `no_std`, takes `i32` samples in any unit at
//! any rate of 1 kHz or more, and returns them in the same unit. Its rate is
//! the rate [`DcBlock::sample`] is called at, which [`DcBlock::new`] is told.
//! A stereo part takes one stage a side, [`DcBlockStereo`].
//!
//! # Examples
//!
//! A part's `audio()` goes through it one sample at a time:
//!
//! ```
//! use dc_block::DcBlock;
//! use units::{Audio, Hertz};
//! # struct Part;
//! # impl Part {
//! #     fn audio(&self) -> Audio<32768> {
//! #         Audio(0)
//! #     }
//! # }
//! # let part = Part;
//!
//! let mut dc = DcBlock::new(Hertz(48_000));
//! let out = dc.sample(part.audio().0);
//! # assert_eq!(out, 0);
//! ```
//!
//! So does a pin level:
//!
//! ```
//! use dc_block::DcBlock;
//! use units::Hertz;
//!
//! let mut dc = DcBlock::new(Hertz(48_000));
//! // A pin resting at 6 V, in microvolts, reads 0 from the first sample.
//! assert_eq!(dc.sample(6_000_000), 0);
//! assert_eq!(dc.sample(6_000_000), 0);
//! // A full-scale step 1 ms after power-on comes out as a few percent of it.
//! for _ in 0..48 {
//!     let _ = dc.sample(6_000_000);
//! }
//! let first = dc.sample(12_000_000);
//! assert!(first > 0 && first < 6_000_000 / 20);
//! ```

#![no_std]

use units::{Hertz, Picoseconds};

/// The high-pass's cutoff frequency.
pub const CUTOFF_HZ: Hertz = Hertz(10);

/// How long the output takes to fade in from silence: 250 ms.
pub const FADE_PS: Picoseconds = Picoseconds(250_000_000_000);

/// The lowest sample rate [`DcBlock::new`] takes.
pub const SAMPLE_RATE_MIN_HZ: Hertz = Hertz(1_000);

/// Fractional bits of the DC level and the coefficient.
const FRAC: u32 = 24;

/// 2π, in billionths.
const TWO_PI_NANO: u128 = 6_283_185_307;

const PS_PER_S: u128 = 1_000_000_000_000;

/// One channel's DC-blocking stage.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DcBlock {
    /// The DC level the output is measured from, with [`FRAC`] fractional
    /// bits; `None` before the first sample.
    dc: Option<i64>,
    /// How far the DC level moves toward the input each sample, as a fraction
    /// with [`FRAC`] fractional bits.
    k: i64,
    /// Samples into the fade-in.
    elapsed: u32,
    /// Samples the fade-in lasts.
    fade_len: u32,
}

impl DcBlock {
    /// A stage for samples at `sample_rate_hz`, the rate [`Self::sample`] is
    /// called at, before its first sample.
    ///
    /// # Panics
    ///
    /// Below [`SAMPLE_RATE_MIN_HZ`].
    #[must_use]
    pub const fn new(sample_rate_hz: Hertz) -> Self {
        assert!(
            sample_rate_hz.0 >= SAMPLE_RATE_MIN_HZ.0,
            "a sample rate below 1 kHz"
        );
        let fs = sample_rate_hz.0 as u128;
        let k = (TWO_PI_NANO * CUTOFF_HZ.0 as u128 * (1 << FRAC) + fs * 500_000_000)
            / (fs * 1_000_000_000);
        let fade_len = fs * FADE_PS.0 as u128 / PS_PER_S;
        let k = if k == 0 { 1 } else { k };
        Self {
            dc: None,
            k: k as i64,
            elapsed: 0,
            fade_len: fade_len as u32,
        }
    }

    /// One sample in, one out, in the input's unit. The first sample's level is
    /// taken as the DC level and reads 0; the output saturates at `i32`.
    #[must_use]
    pub fn sample(&mut self, sample: i32) -> i32 {
        let x = i64::from(sample) << FRAC;
        let dc = *self.dc.get_or_insert(x);
        let y = x - dc;
        self.dc = Some(dc + ((i128::from(y) * i128::from(self.k)) >> FRAC) as i64);
        let mut out = (y + (1 << (FRAC - 1))) >> FRAC;
        if self.elapsed < self.fade_len {
            out = out * i64::from(self.elapsed) / i64::from(self.fade_len);
            self.elapsed += 1;
        }
        out.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    }

    /// Whether the fade-in has finished.
    #[must_use]
    pub const fn is_faded(&self) -> bool {
        self.elapsed >= self.fade_len
    }
}

/// A [`DcBlock`] for each side of a stereo part, `[left, right]`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DcBlockStereo {
    sides: [DcBlock; 2],
}

impl DcBlockStereo {
    /// Both stages for samples at `sample_rate_hz`, the rate [`Self::sample`]
    /// is called at.
    ///
    /// # Panics
    ///
    /// Below [`SAMPLE_RATE_MIN_HZ`].
    #[must_use]
    pub const fn new(sample_rate_hz: Hertz) -> Self {
        Self {
            sides: [DcBlock::new(sample_rate_hz), DcBlock::new(sample_rate_hz)],
        }
    }

    /// One `[left, right]` pair in, one out.
    #[must_use]
    pub fn sample(&mut self, [left, right]: [i32; 2]) -> [i32; 2] {
        [self.sides[0].sample(left), self.sides[1].sample(right)]
    }

    /// Whether the fade-in has finished.
    #[must_use]
    pub const fn is_faded(&self) -> bool {
        self.sides[0].is_faded()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_constant_level_reads_zero_from_the_first_sample() {
        let mut dc = DcBlock::new(Hertz(48_000));
        assert!((0..100_000).all(|_| dc.sample(i32::MAX) == 0));
        let mut dc = DcBlock::new(Hertz(48_000));
        assert!((0..100_000).all(|_| dc.sample(i32::MIN) == 0));
    }

    #[test]
    fn a_held_step_decays_to_zero() {
        let mut dc = DcBlock::new(Hertz(48_000));
        let _ = dc.sample(0);
        let out: [i32; 48_000] = core::array::from_fn(|_| dc.sample(1_000_000));
        assert!(out[1..].iter().all(|&y| y >= 0));
        assert_eq!(out[47_999], 0);
    }

    #[test]
    fn the_extremes_saturate_rather_than_wrap() {
        let mut dc = DcBlock::new(Hertz(48_000));
        let _ = dc.sample(i32::MIN);
        for _ in 0..12_000 {
            let _ = dc.sample(i32::MIN);
        }
        assert!(dc.is_faded());
        assert_eq!(dc.sample(i32::MAX), i32::MAX);
        let mut dc = DcBlock::new(Hertz(48_000));
        let _ = dc.sample(i32::MAX);
        for _ in 0..12_000 {
            let _ = dc.sample(i32::MAX);
        }
        assert_eq!(dc.sample(i32::MIN), i32::MIN);
    }

    #[test]
    fn the_dc_level_still_moves_at_the_highest_rate() {
        assert_eq!(DcBlock::new(Hertz(u32::MAX)).k, 1);
        let mut dc = DcBlock::new(Hertz(u32::MAX));
        let _ = dc.sample(0);
        let _ = dc.sample(i32::MAX);
        assert!(dc.dc.unwrap() > 0);
    }

    #[test]
    fn the_fade_in_lasts_its_stated_time() {
        let mut dc = DcBlock::new(Hertz(48_000));
        for _ in 0..12_000 {
            assert!(!dc.is_faded());
            let _ = dc.sample(0);
        }
        assert!(dc.is_faded());
    }

    #[test]
    fn the_cutoff_sits_at_ten_hertz() {
        let mut dc = DcBlock::new(Hertz(50_000));
        for _ in 0..12_500 {
            let _ = dc.sample(0);
        }
        let out: [i32; 900] = core::array::from_fn(|_| dc.sample(1_000_000));
        let one_over_e = 367_879;
        let crossed = out.iter().position(|&y| y < one_over_e).unwrap();
        assert!((795..=797).contains(&crossed), "crossed at {crossed}");
    }
}
