//! The Texas Instruments NE555 precision timer, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The timing capacitor is on the far side of a pin.** What the part does is
//! compare the voltages on `THRESHOLD` and `TRIGGER` against two levels, hold
//! one bit, and drive `OUTPUT` and `DISCHARGE` from it (`NE555-003`). So
//! [`Ne555::step`] takes **microvolts** at four pins and returns two levels,
//! and the ramp every `555` is known for is the caller's capacitor to step —
//! `NE555-014` and `NE555-015` are the sheet's equations for what that ramp
//! does in the two circuits it draws, and [`monostable_width_ps`],
//! [`astable_high_ps`] and [`astable_low_ps`] are those equations in
//! picoseconds.
//!
//! **The levels are the control pin's.** The threshold level *is* the control
//! voltage and the trigger level is half of it (`NE555-004`, `NE555-007`);
//! with the pin open the control voltage is two-thirds of the supply
//! (`NE555-011`). A caller that drives the pin moves both levels together.
//!
//! **Three priorities, all the sheet's.** Reset overrides everything
//! (`NE555-008`); the trigger prevails over the threshold while both assert
//! (`NE555-010`); and a trigger that finds the flip-flop already set changes
//! nothing (`NE555-017`).
//!
//! **What the sheet does not say, this crate does not say either.** The
//! flip-flop starts on neither branch and `OUTPUT` ([`Outputs::output`]) is
//! [`Logic::Undetermined`] until a comparator or the reset decides it (`NE555-OPEN-02`); the comparisons are
//! strict on the sheet's words *falls below* and *rises above*
//! (`NE555-OPEN-03`); and there is no time in it, because the sheet's
//! propagation delay is a figure and the crate changes its outputs in the
//! sample it is driven (`NE555-OPEN-05`).
//!
//! # Examples
//!
//! ```
//! use ne555::{Corner, Inputs, Logic, Ne555};
//! use units::Microvolts;
//!
//! let mut timer = Ne555::at_corner(Microvolts(5_000_000), Corner::Typ);
//! // A trigger below a third of the supply sets the flip-flop.
//! let idle = Inputs::idle(timer.supply_uv());
//! let triggered = Inputs { trigger_uv: Microvolts(1_000_000), ..idle };
//! assert_eq!(timer.step(triggered).output, Logic::High);
//! // A threshold above two-thirds of it resets the flip-flop.
//! let charged = Inputs { threshold_uv: Microvolts(4_000_000), ..idle };
//! assert_eq!(timer.step(charged).output, Logic::Low);
//! ```

#![no_std]

use units::{Microvolts, Ohms, PerMille, Picoamps, Picofarads, Picoseconds};

/// How many pins the package presents (`NE555-001`).
pub const PIN_COUNT: usize = 8;

/// The supply pin (`NE555-001`).
pub const VCC_PIN: u8 = 8;

/// The ground pin (`NE555-001`).
pub const GND_PIN: u8 = 1;

/// Where the timer's six pins are on the package.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct TimerPinout {
    /// `DISCHARGE`.
    pub discharge: u8,
    /// `THRESHOLD`.
    pub threshold: u8,
    /// `CONTROL VOLTAGE`.
    pub control: u8,
    /// `RESET`.
    pub reset: u8,
    /// `OUTPUT`.
    pub output: u8,
    /// `TRIGGER`.
    pub trigger: u8,
}

/// How many timers the package carries: one, so [`TIMER_PINS`] has the
/// shape the dual part's has.
pub const TIMER_COUNT: usize = 1;

/// The timer's six pins (`NE555-001`).
pub const TIMER_PINS: [TimerPinout; TIMER_COUNT] = [TimerPinout {
    discharge: 7,
    threshold: 6,
    control: 5,
    reset: 4,
    output: 3,
    trigger: 2,
}];

/// The recommended supply for this member, lowest and highest: 4.5 V to
/// 16 V (`NE555-002`).
pub const SUPPLY_UV: (Microvolts, Microvolts) = (Microvolts(4_500_000), Microvolts(16_000_000));

/// The absolute maximum supply: 18 V (`NE555-002`).
pub const SUPPLY_ABSOLUTE_MAX_UV: Microvolts = Microvolts(18_000_000);

/// The reset level, minimum, typical and maximum: 0.4 V, 0.7 V and 1 V
/// (`NE555-008`).
pub const RESET_LEVEL_MIN_TYP_MAX_UV: (Microvolts, Microvolts, Microvolts) = (
    Microvolts(400_000),
    Microvolts(700_000),
    Microvolts(1_000_000),
);

/// The threshold input's current, typical and maximum (`NE555-012`).
pub const THRESHOLD_CURRENT_TYP_MAX_PA: (Picoamps, Picoamps) =
    (Picoamps(100_000), Picoamps(250_000));

/// The trigger input's current, typical (`NE555-012`).
pub const TRIGGER_CURRENT_TYP_PA: Picoamps = Picoamps(500_000);

/// The reset input's current, typical (`NE555-012`).
pub const RESET_CURRENT_TYP_PA: Picoamps = Picoamps(100_000_000);

/// The most the output may sink or source: 200 mA (`NE555-013`).
pub const OUTPUT_CURRENT_MAX_PA: Picoamps = Picoamps(200_000_000_000);

/// One low-level output voltage row: the output low, sinking a load current.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct VolRow {
    /// The supply the row is stated at, `Vcc`.
    pub test_supply_uv: Microvolts,
    /// The current the output sinks.
    pub sink_pa: Picoamps,
    /// The output voltage, typical and at most; `None` where the column
    /// prints no figure.
    pub typ_max_uv: (Option<Microvolts>, Option<Microvolts>),
}

/// One high-level output voltage row: the output high, sourcing a load
/// current.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct VohRow {
    /// The supply the row is stated at, `Vcc`.
    pub test_supply_uv: Microvolts,
    /// The current the output sources; `None` where the row states none.
    pub source_pa: Option<Picoamps>,
    /// The load to ground, at least; `None` where the row states none
    /// (unloaded).
    pub load_ohms: Option<Ohms>,
    /// The output voltage, at least and typical; `None` where the column
    /// prints no figure.
    pub min_typ_uv: (Option<Microvolts>, Option<Microvolts>),
}

const fn vol(test_supply_uv: i32, sink_pa: i64, typ_uv: i32, max_uv: Option<i32>) -> VolRow {
    VolRow {
        test_supply_uv: Microvolts(test_supply_uv),
        sink_pa: Picoamps(sink_pa),
        typ_max_uv: (
            Some(Microvolts(typ_uv)),
            match max_uv {
                Some(uv) => Some(Microvolts(uv)),
                None => None,
            },
        ),
    }
}

const fn voh(test_supply_uv: i32, source_pa: i64, min_uv: Option<i32>, typ_uv: i32) -> VohRow {
    VohRow {
        test_supply_uv: Microvolts(test_supply_uv),
        source_pa: Some(Picoamps(source_pa)),
        load_ohms: None,
        min_typ_uv: (
            match min_uv {
                Some(uv) => Some(Microvolts(uv)),
                None => None,
            },
            Some(Microvolts(typ_uv)),
        ),
    }
}

/// The low-level output voltage rows for this member, at `Vcc` 15 V and then
/// 5 V (`NE555-013`).
pub const VOL_ROWS: [VolRow; 5] = [
    vol(15_000_000, 10_000_000_000, 100_000, Some(250_000)),
    vol(15_000_000, 50_000_000_000, 400_000, Some(750_000)),
    vol(15_000_000, 100_000_000_000, 2_000_000, Some(2_500_000)),
    vol(15_000_000, 200_000_000_000, 2_500_000, None),
    vol(5_000_000, 5_000_000_000, 250_000, Some(350_000)),
];

/// The high-level output voltage rows for this member, at `Vcc` 15 V and
/// then 5 V (`NE555-013`).
pub const VOH_ROWS: [VohRow; 3] = [
    voh(15_000_000, 100_000_000_000, Some(12_750_000), 13_300_000),
    voh(15_000_000, 200_000_000_000, None, 12_500_000),
    voh(5_000_000, 100_000_000_000, Some(2_750_000), 3_300_000),
];

/// What the `CONTROL VOLTAGE` pin is doing (`NE555-004`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Control {
    /// Left open: the divider sets it at two-thirds of the supply
    /// (`NE555-011`).
    Open,
    /// Driven by the board to this voltage.
    Driven {
        /// The pin's voltage.
        level_uv: Microvolts,
    },
}

/// The four input pins, as voltages above `GND`.
///
/// There is no `Default`: no one level of `TRIGGER` is both at or below every
/// `Vcc` and above every trigger level, which is a third of `Vcc` with the
/// control open (`NE555-007`, `NE555-011`). [`Inputs::idle`] takes the supply.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// Pin 6.
    pub threshold_uv: Microvolts,
    /// Pin 2.
    pub trigger_uv: Microvolts,
    /// Pin 5.
    pub control: Control,
    /// Pin 4.
    pub reset_uv: Microvolts,
}

impl Inputs {
    /// The part idle at a supply of `vcc_uv`: `CONTROL VOLTAGE` open,
    /// `THRESHOLD` at 0 V, and `TRIGGER` and `RESET` tied to `Vcc`, above the
    /// trigger level and above the highest reset level (`NE555-008`), so no
    /// comparator and no reset asserts and the flip-flop holds. At every
    /// supply [`SUPPLY_UV`] admits.
    #[must_use]
    pub const fn idle(vcc_uv: Microvolts) -> Self {
        Self {
            threshold_uv: Microvolts(0),
            trigger_uv: vcc_uv,
            control: Control::Open,
            reset_uv: vcc_uv,
        }
    }
}

/// The level an output pin is driven to, or, on `DISCHARGE`, leaves a
/// pulled-up line at; and the flip-flop's `Q`, which the `OUTPUT` pin follows
/// (`NE555-003`).
///
/// On `DISCHARGE`, `Low` is the transistor conducting to `GND` and `High` the
/// transistor off and the line at its pull-up; of `Q`, `Low` is the
/// flip-flop reset and `High` set. `Undetermined` is a part whose flip-flop
/// neither comparator nor the reset has decided since power-up, which the
/// sheet gives no state for (`NE555-OPEN-02`).
pub use signal::Logic;

/// The two output pins.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// Pin 3.
    pub output: Logic,
    /// Pin 7, an open collector: `Low` (conducting) exactly while the output
    /// is low, otherwise `High`, which is the pull-up's (`NE555-009`).
    pub discharge: Logic,
}

impl Outputs {
    /// The two output pins a flip-flop at `q` drives: `OUTPUT` at `Q`,
    /// and the open-collector `DISCHARGE` leaving a pulled-up line at the same
    /// level, `Low` conducting and `High` off (`NE555-009`).
    #[must_use]
    const fn at_q(q: Logic) -> Self {
        Self {
            output: q,
            discharge: q,
        }
    }
}

/// Which of the sheet's reset levels [`Ne555::at_corner`] takes
/// (`NE555-008`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Corner {
    /// The minimum, 0.4 V.
    Min,
    /// The typical, 0.7 V.
    Typ,
    /// The maximum, 1 V.
    Max,
}

/// The timer: a supply, a reset level and the flip-flop.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Ne555 {
    vcc_uv: Microvolts,
    reset_level_uv: Microvolts,
    q: Logic,
}

impl Ne555 {
    /// A timer at this supply whose reset level a caller sets, from a
    /// specimen or a corner of [`RESET_LEVEL_MIN_TYP_MAX_UV`], with its
    /// flip-flop undecided.
    ///
    /// Any supply is taken: [`Ne555::control_uv`] computes at every one.
    #[must_use]
    pub const fn new(vcc_uv: Microvolts, reset_level_uv: Microvolts) -> Self {
        Self::with_state(vcc_uv, reset_level_uv, Logic::Undetermined)
    }

    /// A timer at this supply whose reset level is the sheet's at `corner`
    /// (`NE555-008`), with its flip-flop undecided.
    #[must_use]
    pub const fn at_corner(vcc_uv: Microvolts, corner: Corner) -> Self {
        let reset_level_uv = match corner {
            Corner::Min => RESET_LEVEL_MIN_TYP_MAX_UV.0,
            Corner::Typ => RESET_LEVEL_MIN_TYP_MAX_UV.1,
            Corner::Max => RESET_LEVEL_MIN_TYP_MAX_UV.2,
        };
        Self::new(vcc_uv, reset_level_uv)
    }

    /// A timer whose flip-flop stands at `q`, for a caller that knows what its board has
    /// done to the part already.
    #[must_use]
    pub const fn with_state(vcc_uv: Microvolts, reset_level_uv: Microvolts, q: Logic) -> Self {
        Self {
            vcc_uv,
            reset_level_uv,
            q,
        }
    }

    /// The supply.
    #[must_use]
    pub const fn supply_uv(self) -> Microvolts {
        self.vcc_uv
    }

    /// The reset level: at or below it the reset input is low.
    #[must_use]
    pub const fn reset_level_uv(self) -> Microvolts {
        self.reset_level_uv
    }

    /// `Q`, the flip-flop's bit, now: `High` set, `Low` reset.
    #[must_use]
    pub const fn q(self) -> Logic {
        self.q
    }

    /// The control voltage — the pin's own if driven, two-thirds of the supply
    /// if open (`NE555-004`, `NE555-011`), rounded towards zero.
    ///
    /// Two-thirds of a supply is computed wide, so it is exact for every
    /// supply [`Ne555::new`] takes, out to `i32::MIN` and `i32::MAX`
    /// microvolts, and fits the result, being smaller.
    #[must_use]
    pub const fn control_uv(self, control: Control) -> Microvolts {
        match control {
            Control::Open => Microvolts((self.vcc_uv.0 as i64 * 2 / 3) as i32),
            Control::Driven { level_uv } => level_uv,
        }
    }

    /// The threshold level, which is the control voltage (`NE555-004`).
    #[must_use]
    pub const fn threshold_level_uv(self, control: Control) -> Microvolts {
        self.control_uv(control)
    }

    /// The trigger level, which is half the control voltage (`NE555-007`).
    #[must_use]
    pub const fn trigger_level_uv(self, control: Control) -> Microvolts {
        Microvolts(self.control_uv(control).0 / 2)
    }

    /// The two output pins now.
    #[must_use]
    pub const fn outputs(self) -> Outputs {
        Outputs::at_q(self.q)
    }

    /// The `Q` these pin voltages would leave the flip-flop at, without
    /// moving it.
    ///
    /// The whole of `NE555-005`, `NE555-006`, `NE555-008` and `NE555-010` is
    /// here: reset low resets whatever else is true; otherwise a trigger below
    /// its level sets, whatever the threshold; otherwise a threshold above its
    /// level resets; otherwise the flip-flop holds.
    #[must_use]
    pub const fn held(self, pins: Inputs) -> Logic {
        if pins.reset_uv.0 <= self.reset_level_uv.0 {
            Logic::Low
        } else if pins.trigger_uv.0 < self.trigger_level_uv(pins.control).0 {
            Logic::High
        } else if pins.threshold_uv.0 > self.threshold_level_uv(pins.control).0 {
            Logic::Low
        } else {
            self.q
        }
    }

    /// Hold the four input pins at these voltages, and read the two output
    /// pins.
    ///
    /// There is no time in it: the outputs are what the flip-flop now holds,
    /// and what a specimen does between the comparison and the settled level
    /// is `NE555-OPEN-05`.
    pub const fn step(&mut self, pins: Inputs) -> Outputs {
        self.q = self.held(pins);
        self.outputs()
    }
}

/// The monostable output pulse width, `1.1 RA C`, from a resistor and a
/// capacitor (`NE555-014`).
///
/// An ohm times a picofarad is a picosecond, so no unit conversion is in it;
/// the 1.1 is the sheet's rounding of the natural logarithm of three.
/// Held to `u64::MAX` picoseconds where the width does not fit.
#[must_use]
pub const fn monostable_width_ps(ra_ohms: Ohms, c_pf: Picofarads) -> Picoseconds {
    Picoseconds(held(
        (ra_ohms.0 as u128)
            .saturating_mul(c_pf.0 as u128)
            .saturating_mul(11)
            / 10,
    ))
}

/// `v`, or `u64::MAX` where it does not fit.
const fn held(v: u128) -> u64 {
    if v > u64::MAX as u128 {
        u64::MAX
    } else {
        v as u64
    }
}

/// `0.693 × r × c`, in picoseconds, rounded down and held to `u64::MAX`.
const fn ln2_rc(r_ohms: u128, c_pf: Picofarads) -> Picoseconds {
    Picoseconds(held(
        r_ohms.saturating_mul(c_pf.0 as u128).saturating_mul(693) / 1000,
    ))
}

/// The astable output's high duration, `0.693 (RA + RB) C` (`NE555-015`).
/// Held to `u64::MAX` picoseconds where it does not fit.
#[must_use]
pub const fn astable_high_ps(ra_ohms: Ohms, rb_ohms: Ohms, c_pf: Picofarads) -> Picoseconds {
    ln2_rc(ra_ohms.0 as u128 + rb_ohms.0 as u128, c_pf)
}

/// The astable output's low duration, `0.693 RB C` (`NE555-015`).
/// Held to `u64::MAX` picoseconds where it does not fit.
#[must_use]
pub const fn astable_low_ps(rb_ohms: Ohms, c_pf: Picofarads) -> Picoseconds {
    ln2_rc(rb_ohms.0 as u128, c_pf)
}

/// The astable period, `0.693 (RA + 2 RB) C` (`NE555-015`).
/// Held to `u64::MAX` picoseconds where it does not fit.
#[must_use]
pub const fn astable_period_ps(ra_ohms: Ohms, rb_ohms: Ohms, c_pf: Picofarads) -> Picoseconds {
    ln2_rc(ra_ohms.0 as u128 + 2 * rb_ohms.0 as u128, c_pf)
}

/// The astable output waveform's duty cycle, `1 − RB / (RA + 2 RB)`
/// (`NE555-015`), rounded up: the `RB` share is rounded down. `None` where
/// `ra_ohms` and `rb_ohms` are both zero, where there is no waveform.
#[must_use]
pub const fn astable_output_duty_per_mille(ra_ohms: Ohms, rb_ohms: Ohms) -> Option<PerMille> {
    let rb = rb_ohms.0 as u128;
    match (rb * 1000).checked_div(ra_ohms.0 as u128 + 2 * rb) {
        Some(share) => Some(PerMille(1000 - share as u32)),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_duty_without_resistance() {
        assert_eq!(astable_output_duty_per_mille(Ohms(0), Ohms(0)), None);
        assert_eq!(
            astable_output_duty_per_mille(Ohms(1), Ohms(0)),
            Some(PerMille(1000))
        );
    }

    #[test]
    fn a_duration_too_long_for_a_u64_is_held_at_the_top() {
        let top = Picoseconds(u64::MAX);
        assert_eq!(
            monostable_width_ps(Ohms(u64::MAX), Picofarads(u32::MAX)),
            top
        );
        assert_eq!(
            astable_high_ps(Ohms(u64::MAX), Ohms(u64::MAX), Picofarads(u32::MAX)),
            top
        );
        assert_eq!(astable_low_ps(Ohms(u64::MAX), Picofarads(2)), top);
        assert_eq!(
            astable_period_ps(Ohms(u64::MAX), Ohms(u64::MAX), Picofarads(1)),
            top
        );
        assert_eq!(
            monostable_width_ps(Ohms(u64::MAX / 11), Picofarads(10)),
            Picoseconds(u64::MAX / 11 * 11)
        );
    }

    #[test]
    fn the_idle_inputs_hold_the_flip_flop() {
        for vcc_uv in [5_000_000, 15_000_000, 15_100_000]
            .map(Microvolts)
            .into_iter()
            .chain([SUPPLY_UV.0, SUPPLY_UV.1])
        {
            for held in [Logic::High, Logic::Low, Logic::Undetermined] {
                let part = Ne555::with_state(vcc_uv, RESET_LEVEL_MIN_TYP_MAX_UV.2, held);
                assert_eq!(part.held(Inputs::idle(vcc_uv)), held);
            }
        }
    }

    /// Two-thirds of a supply is exact out to both ends of an `i32`, past
    /// 1 073 741 824 µV, where twice the supply first overflows one.
    #[test]
    fn the_open_control_voltage_computes_at_every_supply() {
        for (vcc, control) in [
            (i32::MAX, 1_431_655_764),
            (1_073_741_824, 715_827_882),
            (i32::MIN, -1_431_655_765),
            (-3, -2),
            (0, 0),
        ] {
            let part = Ne555::new(Microvolts(vcc), RESET_LEVEL_MIN_TYP_MAX_UV.1);
            assert_eq!(part.control_uv(Control::Open), Microvolts(control));
            assert_eq!(
                part.trigger_level_uv(Control::Open),
                Microvolts(control / 2)
            );
        }
    }

    #[test]
    fn at_corner_takes_the_sheets_reset_levels() {
        let vcc_uv = Microvolts(5_000_000);
        for (corner, level) in [
            (Corner::Min, RESET_LEVEL_MIN_TYP_MAX_UV.0),
            (Corner::Typ, RESET_LEVEL_MIN_TYP_MAX_UV.1),
            (Corner::Max, RESET_LEVEL_MIN_TYP_MAX_UV.2),
        ] {
            assert_eq!(Ne555::at_corner(vcc_uv, corner), Ne555::new(vcc_uv, level));
        }
    }
}
