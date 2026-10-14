//! The attenuation ladder as amplitudes.
//!
//! `SN76489-034` says one step is 2 dB of *power*, so it multiplies an
//! amplitude by ten to the power of minus one tenth; `SN76489-019` says the
//! all-ones code is off rather than a sixteenth step. This table is those two
//! sentences and nothing else, and it never touches the machine.
//!
//! **The scale is this crate's choice.** Full scale is
//! [`crate::CHANNEL_FULL_SCALE`], the largest value an `i16` sample carries.
//! Four channels at full scale overflow an `i16`, so
//! [`crate::Sn76489::audio`] sums them in an `i32`. What the part actually
//! puts on pin 7 is analogue, is past the summing buffer of `SN76489-033`,
//! and is not this crate's.

use units::PartsPerBillion;

/// Amplitudes for attenuation codes 0..=15, each 2 dB of power below the one
/// before it and the last silent.
///
/// The ratio between neighbours is [`LADDER_RATIO_PPB`].
pub const LADDER: [i16; 16] = [
    32_767, 26_028, 20_675, 16_422, 13_045, 10_362, 8_231, 6_538, 5_193, 4_125, 3_277, 2_603,
    2_067, 1_642, 1_304, 0,
];

/// The ratio between neighbouring amplitudes of [`LADDER`], ten to the power
/// of minus one tenth.
pub const LADDER_RATIO_PPB: PartsPerBillion = PartsPerBillion(794_328_235);
