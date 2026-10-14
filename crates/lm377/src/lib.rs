//! The National Semiconductor LM377 dual 2 W audio amplifier, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **Each half is an amplifier with its `INPUT` pin at the non-inverting
//! input and its `FEEDBACK` pin at the inverting one** (`LM377-003`), with an
//! open-loop gain the page gives as one typical figure, 90 dB (`LM377-004`).
//! So [`Amplifier::drive`] is the same shape as an `lm324` amplifier's: the
//! difference between the two pins times the gain, held between two limits,
//! with the same [`Amplifier::is_saturated`] and [`Amplifier::linear_range_uv`]
//! beside it.
//!
//! **The limits are the caller's.** No output swing is stated for this part
//! (`LM377-OPEN-01`). [`Swing`] therefore has no constructor that names a
//! figure: a consumer declares the swing it is building to. The successor
//! part's figure, `LM377-SIB-01`, is one a consumer may declare in its place.
//!
//! **It is a function and not a machine**, for the reason the `lm324` crate
//! gives: nothing on the page says how fast the output moves
//! (`LM377-OPEN-03`), so there is no state, no `tick` and no time. A loop
//! closed through it is an equation in the same sample.
//!
//! **Microvolts, and no float anywhere.** 90 dB is a voltage ratio of
//! 10^4.5, which is 31 622.78 and is carried here as 31 623; the product of
//! any difference and any gain is taken in `i128` before it is clamped.
//!
//! # Examples
//!
//! ```
//! use lm377::{AVO_TYP_V_PER_V, Amplifier, Inputs, Swing};
//! use units::Microvolts;
//!
//! // A half at the page's typical gain, held to a swing the caller declares.
//! let swing = Swing::declared(Microvolts(1_000_000), Microvolts(17_000_000)).ok_or("swing")?;
//! let half = Amplifier::new(AVO_TYP_V_PER_V, swing).ok_or("gain")?;
//! let pins = Inputs {
//!     noninverting_uv: Microvolts(9_000_100),
//!     inverting_uv: Microvolts(9_000_000),
//! };
//! assert_eq!(half.drive(pins).output_uv, Microvolts(3_162_300));
//! assert!(!half.is_saturated(pins));
//! # Ok::<(), &str>(())
//! ```

#![no_std]

use units::{Decibels, Microvolts, Milliwatts, Ohms, Picofarads, VoltsPerVolt};

/// How many amplifiers the package carries (`LM377-001`).
pub const AMPLIFIER_COUNT: usize = 2;

/// How many pins the package presents (`LM377-002`).
pub const PIN_COUNT: usize = 14;

/// The open-loop voltage gain, typical, in decibels (`LM377-004`).
pub const AVO_TYP_DB: Decibels = Decibels(90);

/// The same as a voltage ratio: 10^(90/20), rounded to the nearest whole
/// (`LM377-004`).
pub const AVO_TYP_V_PER_V: VoltsPerVolt = VoltsPerVolt(31_623);

/// The input impedance, in ohms: 3 MΩ (`LM377-005`).
pub const INPUT_IMPEDANCE_OHMS: Ohms = Ohms(3_000_000);

/// The supply, lowest and highest, in microvolts: 10 V to 26 V
/// (`LM377-006`).
pub const SUPPLY_UV: (Microvolts, Microvolts) = (Microvolts(10_000_000), Microvolts(26_000_000));

/// One load the rated output power is stated into, and the power there.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct RatedPowerRow {
    /// The load, in ohms.
    pub load_ohms: Ohms,
    /// The output power per half, in milliwatts.
    pub mw: Milliwatts,
}

/// The rated output power per half, 2 W into either load (`LM377-007`).
pub const RATED_POWER_ROWS: [RatedPowerRow; 2] = [
    RatedPowerRow {
        load_ohms: Ohms(8),
        mw: Milliwatts(2_000),
    },
    RatedPowerRow {
        load_ohms: Ohms(16),
        mw: Milliwatts(2_000),
    },
];

/// The maker's own circuit: the feedback resistor from output to feedback
/// pin, in ohms (`LM377-010`).
pub const APPLICATION_FEEDBACK_OHMS: Ohms = Ohms(100_000);

/// The maker's own circuit: the resistor from the feedback pin to ground, in
/// ohms (`LM377-010`).
pub const APPLICATION_FEEDBACK_TO_GROUND_OHMS: Ohms = Ohms(2_000);

/// The maker's own circuit: the resistor from each input to the `BIAS` pin,
/// in ohms (`LM377-008`).
pub const APPLICATION_BIAS_OHMS: Ohms = Ohms(100_000);

/// The maker's own circuit: the capacitor decoupling the `BIAS` pin, in
/// picofarads: 250 µF (`LM377-008`).
pub const APPLICATION_BIAS_DECOUPLING_PF: Picofarads = Picofarads(250_000_000);

/// The gain the maker's circuit sets above its corner, in volts per volt: one
/// plus the feedback ratio, which is the reader's arithmetic and no figure on
/// the page (`LM377-010`).
#[must_use]
pub const fn application_gain_v_per_v() -> VoltsPerVolt {
    VoltsPerVolt(1 + (APPLICATION_FEEDBACK_OHMS.0 / APPLICATION_FEEDBACK_TO_GROUND_OHMS.0) as u32)
}

/// The two voltages the output cannot pass, in microvolts, the caller's
/// (`LM377-OPEN-01`).
///
/// There is deliberately no constructor here that names a figure. A caller
/// declares both limits.
pub use units::Swing;

/// One amplifier's two input pins, in microvolts.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `INPUT`, the non-inverting input (`LM377-003`).
    pub noninverting_uv: Microvolts,
    /// `FEEDBACK`, the inverting input (`LM377-003`).
    pub inverting_uv: Microvolts,
}

impl Inputs {
    /// Both inputs at ground.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            noninverting_uv: Microvolts(0),
            inverting_uv: Microvolts(0),
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// One amplifier's output pin, in microvolts.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub struct Outputs {
    /// `OUTPUT`.
    pub output_uv: Microvolts,
}

/// One amplifier, one half of the package (`LM377-001`): a gain
/// and a declared swing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Amplifier {
    gain_v_per_v: VoltsPerVolt,
    swing: Swing,
}

impl Amplifier {
    /// A half at this gain, the page's typical [`AVO_TYP_V_PER_V`] or a
    /// specimen's, with the caller's swing, or `None` for a gain of zero.
    #[must_use]
    pub const fn new(gain_v_per_v: VoltsPerVolt, swing: Swing) -> Option<Self> {
        if gain_v_per_v.0 == 0 {
            return None;
        }
        Some(Self {
            gain_v_per_v,
            swing,
        })
    }

    /// The gain, in volts per volt.
    #[must_use]
    pub const fn gain_v_per_v(self) -> VoltsPerVolt {
        self.gain_v_per_v
    }

    /// The declared swing.
    #[must_use]
    pub const fn swing(self) -> Swing {
        self.swing
    }

    /// The output pin with `INPUT` and `FEEDBACK` held at these voltages
    /// (`LM377-003`, `LM377-004`).
    ///
    /// The whole of what the page supports is here: the difference times the
    /// gain, held to the swing the caller declared.
    #[must_use]
    pub const fn drive(self, inputs: Inputs) -> Outputs {
        let difference = inputs.noninverting_uv.0 as i128 - inputs.inverting_uv.0 as i128;
        let product = difference * self.gain_v_per_v.0 as i128;
        let output_uv = if product >= self.swing.high_uv().0 as i128 {
            self.swing.high_uv()
        } else if product <= self.swing.low_uv().0 as i128 {
            self.swing.low_uv()
        } else {
            Microvolts(product as i32)
        };
        Outputs { output_uv }
    }

    /// Whether these inputs drive the output to one of its two limits.
    #[must_use]
    pub const fn is_saturated(self, inputs: Inputs) -> bool {
        let output = self.drive(inputs).output_uv.0;
        output == self.swing.high_uv().0 || output == self.swing.low_uv().0
    }

    /// The input differences, in microvolts, at which the output reaches each
    /// limit, `(min, max)`: the largest difference that drives it to the low
    /// limit, then the least that drives it to the high one. The linear region
    /// is between them.
    #[must_use]
    pub const fn linear_range_uv(self) -> (Microvolts, Microvolts) {
        let gain = self.gain_v_per_v.0 as i64;
        let low = (self.swing.low_uv().0 as i64).div_euclid(gain);
        let high = -(-(self.swing.high_uv().0 as i64)).div_euclid(gain);
        (Microvolts(low as i32), Microvolts(high as i32))
    }
}

/// The package: two halves sharing the supply, the grounds and the `BIAS` pin
/// (`LM377-001`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Lm377 {
    amplifiers: [Amplifier; AMPLIFIER_COUNT],
}

impl Lm377 {
    /// A package of these two amplifiers, the one the page numbers 1 first.
    #[must_use]
    pub const fn with_amplifiers(amplifiers: [Amplifier; AMPLIFIER_COUNT]) -> Self {
        Self { amplifiers }
    }

    /// Amplifier `index`, numbered from 0 for the one the page numbers 1.
    ///
    /// # Panics
    ///
    /// If `index` is not below [`AMPLIFIER_COUNT`].
    #[must_use]
    pub const fn amplifier(&self, index: usize) -> Amplifier {
        self.amplifiers[index]
    }

    /// The two output pins with each amplifier's pins held at these voltages.
    #[must_use]
    pub const fn drive(&self, inputs: [Inputs; AMPLIFIER_COUNT]) -> [Outputs; AMPLIFIER_COUNT] {
        [
            self.amplifiers[0].drive(inputs[0]),
            self.amplifiers[1].drive(inputs[1]),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn uv(v: i32) -> Microvolts {
        Microvolts(v)
    }

    const fn pins(noninverting_uv: i32, inverting_uv: i32) -> Inputs {
        Inputs {
            noninverting_uv: uv(noninverting_uv),
            inverting_uv: uv(inverting_uv),
        }
    }

    #[test]
    fn the_widest_input_difference_at_the_largest_gain_saturates() {
        let swing = Swing::declared(uv(i32::MIN), uv(i32::MAX)).unwrap();
        let half = Amplifier::new(VoltsPerVolt(u32::MAX), swing).unwrap();
        assert_eq!(half.drive(pins(i32::MAX, i32::MIN)).output_uv, uv(i32::MAX));
        assert_eq!(half.drive(pins(i32::MIN, i32::MAX)).output_uv, uv(i32::MIN));
        assert!(half.is_saturated(pins(1, 0)));
        assert_eq!(half.linear_range_uv(), (uv(-1), uv(1)));
    }

    #[test]
    fn the_linear_region_is_where_the_output_leaves_its_limits() {
        let swing = Swing::declared(uv(-1_000_000), uv(2_000_000)).unwrap();
        let half = Amplifier::new(AVO_TYP_V_PER_V, swing).unwrap();
        let (low, high) = half.linear_range_uv();
        assert!(half.is_saturated(pins(low.0, 0)));
        assert!(!half.is_saturated(pins(low.0 + 1, 0)));
        assert!(!half.is_saturated(pins(high.0 - 1, 0)));
        assert!(half.is_saturated(pins(high.0, 0)));
    }

    #[test]
    fn a_swing_of_no_width_is_refused() {
        assert!(Swing::declared(uv(5), uv(5)).is_none());
        assert!(Swing::declared(uv(4), uv(5)).is_some());
    }
}
