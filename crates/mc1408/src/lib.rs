//! The Motorola MC1408 eight-bit multiplying D-to-A converter, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The transfer is the sheet's equation, ideal.** Figure 3 gives the current
//! into pin 4 as `K (A1/2 + A2/4 + … + A8/256)` with `K ≅ Vref/R14`
//! (`MC1408-004`). Read with `A1` as the most significant input
//! (`MC1408-003`), that is the reference current times the input byte over
//! 256. [`output_pa`] computes it and rounds to the nearest nanoampere, half
//! up; the rounding is the crate's, not the sheet's. How far a specimen may
//! stray from it is [`Grade::relative_accuracy_ppm`] and the full-scale limits
//! (`MC1408-009`, `MC1408-010`), which the crate states and does not apply.
//!
//! **What the crate chooses**, each an open item: an input between 0.8 V and
//! 2.0 V is [`Logic::Undetermined`] (`MC1408-OPEN-04`); the ladder's current
//! is the reference current (`MC1408-OPEN-05`); and the equation holds at any
//! reference current it is given (`MC1408-OPEN-06`).
//!
//! **Units.** Currents are picoamperes, voltages microvolts, resistances ohms,
//! times picoseconds, all integers. The output is a **sink** current: a
//! magnitude flowing into pin 4 (`MC1408-006`).
//!
//! **No state.** Nothing stands between an input and its current switch
//! (`MC1408-012`), so the whole part is [`drive`]: a function from the
//! levels on its inputs to the current pin 4 sinks. It switches in zero
//! time, and the sheet's times are constants for a consumer to weigh
//! (`MC1408-013` to `MC1408-015`).
//!
//! # Examples
//!
//! ```
//! use mc1408::{Inputs, drive, reference_pa};
//! use units::{Microvolts, Ohms, Picoamps};
//!
//! // 2 V across 1 kΩ is a 2 mA reference; code 0x80 sinks half of it.
//! let reference_pa = reference_pa(Microvolts(2_000_000), Ohms(1_000)).ok_or("no resistance")?;
//! let outputs = drive(Inputs { a: 0x80, reference_pa });
//! assert_eq!(outputs.output_pa, Picoamps(1_000_000_000));
//! # Ok::<(), &str>(())
//! ```

#![no_std]

use units::{
    Celsius, Microvolts, Ohms, PartsPerMillion, PartsPerMillionPerCelsius, PerMille, Picoamps,
    PicoampsPerPicosecond, Picofarads, Picoseconds,
};

/// How many pins the package presents (`MC1408-001`).
pub const PIN_COUNT: usize = 16;

/// The output range control (`MC1408-002`, `MC1408-017`).
pub const RANGE_PIN: u8 = 1;
/// Ground (`MC1408-002`).
pub const GND_PIN: u8 = 2;
/// The negative supply (`MC1408-002`).
pub const VEE_PIN: u8 = 3;
/// The output, `IO`, which sinks (`MC1408-002`, `MC1408-006`).
pub const OUTPUT_PIN: u8 = 4;
/// The digital inputs `A1` (most significant) to `A8`, in that order
/// (`MC1408-002`, `MC1408-003`).
pub const A_PINS: [u8; 8] = [5, 6, 7, 8, 9, 10, 11, 12];
/// The positive supply (`MC1408-002`).
pub const VCC_PIN: u8 = 13;
/// The reference amplifier's positive input, into which `I14` flows
/// (`MC1408-002`, `MC1408-008`).
pub const VREF_POS_PIN: u8 = 14;
/// The reference amplifier's negative input (`MC1408-002`).
pub const VREF_NEG_PIN: u8 = 15;
/// Compensation (`MC1408-002`, `MC1408-022`).
pub const COMPENSATION_PIN: u8 = 16;

/// The number of codes; the equation's denominator (`MC1408-004`).
pub const CODE_COUNT: usize = 256;

/// The current into pin 4 for an input byte (`A1` its bit 7) and a
/// reference current into pin 14 (`MC1408-003`,
/// `MC1408-004`, `MC1408-005`): `reference_pa × code / 256`, rounded to the
/// nearest nanoampere, half up, so always a whole number of nanoamperes. At
/// code 255 it is 255/256 of the reference (`MC1408-007`); at code 0 it is
/// zero (`MC1408-011`). It applies at any reference current, inside the
/// sheet's range or not (`MC1408-OPEN-06`).
#[must_use]
pub const fn output_pa(code: u8, reference_pa: Picoamps) -> Picoamps {
    let product = reference_pa.0 as i128 * code as i128;
    let per_na = CODE_COUNT as i128 * 1_000;
    Picoamps((product + per_na / 2).div_euclid(per_na) as i64 * 1_000)
}

/// The reference current into pin 14 for a reference voltage
/// across `R14` (`MC1408-004`, `MC1408-008`): `Vref / R14`, rounded to the
/// nearest nanoampere, half up. `None` for a zero resistance. The sheet
/// writes `K ≅ Vref/R14`; how far a specimen's ladder current departs from it
/// is `MC1408-OPEN-05`.
#[must_use]
pub const fn reference_pa(vref_uv: Microvolts, r14_ohms: Ohms) -> Option<Picoamps> {
    if r14_ohms.0 == 0 {
        return None;
    }
    let r14_ohms = r14_ohms.0 as i128;
    let na = (vref_uv.0 as i128 * 1000 + r14_ohms / 2).div_euclid(r14_ohms);
    Some(Picoamps(na as i64 * 1_000))
}

/// `VIH`, a high input level, at least: 2.0 V (`MC1408-016`).
pub const VIH_MIN_UV: Microvolts = Microvolts(2_000_000);
/// `VIL`, a low input level, at most: 0.8 V (`MC1408-016`).
pub const VIL_MAX_UV: Microvolts = Microvolts(800_000);
/// `IIH`, the most a high input draws at 5.0 V: 0.04 mA (`MC1408-016`).
pub const IIH_MAX_PA: Picoamps = Picoamps(40_000_000);
/// `IIL`, what a low input sources at 0.8 V, typical and at most: 0.4 mA and
/// 0.8 mA (`MC1408-016`).
pub const IIL_TYP_MAX_PA: (Picoamps, Picoamps) = (Picoamps(400_000_000), Picoamps(800_000_000));

/// The level a digital input is read as: `Undetermined` between the two
/// thresholds, where the sheet guarantees neither (`MC1408-OPEN-04`).
pub use signal::Logic;

/// The level an input voltage is guaranteed to be read as: High at 2.0 V or
/// more, Low at 0.8 V or less, and undetermined between (`MC1408-016`,
/// `MC1408-OPEN-04`).
#[must_use]
pub const fn input_level(uv: Microvolts) -> Logic {
    if uv.0 >= VIH_MIN_UV.0 {
        Logic::High
    } else if uv.0 <= VIL_MAX_UV.0 {
        Logic::Low
    } else {
        Logic::Undetermined
    }
}

/// An accuracy grade, the suffix after the package letter (`MC1408-010`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Grade {
    /// `MC1408L8`, `MC1408P8`: eight-bit accuracy.
    Eight,
    /// `MC1408L7`, `MC1408P7`: seven-bit.
    Seven,
    /// `MC1408L6`, `MC1408P6`: six-bit.
    Six,
}

impl Grade {
    /// Every grade the sheet orders.
    pub const ALL: [Self; 3] = [Self::Eight, Self::Seven, Self::Six];

    /// The relative accuracy, the largest error relative to full scale, as a
    /// magnitude: ±0.19 %, ±0.39 % and ±0.78 % (`MC1408-010`).
    #[must_use]
    pub const fn relative_accuracy_ppm(self) -> PartsPerMillion {
        PartsPerMillion(match self {
            Self::Eight => 1_900,
            Self::Seven => 3_900,
            Self::Six => 7_800,
        })
    }

    /// The share of rated output current each switch is guaranteed to give:
    /// the `7` and `6` grades are tested to at least half (`MC1408-010`,
    /// Note 1). The `8` grade's row carries no such note.
    #[must_use]
    pub const fn switch_guarantee_per_mille(self) -> Option<PerMille> {
        match self {
            Self::Eight => None,
            Self::Seven | Self::Six => Some(PerMille(500)),
        }
    }
}

/// Full scale at `Vref` 2.000 V and `R14` 1000 Ω, every input high:
/// minimum, typical, maximum — 1.9, 1.99 and 2.1 mA (`MC1408-009`).
pub const FULL_SCALE_MIN_TYP_MAX_PA: (Picoamps, Picoamps, Picoamps) = (
    Picoamps(1_900_000_000),
    Picoamps(1_990_000_000),
    Picoamps(2_100_000_000),
);
/// The reference voltage of that row (`MC1408-009`).
pub const FULL_SCALE_TEST_VREF_UV: Microvolts = Microvolts(2_000_000);
/// The `R14` of that row (`MC1408-009`).
pub const FULL_SCALE_TEST_R14_OHMS: Ohms = Ohms(1_000);

/// `IO(min)`, the output with every input low, at most: 4.0 µA
/// (`MC1408-011`); the row's other figure is 0.
pub const IOMIN_MAX_PA: Picoamps = Picoamps(4_000_000);

/// `ts`, settling to within ±1/2 LSB with every input switched, typical:
/// 300 ns (`MC1408-013`).
pub const TS_TYP_PS: Picoseconds = Picoseconds(300_000);
/// `TPD`, the propagation delay at 25 °C, one figure for both edges, typical
/// and at most: 30 ns and 100 ns (`MC1408-013`).
pub const TPD_TYP_MAX_PS: (Picoseconds, Picoseconds) = (Picoseconds(30_000), Picoseconds(100_000));

/// One switch's typical turn-on and turn-off times (`MC1408-014`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct SwitchingRow {
    /// The input's number, 1 for `A1` to 8 for `A8`.
    pub input: usize,
    /// Turning on and settling, typical.
    pub on_typ_ps: Picoseconds,
    /// Turning off, typical.
    pub off_typ_ps: Picoseconds,
}

/// The three switches the sheet times alone: `A8`, `A7` and `A6`
/// (`MC1408-014`).
pub const SWITCHING_ROWS: [SwitchingRow; 3] = [
    SwitchingRow {
        input: 8,
        on_typ_ps: Picoseconds(250_000),
        off_typ_ps: Picoseconds(80_000),
    },
    SwitchingRow {
        input: 7,
        on_typ_ps: Picoseconds(200_000),
        off_typ_ps: Picoseconds(80_000),
    },
    SwitchingRow {
        input: 6,
        on_typ_ps: Picoseconds(150_000),
        off_typ_ps: Picoseconds(80_000),
    },
];
/// Every bit switched off: typically under 100 ns (`MC1408-014`).
pub const ALL_OFF_UNDER_TYP_PS: Picoseconds = Picoseconds(100_000);
/// `ts` to ±1/2 LSB at seven- and six-bit accuracy, typical: 200 ns
/// (`MC1408-014`).
pub const SEVEN_SIX_BIT_TS_TYP_PS: Picoseconds = Picoseconds(200_000);

/// The reference current's slew rate, typical, in picoamperes per
/// picosecond: 4.0 mA/µs (`MC1408-015`).
pub const REFERENCE_SLEW_TYP_PA_PER_PS: PicoampsPerPicosecond = PicoampsPerPicosecond(4_000);
/// The full-scale current's drift, typical (`MC1408-015`).
pub const FULL_SCALE_DRIFT_TYP_PPM_PER_C: PartsPerMillionPerCelsius =
    PartsPerMillionPerCelsius(-20);

/// The output voltage window on pin 4 inside which the accuracy holds, with
/// pin 1 grounded, `(min, max)`: −0.55 V to +0.4 V (`MC1408-017`).
pub const COMPLIANCE_PIN1_GROUNDED_RANGE_UV: (Microvolts, Microvolts) =
    (Microvolts(-550_000), Microvolts(400_000));
/// The same with pin 1 open and `VEE` below −10 V: −5.0 V to +0.4 V
/// (`MC1408-017`).
pub const COMPLIANCE_PIN1_OPEN_RANGE_UV: (Microvolts, Microvolts) =
    (Microvolts(-5_000_000), Microvolts(400_000));

/// One `IOR` row, the output current range: the most the output sinks at a `VEE`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct IorRow {
    /// The `VEE` the row is stated at.
    pub vee_test_supply_uv: Microvolts,
    /// The output current, at most.
    pub max_pa: Picoamps,
}

/// The output current's maximum: 2.1 mA with `VEE` −5.0 V, and 4.2 mA with
/// `VEE` −15 V, usable only for `VEE` more negative than about −8 V
/// (`MC1408-018`).
pub const IOR_ROWS: [IorRow; 2] = [
    IorRow {
        vee_test_supply_uv: Microvolts(-5_000_000),
        max_pa: Picoamps(2_100_000_000),
    },
    IorRow {
        vee_test_supply_uv: Microvolts(-15_000_000),
        max_pa: Picoamps(4_200_000_000),
    },
];

/// The recommended range of a DC reference current, `(min, max)`: 0.5 to
/// 4.0 mA. Above its lower end the sheet expects the part to be monotonic,
/// without guaranteeing it (`MC1408-019`).
pub const REFERENCE_RECOMMENDED_RANGE_PA: (Picoamps, Picoamps) =
    (Picoamps(500_000_000), Picoamps(4_000_000_000));

/// The absolute maximum `VCC`: +5.5 V (`MC1408-020`).
pub const VCC_SUPPLY_ABSOLUTE_MAX_UV: Microvolts = Microvolts(5_500_000);
/// The absolute maximum `VEE`: −16.5 V. The supply is negative, so this is
/// the most negative `VEE` the rating allows (`MC1408-020`).
pub const VEE_SUPPLY_ABSOLUTE_MAX_UV: Microvolts = Microvolts(-16_500_000);
/// Absolute maximum rating: the digital inputs, `(min, max)` (`MC1408-020`).
pub const INPUT_ABSOLUTE_MAX_RANGE_UV: (Microvolts, Microvolts) =
    (Microvolts(0), Microvolts(5_500_000));
/// Absolute maximum rating: voltage applied to the output, `(min, max)`
/// (`MC1408-020`).
pub const OUTPUT_ABSOLUTE_MAX_RANGE_UV: (Microvolts, Microvolts) =
    (Microvolts(-5_200_000), Microvolts(500_000));
/// Absolute maximum rating: the reference current: 5.0 mA (`MC1408-020`).
pub const REFERENCE_ABSOLUTE_MAX_PA: Picoamps = Picoamps(5_000_000_000);

/// The operating `VCC` range, `(min, max)`: 4.5 V to 5.5 V (`MC1408-021`).
pub const VCC_SUPPLY_UV: (Microvolts, Microvolts) = (Microvolts(4_500_000), Microvolts(5_500_000));
/// The operating `VEE` range, `(min, max)`: −16.5 V to −4.5 V
/// (`MC1408-021`).
pub const VEE_SUPPLY_UV: (Microvolts, Microvolts) =
    (Microvolts(-16_500_000), Microvolts(-4_500_000));
/// Supply current from `VCC` with every input low, typical and at most:
/// 13.5 mA and 22 mA (`MC1408-021`).
pub const ICC_TYP_MAX_PA: (Picoamps, Picoamps) =
    (Picoamps(13_500_000_000), Picoamps(22_000_000_000));
/// Supply current from `VEE` with every input low, typical and at most, as
/// magnitudes: 7.5 mA and 13 mA (`MC1408-021`).
pub const IEE_TYP_MAX_PA: (Picoamps, Picoamps) =
    (Picoamps(7_500_000_000), Picoamps(13_000_000_000));

/// The least compensation capacitance on pin 16, to `VEE`, for an `R14`
/// (`MC1408-022`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct CompensationRow {
    /// `R14`.
    pub r14_ohms: Ohms,
    /// The least capacitance.
    pub min_pf: Picofarads,
}

/// The sheet's three compensation rows (`MC1408-022`).
pub const COMPENSATION_ROWS: [CompensationRow; 3] = [
    CompensationRow {
        r14_ohms: Ohms(1_000),
        min_pf: Picofarads(15),
    },
    CompensationRow {
        r14_ohms: Ohms(2_500),
        min_pf: Picofarads(37),
    },
    CompensationRow {
        r14_ohms: Ohms(5_000),
        min_pf: Picofarads(75),
    },
];

/// The MC1408 series' operating temperature range, `(min, max)`
/// (`MC1408-023`).
pub const OPERATING_RANGE_C: (Celsius, Celsius) = (Celsius(0), Celsius(75));

/// The levels the outside world holds the part's inputs at.
///
/// [`Inputs::default`] is every input Low and the reference current of the
/// sheet's full-scale row, 2 mA ([`FULL_SCALE_TEST_VREF_UV`] across
/// [`FULL_SCALE_TEST_R14_OHMS`]), so `Inputs { a, ..Default::default() }` sinks
/// that code's share of it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// The eight inputs `A1` to `A8` as a byte, `A1` its bit 7 and `A8` its
    /// bit 0, a set bit High (`MC1408-003`, `MC1408-005`).
    pub a: u8,
    /// The reference current into pin 14 (`MC1408-008`); [`reference_pa`]
    /// computes it from `Vref` and `R14`.
    pub reference_pa: Picoamps,
}

impl Inputs {
    /// The part idle; see [`Inputs`].
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            a: 0,
            reference_pa: FULL_SCALE_TEST_REFERENCE_PA,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// The reference current of the full-scale row: 2 mA,
/// [`FULL_SCALE_TEST_VREF_UV`] across [`FULL_SCALE_TEST_R14_OHMS`] (`MC1408-009`).
pub const FULL_SCALE_TEST_REFERENCE_PA: Picoamps =
    Picoamps(FULL_SCALE_TEST_VREF_UV.0 as i64 * 1_000_000 / FULL_SCALE_TEST_R14_OHMS.0 as i64);

/// What the part drives.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub struct Outputs {
    /// The current pin 4 sinks (`MC1408-004`, `MC1408-006`), as
    /// [`output_pa`] rounds it.
    pub output_pa: Picoamps,
}

/// The whole part: what pin 4 sinks for the levels on its inputs now,
/// whatever was driven before (`MC1408-004`, `MC1408-006`, `MC1408-012`).
#[must_use]
pub const fn drive(inputs: Inputs) -> Outputs {
    Outputs {
        output_pa: output_pa(inputs.a, inputs.reference_pa),
    }
}
