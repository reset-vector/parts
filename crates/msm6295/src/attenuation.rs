//! The attenuation as multipliers (`MSM6295-OPEN-04`, `MSM6295-OPEN-05`).

/// What code 0 multiplies by: `ATTENUATION_GAIN` is over this.
pub(crate) const GAIN_UNITY: u8 = 32;

/// Each attenuation code's multiplier over [`GAIN_UNITY`]: the smallest
/// integers whose decibels truncate to the printed nine (`MSM6295-OPEN-04`),
/// and silence for 9 to 15 (`MSM6295-OPEN-05`).
pub(crate) const ATTENUATION_GAIN: [u8; 16] = [32, 22, 16, 11, 8, 6, 4, 3, 2, 0, 0, 0, 0, 0, 0, 0];
