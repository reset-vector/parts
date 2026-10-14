//! The physical quantities the chip crates take and return, one type per unit.
//!
//! Each type wraps the integer its unit is carried in, so a voltage cannot be
//! passed where a time is wanted, nor a resistance where a capacitance is. The
//! field is public: a constant is written `Microvolts(5_000_000)` and a value
//! read back by `.0` or `i32::from(..)`. Arithmetic within one unit (`+`, `-`,
//! `Sum`, and scaling by the bare integer, `*` and `/`) keeps the unit; anything that
//! crosses units unwraps, computes, and wraps the result in its own unit.
//! Each operator saturates at its integer's range, so a debug and a release
//! build give one answer.
//!
//! A name carrying one of these still ends in its unit (`vcc_uv: Microvolts`).
//!
//! # Examples
//!
//! ```
//! use units::{Microvolts, Ohms};
//!
//! let vcc = Microvolts(5_000_000);
//! let threshold = vcc * 2 / 3;
//! assert_eq!(threshold, Microvolts(3_333_333));
//! assert_eq!(threshold.to_string(), "3333333 µV");
//! assert!(Ohms(10_000) > Ohms(1_000));
//! ```
//!
//! # Panics
//!
//! `/` by zero panics, as the integer's own does.

#![no_std]

use core::fmt;
use core::iter::Sum;
use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

/// A voltage in microvolts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Microvolts(pub i32);

/// A time in picoseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Picoseconds(pub u64);

/// A current in picoamperes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Picoamps(pub i64);

/// A resistance in ohms.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ohms(pub u64);

/// A capacitance in picofarads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Picofarads(pub u32);

/// A capacitance in tenths of a picofarad, for figures the sheet gives finer
/// than [`Picofarads`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TenthPicofarads(pub u32);

/// A frequency in hertz.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hertz(pub u32);

/// A frequency in millihertz, for figures the sheet gives finer than [`Hertz`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Millihertz(pub u64);

/// A power in milliwatts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Milliwatts(pub u32);

/// A level ratio in decibels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Decibels(pub i32);

/// A level ratio in centibels (0.1 dB). An attenuation is a loss and positive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Centibels(pub i32);

/// A voltage gain in volts per volt, where the sheet's figures are whole.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VoltsPerVolt(pub u32);

/// A temperature in degrees Celsius.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Celsius(pub i32);

/// A dimensionless ratio in parts per billion: a gain finer than
/// [`VoltsPerVolt`], or a signed coefficient.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PartsPerBillion(pub i64);

/// A dimensionless ratio in parts per million.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PartsPerMillion(pub i32);

/// A dimensionless ratio in parts per million per degree Celsius, a drift.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PartsPerMillionPerCelsius(pub i32);

/// A dimensionless ratio in parts per thousand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PerMille(pub u32);

/// A slew rate in microvolts per second.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MicrovoltsPerSecond(pub u64);

/// A transresistance in microvolts per picoampere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MicrovoltsPerPicoamp(pub u32);

/// A current slew rate in picoamperes per picosecond.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PicoampsPerPicosecond(pub i64);

/// A time per unit of capacitance in picoseconds per picofarad, a timing
/// component's coefficient.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PicosecondsPerPicofarad(pub u64);

/// The two voltages an analog output reaches and does not pass, in
/// microvolts: its lowest and its highest.
///
/// The invariant [`Swing::declared`] holds is the one a model cannot do
/// without: the low limit is strictly below the high one. Where the limits
/// come from — a caller's board, a figure of the sheet — is the part's to say.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Swing {
    low_uv: Microvolts,
    high_uv: Microvolts,
}

impl Swing {
    /// A caller's declared swing, or `None` if the low limit is not strictly
    /// below the high one.
    #[must_use]
    pub const fn declared(low_uv: Microvolts, high_uv: Microvolts) -> Option<Self> {
        if low_uv.0 >= high_uv.0 {
            return None;
        }
        Some(Self { low_uv, high_uv })
    }

    /// The lowest and highest the output goes, in microvolts, `(min, max)`.
    #[must_use]
    pub const fn range_uv(self) -> (Microvolts, Microvolts) {
        (self.low_uv, self.high_uv)
    }

    /// The lowest the output goes, in microvolts.
    #[must_use]
    pub const fn low_uv(self) -> Microvolts {
        self.low_uv
    }

    /// The highest the output goes, in microvolts.
    #[must_use]
    pub const fn high_uv(self) -> Microvolts {
        self.high_uv
    }
}

/// A sound part's output level in the part's own steps: signed and 0 at rest.
/// A part gives at most `FULL_SCALE` in magnitude, its `AUDIO_FULL_SCALE` or
/// `CHANNEL_FULL_SCALE`; the type does not hold that bound, so a level built
/// by hand (`Audio(n)`, `From<i32>`), summed or scaled (`+`, `*`, which
/// saturate only at `i32`'s range) can pass it.
///
/// The type tells scales apart by their number: two parts' samples are one
/// type where their full scales are equal, whatever each step means, so a
/// mixer that adds levels from two parts normalises each first
/// ([`Audio::to_ppb`]), and the type refuses a bare sum only where the numbers
/// differ. A level the part gives in a physical unit is that unit's type
/// instead (`Microvolts`). A `FULL_SCALE` that is not positive does not
/// compile where the level is normalised.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Audio<const FULL_SCALE: i32>(pub i32);

impl<const FULL_SCALE: i32> Audio<FULL_SCALE> {
    /// The level as a fraction of `FULL_SCALE`, in parts per billion, which
    /// is ±1 000 000 000 at full scale and the scale two parts' samples are
    /// mixed on. It is rounded to the nearest part, a half part away from
    /// zero, and does not clamp: a level past full scale gives past
    /// ±1 000 000 000 in proportion.
    ///
    /// ```
    /// assert_eq!(units::Audio::<3>(1).to_ppb().0, 333_333_333);
    /// assert_eq!(units::Audio::<3>(-2).to_ppb().0, -666_666_667);
    /// ```
    ///
    /// ```compile_fail
    /// let _ = units::Audio::<0>(3).to_ppb();
    /// ```
    #[must_use]
    pub const fn to_ppb(self) -> PartsPerBillion {
        const { assert!(FULL_SCALE > 0, "an Audio's FULL_SCALE is positive") };
        let n = self.0 as i64 * 1_000_000_000;
        let d = FULL_SCALE as i64;
        let (q, r) = (n / d, n % d);
        let away = if 2 * r.abs() >= d { n.signum() } else { 0 };
        PartsPerBillion(q + away)
    }
}

/// Room for the longest unit's text: an `i64` and its symbol, or an
/// [`Audio`]'s two `i32`s.
const TEXT_BYTES: usize = 48;

/// A unit's text, written whole before it is padded to the caller's width.
struct Text {
    bytes: [u8; TEXT_BYTES],
    len: usize,
}

impl Text {
    const fn new() -> Self {
        Self {
            bytes: [0; TEXT_BYTES],
            len: 0,
        }
    }

    fn as_str(&self) -> Result<&str, fmt::Error> {
        core::str::from_utf8(&self.bytes[..self.len]).map_err(|_| fmt::Error)
    }
}

/// Writes `value` and then `tail` as one field of `f`. The `+` flag and `0`
/// padding apply to the integer, as they do to the integer's own `Display`;
/// width, fill and alignment (left unless given) apply to the whole text; and
/// a precision is ignored, as the integer ignores it.
fn field(f: &mut fmt::Formatter<'_>, value: i128, tail: fmt::Arguments<'_>) -> fmt::Result {
    use fmt::Write as _;
    let mut after = Text::new();
    after.write_fmt(tail)?;
    let after = after.as_str()?;
    let width = f.width().unwrap_or(0);
    if f.sign_aware_zero_pad() {
        let digits = width.saturating_sub(after.chars().count());
        if f.sign_plus() {
            write!(f, "{value:+0digits$}")?;
        } else {
            write!(f, "{value:0digits$}")?;
        }
        return f.write_str(after);
    }
    let mut text = Text::new();
    if f.sign_plus() {
        write!(text, "{value:+}{after}")?;
    } else {
        write!(text, "{value}{after}")?;
    }
    let text = text.as_str()?;
    let gap = width.saturating_sub(text.chars().count());
    let (before, behind) = match f.align() {
        Some(fmt::Alignment::Right) => (gap, 0),
        Some(fmt::Alignment::Center) => (gap / 2, gap - gap / 2),
        Some(fmt::Alignment::Left) | None => (0, gap),
    };
    let fill = f.fill();
    for _ in 0..before {
        f.write_char(fill)?;
    }
    f.write_str(text)?;
    for _ in 0..behind {
        f.write_char(fill)?;
    }
    Ok(())
}

impl fmt::Write for Text {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len + s.len();
        self.bytes
            .get_mut(self.len..end)
            .ok_or(fmt::Error)?
            .copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

impl<const FULL_SCALE: i32> From<i32> for Audio<FULL_SCALE> {
    fn from(value: i32) -> Self {
        Self(value)
    }
}

impl<const FULL_SCALE: i32> From<Audio<FULL_SCALE>> for i32 {
    fn from(value: Audio<FULL_SCALE>) -> Self {
        value.0
    }
}

impl<const FULL_SCALE: i32> Add for Audio<FULL_SCALE> {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }
}

/// The saturating sum, as `+` gives it: a mix of levels on one scale.
impl<const FULL_SCALE: i32> Sum for Audio<FULL_SCALE> {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self(0), Add::add)
    }
}

impl<'a, const FULL_SCALE: i32> Sum<&'a Self> for Audio<FULL_SCALE> {
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.copied().sum()
    }
}

impl<const FULL_SCALE: i32> Sub for Audio<FULL_SCALE> {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }
}

impl<const FULL_SCALE: i32> AddAssign for Audio<FULL_SCALE> {
    fn add_assign(&mut self, rhs: Self) {
        self.0 = self.0.saturating_add(rhs.0);
    }
}

impl<const FULL_SCALE: i32> SubAssign for Audio<FULL_SCALE> {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 = self.0.saturating_sub(rhs.0);
    }
}

impl<const FULL_SCALE: i32> Mul<i32> for Audio<FULL_SCALE> {
    type Output = Self;
    fn mul(self, rhs: i32) -> Self {
        Self(self.0.saturating_mul(rhs))
    }
}

impl<const FULL_SCALE: i32> Div<i32> for Audio<FULL_SCALE> {
    type Output = Self;
    fn div(self, rhs: i32) -> Self {
        Self(self.0.saturating_div(rhs))
    }
}

impl<const FULL_SCALE: i32> MulAssign<i32> for Audio<FULL_SCALE> {
    fn mul_assign(&mut self, rhs: i32) {
        self.0 = self.0.saturating_mul(rhs);
    }
}

impl<const FULL_SCALE: i32> DivAssign<i32> for Audio<FULL_SCALE> {
    fn div_assign(&mut self, rhs: i32) {
        self.0 = self.0.saturating_div(rhs);
    }
}

impl<const FULL_SCALE: i32> Neg for Audio<FULL_SCALE> {
    type Output = Self;
    fn neg(self) -> Self {
        Self(self.0.saturating_neg())
    }
}

impl<const FULL_SCALE: i32> fmt::Display for Audio<FULL_SCALE> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        field(f, i128::from(self.0), format_args!(" / {FULL_SCALE}"))
    }
}

/// The traits every unit has: conversion to and from its integer, arithmetic
/// within the unit (`Sum` included, saturating as `+` does), scaling by the integer, and `Display` with the symbol.
macro_rules! unit {
    ($unit:ident, $int:ty, $symbol:literal) => {
        impl From<$int> for $unit {
            fn from(value: $int) -> Self {
                Self(value)
            }
        }

        impl From<$unit> for $int {
            fn from(value: $unit) -> Self {
                value.0
            }
        }

        impl Add for $unit {
            type Output = Self;
            fn add(self, rhs: Self) -> Self {
                Self(self.0.saturating_add(rhs.0))
            }
        }

        impl Sum for $unit {
            fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
                iter.fold(Self(0), Add::add)
            }
        }

        impl<'a> Sum<&'a Self> for $unit {
            fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
                iter.copied().sum()
            }
        }

        impl Sub for $unit {
            type Output = Self;
            fn sub(self, rhs: Self) -> Self {
                Self(self.0.saturating_sub(rhs.0))
            }
        }

        impl AddAssign for $unit {
            fn add_assign(&mut self, rhs: Self) {
                self.0 = self.0.saturating_add(rhs.0);
            }
        }

        impl SubAssign for $unit {
            fn sub_assign(&mut self, rhs: Self) {
                self.0 = self.0.saturating_sub(rhs.0);
            }
        }

        impl Mul<$int> for $unit {
            type Output = Self;
            fn mul(self, rhs: $int) -> Self {
                Self(self.0.saturating_mul(rhs))
            }
        }

        impl Div<$int> for $unit {
            type Output = Self;
            fn div(self, rhs: $int) -> Self {
                Self(self.0.saturating_div(rhs))
            }
        }

        impl MulAssign<$int> for $unit {
            fn mul_assign(&mut self, rhs: $int) {
                self.0 = self.0.saturating_mul(rhs);
            }
        }

        impl DivAssign<$int> for $unit {
            fn div_assign(&mut self, rhs: $int) {
                self.0 = self.0.saturating_div(rhs);
            }
        }

        impl fmt::Display for $unit {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                field(f, i128::from(self.0), format_args!(concat!(" ", $symbol)))
            }
        }
    };
}

/// `Neg` for a unit carried in a signed integer.
macro_rules! signed {
    ($unit:ident) => {
        impl Neg for $unit {
            type Output = Self;
            fn neg(self) -> Self {
                Self(self.0.saturating_neg())
            }
        }
    };
}

unit!(Microvolts, i32, "µV");
unit!(Picoseconds, u64, "ps");
unit!(Picoamps, i64, "pA");
unit!(Ohms, u64, "Ω");
unit!(Picofarads, u32, "pF");
unit!(TenthPicofarads, u32, "× 0.1 pF");
unit!(Hertz, u32, "Hz");
unit!(Millihertz, u64, "mHz");
unit!(Milliwatts, u32, "mW");
unit!(Decibels, i32, "dB");
unit!(Centibels, i32, "cB");
unit!(VoltsPerVolt, u32, "V/V");
unit!(Celsius, i32, "°C");
unit!(PartsPerBillion, i64, "ppb");
unit!(PartsPerMillion, i32, "ppm");
unit!(PartsPerMillionPerCelsius, i32, "ppm/°C");
unit!(PerMille, u32, "‰");
unit!(MicrovoltsPerSecond, u64, "µV/s");
unit!(MicrovoltsPerPicoamp, u32, "µV/pA");
unit!(PicoampsPerPicosecond, i64, "pA/ps");
unit!(PicosecondsPerPicofarad, u64, "ps/pF");

signed!(Microvolts);
signed!(Picoamps);
signed!(Decibels);
signed!(Centibels);
signed!(Celsius);
signed!(PartsPerBillion);
signed!(PartsPerMillion);
signed!(PartsPerMillionPerCelsius);
signed!(PicoampsPerPicosecond);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_keeps_the_unit() {
        let mut v = Microvolts(1_000) + Microvolts(500) - Microvolts(250);
        v += Microvolts(50);
        v -= Microvolts(300);
        assert_eq!(v, Microvolts(1_000));
        assert_eq!(-v, Microvolts(-1_000));
        assert_eq!(Picoseconds(9) * 2 / 3, Picoseconds(6));
        let mut t = Picoseconds(9);
        t *= 2;
        t /= 3;
        assert_eq!(t, Picoseconds(6));
        let mut a = Audio::<100>(30);
        a *= 3;
        a /= 2;
        assert_eq!(a, Audio(45));
    }

    #[test]
    fn arithmetic_saturates_at_the_integers_range() {
        assert_eq!(Picoseconds(1) - Picoseconds(2), Picoseconds(0));
        assert_eq!(Microvolts(i32::MAX / 2) * 3, Microvolts(i32::MAX));
        assert_eq!(Microvolts(i32::MIN) - Microvolts(1), Microvolts(i32::MIN));
        assert_eq!(-Microvolts(i32::MIN), Microvolts(i32::MAX));
        assert_eq!(Microvolts(i32::MIN) / -1, Microvolts(i32::MAX));
        let mut t = Picoseconds(u64::MAX);
        t += Picoseconds(1);
        assert_eq!(t, Picoseconds(u64::MAX));
        t -= Picoseconds(u64::MAX);
        t -= Picoseconds(1);
        assert_eq!(t, Picoseconds(0));
    }

    #[test]
    fn a_sample_normalises_against_its_full_scale() {
        assert_eq!(Audio::<4>(-2).to_ppb(), PartsPerBillion(-500_000_000));
        assert_eq!(Audio::<4>(4).to_ppb(), PartsPerBillion(1_000_000_000));
        assert_eq!(Audio::<4>(8).to_ppb(), PartsPerBillion(2_000_000_000));
        assert_eq!(Audio::<4>(3) + Audio(2), Audio(5));
        assert_eq!(Audio::<4>(i32::MIN) - Audio(1), Audio(i32::MIN));
    }

    #[test]
    fn sum_adds_as_plus_does_and_saturates() {
        let levels = [Audio::<4>(1), Audio(-3), Audio(4)];
        assert_eq!(levels.iter().sum::<Audio<4>>(), Audio(2));
        assert_eq!(levels.into_iter().sum::<Audio<4>>(), Audio(2));
        assert_eq!(
            [Audio::<4>(i32::MAX), Audio(1)]
                .into_iter()
                .sum::<Audio<4>>(),
            Audio(i32::MAX)
        );
        assert_eq!(
            core::iter::empty::<Microvolts>().sum::<Microvolts>(),
            Microvolts(0)
        );
        assert_eq!([Ohms(10), Ohms(5)].iter().sum::<Ohms>(), Ohms(15));
        assert_eq!(
            [Picoseconds(u64::MAX), Picoseconds(1)]
                .into_iter()
                .sum::<Picoseconds>(),
            Picoseconds(u64::MAX)
        );
    }

    #[test]
    fn a_sample_has_every_units_operators() {
        let mut mix = Audio::<4>(1);
        mix += Audio(3);
        mix -= Audio(1);
        assert_eq!(mix, Audio(3));
        assert_eq!(mix * 4 / 3, Audio(4));
        assert_eq!(Audio::<4>(i32::MAX / 2) * 3, Audio(i32::MAX));
        assert_eq!(Audio::<4>(i32::MIN) / -1, Audio(i32::MAX));
        mix = Audio(i32::MAX);
        mix += Audio(1);
        assert_eq!(mix, Audio(i32::MAX));
    }

    #[test]
    fn to_ppb_rounds_to_the_nearest_part_a_half_away_from_zero() {
        assert_eq!(Audio::<3>(1).to_ppb(), PartsPerBillion(333_333_333));
        assert_eq!(Audio::<3>(2).to_ppb(), PartsPerBillion(666_666_667));
        assert_eq!(Audio::<3>(-2).to_ppb(), PartsPerBillion(-666_666_667));
        let half = Audio::<2_000_000_000>(1).to_ppb();
        assert_eq!(half, PartsPerBillion(1));
        assert_eq!(Audio::<2_000_000_000>(-1).to_ppb(), PartsPerBillion(-1));
        assert_eq!(
            Audio::<{ i32::MAX }>(i32::MIN).to_ppb(),
            PartsPerBillion(-1_000_000_000)
        );
    }

    #[test]
    fn display_honours_width_fill_and_alignment() {
        extern crate std;
        use std::format;
        assert_eq!(format!("{}", Microvolts(5)), "5 µV");
        assert_eq!(format!("{:>10}", Microvolts(5)), "      5 µV");
        assert_eq!(format!("{:*<8}", Ohms(47)), "47 Ω****");
        assert_eq!(format!("{:^9}", Hertz(60)), "  60 Hz  ");
        assert_eq!(format!("{:>12}", Audio::<4>(-2)), "      -2 / 4");
        assert_eq!(
            format!("{}", PartsPerBillion(i64::MIN)),
            "-9223372036854775808 ppb"
        );
        assert_eq!(
            format!("{}", Audio::<{ i32::MAX }>(i32::MIN)),
            "-2147483648 / 2147483647"
        );
    }

    #[test]
    fn display_ignores_a_precision_and_signs_and_zero_pads_the_integer() {
        extern crate std;
        use std::format;
        assert_eq!(format!("{:.2}", Microvolts(1234)), "1234 µV");
        assert_eq!(format!("{:.3}", Hertz(44_100)), "44100 Hz");
        assert_eq!(format!("{:.1}", Audio::<32767>(-12)), "-12 / 32767");
        assert_eq!(format!("{:>9.1}", Hertz(60)), "    60 Hz");
        assert_eq!(format!("{:+}", Microvolts(5)), "+5 µV");
        assert_eq!(format!("{:+}", Microvolts(-5)), "-5 µV");
        assert_eq!(format!("{:07}", Microvolts(-5)), "-005 µV");
        assert_eq!(format!("{:+08}", Ohms(47)), "+00047 Ω");
        assert_eq!(format!("{:010}", Audio::<4>(-2)), "-00002 / 4");
        assert_eq!(format!("{:02}", Hertz(60)), "60 Hz");
        assert_eq!(format!("{:>60}", Hertz(60)).len(), 60);
    }

    #[test]
    fn conversions_round_trip_through_the_integer() {
        assert_eq!(Ohms::from(470), Ohms(470));
        assert_eq!(u64::from(Ohms(470)), 470);
    }

    #[test]
    fn a_swing_is_declared_only_in_order() {
        assert_eq!(Swing::declared(Microvolts(5), Microvolts(5)), None);
        assert_eq!(Swing::declared(Microvolts(6), Microvolts(5)), None);
        let wide = Swing::declared(Microvolts(i32::MIN), Microvolts(i32::MAX));
        assert_eq!(
            wide.map(Swing::range_uv),
            Some((Microvolts(i32::MIN), Microvolts(i32::MAX)))
        );
    }
}
