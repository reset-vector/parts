//! The Texas Instruments LM324 quadruple operational amplifier, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **An amplifier is a gain and two limits** (`LM324-004`, `LM324-005`), which
//! is the whole of what the sheet states about its transfer: a large-signal
//! differential voltage amplification, a high-level output voltage the output
//! cannot pass and a low-level one it cannot fall below. So [`Amplifier::drive`]
//! multiplies the difference between the two inputs by the gain and holds the
//! product between the limits, and that is the part.
//!
//! **It is a function and not a machine.** No slew rate, no bandwidth and no
//! settling time are printed (`LM324-OPEN-02`), so there is no state here, no
//! `tick` and no time. A loop closed through this part is an equation in the
//! same sample and not a step from the last one: the consumer finds the fixed
//! point, and it can be found in integers by bisection.
//!
//! **An input past the common-mode range is driven as one inside it**
//! (`LM324-OPEN-05`): [`Amplifier::drive`] applies the same gain and limits,
//! and [`is_in_vicr_25c_range`] says which side a caller is on.
//!
//! **Microvolts, and no float anywhere.** The sheet gives its gain in volts
//! per millivolt and its limits in volts and millivolts; the crate carries the
//! gain in volts per volt and the limits in microvolts, and takes the product
//! of any difference and any gain in an `i128` before it is clamped back into
//! the swing.
//!
//! **Built at a corner, at the one supply the limits are printed for.** The
//! sheet gives the gain a minimum and a typical, and the high limit at 30 V
//! only (`LM324-OPEN-03`); [`Amplifier::at_corner`] holds
//! the two corners a caller with no specimen can name, and
//! [`Amplifier::new`] takes a caller's own gain and [`Swing`] for any other
//! supply.
//!
//! # Examples
//!
//! ```
//! use lm324::{Amplifier, Corner, Inputs, Swing};
//! use units::{Microvolts, VoltsPerVolt};
//!
//! // The typical part: 100 V/mV between 5 mV and 28 V.
//! let typical = Amplifier::at_corner(Corner::Typ);
//! let pins = Inputs {
//!     noninverting_uv: Microvolts(100),
//!     inverting_uv: Microvolts(0),
//! };
//! assert_eq!(typical.drive(pins).output_uv, Microvolts(10_000_000));
//!
//! // A caller's own gain and swing, for a part at 5 V: the same 100 µV
//! // drives it to its high limit.
//! let swing = Swing::declared(Microvolts(0), Microvolts(3_500_000)).ok_or("swing")?;
//! let own = Amplifier::new(VoltsPerVolt(50_000), swing).ok_or("gain")?;
//! assert!(own.is_saturated(pins));
//! # Ok::<(), &str>(())
//! ```

#![no_std]

use units::{Microvolts, Ohms, Picoamps, VoltsPerVolt};

/// How many amplifiers the package carries (`LM324-001`).
pub const AMPLIFIER_COUNT: usize = 4;

/// How many pins the package presents (`LM324-002`).
pub const PIN_COUNT: usize = 14;

/// The large-signal differential voltage amplification at 25 °C, minimum and
/// typical, in volts per volt: 25 V/mV and 100 V/mV (`LM324-004`).
pub const AVD_25C_MIN_TYP_V_PER_V: (VoltsPerVolt, VoltsPerVolt) =
    (VoltsPerVolt(25_000), VoltsPerVolt(100_000));

/// The same, minimum over the full temperature range: 15 V/mV
/// (`LM324-004`).
pub const AVD_FULL_RANGE_MIN_V_PER_V: VoltsPerVolt = VoltsPerVolt(15_000);

/// The supply the gain is stated at, in microvolts: 15 V (`LM324-004`).
pub const AVD_TEST_SUPPLY_UV: Microvolts = Microvolts(15_000_000);

/// One `VOH` row: the output high into a load to ground.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct VohRow {
    /// The supply the row is stated at, `Vcc`, in microvolts.
    pub test_supply_uv: Microvolts,
    /// The current the output sources, in picoamps; `None` where the row
    /// states none.
    pub source_pa: Option<Picoamps>,
    /// The load to ground, at least, in ohms; `None` where the row states
    /// none (unloaded).
    pub load_ohms: Option<Ohms>,
    /// The output voltage, at least and typical, in microvolts; `None` where
    /// the column prints no figure.
    pub min_typ_uv: (Option<Microvolts>, Option<Microvolts>),
}

/// The high-level output voltage rows, at `Vcc` 30 V, into 2 kΩ and then
/// 10 kΩ (`LM324-005`).
pub const VOH_ROWS: [VohRow; 2] = [
    VohRow {
        test_supply_uv: Microvolts(30_000_000),
        source_pa: None,
        load_ohms: Some(Ohms(2_000)),
        min_typ_uv: (Some(Microvolts(26_000_000)), None),
    },
    VohRow {
        test_supply_uv: Microvolts(30_000_000),
        source_pa: None,
        load_ohms: Some(Ohms(10_000)),
        min_typ_uv: (Some(Microvolts(27_000_000)), Some(Microvolts(28_000_000))),
    },
];

/// The low-level output voltage, typical and maximum, into 10 kΩ or less, in
/// microvolts: 5 mV and 20 mV (`LM324-005`).
pub const VOL_TYP_MAX_UV: (Microvolts, Microvolts) = (Microvolts(5_000), Microvolts(20_000));

/// The single supply, lowest and highest, in microvolts: 3 V to 30 V
/// (`LM324-003`).
pub const SUPPLY_UV: (Microvolts, Microvolts) = (Microvolts(3_000_000), Microvolts(30_000_000));

/// The absolute maximum supply, in microvolts: 32 V (`LM324-003`).
pub const SUPPLY_ABSOLUTE_MAX_UV: Microvolts = Microvolts(32_000_000);

/// `VICR`, the common-mode input voltage range at 25 °C: how far below `Vcc`
/// its top stops, in microvolts, the range being 0 V to `Vcc − 1.5` V
/// (`LM324-007`).
pub const VICR_25C_HEADROOM_UV: Microvolts = Microvolts(1_500_000);

/// The same over the full temperature range: 2 V (`LM324-007`).
pub const VICR_FULL_RANGE_HEADROOM_UV: Microvolts = Microvolts(2_000_000);

/// `IIB`, the input bias current, typical and maximum at 25 °C, in picoamps,
/// out of the device: 45 nA and 250 nA (`LM324-008`).
pub const IIB_TYP_MAX_PA: (Picoamps, Picoamps) = (Picoamps(45_000), Picoamps(250_000));

/// `IIO`, the input offset current, typical and maximum at 25 °C, in
/// picoamps: 5 nA and 50 nA (`LM324-008`).
pub const IIO_TYP_MAX_PA: (Picoamps, Picoamps) = (Picoamps(5_000), Picoamps(50_000));

/// `VIO`, the input offset voltage, typical and maximum at 25 °C, in
/// microvolts (`LM324-008`).
pub const VIO_TYP_MAX_UV: (Microvolts, Microvolts) = (Microvolts(2_000), Microvolts(7_000));

/// `IO`, the output source current, minimum and typical at 25 °C, in
/// picoamps: 20 mA and 40 mA, with the output at 0 V and the differential
/// input at 1 V (`LM324-009`).
pub const IO_SOURCE_MIN_TYP_PA: (Picoamps, Picoamps) =
    (Picoamps(20_000_000_000), Picoamps(40_000_000_000));

/// One `IO` sink row: the current the output sinks with the output held at a
/// voltage.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct IoSinkRow {
    /// The output's voltage the row is stated at, `VO`, in microvolts.
    pub vo_uv: Microvolts,
    /// The current, minimum and typical, in picoamps.
    pub min_typ_pa: (Picoamps, Picoamps),
}

/// `IO`, the output sink current, minimum and typical at 25 °C: 10 mA and
/// 20 mA with the output at 5 V and the differential input at −1 V, and
/// 12 µA and 50 µA with the output at 200 mV (`LM324-009`).
pub const IO_SINK_ROWS: [IoSinkRow; 2] = [
    IoSinkRow {
        vo_uv: Microvolts(5_000_000),
        min_typ_pa: (Picoamps(10_000_000_000), Picoamps(20_000_000_000)),
    },
    IoSinkRow {
        vo_uv: Microvolts(200_000),
        min_typ_pa: (Picoamps(12_000_000), Picoamps(50_000_000)),
    },
];

/// `ICC`, the supply current for all four amplifiers, typical at 25 °C and
/// maximum over the full range, in picoamps: 800 µA and 1.2 mA (`LM324-010`).
pub const ICC_TYP_MAX_PA: (Picoamps, Picoamps) = (Picoamps(800_000_000), Picoamps(1_200_000_000));

/// One amplifier's two input pins, in microvolts.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// The non-inverting input, `IN+`.
    pub noninverting_uv: Microvolts,
    /// The inverting input, `IN−`.
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
    /// `OUT`.
    pub output_uv: Microvolts,
}

/// The two voltages the output cannot pass, in microvolts (`LM324-005`).
pub use units::Swing;

/// One amplifier: a gain and a swing (`LM324-004`, `LM324-005`).
///
/// The gain is positive, so the output is a monotone function of the input
/// difference.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Amplifier {
    gain_v_per_v: VoltsPerVolt,
    swing: Swing,
}

impl Amplifier {
    /// An amplifier from a caller's own gain, in volts per volt, and swing, or
    /// `None` for a gain of zero.
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

    /// The swing.
    #[must_use]
    pub const fn swing(self) -> Swing {
        self.swing
    }

    /// The output pin with the two inputs held at these voltages
    /// (`LM324-004`, `LM324-005`, `LM324-006`).
    ///
    /// The whole of the part is here. There is no time in it and no state,
    /// because the sheet prints neither.
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

/// Which of the sheet's figures for an amplifier to take
/// ([`Amplifier::at_corner`]). The sheet prints no maximum part.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Corner {
    /// The least the sheet guarantees.
    Min,
    /// The typical part.
    Typ,
}

/// [`AT_MIN`]'s swing.
const SWING_MIN: Swing = match Swing::declared(
    VOL_TYP_MAX_UV.1,
    VOH_ROWS[0]
        .min_typ_uv
        .0
        .expect("the 2 kΩ row prints a minimum"),
) {
    Some(swing) => swing,
    None => panic!("the sheet's low level is below its high"),
};

/// [`AT_TYP`]'s swing.
const SWING_TYP: Swing = match Swing::declared(
    VOL_TYP_MAX_UV.0,
    VOH_ROWS[1]
        .min_typ_uv
        .1
        .expect("the 10 kΩ row prints a typical"),
) {
    Some(swing) => swing,
    None => panic!("the sheet's low level is below its high"),
};

/// The least the sheet guarantees, built when the crate is compiled, so a
/// table cell it reads that held no figure would fail the build.
const AT_MIN: Amplifier = Amplifier {
    gain_v_per_v: AVD_25C_MIN_TYP_V_PER_V.0,
    swing: SWING_MIN,
};

/// The typical part, built as [`AT_MIN`] is.
const AT_TYP: Amplifier = Amplifier {
    gain_v_per_v: AVD_25C_MIN_TYP_V_PER_V.1,
    swing: SWING_TYP,
};

impl Amplifier {
    /// An amplifier at the sheet's figures: the least it guarantees and the
    /// typical part, each at 25 °C and
    /// `Vcc` 30 V and each at its own load: the minimum into 2 kΩ, gain
    /// 25 V/mV, limits 20 mV and 26 V; the typical into 10 kΩ, gain 100 V/mV,
    /// limits 5 mV and 28 V (`LM324-004`, `LM324-005`).
    #[must_use]
    pub const fn at_corner(corner: Corner) -> Self {
        match corner {
            Corner::Min => AT_MIN,
            Corner::Typ => AT_TYP,
        }
    }
}

/// Whether an input held at this voltage above ground is inside the
/// common-mode range the sheet specifies at 25 °C, 0 V to `Vcc − 1.5` V
/// (`LM324-007`).
///
/// The question is answered and not acted on: what the amplifier does past
/// the range is on no page (`LM324-OPEN-05`).
#[must_use]
pub const fn is_in_vicr_25c_range(vcc_uv: Microvolts, input_uv: Microvolts) -> bool {
    input_uv.0 >= 0 && input_uv.0 as i64 <= vcc_uv.0 as i64 - VICR_25C_HEADROOM_UV.0 as i64
}

/// The package: four amplifiers sharing nothing but the two supply pins
/// (`LM324-001`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Lm324 {
    amplifiers: [Amplifier; AMPLIFIER_COUNT],
}

impl Lm324 {
    /// A package of these four amplifiers, the one the sheet numbers 1 first.
    #[must_use]
    pub const fn with_amplifiers(amplifiers: [Amplifier; AMPLIFIER_COUNT]) -> Self {
        Self { amplifiers }
    }

    /// Amplifier `index`, numbered from 0 for the one the sheet numbers 1.
    ///
    /// # Panics
    ///
    /// If `index` is not below [`AMPLIFIER_COUNT`].
    #[must_use]
    pub const fn amplifier(&self, index: usize) -> Amplifier {
        self.amplifiers[index]
    }

    /// The four output pins with each amplifier's two inputs held at these
    /// voltages.
    #[must_use]
    pub const fn drive(&self, inputs: [Inputs; AMPLIFIER_COUNT]) -> [Outputs; AMPLIFIER_COUNT] {
        [
            self.amplifiers[0].drive(inputs[0]),
            self.amplifiers[1].drive(inputs[1]),
            self.amplifiers[2].drive(inputs[2]),
            self.amplifiers[3].drive(inputs[3]),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn uv(v: i32) -> Microvolts {
        Microvolts(v)
    }

    #[test]
    fn a_zero_gain_is_refused_and_the_largest_is_not() {
        let swing = Swing::declared(uv(i32::MIN), uv(i32::MAX)).unwrap();
        assert!(Amplifier::new(VoltsPerVolt(0), swing).is_none());
        let largest = VoltsPerVolt(u32::MAX / 1_000 * 1_000);
        let widest = Amplifier::new(largest, swing).unwrap();
        assert_eq!(widest.gain_v_per_v(), largest);
        assert_eq!(widest.linear_range_uv(), (uv(-1), uv(1)));
    }

    #[test]
    fn the_widest_input_difference_at_the_largest_gain_saturates() {
        let swing = Swing::declared(uv(i32::MIN), uv(i32::MAX)).unwrap();
        let widest = Amplifier::new(VoltsPerVolt(u32::MAX / 1_000 * 1_000), swing).unwrap();
        let pins = |noninverting_uv, inverting_uv| Inputs {
            noninverting_uv: uv(noninverting_uv),
            inverting_uv: uv(inverting_uv),
        };
        assert_eq!(
            widest.drive(pins(i32::MAX, i32::MIN)).output_uv,
            uv(i32::MAX)
        );
        assert_eq!(
            widest.drive(pins(i32::MIN, i32::MAX)).output_uv,
            uv(i32::MIN)
        );
    }

    #[test]
    fn a_swing_of_no_width_is_refused() {
        assert!(Swing::declared(uv(5), uv(5)).is_none());
        assert!(Swing::declared(uv(6), uv(5)).is_none());
        assert!(Swing::declared(uv(4), uv(5)).is_some());
    }
}
