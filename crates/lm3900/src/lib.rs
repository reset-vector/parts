//! The Texas Instruments LM3900 quadruple Norton operational amplifier, at
//! its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The inputs are currents** (`LM3900-005`). The sheet's one sentence of
//! mechanism is that the input currents are differenced at the inverting
//! input and the difference flows through the external feedback resistor to
//! make the output voltage; so [`Amplifier::drive`] takes two currents in
//! picoamps, weights the non-inverting one by the mirror gain (`LM3900-006`),
//! subtracts it and the bias current (`LM3900-008`) from the inverting one,
//! multiplies what is left by a transresistance and takes the product away
//! from the balance point, holding the result between the two printed limits
//! (`LM3900-015`, `LM3900-016`). That is the part.
//!
//! **The transresistance is a reading of two rows and not of one.** The sheet
//! prints a voltage gain, 1.2 to 2.8 V/mV (`LM3900-012`), and an input
//! resistance, 1 MΩ (`LM3900-009`); their product is the volts per picoamp
//! the mechanism needs and is what [`AVD_25C_MIN_TYP_V_PER_V`]'s typical times
//! [`RI_TYP_OHMS`] gives, 2800 µV/pA (2.8 mV/pA) typical.
//!
//! **The zero is the balance and not ground.** The bias current is stated
//! with the output balanced midway between the supply and ground, so it is
//! the operating point: exactly that much into the inverting input, with the
//! mirror off, puts the output at the balance, which is the constructor's and
//! 7.5 V at the 15 V the table is stated at ([`TEST_SUPPLY_UV`]).
//!
//! **It is a function, with a bound beside it.** No state, no clock and no
//! time are in the transfer, and a loop closed through it is an equation in
//! the same sample, which the consumer solves. The sheet does print two slew
//! rates (`LM3900-020`), and [`Slew`] is those two figures for the consumer
//! that steps a network at a sample rate and wants the output's movement per
//! step bounded.
//!
//! **Picoamps and microvolts, and no float anywhere.** A microvolt across a
//! megohm is a picoamp, which is why the two units go together; every
//! product of a caller's currents is taken in `i128` and is clamped before it
//! is returned.
//!
//! **What the crate chooses**, each an open item: the inputs carry no
//! voltage (`LM3900-OPEN-02`); the typical corner's mirror gain is unity
//! (`LM3900-OPEN-03`); and the transfer has no pole, which a consumer that
//! needs one builds from [`B1_TYP_HZ`] (`LM3900-OPEN-05`); the typical
//! corner's high limit is the 15 V minimum, 13.5 V (`LM3900-OPEN-06`); and
//! below 10 µA the mirror reflects at the same gain (`LM3900-OPEN-08`).
//!
//! # Examples
//!
//! ```
//! use lm3900::{Amplifier, Corner, Inputs};
//! use units::{Microvolts, Picoamps};
//!
//! // The typical part, its inverting input fed exactly the bias current:
//! // the output stands at the balance, 7.5 V.
//! let typical = Amplifier::at_corner(Corner::Typ);
//! let pins = Inputs {
//!     noninverting_pa: Picoamps(0),
//!     inverting_pa: typical.bias_pa(),
//! };
//! assert_eq!(typical.drive(pins).output_uv, Microvolts(7_500_000));
//!
//! // A nanoamp more pulls it down by 2.8 mV a picoamp, to 4.7 V.
//! let more = Inputs {
//!     inverting_pa: typical.bias_pa() + Picoamps(1_000),
//!     ..pins
//! };
//! assert_eq!(typical.drive(more).output_uv, Microvolts(4_700_000));
//! ```

#![no_std]

use units::{
    Decibels, Hertz, Microvolts, MicrovoltsPerPicoamp, MicrovoltsPerSecond, Ohms, PartsPerBillion,
    Picoamps, Picoseconds, VoltsPerVolt,
};

/// How many amplifiers the package carries (`LM3900-001`).
pub const AMPLIFIER_COUNT: usize = 4;

/// How many pins the package presents (`LM3900-002`).
pub const PIN_COUNT: usize = 14;

/// The output pin of each amplifier, in the order this crate numbers them,
/// 0 to 3 (`LM3900-002`). The sheet numbers none; the order is the pins'.
pub const OUTPUT_PINS: [u8; AMPLIFIER_COUNT] = [4, 5, 9, 10];

/// The large-signal differential voltage amplification at 25 °C, minimum and
/// typical, in volts per volt: 1.2 V/mV and 2.8 V/mV (`LM3900-012`).
pub const AVD_25C_MIN_TYP_V_PER_V: (VoltsPerVolt, VoltsPerVolt) =
    (VoltsPerVolt(1_200), VoltsPerVolt(2_800));

/// `ri`, the input resistance at the inverting input, typical, in ohms: 1 MΩ
/// (`LM3900-009`).
pub const RI_TYP_OHMS: Ohms = Ohms(1_000_000);

/// The transresistance the two rows above multiply to, minimum and typical,
/// in microvolts per picoamp: the gain's 25 °C minimum and typical, each
/// times the resistance's typical, which has no minimum of its own
/// (`LM3900-012`, `LM3900-009`). A volt per volt across a megohm is a
/// microvolt per picoamp.
pub const TRANSRESISTANCE_MIN_TYP_UV_PER_PA: (MicrovoltsPerPicoamp, MicrovoltsPerPicoamp) = (
    MicrovoltsPerPicoamp(AVD_25C_MIN_TYP_V_PER_V.0.0 * (RI_TYP_OHMS.0 / 1_000_000) as u32),
    MicrovoltsPerPicoamp(AVD_25C_MIN_TYP_V_PER_V.1.0 * (RI_TYP_OHMS.0 / 1_000_000) as u32),
);

/// The supply the electrical table is stated at, in microvolts: 15 V.
pub const TEST_SUPPLY_UV: Microvolts = Microvolts(15_000_000);

/// The output the bias current is stated at, in microvolts: midway between
/// [`TEST_SUPPLY_UV`] and ground, 7.5 V (`LM3900-008`).
pub const BALANCE_UV: Microvolts = Microvolts(TEST_SUPPLY_UV.0 / 2);

/// The mirror gain's printed band, in parts per billion, `(min, max)`: 0.9
/// and 1.1 (`LM3900-006`). The sheet prints no figure between
/// (`LM3900-OPEN-03`).
pub const MIRROR_GAIN_RANGE_PPB: (PartsPerBillion, PartsPerBillion) =
    (PartsPerBillion(900_000_000), PartsPerBillion(1_100_000_000));

/// Unity, in parts per billion: the description's word for what the mirror
/// does, and the middle corner's choice in the absence of a figure
/// (`LM3900-OPEN-03`).
pub const MIRROR_GAIN_UNITY_PPB: PartsPerBillion = PartsPerBillion(1_000_000_000);

/// The non-inverting current over which the mirror gain is specified, in
/// picoamps: 20 µA to 200 µA (`LM3900-006`).
pub const MIRROR_GAIN_TEST_CURRENT_RANGE_PA: (Picoamps, Picoamps) =
    (Picoamps(20_000_000), Picoamps(200_000_000));

/// The mirror current's specified range, in picoamps: 10 µA minimum and
/// 500 µA maximum (`LM3900-007`).
pub const MIRROR_CURRENT_RANGE_PA: (Picoamps, Picoamps) =
    (Picoamps(10_000_000), Picoamps(500_000_000));

/// `IIB`, the inverting input's bias current, typical and maximum at 25 °C,
/// in picoamps: 30 nA and 200 nA (`LM3900-008`).
pub const IIB_TYP_MAX_PA: (Picoamps, Picoamps) = (Picoamps(30_000), Picoamps(200_000));

/// One `VOH` row.
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

/// The high-level output voltage rows (`LM3900-015`): at least 13.5 V at
/// `Vcc` 15 V into 2 kΩ, the only high-level figure printed at 15 V, and
/// 29.5 V typical at `Vcc` 30 V unloaded.
pub const VOH_ROWS: [VohRow; 2] = [
    VohRow {
        test_supply_uv: TEST_SUPPLY_UV,
        source_pa: None,
        load_ohms: Some(Ohms(2_000)),
        min_typ_uv: (Some(Microvolts(13_500_000)), None),
    },
    VohRow {
        test_supply_uv: Microvolts(30_000_000),
        source_pa: None,
        load_ohms: None,
        min_typ_uv: (None, Some(Microvolts(29_500_000))),
    },
];

/// The low-level output voltage, typical and maximum, at 10 µA into the
/// inverting input into 2 kΩ, in microvolts: 90 mV and 200 mV (`LM3900-016`).
pub const VOL_TYP_MAX_UV: (Microvolts, Microvolts) = (Microvolts(90_000), Microvolts(200_000));

/// The absolute maximum supply, in microvolts: 32 V (`LM3900-003`).
pub const SUPPLY_ABSOLUTE_MAX_UV: Microvolts = Microvolts(32_000_000);

/// The recommended input current, in picoamps, `(min, max)`: −1 mA and up
/// (`LM3900-010`). The table gives the range no positive bound, so the
/// absolute maximum, 20 mA, is taken as one (`LM3900-011`).
pub const INPUT_CURRENT_RECOMMENDED_RANGE_PA: (Picoamps, Picoamps) =
    (Picoamps(-1_000_000_000), Picoamps(20_000_000_000));

/// The negative input current beyond which the output drops low, in
/// picoamps: −4 mA (`LM3900-010`).
pub const INPUT_CURRENT_DROPS_OUTPUT_PA: Picoamps = Picoamps(-4_000_000_000);

/// The input current's absolute maximum, in picoamps: 20 mA (`LM3900-011`).
pub const INPUT_CURRENT_ABSOLUTE_MAX_PA: Picoamps = Picoamps(20_000_000_000);

/// The voltage the inputs are clamped at below ground, approximately, in
/// microvolts: −0.3 V (`LM3900-010`).
pub const INPUT_CLAMP_UV: Microvolts = Microvolts(-300_000);

/// `ro`, the output resistance, typical, in ohms (`LM3900-017`).
pub const RO_TYP_OHMS: Ohms = Ohms(8_000);

/// `kSVR`, the supply-voltage rejection ratio, typical, in decibels
/// (`LM3900-018`).
pub const KSVR_TYP_DB: Decibels = Decibels(70);

/// `B1`, the unity-gain bandwidth at the inverting input, typical, in hertz
/// (`LM3900-019`). The crate has no pole; `LM3900-OPEN-05`.
pub const B1_TYP_HZ: Hertz = Hertz(2_500_000);

/// The slew rate at unity gain, low to high, typical, in microvolts per
/// second: 0.5 V/µs (`LM3900-020`).
pub const SLEW_RISE_TYP_UV_PER_S: MicrovoltsPerSecond = MicrovoltsPerSecond(500_000_000_000);

/// The same, high to low: 20 V/µs (`LM3900-020`).
pub const SLEW_FALL_TYP_UV_PER_S: MicrovoltsPerSecond = MicrovoltsPerSecond(20_000_000_000_000);

/// `IOHS`, the short-circuit output current with the output internally high,
/// minimum and typical, in picoamps out of the pin: 6 mA and 10 mA
/// (`LM3900-021`).
pub const IOHS_MIN_TYP_PA: (Picoamps, Picoamps) =
    (Picoamps(6_000_000_000), Picoamps(10_000_000_000));

/// The pull-down current, minimum and typical, in picoamps: 500 µA and
/// 1.3 mA (`LM3900-022`).
pub const PULL_DOWN_CURRENT_MIN_TYP_PA: (Picoamps, Picoamps) =
    (Picoamps(500_000_000), Picoamps(1_300_000_000));

/// `IOL`, the low-level output current, typical, in picoamps: 5 mA, at 5 µA
/// into the inverting input and the output at 1 V (`LM3900-023`).
pub const IOL_TYP_PA: Picoamps = Picoamps(5_000_000_000);

/// `ICC`, the supply current for all four amplifiers, typical and maximum,
/// in picoamps: 6.2 mA and 10 mA (`LM3900-025`).
pub const ICC_TYP_MAX_PA: (Picoamps, Picoamps) =
    (Picoamps(6_200_000_000), Picoamps(10_000_000_000));

/// One amplifier's two input pins, as the currents fed into them, in
/// picoamps. The voltage the inputs sit at is no term here
/// (`LM3900-OPEN-02`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// Into the non-inverting input, `IN+`.
    pub noninverting_pa: Picoamps,
    /// Into the inverting input, `IN−`.
    pub inverting_pa: Picoamps,
}

impl Inputs {
    /// No current into either input.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            noninverting_pa: Picoamps(0),
            inverting_pa: Picoamps(0),
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

/// Picoseconds in a second.
const PS_PER_S: u128 = 1_000_000_000_000;

/// The two voltages the output cannot pass, in microvolts (`LM3900-015`,
/// `LM3900-016`).
pub use units::Swing;

/// Why [`Amplifier::new`] refused a caller's figures.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum AmplifierError {
    /// The transresistance is zero, so no current would move the output.
    ZeroTransresistance,
    /// The mirror gain is zero, so the non-inverting input would do nothing.
    ZeroMirrorGain,
    /// The mirror gain is below zero, which would turn the non-inverting
    /// input into a second inverting one.
    NegativeMirrorGain,
    /// The balance is not strictly inside the swing.
    BalanceOutsideSwing,
}

impl core::fmt::Display for AmplifierError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::ZeroTransresistance => "a transresistance of zero",
            Self::ZeroMirrorGain => "a mirror gain of zero",
            Self::NegativeMirrorGain => "a mirror gain below zero",
            Self::BalanceOutsideSwing => "a balance not strictly inside the swing",
        })
    }
}

impl core::error::Error for AmplifierError {}

/// One amplifier: a mirror gain, a bias, a balance, a transresistance and a
/// swing (`LM3900-005`, `LM3900-006`, `LM3900-008`, `LM3900-012`,
/// `LM3900-015`, `LM3900-016`).
///
/// The invariant [`Amplifier::new`] holds is the one the model cannot do
/// without: the balance is inside the swing, and the transresistance and the
/// mirror gain are positive, so the output is a monotone function of the
/// current difference.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Amplifier {
    transresistance_uv_per_pa: MicrovoltsPerPicoamp,
    mirror_gain_ppb: PartsPerBillion,
    bias_pa: Picoamps,
    balance_uv: Microvolts,
    swing: Swing,
}

impl Amplifier {
    /// An amplifier from a caller's own figures.
    ///
    /// # Errors
    ///
    /// Each [`AmplifierError`]: a transresistance of zero, a mirror gain of
    /// zero or below, or a balance not strictly inside the swing, checked in
    /// that order.
    ///
    /// # Examples
    ///
    /// ```
    /// use lm3900::{Amplifier, AmplifierError, Swing};
    /// use units::{Microvolts, MicrovoltsPerPicoamp, PartsPerBillion, Picoamps};
    ///
    /// let swing = Swing::declared(Microvolts(0), Microvolts(15_000_000))
    ///     .ok_or("a swing with no width")?;
    /// let unity = PartsPerBillion(1_000_000_000);
    /// let amplifier = Amplifier::new(
    ///     MicrovoltsPerPicoamp(2_800),
    ///     unity,
    ///     Picoamps(30_000),
    ///     Microvolts(7_500_000),
    ///     swing,
    /// )?;
    /// assert_eq!(amplifier.bias_pa(), Picoamps(30_000));
    ///
    /// // A balance on a limit of the swing is refused.
    /// assert_eq!(
    ///     Amplifier::new(MicrovoltsPerPicoamp(2_800), unity, Picoamps(30_000), Microvolts(0), swing),
    ///     Err(AmplifierError::BalanceOutsideSwing)
    /// );
    /// # Ok::<(), Box<dyn core::error::Error>>(())
    /// ```
    pub const fn new(
        transresistance_uv_per_pa: MicrovoltsPerPicoamp,
        mirror_gain_ppb: PartsPerBillion,
        bias_pa: Picoamps,
        balance_uv: Microvolts,
        swing: Swing,
    ) -> Result<Self, AmplifierError> {
        if transresistance_uv_per_pa.0 == 0 {
            return Err(AmplifierError::ZeroTransresistance);
        }
        if mirror_gain_ppb.0 == 0 {
            return Err(AmplifierError::ZeroMirrorGain);
        }
        if mirror_gain_ppb.0 < 0 {
            return Err(AmplifierError::NegativeMirrorGain);
        }
        if balance_uv.0 <= swing.low_uv().0 || balance_uv.0 >= swing.high_uv().0 {
            return Err(AmplifierError::BalanceOutsideSwing);
        }
        Ok(Self {
            transresistance_uv_per_pa,
            mirror_gain_ppb,
            bias_pa,
            balance_uv,
            swing,
        })
    }

    /// The transresistance, in microvolts per picoamp.
    #[must_use]
    pub const fn transresistance_uv_per_pa(self) -> MicrovoltsPerPicoamp {
        self.transresistance_uv_per_pa
    }

    /// The mirror gain, in parts per billion.
    #[must_use]
    pub const fn mirror_gain_ppb(self) -> PartsPerBillion {
        self.mirror_gain_ppb
    }

    /// The bias current, in picoamps.
    #[must_use]
    pub const fn bias_pa(self) -> Picoamps {
        self.bias_pa
    }

    /// The output at which the bias current holds, in microvolts.
    #[must_use]
    pub const fn balance_uv(self) -> Microvolts {
        self.balance_uv
    }

    /// The swing.
    #[must_use]
    pub const fn swing(self) -> Swing {
        self.swing
    }

    /// The current the mirror reflects onto the inverting node for this
    /// non-inverting current, in picoamps, rounded towards zero
    /// (`LM3900-006`).
    const fn mirrored_wide_pa(self, noninverting_pa: Picoamps) -> i128 {
        noninverting_pa.0 as i128 * self.mirror_gain_ppb.0 as i128 / 1_000_000_000
    }

    /// The current the amplifier acts on: the inverting current less the
    /// mirrored non-inverting one, in picoamps (`LM3900-005`).
    const fn difference_wide_pa(self, inputs: Inputs) -> i128 {
        inputs.inverting_pa.0 as i128 - self.mirrored_wide_pa(inputs.noninverting_pa)
    }

    /// The output pin with these currents into the non-inverting and the
    /// inverting input (`LM3900-005`, `LM3900-008`, `LM3900-012`,
    /// `LM3900-014`, `LM3900-015`, `LM3900-016`).
    ///
    /// The whole of the part is here. There is no time in it and no state,
    /// because the sheet prints neither for the transfer.
    #[must_use]
    pub const fn drive(self, inputs: Inputs) -> Outputs {
        let excess = self.difference_wide_pa(inputs) - self.bias_pa.0 as i128;
        let product = (self.balance_uv.0 as i128)
            .saturating_sub(excess.saturating_mul(self.transresistance_uv_per_pa.0 as i128));
        let output_uv = if product >= self.swing.high_uv().0 as i128 {
            self.swing.high_uv()
        } else if product <= self.swing.low_uv().0 as i128 {
            self.swing.low_uv()
        } else {
            Microvolts(product as i32)
        };
        Outputs { output_uv }
    }

    /// Whether these currents drive the output to one of its two limits.
    #[must_use]
    pub const fn is_saturated(self, inputs: Inputs) -> bool {
        let output = self.drive(inputs).output_uv.0;
        output == self.swing.high_uv().0 || output == self.swing.low_uv().0
    }

    /// The excess over the bias, in picoamps, at which the output reaches
    /// each limit: the high one first, which is the more negative, then the
    /// low. The linear region is between them.
    #[must_use]
    pub const fn linear_range_pa(self) -> (Picoamps, Picoamps) {
        let rt = self.transresistance_uv_per_pa.0 as u64;
        let to_high = (self.swing.high_uv().0 as i64 - self.balance_uv.0 as i64) as u64;
        let to_low = (self.balance_uv.0 as i64 - self.swing.low_uv().0 as i64) as u64;
        (
            Picoamps(-(to_high.div_ceil(rt) as i64)),
            Picoamps(to_low.div_ceil(rt) as i64),
        )
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

/// The high limit both corners take, read when the crate is compiled, so a
/// table cell that held no figure would fail the build.
const VOH_15V_MIN_UV: Microvolts = VOH_ROWS[0]
    .min_typ_uv
    .0
    .expect("the 15 V row prints a minimum");

impl Amplifier {
    /// An amplifier at the sheet's figures: the least it guarantees at 25 °C
    /// and the typical part, both at
    /// [`TEST_SUPPLY_UV`] and balanced at 7.5 V: the minimum 1200 µV/pA, a mirror
    /// at 0.9, 200 nA of bias, limits 200 mV and 13.5 V; the typical 2800 µV/pA, a
    /// unity mirror, 30 nA of bias, limits 90 mV and 13.5 V — the last the only
    /// high-level figure printed for this part at that supply (`LM3900-OPEN-06`).
    #[must_use]
    pub const fn at_corner(corner: Corner) -> Self {
        match corner {
            Corner::Min => Self {
                transresistance_uv_per_pa: TRANSRESISTANCE_MIN_TYP_UV_PER_PA.0,
                mirror_gain_ppb: MIRROR_GAIN_RANGE_PPB.0,
                bias_pa: IIB_TYP_MAX_PA.1,
                balance_uv: BALANCE_UV,
                swing: SWING_MIN,
            },
            Corner::Typ => Self {
                transresistance_uv_per_pa: TRANSRESISTANCE_MIN_TYP_UV_PER_PA.1,
                mirror_gain_ppb: MIRROR_GAIN_UNITY_PPB,
                bias_pa: IIB_TYP_MAX_PA.0,
                balance_uv: BALANCE_UV,
                swing: SWING_TYP,
            },
        }
    }
}

/// The minimum part's swing, built when the crate is compiled.
const SWING_MIN: Swing = match Swing::declared(VOL_TYP_MAX_UV.1, VOH_15V_MIN_UV) {
    Some(swing) => swing,
    None => panic!("the sheet's low level is below its high"),
};

/// The typical part's swing, built as [`SWING_MIN`] is.
const SWING_TYP: Swing = match Swing::declared(VOL_TYP_MAX_UV.0, VOH_15V_MIN_UV) {
    Some(swing) => swing,
    None => panic!("the sheet's low level is below its high"),
};

/// Whether a non-inverting current is inside the range the mirror is
/// specified over, 10 µA to 500 µA (`LM3900-007`).
///
/// The question is answered and not acted on: what the mirror does below
/// the range is on no page (`LM3900-OPEN-08`).
#[must_use]
pub const fn is_in_mirror_current_range(noninverting_pa: Picoamps) -> bool {
    noninverting_pa.0 >= MIRROR_CURRENT_RANGE_PA.0.0
        && noninverting_pa.0 <= MIRROR_CURRENT_RANGE_PA.1.0
}

/// Whether an input current is inside
/// [`INPUT_CURRENT_RECOMMENDED_RANGE_PA`] (`LM3900-010`, `LM3900-011`).
#[must_use]
pub const fn is_in_input_current_recommended_range(input_pa: Picoamps) -> bool {
    input_pa.0 >= INPUT_CURRENT_RECOMMENDED_RANGE_PA.0.0
        && input_pa.0 <= INPUT_CURRENT_RECOMMENDED_RANGE_PA.1.0
}

/// Whether a negative input current is past the point the sheet says the
/// output drops low, [`INPUT_CURRENT_DROPS_OUTPUT_PA`] (`LM3900-010`).
/// Answered, not acted on.
#[must_use]
pub const fn is_dropping_output(input_pa: Picoamps) -> bool {
    input_pa.0 < INPUT_CURRENT_DROPS_OUTPUT_PA.0
}

/// The two slew rates (`LM3900-020`): how far the output may move in a step
/// of a stated length, rising and falling.
///
/// Nothing in [`Amplifier`] consults this. It is for the consumer that steps
/// a network at a sample rate and wants each step's movement bounded by what
/// the sheet says the output can do; [`Slew::limit`] is that bound applied
/// to a target the amplifier returned.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Slew {
    rise_uv_per_s: MicrovoltsPerSecond,
    fall_uv_per_s: MicrovoltsPerSecond,
}

impl Slew {
    /// The sheet's two rates at `corner`: typically 0.5 V/µs up and 20 V/µs
    /// down (`LM3900-020`). `None` at [`Corner::Min`], where the sheet prints
    /// none.
    #[must_use]
    pub const fn at_corner(corner: Corner) -> Option<Self> {
        match corner {
            Corner::Min => None,
            Corner::Typ => Some(Self {
                rise_uv_per_s: SLEW_RISE_TYP_UV_PER_S,
                fall_uv_per_s: SLEW_FALL_TYP_UV_PER_S,
            }),
        }
    }

    /// A caller's own two rates, in microvolts per second, or `None` for a
    /// zero.
    #[must_use]
    pub const fn new(
        rise_uv_per_s: MicrovoltsPerSecond,
        fall_uv_per_s: MicrovoltsPerSecond,
    ) -> Option<Self> {
        if rise_uv_per_s.0 == 0 || fall_uv_per_s.0 == 0 {
            return None;
        }
        Some(Self {
            rise_uv_per_s,
            fall_uv_per_s,
        })
    }

    /// The rising rate, in microvolts per second.
    #[must_use]
    pub const fn rise_uv_per_s(self) -> MicrovoltsPerSecond {
        self.rise_uv_per_s
    }

    /// The falling rate, in microvolts per second.
    #[must_use]
    pub const fn fall_uv_per_s(self) -> MicrovoltsPerSecond {
        self.fall_uv_per_s
    }

    /// How far a rate moves the output in `step_ps`, in microvolts, rounded
    /// down. Two `u64`s multiply inside a `u128`.
    const fn moved_uv(uv_per_s: MicrovoltsPerSecond, step_ps: Picoseconds) -> i128 {
        (uv_per_s.0 as u128 * step_ps.0 as u128 / PS_PER_S) as i128
    }

    /// The furthest the output can have got towards `target_uv` from
    /// `previous_uv` in `step_ps`, in microvolts. A movement that is not a
    /// whole microvolt is rounded towards `previous_uv`.
    #[must_use]
    pub const fn limit(
        self,
        previous_uv: Microvolts,
        target_uv: Microvolts,
        step_ps: Picoseconds,
    ) -> Microvolts {
        let previous = previous_uv.0 as i128;
        let target = target_uv.0 as i128;
        if target > previous {
            let reach = previous + Self::moved_uv(self.rise_uv_per_s, step_ps);
            if reach < target {
                Microvolts(reach as i32)
            } else {
                target_uv
            }
        } else {
            let reach = previous - Self::moved_uv(self.fall_uv_per_s, step_ps);
            if reach > target {
                Microvolts(reach as i32)
            } else {
                target_uv
            }
        }
    }
}

/// The package: four amplifiers sharing nothing but the two supply pins
/// (`LM3900-001`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Lm3900 {
    amplifiers: [Amplifier; AMPLIFIER_COUNT],
}

impl Lm3900 {
    /// A package of these four amplifiers, in the order of [`OUTPUT_PINS`].
    #[must_use]
    pub const fn with_amplifiers(amplifiers: [Amplifier; AMPLIFIER_COUNT]) -> Self {
        Self { amplifiers }
    }

    /// Amplifier `index`, 0 to 3 in the order of [`OUTPUT_PINS`].
    ///
    /// # Panics
    ///
    /// If `index` is not below [`AMPLIFIER_COUNT`].
    #[must_use]
    pub const fn amplifier(&self, index: usize) -> Amplifier {
        self.amplifiers[index]
    }

    /// The four output pins with each amplifier's two inputs fed these
    /// currents.
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

    fn widest() -> Amplifier {
        let swing = Swing::declared(uv(i32::MIN), uv(i32::MAX)).unwrap();
        Amplifier::new(
            MicrovoltsPerPicoamp(u32::MAX),
            PartsPerBillion(i64::MAX),
            Picoamps(i64::MIN),
            uv(0),
            swing,
        )
        .unwrap()
    }

    #[test]
    fn extreme_currents_saturate_instead_of_overflowing() {
        let t = widest();
        let pins = |noninverting_pa, inverting_pa| Inputs {
            noninverting_pa: Picoamps(noninverting_pa),
            inverting_pa: Picoamps(inverting_pa),
        };
        assert_eq!(t.drive(pins(i64::MIN, i64::MAX)).output_uv, uv(i32::MIN));
        assert_eq!(t.drive(pins(i64::MAX, i64::MIN)).output_uv, uv(i32::MAX));
    }

    #[test]
    fn the_linear_region_of_the_widest_limits_does_not_overflow() {
        let swing = Swing::declared(uv(i32::MIN), uv(i32::MAX)).unwrap();
        let t = Amplifier::new(
            MicrovoltsPerPicoamp(1),
            PartsPerBillion(1_000),
            Picoamps(0),
            uv(-1),
            swing,
        )
        .unwrap();
        assert_eq!(
            t.linear_range_pa(),
            (Picoamps(-(1_i64 << 31)), Picoamps((1_i64 << 31) - 1))
        );
    }

    #[test]
    fn each_refusal_is_named() {
        let swing = Swing::declared(uv(0), uv(2)).unwrap();
        let new = |rt, ppb, balance| {
            Amplifier::new(
                MicrovoltsPerPicoamp(rt),
                PartsPerBillion(ppb),
                Picoamps(0),
                uv(balance),
                swing,
            )
        };
        assert_eq!(new(0, 1, 1), Err(AmplifierError::ZeroTransresistance));
        assert_eq!(new(1, 0, 1), Err(AmplifierError::ZeroMirrorGain));
        assert_eq!(new(1, -1, 1), Err(AmplifierError::NegativeMirrorGain));
        assert_eq!(new(1, 1, 0), Err(AmplifierError::BalanceOutsideSwing));
        assert_eq!(new(1, 1, 2), Err(AmplifierError::BalanceOutsideSwing));
        assert!(new(1, 1, 1).is_ok());
    }

    #[test]
    fn a_swing_of_no_width_is_refused() {
        assert!(Swing::declared(uv(5), uv(5)).is_none());
        assert!(Swing::declared(uv(4), uv(5)).is_some());
    }
}
