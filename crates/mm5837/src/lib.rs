//! The National Semiconductor MM5837 digital noise source, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part has no inputs and its own clock.** A seventeen-stage shift
//! register fed back through an exclusive-OR of stages 17 and 14
//! (`MM5837-003`, `MM5837-004`), stage 17 at pin 3 (`MM5837-005`), clocked by
//! an oscillator no pin sets. So the consumer supplies time: [`Mm5837::advance`]
//! takes picoseconds and [`Mm5837::tick`] takes one clock, and the clock's
//! period is an argument, because it is not one number. The sheet gives a band ([`SHEET_CLOCK_PERIOD_RANGE_PS`], `MM5837-014`) and one
//! specimen gives a curve
//! against supply ([`specimen_clock_period_ps`], `MM5837-015`) that leaves that
//! band at the sheet's own supply (`MM5837-016`).
//!
//! **The part starts somewhere non-zero** (`MM5837-006`).
//! [`Mm5837::new`] takes the starting state from the consumer and refuses zero.
//!
//! **The output is a level, and its voltage is the consumer's choice of
//! figure.** The sheet gives each level as a band against the rails with no
//! typical (`MM5837-009`); one specimen gives one reading each at four supplies
//! (`MM5837-011`). [`swing_at_corner`] and [`specimen_swing`] build either, in
//! the consumer's own frame.
//!
//! **What the crate defines:** which voltage a register `1`
//! is (`MM5837-OPEN-02`) — here a `1` in stage 17 is the high level; and
//! every clock is one fixed period, with no wander (`MM5837-OPEN-06`).
//!
//! # Examples
//!
//! ```
//! use mm5837::{Corner, Mm5837, Vgg, swing_at_corner};
//! use units::{Microvolts, Picoseconds};
//!
//! // A part clocked every 10 µs, between the sheet's rails at a −14 V supply.
//! let mut noise = Mm5837::new(0x1_5a5a, Picoseconds(10_000_000))?;
//! let swing = swing_at_corner(Microvolts(0), Microvolts(-14_000_000), Vgg::Separate, Corner::Max)
//!     .ok_or("VDD below VSS")?;
//! // Stage 17 holds a `1`: pin 3 sits at VSS, half the swing above its middle.
//! assert_eq!(noise.output_uv(swing), Microvolts(0));
//! assert_eq!(noise.audio(swing), Microvolts(7_000_000));
//!
//! // Thirty clocks on, the register has moved and pin 3 follows stage 17.
//! noise.advance(Picoseconds(300_000_000));
//! assert_ne!(noise.noise_shift_register(), 0x1_5a5a);
//! assert_eq!(noise.output_level().is_high(), noise.noise_shift_register() >> 16 == 1);
//! # Ok::<(), Box<dyn core::error::Error>>(())
//! ```

#![no_std]

use units::{Hertz, Microvolts, Picoamps, Picoseconds};

/// How many pins the package presents (`MM5837-001`).
pub const PIN_COUNT: usize = 8;

/// The pin names from pin 1, a pin with no connection `NC` (`MM5837-001`).
pub const PIN_NAMES: [&str; PIN_COUNT] = ["VDD", "VGG", "OUTPUT", "VSS", "NC", "NC", "NC", "NC"];

/// Stages in the register (`MM5837-003`).
pub const STAGE_COUNT: usize = 17;

/// The stage pin 3 follows, counted from the input end; the exclusive-OR
/// takes it with [`FEEDBACK_TAP_STAGE`] (`MM5837-004`, `MM5837-005`).
pub const OUTPUT_STAGE: usize = 17;

/// The other stage the exclusive-OR takes, counted from the input end
/// (`MM5837-004`).
pub const FEEDBACK_TAP_STAGE: usize = 14;

/// Clock periods in one repetition of the sequence, `2^17 - 1`
/// (`MM5837-013`).
pub const SEQUENCE_CYCLES: usize = 131_071;

/// Rising edges at the output in one repetition (`MM5837-015`'s arithmetic).
pub const SEQUENCE_RISING_EDGE_COUNT: usize = 32_768;

const MASK: u32 = (1 << STAGE_COUNT) - 1;

const PS_PER_S: u64 = 1_000_000_000_000;

/// The supply range every figure on the sheet is characterised over, `VSS`
/// less `VDD`, in microvolts, `(min, max)`: 14 V ± 1 V (`MM5837-002`).
pub const VDD_SUPPLY_UV: (Microvolts, Microvolts) =
    (Microvolts(13_000_000), Microvolts(15_000_000));

/// The gate supply range every figure on the sheet is characterised over,
/// `VSS` less `VGG`, in microvolts, `(min, max)`: 27 V ± 2 V (`MM5837-002`).
pub const VGG_SUPPLY_UV: (Microvolts, Microvolts) =
    (Microvolts(25_000_000), Microvolts(29_000_000));

/// The absolute maximum supply: the most `VDD` may sit below `VSS`, in
/// microvolts (`MM5837-002`).
pub const VDD_SUPPLY_ABSOLUTE_MAX_UV: Microvolts = Microvolts(25_000_000);

/// The absolute maximum gate supply: the most `VGG` may sit below `VSS`, in
/// microvolts (`MM5837-002`).
pub const VGG_SUPPLY_ABSOLUTE_MAX_UV: Microvolts = Microvolts(33_000_000);

/// The cycle time, minimum and maximum, in picoseconds: 1.1 s and 2.4 s
/// (`MM5837-007`).
pub const SHEET_CYCLE_TIME_RANGE_PS: (Picoseconds, Picoseconds) = (
    Picoseconds(1_100_000_000_000),
    Picoseconds(2_400_000_000_000),
);

/// The half-power point, minimum and maximum, in hertz (`MM5837-008`).
pub const SHEET_HALF_POWER_RANGE_HZ: (Hertz, Hertz) = (Hertz(24_000), Hertz(56_000));

/// The shortest and longest clock periods the sheet's cycle time allows, in
/// picoseconds, the shortest rounded down and the longest up (`MM5837-014`).
pub const SHEET_CLOCK_PERIOD_RANGE_PS: (Picoseconds, Picoseconds) = (
    Picoseconds(SHEET_CYCLE_TIME_RANGE_PS.0.0 / SEQUENCE_CYCLES as u64),
    Picoseconds(
        SHEET_CYCLE_TIME_RANGE_PS
            .1
            .0
            .div_ceil(SEQUENCE_CYCLES as u64),
    ),
);

/// The supply current `IDD` with no output load, minimum and maximum, in
/// picoamps (`MM5837-010`).
pub const SHEET_IDD_RANGE_PA: (Picoamps, Picoamps) =
    (Picoamps(3_000_000_000), Picoamps(8_000_000_000));

/// The gate supply current `IGG` with no output load, maximum, in picoamps
/// (`MM5837-010`).
pub const SHEET_IGG_MAX_PA: Picoamps = Picoamps(7_000_000_000);

/// The largest magnitude [`Mm5837::audio`] returns for any [`Swing`] inside
/// the absolute maximum supply, in microvolts: half of
/// [`VDD_SUPPLY_ABSOLUTE_MAX_UV`], the widest swing the output's rails allow.
pub const AUDIO_FULL_SCALE_UV: Microvolts = Microvolts(VDD_SUPPLY_ABSOLUTE_MAX_UV.0 / 2);

/// A pin's level, as a getter named for the pin returns it (`output_level()`).
pub use signal::Level;

/// One row of [`SPECIMEN_PIN3_ROWS`]: a supply and the frequency at pin 3 there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SpecimenPin3Row {
    /// The supply, `VSS` less `VDD`.
    pub test_supply_uv: Microvolts,
    /// The frequency at pin 3.
    pub hz: Hertz,
}

/// One row of [`SPECIMEN_SWING_ROWS`]: a supply and the output's extremes there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SpecimenSwingRow {
    /// The supply, `VSS` less `VDD`.
    pub test_supply_uv: Microvolts,
    /// The low level and the high, `(min, max)`, each above `VDD`.
    pub range_above_vdd_uv: (Microvolts, Microvolts),
}

/// One specimen's frequency at pin 3 against its supply (`MM5837-012`).
pub const SPECIMEN_PIN3_ROWS: [SpecimenPin3Row; 40] = [
    SpecimenPin3Row {
        test_supply_uv: Microvolts(7_200_000),
        hz: Hertz(1),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(7_400_000),
        hz: Hertz(26),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(7_500_000),
        hz: Hertz(110),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(7_600_000),
        hz: Hertz(205),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(7_700_000),
        hz: Hertz(370),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(8_000_000),
        hz: Hertz(950),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(8_500_000),
        hz: Hertz(2_900),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(9_000_000),
        hz: Hertz(5_570),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(9_500_000),
        hz: Hertz(8_300),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(10_000_000),
        hz: Hertz(11_300),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(10_500_000),
        hz: Hertz(14_800),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(11_000_000),
        hz: Hertz(18_100),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(11_500_000),
        hz: Hertz(22_050),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(12_000_000),
        hz: Hertz(25_800),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(12_500_000),
        hz: Hertz(29_750),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(12_910_000),
        hz: Hertz(32_768),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(13_000_000),
        hz: Hertz(33_670),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(13_500_000),
        hz: Hertz(37_800),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(14_000_000),
        hz: Hertz(41_900),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(14_500_000),
        hz: Hertz(46_050),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(15_000_000),
        hz: Hertz(49_700),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(15_500_000),
        hz: Hertz(52_450),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(16_000_000),
        hz: Hertz(53_800),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(16_500_000),
        hz: Hertz(53_900),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(17_000_000),
        hz: Hertz(53_000),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(17_500_000),
        hz: Hertz(51_500),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(18_000_000),
        hz: Hertz(49_000),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(18_500_000),
        hz: Hertz(45_900),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(19_000_000),
        hz: Hertz(42_500),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(19_500_000),
        hz: Hertz(39_200),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(20_000_000),
        hz: Hertz(36_000),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(20_500_000),
        hz: Hertz(32_900),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(21_000_000),
        hz: Hertz(29_600),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(21_500_000),
        hz: Hertz(26_700),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(22_000_000),
        hz: Hertz(24_200),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(22_500_000),
        hz: Hertz(21_450),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(23_000_000),
        hz: Hertz(18_800),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(23_500_000),
        hz: Hertz(16_650),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(24_000_000),
        hz: Hertz(13_900),
    },
    SpecimenPin3Row {
        test_supply_uv: Microvolts(24_500_000),
        hz: Hertz(11_950),
    },
];

/// One specimen's output extremes with `VGG` tied to `VDD` and 22 kΩ to `VDD`
/// (`MM5837-011`).
pub const SPECIMEN_SWING_ROWS: [SpecimenSwingRow; 4] = [
    SpecimenSwingRow {
        test_supply_uv: Microvolts(20_000_000),
        range_above_vdd_uv: (Microvolts(6_000_000), Microvolts(18_000_000)),
    },
    SpecimenSwingRow {
        test_supply_uv: Microvolts(15_000_000),
        range_above_vdd_uv: (Microvolts(4_000_000), Microvolts(13_500_000)),
    },
    SpecimenSwingRow {
        test_supply_uv: Microvolts(12_000_000),
        range_above_vdd_uv: (Microvolts(3_000_000), Microvolts(10_500_000)),
    },
    SpecimenSwingRow {
        test_supply_uv: Microvolts(10_000_000),
        range_above_vdd_uv: (Microvolts(2_500_000), Microvolts(9_000_000)),
    },
];

/// The clock period, in picoseconds and rounded to nearest, of a part whose
/// output rises `pin3_hz` times a second: `SEQUENCE_RISING_EDGE_COUNT` rises in
/// `SEQUENCE_CYCLES` clocks (`MM5837-015`). `None` at zero hertz.
#[must_use]
pub const fn clock_period_ps(pin3_hz: Hertz) -> Option<Picoseconds> {
    if pin3_hz.0 == 0 {
        return None;
    }
    let num = SEQUENCE_RISING_EDGE_COUNT as u128 * PS_PER_S as u128;
    let den = SEQUENCE_CYCLES as u128 * pin3_hz.0 as u128;
    Some(Picoseconds(((num + den / 2) / den) as u64))
}

/// The specimen's clock period at a supply it was measured at, in picoseconds
/// (`MM5837-015`). `None` at a supply not in [`SPECIMEN_PIN3_ROWS`]: nothing is
/// interpolated.
#[must_use]
pub const fn specimen_clock_period_ps(supply_uv: Microvolts) -> Option<Picoseconds> {
    let mut i = 0;
    while i < SPECIMEN_PIN3_ROWS.len() {
        if SPECIMEN_PIN3_ROWS[i].test_supply_uv.0 == supply_uv.0 {
            return clock_period_ps(SPECIMEN_PIN3_ROWS[i].hz);
        }
        i += 1;
    }
    None
}

/// What the `VGG` pin is wired to, which selects the sheet's low-level row
/// (`MM5837-009`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Vgg {
    /// `VGG` on its own supply, 27 V ± 2 V below `VSS`.
    Separate,
    /// `VGG` tied to `VDD`, the sheet's `VGG = −14 V ± 1 V` row.
    TiedToVdd,
}

/// Which of the sheet's two limits for each level to take ([`swing_at_corner`],
/// `MM5837-009`). The sheet gives each level as a band and prints no
/// typical.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Corner {
    /// The narrowest swing the sheet guarantees: each level at the limit of
    /// its band away from its rail.
    Min,
    /// The widest swing the sheet allows: each level at its rail.
    Max,
}

/// The two voltages the output takes, in microvolts in the consumer's frame:
/// the level for a `0` in stage 17 and the level for a `1`.
pub use units::Swing;

/// The sheet's limits at `corner` (`MM5837-009`), saturating at the range
/// of an `i32`: at [`Corner::Max`] the rails, high at `VSS` and low at
/// `VDD`, whatever `vgg` is wired to; at [`Corner::Min`] away from them,
/// high at `VSS − 1.5 V` and low at `VDD + 1.5 V`, or `VDD + 3.5 V` with
/// `VGG` tied to `VDD`. `None` where the supply is too small for the low
/// level to stay below the high, `VDD` above `VSS` included.
#[must_use]
pub const fn swing_at_corner(
    vss_uv: Microvolts,
    vdd_uv: Microvolts,
    vgg: Vgg,
    corner: Corner,
) -> Option<Swing> {
    let (low_above_vdd, high_below_vss) = match (corner, vgg) {
        (Corner::Max, _) => (0, 0),
        (Corner::Min, Vgg::Separate) => (1_500_000, 1_500_000),
        (Corner::Min, Vgg::TiedToVdd) => (3_500_000, 1_500_000),
    };
    Swing::declared(
        Microvolts(vdd_uv.0.saturating_add(low_above_vdd)),
        Microvolts(vss_uv.0.saturating_sub(high_below_vss)),
    )
}

/// One specimen's extremes at a supply it was measured at, placed above
/// `vdd_uv` (`MM5837-011`), saturating at the range of an `i32`. `None`
/// at any other supply. A reading of one part, not a corner of the
/// sheet's, so it is a function of its own beside
/// [`swing_at_corner`].
#[must_use]
pub const fn specimen_swing(vdd_uv: Microvolts, test_supply_uv: Microvolts) -> Option<Swing> {
    let mut i = 0;
    while i < SPECIMEN_SWING_ROWS.len() {
        let row = SPECIMEN_SWING_ROWS[i];
        if row.test_supply_uv.0 == test_supply_uv.0 {
            let (low, high) = row.range_above_vdd_uv;
            return Swing::declared(
                Microvolts(vdd_uv.0.saturating_add(low.0)),
                Microvolts(vdd_uv.0.saturating_add(high.0)),
            );
        }
        i += 1;
    }
    None
}

/// Why [`Mm5837::new`] refused a starting state or a clock period.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Mm5837Error {
    /// Every stage holds `0`, a state the part does not start in and never
    /// leaves (`MM5837-006`).
    ZeroShiftRegister,
    /// A bit above stage 17 is set; the register has no stage for it.
    ExtraBits,
    /// The clock period is zero.
    ZeroClockPeriod,
}

impl core::fmt::Display for Mm5837Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::ZeroShiftRegister => "a shift register of all zeros",
            Self::ExtraBits => "a shift register wider than seventeen stages",
            Self::ZeroClockPeriod => "a clock period of zero",
        })
    }
}

impl core::error::Error for Mm5837Error {}

/// The part: seventeen stages, the clock period and how far into it time
/// stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Mm5837 {
    /// Bit `n` is stage `n + 1`.
    shift_register: u32,
    period_ps: u64,
    phase_ps: u64,
}

impl Mm5837 {
    /// A part holding `shift_register` — bit `n` is stage `n + 1` — clocked every
    /// `clock_period_ps`.
    ///
    /// # Errors
    ///
    /// Each [`Mm5837Error`]: the all-zero state, which the part does not
    /// start in (`MM5837-006`), a state wider than seventeen bits, or a zero
    /// period, checked in that order.
    ///
    /// # Examples
    ///
    /// ```
    /// use mm5837::{Mm5837, Mm5837Error};
    /// use units::Picoseconds;
    ///
    /// let noise = Mm5837::new(0x1_5a5a, Picoseconds(10_000_000))?;
    /// assert_eq!(noise.noise_shift_register(), 0x1_5a5a);
    ///
    /// // The register never leaves all zeros, so the part never starts there.
    /// assert_eq!(
    ///     Mm5837::new(0, Picoseconds(10_000_000)),
    ///     Err(Mm5837Error::ZeroShiftRegister)
    /// );
    /// # Ok::<(), Mm5837Error>(())
    /// ```
    pub const fn new(
        shift_register: u32,
        clock_period_ps: Picoseconds,
    ) -> Result<Self, Mm5837Error> {
        if shift_register == 0 {
            return Err(Mm5837Error::ZeroShiftRegister);
        }
        if shift_register & !MASK != 0 {
            return Err(Mm5837Error::ExtraBits);
        }
        if clock_period_ps.0 == 0 {
            return Err(Mm5837Error::ZeroClockPeriod);
        }
        Ok(Self {
            shift_register,
            period_ps: clock_period_ps.0,
            phase_ps: 0,
        })
    }

    /// The noise shift register's seventeen stages, bit `n` stage `n + 1`.
    #[must_use]
    pub const fn noise_shift_register(self) -> u32 {
        self.shift_register
    }

    /// The clock period this part was built with, in picoseconds.
    #[must_use]
    pub const fn clock_period_ps(self) -> Picoseconds {
        Picoseconds(self.period_ps)
    }

    /// The logic level of pin 3, `OUTPUT`: stage 17, which the pin follows,
    /// [`Level::High`] for a `1` (`MM5837-005`).
    #[must_use]
    pub const fn output_level(self) -> Level {
        Level::from_bool(self.shift_register >> (OUTPUT_STAGE - 1) & 1 != 0)
    }

    /// Pin 3's voltage under `swing`.
    #[must_use]
    pub const fn output_uv(self, swing: Swing) -> Microvolts {
        let (low_uv, high_uv) = swing.range_uv();
        if self.output_level().is_high() {
            high_uv
        } else {
            low_uv
        }
    }

    /// Pin 3 as a signed sample under `swing`: half of it, positive for
    /// the high level and negative for the low, measured from the midpoint of
    /// the two and rounded toward it. The part has no rest; 0 is that
    /// midpoint, and full scale is half of the swing's width, at most
    /// [`AUDIO_FULL_SCALE_UV`] for levels inside the part's supply.
    #[must_use]
    pub const fn audio(&self, swing: Swing) -> Microvolts {
        let (low_uv, high_uv) = swing.range_uv();
        let half = (high_uv.0 as i64 - low_uv.0 as i64) / 2;
        let signed = if self.output_level().is_high() {
            half
        } else {
            -half
        };
        Microvolts(signed as i32)
    }

    /// One period of the part's own clock: every stage takes the one before it
    /// and stage 1 takes stage 17 exclusive-OR stage 14 (`MM5837-004`). The new
    /// output is [`Mm5837::output_level`].
    pub const fn tick(&mut self) {
        let feedback = (self.shift_register >> (OUTPUT_STAGE - 1)
            ^ self.shift_register >> (FEEDBACK_TAP_STAGE - 1))
            & 1;
        self.shift_register = (self.shift_register << 1 | feedback) & MASK;
    }

    /// Picoseconds until the next clock.
    #[must_use]
    pub const fn next_clock_ps(self) -> Picoseconds {
        Picoseconds(self.period_ps - self.phase_ps)
    }

    /// Let `dt_ps` pass, clocking once at each period boundary it crosses.
    /// Any span is taken whole: nothing overflows.
    pub const fn advance(&mut self, dt_ps: Picoseconds) {
        let total = self.phase_ps as u128 + dt_ps.0 as u128;
        let period = self.period_ps as u128;
        let clocks = total / period;
        self.phase_ps = (total % period) as u64;
        let mut n = (clocks % SEQUENCE_CYCLES as u128) as u64;
        while n > 0 {
            self.tick();
            n -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: Picoseconds = Picoseconds(1_000);

    #[test]
    fn each_refusal_is_named() {
        assert_eq!(Mm5837::new(0, PERIOD), Err(Mm5837Error::ZeroShiftRegister));
        assert_eq!(Mm5837::new(1 << 17, PERIOD), Err(Mm5837Error::ExtraBits));
        assert_eq!(
            Mm5837::new(1, Picoseconds(0)),
            Err(Mm5837Error::ZeroClockPeriod)
        );
        assert!(Mm5837::new(MASK, PERIOD).is_ok());
    }

    #[test]
    fn a_swing_of_no_width_is_refused() {
        let uv = Microvolts;
        assert_eq!(Swing::declared(uv(5), uv(5)), None);
        assert!(Swing::declared(uv(4), uv(5)).is_some());
        assert_eq!(
            swing_at_corner(uv(0), uv(0), Vgg::Separate, Corner::Max),
            None
        );
    }

    #[test]
    fn the_corners_are_the_rails_and_the_inner_limits() {
        let uv = Microvolts;
        let (vss, vdd) = (uv(0), uv(-14_000_000));
        let rails = swing_at_corner(vss, vdd, Vgg::TiedToVdd, Corner::Max).unwrap();
        assert_eq!(rails.range_uv(), (vdd, vss));
        let inner = swing_at_corner(vss, vdd, Vgg::TiedToVdd, Corner::Min).unwrap();
        assert_eq!(inner.range_uv(), (uv(-10_500_000), uv(-1_500_000)));
        let separate = swing_at_corner(vss, vdd, Vgg::Separate, Corner::Min).unwrap();
        assert_eq!(separate.range_uv(), (uv(-12_500_000), uv(-1_500_000)));
    }
}
