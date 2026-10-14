//! The Fujitsu MB3731 bridge audio power amplifier, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **One connection.** The data sheet's figures are all taken in its own
//! typical connection (`MB3731-014`) and it prints no internal resistance
//! (`MB3731-003`), so the gain here is that connection's and no other. A
//! board that connects the feedback pin some other way is outside what this
//! crate can say.
//!
//! **A static transfer.** [`Mb3731::drive`] gives the two output pins, and
//! [`Mb3731::bridge_uv`] the voltage across the load, pin 10 less pin 8, for
//! a voltage on pin 1 relative to the input's DC level. It holds no state and
//! keeps no time, so it is the same at any step a consumer calls it at. It
//! does not clip (`MB3731-OPEN-02`): the one output limit the sheet gives is a
//! power at one supply and load, offered as [`rated_peak_uv`].
//!
//! # Examples
//!
//! ```
//! use mb3731::{Corner, Inputs, Mb3731};
//! use units::{Microvolts, PartsPerBillion};
//!
//! // The typical part, 47 dB: a millivolt in is about 224 mV across the load.
//! let typical = Mb3731::at_corner(false, Corner::Typ);
//! let pins = Inputs {
//!     in_signal_uv: Microvolts(1_000),
//! };
//! assert_eq!(typical.bridge_uv(pins), Microvolts(223_872));
//!
//! // A specimen's own gain, 200 V/V, muted to a hundredth of its output.
//! let specimen = Mb3731::new(
//!     PartsPerBillion(200_000_000_000),
//!     Some(PartsPerBillion(10_000_000)),
//! )?;
//! assert!(specimen.is_muted());
//! assert_eq!(specimen.bridge_uv(pins), Microvolts(2_000));
//! # Ok::<(), mb3731::Mb3731Error>(())
//! ```

#![no_std]

use units::{Centibels, Microvolts, Milliwatts, Ohms, PartsPerBillion};

/// How many pins the package presents (`MB3731-002`).
pub const PIN_COUNT: usize = 12;

/// Which of the sheet's three figures for a quantity to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Corner {
    /// The table's minimum.
    Min,
    /// The table's typical.
    Typ,
    /// The table's maximum.
    Max,
}

/// The voltage gain in centibels, tenths of a decibel, minimum, typical and
/// maximum, at 1 W in the sheet's connection (`MB3731-006`).
pub const GAIN_MIN_TYP_MAX_CB: (Centibels, Centibels, Centibels) =
    (Centibels(445), Centibels(470), Centibels(495));

/// The same three gains as voltage ratios in parts per billion, minimum,
/// typical and maximum: 10^(dB/20) to the nearest thousandth (`MB3731-006`).
pub const GAIN_MIN_TYP_MAX_PPB: (PartsPerBillion, PartsPerBillion, PartsPerBillion) = (
    PartsPerBillion(167_880_000_000),
    PartsPerBillion(223_872_000_000),
    PartsPerBillion(298_538_000_000),
);

/// The typical mute attenuation, 43 dB, as the fraction of the output left,
/// in parts per billion: 10^(−43/20) to the nearest millionth
/// (`MB3731-013`).
pub const MUTE_TYP_PPB: PartsPerBillion = PartsPerBillion(7_079_000);

/// `RIN`, the input resistance, minimum and typical, in ohms (`MB3731-010`).
pub const RIN_MIN_TYP_OHMS: (Ohms, Ohms) = (Ohms(40_000), Ohms(70_000));

/// The supply the electrical table is taken at, in microvolts: 13.2 V
/// (`MB3731-006`).
pub const TEST_SUPPLY_UV: Microvolts = Microvolts(13_200_000);
/// The load the electrical table is taken into, in ohms (`MB3731-006`).
pub const TEST_LOAD_OHMS: Ohms = Ohms(4);
/// `Po`, the output power at 10 % distortion, minimum and typical, in
/// milliwatts (`MB3731-007`).
pub const PO_MIN_TYP_MW: (Milliwatts, Milliwatts) = (Milliwatts(15_000), Milliwatts(18_000));

/// The recommended supply range, in microvolts: 8 V to 16 V (`MB3731-005`).
pub const SUPPLY_UV: (Microvolts, Microvolts) = (Microvolts(8_000_000), Microvolts(16_000_000));

/// The peak of a sine of `power_mw` into [`TEST_LOAD_OHMS`], in
/// microvolts, rounded down and held to `i32::MAX`: `sqrt(2 P R)`
/// (`MB3731-007`).
#[must_use]
pub const fn rated_peak_uv(power_mw: Milliwatts) -> Microvolts {
    let square = 2 * (power_mw.0 as u128) * (TEST_LOAD_OHMS.0 as u128) * 1_000_000_000;
    let root = square.isqrt();
    if root > i32::MAX as u128 {
        Microvolts(i32::MAX)
    } else {
        Microvolts(root as i32)
    }
}

/// The input pin, `IN` (pin 1), from its DC level, in microvolts.
///
/// [`Inputs::default`] is no signal on the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Inputs {
    /// `IN`, pin 1.
    pub in_signal_uv: Microvolts,
}

impl Inputs {
    /// No signal at `IN`.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            in_signal_uv: Microvolts(0),
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// The two output pins, `OUT-M` (pin 10) and `OUT-I` (pin 8), from their
/// DC levels, in microvolts (`MB3731-003`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Outputs {
    /// `OUT-M`, pin 10, which follows `IN`.
    pub out_m_signal_uv: Microvolts,
    /// `OUT-I`, pin 8, `OUT-M` inverted.
    pub out_i_signal_uv: Microvolts,
}

/// Why [`Mb3731::new`] refused a caller's figures.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum Mb3731Error {
    /// The gain is zero, so no input would reach the outputs.
    ZeroGain,
    /// The gain is below zero, which would turn `OUT-M` against `IN`.
    NegativeGain,
    /// The mute leaves a fraction below zero, which would turn the muted
    /// output against the open one.
    NegativeMute,
    /// The mute leaves a fraction above one, which would raise the output
    /// that pin 6 tied to ground lowers.
    MuteAboveUnity,
}

impl core::fmt::Display for Mb3731Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::ZeroGain => "a gain of zero",
            Self::NegativeGain => "a gain below zero",
            Self::NegativeMute => "a mute below zero",
            Self::MuteAboveUnity => "a mute above unity",
        })
    }
}

impl core::error::Error for Mb3731Error {}

/// A ratio of one, in parts per billion.
const UNITY_PPB: i64 = 1_000_000_000;

/// The part in the sheet's connection, at one gain, muted or not.
///
/// The invariant [`Mb3731::new`] holds is what every part has: the gain is
/// above zero and the mute's fraction is from zero to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mb3731 {
    gain_ppb: PartsPerBillion,
    mute_ppb: Option<PartsPerBillion>,
}

impl Mb3731 {
    /// The part at this voltage gain, as a ratio in parts per billion
    /// (`MB3731-006`), with pin 6 open (`None`) or tied to ground, leaving
    /// this fraction of the output, in parts per billion (`MB3731-013`).
    ///
    /// Muting is a connection, not a level: the sheet says what pin 6 tied
    /// to ground does and nothing of the pin left open or held at a voltage.
    ///
    /// # Errors
    ///
    /// Each [`Mb3731Error`]: a gain of zero or below, or a mute below zero or
    /// above one, checked in that order. No part has one.
    ///
    /// # Examples
    ///
    /// ```
    /// use mb3731::{Mb3731, Mb3731Error};
    /// use units::PartsPerBillion;
    ///
    /// // 200 V/V with pin 6 open.
    /// let part = Mb3731::new(PartsPerBillion(200_000_000_000), None)?;
    /// assert!(!part.is_muted());
    ///
    /// // A mute that leaves more than the whole output is refused.
    /// assert_eq!(
    ///     Mb3731::new(
    ///         PartsPerBillion(200_000_000_000),
    ///         Some(PartsPerBillion(2_000_000_000)),
    ///     ),
    ///     Err(Mb3731Error::MuteAboveUnity)
    /// );
    /// # Ok::<(), Mb3731Error>(())
    /// ```
    pub const fn new(
        gain_ppb: PartsPerBillion,
        mute_ppb: Option<PartsPerBillion>,
    ) -> Result<Self, Mb3731Error> {
        if gain_ppb.0 == 0 {
            return Err(Mb3731Error::ZeroGain);
        }
        if gain_ppb.0 < 0 {
            return Err(Mb3731Error::NegativeGain);
        }
        if let Some(mute) = mute_ppb {
            if mute.0 < 0 {
                return Err(Mb3731Error::NegativeMute);
            }
            if mute.0 > UNITY_PPB {
                return Err(Mb3731Error::MuteAboveUnity);
            }
        }
        Ok(Self { gain_ppb, mute_ppb })
    }

    /// The part at one of the table's gain corners, [`GAIN_MIN_TYP_MAX_PPB`]
    /// (`MB3731-006`), and, `muted`, at the sheet's mute, [`MUTE_TYP_PPB`]
    /// (`MB3731-013`): the sheet prints the mute only as a typical, which
    /// every corner therefore takes. `muted` is pin 6's connection to ground,
    /// a condition and not a level.
    #[must_use]
    pub const fn at_corner(muted: bool, corner: Corner) -> Self {
        let gain_ppb = match corner {
            Corner::Min => GAIN_MIN_TYP_MAX_PPB.0,
            Corner::Typ => GAIN_MIN_TYP_MAX_PPB.1,
            Corner::Max => GAIN_MIN_TYP_MAX_PPB.2,
        };
        let mute_ppb = if muted { Some(MUTE_TYP_PPB) } else { None };
        Self { gain_ppb, mute_ppb }
    }

    /// The voltage gain the part is built at, in parts per billion.
    #[must_use]
    pub const fn gain_ppb(&self) -> PartsPerBillion {
        self.gain_ppb
    }

    /// The fraction of the output pin 6 tied to ground leaves, in parts per
    /// billion; `None` with pin 6 open.
    #[must_use]
    pub const fn mute_ppb(&self) -> Option<PartsPerBillion> {
        self.mute_ppb
    }

    /// Whether pin 6 is tied to ground (`MB3731-013`).
    #[must_use]
    pub const fn is_muted(&self) -> bool {
        self.mute_ppb.is_some()
    }

    /// The voltage across the load, pin 10 less pin 8, in microvolts, with
    /// pin 1 at these levels (`MB3731-006`); muted, that times the fraction
    /// pin 6 leaves (`MB3731-013`). Each product is rounded toward zero, and
    /// the result saturated to the range of an `i32`.
    #[must_use]
    pub const fn bridge_uv(&self, inputs: Inputs) -> Microvolts {
        let mut v = inputs.in_signal_uv.0 as i128 * self.gain_ppb.0 as i128 / 1_000_000_000;
        if let Some(mute) = self.mute_ppb {
            v = v.saturating_mul(mute.0 as i128) / 1_000_000_000;
        }
        if v > i32::MAX as i128 {
            Microvolts(i32::MAX)
        } else if v < i32::MIN as i128 {
            Microvolts(i32::MIN)
        } else {
            Microvolts(v as i32)
        }
    }

    /// Pins 10 and 8 from their DC levels, with pin 1 at these levels:
    /// opposite, their difference exactly [`Self::bridge_uv`], and equal in
    /// size but for an odd microvolt of the bridge, which `OUT-M` takes
    /// (`MB3731-003`).
    #[must_use]
    pub const fn drive(&self, inputs: Inputs) -> Outputs {
        let bridge = self.bridge_uv(inputs).0;
        let half = bridge / 2;
        Outputs {
            out_m_signal_uv: Microvolts(bridge - half),
            out_i_signal_uv: Microvolts(-half),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_takes_the_callers_mute_and_at_corner_the_sheets() {
        let quarter = PartsPerBillion(250_000_000);
        let own = Mb3731::new(PartsPerBillion(1_000_000_000), Some(quarter)).expect("positive");
        let pins = Inputs {
            in_signal_uv: Microvolts(4_000),
        };
        assert_eq!(own.mute_ppb(), Some(quarter));
        assert_eq!(own.bridge_uv(pins), Microvolts(1_000));
        assert_eq!(
            Mb3731::at_corner(true, Corner::Max).mute_ppb(),
            Some(MUTE_TYP_PPB)
        );
        assert_eq!(Mb3731::at_corner(false, Corner::Min).mute_ppb(), None);
    }

    #[test]
    fn the_largest_figures_saturate_instead_of_overflowing() {
        let widest = Mb3731::new(PartsPerBillion(i64::MAX), Some(PartsPerBillion(UNITY_PPB)))
            .expect("positive");
        let pins = |uv| Inputs {
            in_signal_uv: Microvolts(uv),
        };
        assert_eq!(widest.bridge_uv(pins(i32::MAX)), Microvolts(i32::MAX));
        assert_eq!(widest.bridge_uv(pins(i32::MIN)), Microvolts(i32::MIN));
    }

    #[test]
    fn an_odd_bridge_puts_its_odd_microvolt_on_out_m() {
        let unity = Mb3731::new(PartsPerBillion(1_000_000_000), None).expect("positive");
        let out = unity.drive(Inputs {
            in_signal_uv: Microvolts(223),
        });
        assert_eq!(out.out_m_signal_uv, Microvolts(112));
        assert_eq!(out.out_i_signal_uv, Microvolts(-111));
    }

    #[test]
    fn a_gain_or_mute_no_part_has_is_refused() {
        let unity = PartsPerBillion(1_000_000_000);
        assert_eq!(
            Mb3731::new(PartsPerBillion(0), None),
            Err(Mb3731Error::ZeroGain)
        );
        assert_eq!(
            Mb3731::new(PartsPerBillion(-1), None),
            Err(Mb3731Error::NegativeGain)
        );
        assert_eq!(
            Mb3731::new(unity, Some(PartsPerBillion(-1))),
            Err(Mb3731Error::NegativeMute)
        );
        assert_eq!(
            Mb3731::new(unity, Some(PartsPerBillion(1_000_000_001))),
            Err(Mb3731Error::MuteAboveUnity)
        );
        assert!(Mb3731::new(unity, Some(PartsPerBillion(0))).is_ok());
        assert!(Mb3731::new(unity, Some(unity)).is_ok());
    }
}
