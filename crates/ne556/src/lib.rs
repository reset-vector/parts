//! The Texas Instruments NE556 dual precision timer, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **Two timers on one supply.** The package holds two independent timing
//! circuits of the `555` type (`NE556-004`, `NE556-005`) that share `Vcc`
//! and `GND` and nothing else (`NE556-002`). So [`Ne556`] holds one supply and
//! two flip-flops, [`Ne556::timer`] reads one, and [`Ne556::step`] moves each
//! timer by its own pins and never by the other's.
//!
//! **The timing capacitors are on the far side of a pin.** Each timer compares
//! the voltages on its `THRESHOLD` and `TRIGGER` against two levels, holds one
//! bit, and drives its `OUTPUT` and `DISCHARGE` from it (`NE556-006`). The
//! crate takes **microvolts** at four pins a timer and returns two levels; the
//! ramp is the caller's capacitor to step. [`monostable_width_ps`],
//! [`astable_high_ps`] and [`astable_low_ps`] are the sheet's equations for
//! what that ramp does (`NE556-018`, `NE556-019`).
//!
//! **The levels are the control pin's.** The threshold level is the control
//! voltage and the trigger level is half of it (`NE556-009`); with the pin open
//! the control voltage is two-thirds of the supply (`NE556-014`).
//!
//! **Three priorities.** Reset overrides everything (`NE556-010`); the trigger
//! prevails over the threshold while both assert (`NE556-012`); and a trigger
//! that finds the flip-flop already set changes nothing (`NE556-013`).
//!
//! **What the sheet does not say, this crate does not say either.** Each
//! flip-flop starts on neither branch and its output is
//! [`Logic::Undetermined`] until a comparator or the reset decides it (`NE556-OPEN-02`); the comparisons are
//! strict (`NE556-OPEN-03`); there is no time in it (`NE556-OPEN-05`); and one
//! timer's switching reaches the other through no shared rail
//! (`NE556-OPEN-07`).
//!
//! # Examples
//!
//! ```
//! use ne556::{Corner, Inputs, Logic, Ne556, Timer};
//! use units::Microvolts;
//!
//! let mut package = Ne556::with_timers(Microvolts(5_000_000), [Timer::at_corner(Corner::Typ); 2]);
//! // Timer 1's trigger below a third of the supply sets it; timer 2 is shown
//! // the idle levels and stays undecided.
//! let idle = Inputs::idle(Microvolts(5_000_000));
//! let triggered = Inputs { trigger_uv: Microvolts(1_000_000), ..idle };
//! let outputs = package.step([triggered, idle]);
//! assert_eq!(outputs[0].output, Logic::High);
//! assert_eq!(outputs[1].output, Logic::Undetermined);
//! ```

#![no_std]

use units::{Microvolts, Ohms, PerMille, Picoamps, Picofarads, Picoseconds};

/// How many pins the package presents (`NE556-001`).
pub const PIN_COUNT: usize = 14;

/// The supply pin, shared by both timers (`NE556-001`, `NE556-002`).
pub const VCC_PIN: u8 = 14;

/// The ground pin, shared by both timers (`NE556-001`, `NE556-002`).
pub const GND_PIN: u8 = 7;

/// The recommended supply for this member, lowest and highest: 4.5 V to
/// 16 V (`NE556-003`).
pub const SUPPLY_UV: (Microvolts, Microvolts) = (Microvolts(4_500_000), Microvolts(16_000_000));

/// The absolute maximum supply: 18 V (`NE556-003`).
pub const SUPPLY_ABSOLUTE_MAX_UV: Microvolts = Microvolts(18_000_000);

/// The reset level, minimum, typical and maximum: 0.4 V, 0.7 V and 1 V
/// (`NE556-010`).
pub const RESET_LEVEL_MIN_TYP_MAX_UV: (Microvolts, Microvolts, Microvolts) = (
    Microvolts(400_000),
    Microvolts(700_000),
    Microvolts(1_000_000),
);

/// The threshold input's current, typical and maximum (`NE556-015`).
pub const THRESHOLD_CURRENT_TYP_MAX_PA: (Picoamps, Picoamps) =
    (Picoamps(30_000), Picoamps(100_000));

/// The trigger input's current, typical (`NE556-015`).
pub const TRIGGER_CURRENT_TYP_PA: Picoamps = Picoamps(500_000);

/// The reset input's current, typical (`NE556-015`).
pub const RESET_CURRENT_TYP_PA: Picoamps = Picoamps(100_000_000);

/// The recommended output current, either way: 200 mA (`NE556-016`).
/// The third feature bullet says 150; which bounds a fitted part is
/// `NE556-OPEN-08`.
pub const OUTPUT_CURRENT_MAX_PA: Picoamps = Picoamps(200_000_000_000);

/// The absolute maximum output current, either way: 225 mA (`NE556-016`).
pub const OUTPUT_CURRENT_ABSOLUTE_MAX_PA: Picoamps = Picoamps(225_000_000_000);

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
/// 5 V (`NE556-016`).
pub const VOL_ROWS: [VolRow; 5] = [
    vol(15_000_000, 10_000_000_000, 100_000, Some(250_000)),
    vol(15_000_000, 50_000_000_000, 400_000, Some(750_000)),
    vol(15_000_000, 100_000_000_000, 2_000_000, Some(2_750_000)),
    vol(15_000_000, 200_000_000_000, 2_500_000, None),
    vol(5_000_000, 5_000_000_000, 250_000, Some(350_000)),
];

/// The high-level output voltage rows for this member, at `Vcc` 15 V and
/// then 5 V (`NE556-016`).
pub const VOH_ROWS: [VohRow; 3] = [
    voh(15_000_000, 100_000_000_000, Some(12_750_000), 13_300_000),
    voh(15_000_000, 200_000_000_000, None, 12_500_000),
    voh(5_000_000, 100_000_000_000, Some(2_750_000), 3_300_000),
];

/// One supply current row: the current per timer with no load.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct IccPerTimerRow {
    /// The supply the row is stated at, `Vcc`.
    pub test_supply_uv: Microvolts,
    /// The level of the `OUTPUT` pin the row is stated with.
    pub output: Level,
    /// The current, typical and maximum.
    pub typ_max_pa: (Picoamps, Picoamps),
}

const fn icc(test_supply_uv: i32, output: Level, typ_pa: i64, max_pa: i64) -> IccPerTimerRow {
    IccPerTimerRow {
        test_supply_uv: Microvolts(test_supply_uv),
        output,
        typ_max_pa: (Picoamps(typ_pa), Picoamps(max_pa)),
    }
}

/// The supply current per timer with no load, at `Vcc` 5 V and 15 V, the
/// output low and then high at each (`NE556-017`).
pub const ICC_PER_TIMER_ROWS: [IccPerTimerRow; 4] = [
    icc(5_000_000, Level::Low, 3_000_000_000, 6_000_000_000),
    icc(5_000_000, Level::High, 2_000_000_000, 5_000_000_000),
    icc(15_000_000, Level::Low, 10_000_000_000, 14_000_000_000),
    icc(15_000_000, Level::High, 9_000_000_000, 13_000_000_000),
];

/// How many timers the package carries (`NE556-004`).
pub const TIMER_COUNT: usize = 2;

/// Where one timer's six pins are on the package.
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

/// Each timer's six pins, timer 1 (pins 1 to 6) first, timer 2 (pins 8 to
/// 13) second (`NE556-001`).
pub const TIMER_PINS: [TimerPinout; TIMER_COUNT] = [
    TimerPinout {
        discharge: 1,
        threshold: 2,
        control: 3,
        reset: 4,
        output: 5,
        trigger: 6,
    },
    TimerPinout {
        discharge: 13,
        threshold: 12,
        control: 11,
        reset: 10,
        output: 9,
        trigger: 8,
    },
];

/// What a `CONTROL VOLTAGE` pin is doing (`NE556-009`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Control {
    /// Left open: the divider sets it at two-thirds of the supply
    /// (`NE556-014`).
    Open,
    /// Driven by the board to this voltage.
    Driven {
        /// The pin's voltage.
        level_uv: Microvolts,
    },
}

/// One timer's four input pins, as voltages above `GND`.
///
/// There is no `Default`: no one level of `TRIGGER` is both at or below every
/// `Vcc` (`NE556-003`) and above every trigger level, which is a third of
/// `Vcc` with the control open (`NE556-009`, `NE556-014`). [`Inputs::idle`]
/// takes the supply.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `THRESHOLD`.
    pub threshold_uv: Microvolts,
    /// `TRIGGER`.
    pub trigger_uv: Microvolts,
    /// `CONTROL VOLTAGE`.
    pub control: Control,
    /// `RESET`.
    pub reset_uv: Microvolts,
}

impl Inputs {
    /// The timer idle at a supply of `vcc_uv`: `CONTROL VOLTAGE` open,
    /// `THRESHOLD` at 0 V, and `TRIGGER` and `RESET` tied to `Vcc`, above the
    /// trigger level and above the highest reset level (`NE556-010`), so no
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
/// pulled-up line at; and a timer flip-flop's `Q`, which its `OUTPUT` pin
/// follows (`NE556-006`).
///
/// On `DISCHARGE`, `Low` is the transistor conducting to `GND` and `High` the
/// transistor off and the line at its pull-up; of `Q`, `Low` is the
/// flip-flop reset and `High` set. `Undetermined` is a timer whose flip-flop
/// nothing has decided since power-up (`NE556-OPEN-02`).
pub use signal::Logic;

/// A pin's level where it is always decided, in a table row's field.
pub use signal::Level;

/// One timer's two output pins.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `OUTPUT`.
    pub output: Logic,
    /// `DISCHARGE`, an open collector: `Low` (conducting) exactly while the
    /// output is low, otherwise `High`, which is the pull-up's (`NE556-011`).
    pub discharge: Logic,
}

impl Outputs {
    /// The two output pins a flip-flop at `q` drives: `OUTPUT` at `Q`,
    /// and the open-collector `DISCHARGE` leaving a pulled-up line at the same
    /// level, `Low` conducting and `High` off (`NE556-011`).
    #[must_use]
    const fn at_q(q: Logic) -> Self {
        Self {
            output: q,
            discharge: q,
        }
    }
}

/// Which of the sheet's reset levels [`Timer::at_corner`] takes
/// (`NE556-010`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Corner {
    /// The minimum, 0.4 V.
    Min,
    /// The typical, 0.7 V.
    Typ,
    /// The maximum, 1 V.
    Max,
}

/// One timer: its reset level and its flip-flop.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Timer {
    reset_level_uv: Microvolts,
    q: Logic,
}

impl Timer {
    /// A timer at this reset level, a specimen's or a corner of
    /// [`RESET_LEVEL_MIN_TYP_MAX_UV`], with its flip-flop undecided.
    #[must_use]
    pub const fn new(reset_level_uv: Microvolts) -> Self {
        Self::with_state(reset_level_uv, Logic::Undetermined)
    }

    /// A timer at the sheet's reset level at `corner` (`NE556-010`), with
    /// its flip-flop undecided.
    #[must_use]
    pub const fn at_corner(corner: Corner) -> Self {
        Self::new(match corner {
            Corner::Min => RESET_LEVEL_MIN_TYP_MAX_UV.0,
            Corner::Typ => RESET_LEVEL_MIN_TYP_MAX_UV.1,
            Corner::Max => RESET_LEVEL_MIN_TYP_MAX_UV.2,
        })
    }

    /// A timer whose flip-flop stands at `q`, for a caller that knows what its board has
    /// done to the part already.
    #[must_use]
    pub const fn with_state(reset_level_uv: Microvolts, q: Logic) -> Self {
        Self { reset_level_uv, q }
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

    /// The two output pins now.
    #[must_use]
    pub const fn outputs(self) -> Outputs {
        Outputs::at_q(self.q)
    }
}

/// The package: one supply, and each timer's reset level and flip-flop.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Ne556 {
    vcc_uv: Microvolts,
    timers: [Timer; TIMER_COUNT],
}

impl Ne556 {
    /// A package at this supply with the two timers a caller sets up, timer
    /// 1's first.
    ///
    /// Any supply is taken: [`Ne556::control_uv`] computes at every one.
    #[must_use]
    pub const fn with_timers(vcc_uv: Microvolts, timers: [Timer; TIMER_COUNT]) -> Self {
        Self { vcc_uv, timers }
    }

    /// The supply.
    #[must_use]
    pub const fn supply_uv(self) -> Microvolts {
        self.vcc_uv
    }

    /// One of the two timers, index 0 being the one the sheet numbers 1.
    ///
    /// # Panics
    ///
    /// If `index` is not 0 or 1.
    #[must_use]
    pub const fn timer(self, index: usize) -> Timer {
        self.timers[index]
    }

    /// The control voltage — the pin's own if driven, two-thirds of the supply
    /// if open (`NE556-009`, `NE556-014`), rounded towards zero.
    ///
    /// Two-thirds of a supply is computed wide, so it is exact for every
    /// supply [`Ne556::with_timers`] takes, out to `i32::MIN` and `i32::MAX`
    /// microvolts, and fits the result, being smaller.
    #[must_use]
    pub const fn control_uv(self, control: Control) -> Microvolts {
        match control {
            Control::Open => Microvolts((self.vcc_uv.0 as i64 * 2 / 3) as i32),
            Control::Driven { level_uv } => level_uv,
        }
    }

    /// The threshold level, which is the control voltage (`NE556-009`).
    #[must_use]
    pub const fn threshold_level_uv(self, control: Control) -> Microvolts {
        self.control_uv(control)
    }

    /// The trigger level, which is half the control voltage (`NE556-009`).
    #[must_use]
    pub const fn trigger_level_uv(self, control: Control) -> Microvolts {
        Microvolts(self.control_uv(control).0 / 2)
    }

    /// Both timers' two output pins now, timer 1's first.
    #[must_use]
    pub const fn outputs(self) -> [Outputs; TIMER_COUNT] {
        [self.timers[0].outputs(), self.timers[1].outputs()]
    }

    /// The `Q`s these pin voltages, timer 1's first, would leave the two
    /// flip-flops at, without moving them. Each timer sees only its own pins
    /// (`NE556-005`).
    ///
    /// The whole of `NE556-007`, `NE556-008`, `NE556-010` and `NE556-012` is
    /// here: reset low resets whatever else is true; otherwise a trigger below
    /// its level sets, whatever the threshold; otherwise a threshold above its
    /// level resets; otherwise the flip-flop holds. [`Inputs::idle`] holds
    /// a timer, so a caller moving one timer shows the other that.
    #[must_use]
    pub const fn held(self, pins: [Inputs; TIMER_COUNT]) -> [Logic; TIMER_COUNT] {
        [self.held_one(0, pins[0]), self.held_one(1, pins[1])]
    }

    const fn held_one(self, index: usize, pins: Inputs) -> Logic {
        let timer = self.timers[index];
        if pins.reset_uv.0 <= timer.reset_level_uv.0 {
            Logic::Low
        } else if pins.trigger_uv.0 < self.trigger_level_uv(pins.control).0 {
            Logic::High
        } else if pins.threshold_uv.0 > self.threshold_level_uv(pins.control).0 {
            Logic::Low
        } else {
            timer.q
        }
    }

    /// Hold both timers' input pins at these voltages, timer 1's first, and
    /// read both timers' output pins. Each timer sees only its own pins
    /// (`NE556-005`).
    ///
    /// There is no time in it: the outputs are what the flip-flops now hold,
    /// and what a specimen does between the comparison and the settled level
    /// is `NE556-OPEN-05`.
    pub const fn step(&mut self, pins: [Inputs; TIMER_COUNT]) -> [Outputs; TIMER_COUNT] {
        let held = self.held(pins);
        self.timers[0].q = held[0];
        self.timers[1].q = held[1];
        self.outputs()
    }
}

/// The monostable output pulse width, `1.1 RA C`, from a resistor and a
/// capacitor (`NE556-018`).
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

/// The astable output's high duration, `0.693 (RA + RB) C` (`NE556-019`).
/// Held to `u64::MAX` picoseconds where it does not fit.
#[must_use]
pub const fn astable_high_ps(ra_ohms: Ohms, rb_ohms: Ohms, c_pf: Picofarads) -> Picoseconds {
    ln2_rc(ra_ohms.0 as u128 + rb_ohms.0 as u128, c_pf)
}

/// The astable output's low duration, `0.693 RB C` (`NE556-019`).
/// Held to `u64::MAX` picoseconds where it does not fit.
#[must_use]
pub const fn astable_low_ps(rb_ohms: Ohms, c_pf: Picofarads) -> Picoseconds {
    ln2_rc(rb_ohms.0 as u128, c_pf)
}

/// The astable period, `0.693 (RA + 2 RB) C` (`NE556-019`).
/// Held to `u64::MAX` picoseconds where it does not fit.
#[must_use]
pub const fn astable_period_ps(ra_ohms: Ohms, rb_ohms: Ohms, c_pf: Picofarads) -> Picoseconds {
    ln2_rc(ra_ohms.0 as u128 + 2 * rb_ohms.0 as u128, c_pf)
}

/// The astable output waveform's duty cycle, `1 − RB / (RA + 2 RB)`
/// (`NE556-019`), rounded up: the `RB` share is rounded down. `None` where
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
                let part = Ne556::with_timers(
                    vcc_uv,
                    [Timer::with_state(RESET_LEVEL_MIN_TYP_MAX_UV.2, held); 2],
                );
                assert_eq!(part.held([Inputs::idle(vcc_uv); 2]), [held; 2]);
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
            let part = Ne556::with_timers(Microvolts(vcc), [Timer::at_corner(Corner::Typ); 2]);
            assert_eq!(part.control_uv(Control::Open), Microvolts(control));
            assert_eq!(
                part.trigger_level_uv(Control::Open),
                Microvolts(control / 2)
            );
        }
    }

    #[test]
    fn at_corner_takes_the_sheets_reset_levels() {
        for (corner, level) in [
            (Corner::Min, RESET_LEVEL_MIN_TYP_MAX_UV.0),
            (Corner::Typ, RESET_LEVEL_MIN_TYP_MAX_UV.1),
            (Corner::Max, RESET_LEVEL_MIN_TYP_MAX_UV.2),
        ] {
            assert_eq!(Timer::at_corner(corner), Timer::new(level));
        }
    }
}
