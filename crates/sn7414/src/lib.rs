//! The Texas Instruments SN7414 hex Schmitt-trigger inverter, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The input is a voltage, not a level** (`SN7414-008`). A plain gate of
//! this family is modelled with `bool` inputs, because its sheet states it in
//! a table of two marks. This one's function table
//! (`SN7414-005`) is exactly a plain inverter's, and three further pages say
//! that the table is not the part: the input has two thresholds, which one
//! applies depends on the direction the input is travelling, and between them
//! the part holds the branch it last reached (`SN7414-013`). So [`Inverter`] is
//! a machine, [`Inverter::step`] takes **microvolts**, and there is no `tick`
//! — the sheet specifies no clock and no edge for one to correspond to.
//!
//! **Microvolts, and no float anywhere.** The sheet states its thresholds in
//! tenths of a volt and plots them in millivolts; a microvolt carries both with
//! four decimal places to spare, and an `i32` reaches ±2.1 kV. Nothing here
//! wants a float: a threshold crossing is a comparison.
//!
//! **This crate is built at a corner, not at a value.** `SDLS049C` gives each
//! threshold a minimum, a typical and a maximum (`SN7414-009`, `SN7414-010`) and
//! guarantees only that they are 0.4 V apart (`SN7414-011`). A device is one
//! pair out of that band, so [`Thresholds`] is a parameter of the model and
//! [`Thresholds::at_corner`] gives the three corners a caller with no
//! specimen has any business choosing between.
//!
//! **What the sheet does not say, this crate does not say either.**
//! [`Inverter::outputs`] returns [`Outputs`] of a [`Logic`], and the one way to reach
//! [`Logic::Undetermined`] is `SN7414-OPEN-03`: a part whose input has crossed neither threshold since
//! it was made. The sheet gives no power-on state, so nothing here invents one.
//! An input standing exactly at a threshold has crossed it (`SN7414-OPEN-04`).
//!
//! **The output is a level and not a voltage** (`SN7414-017`). `VOH` and `VOL`
//! are given at one stated load current each and no page carries a source
//! impedance, so a `Y` pin driving anything but those two currents is
//! `SN7414-OPEN-09`. The four figures are here as constants for a caller that
//! wants them; the part drives a branch.
//!
//! # Examples
//!
//! ```
//! use sn7414::{Corner, Inputs, Inverter, Logic, Thresholds};
//! use units::Microvolts;
//!
//! let mut inverter = Inverter::new(Thresholds::at_corner(Corner::Typ));
//! // Rising through `VT+` (1.7 V) puts the output Low.
//! assert_eq!(inverter.step(Inputs { a_uv: Microvolts(2_000_000) }).y, Logic::Low);
//! // Between the thresholds it holds the branch it is on.
//! assert_eq!(inverter.step(Inputs { a_uv: Microvolts(1_200_000) }).y, Logic::Low);
//! // Falling through `VT−` (0.9 V) puts it High.
//! assert_eq!(inverter.step(Inputs { a_uv: Microvolts(800_000) }).y, Logic::High);
//! ```

#![no_std]

use units::Microvolts;

/// How many inverters the package carries (`SN7414-001`).
pub const INVERTER_COUNT: usize = 6;

/// How many pins the package presents (`SN7414-002`).
pub const PIN_COUNT: usize = 14;

/// `VT+`, `(minimum, typical, maximum)`: 1.5 V, 1.7 V and 2 V (`SN7414-009`).
pub const VT_PLUS_MIN_TYP_MAX_UV: (Microvolts, Microvolts, Microvolts) = (
    Microvolts(1_500_000),
    Microvolts(1_700_000),
    Microvolts(2_000_000),
);

/// `VT−`, `(minimum, typical, maximum)`: 0.6 V, 0.9 V and 1.1 V
/// (`SN7414-010`).
pub const VT_MINUS_MIN_TYP_MAX_UV: (Microvolts, Microvolts, Microvolts) = (
    Microvolts(600_000),
    Microvolts(900_000),
    Microvolts(1_100_000),
);

/// The separation of the two thresholds, `(minimum, typical)`: 0.4 V and
/// 0.8 V (`SN7414-011`).
///
/// The minimum is implied by the two rows above, exactly and on both families,
/// which is `SN7414-020`.
pub const HYSTERESIS_MIN_TYP_UV: (Microvolts, Microvolts) =
    (Microvolts(400_000), Microvolts(800_000));

/// `VOH`, `(minimum, typical)`, at `IOH` = −0.8 mA and `VCC` = MIN
/// (`SN7414-017`).
pub const VOH_MIN_TYP_UV: (Microvolts, Microvolts) = (Microvolts(2_400_000), Microvolts(3_400_000));

/// `VOL`, `(typical, maximum)`, at `IOL` = 16 mA and `VCC` = MIN
/// (`SN7414-017`).
pub const VOL_TYP_MAX_UV: (Microvolts, Microvolts) = (Microvolts(200_000), Microvolts(400_000));

/// Which of the sheet's figures for the hysteresis to take
/// ([`Thresholds::at_corner`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Corner {
    /// The least hysteresis.
    Min,
    /// The typical part.
    Typ,
    /// The most hysteresis.
    Max,
}

/// The two input thresholds of one Schmitt-trigger input.
///
/// A device is one pair out of the band `SDLS049C` guarantees, and this type is
/// that pair. The invariant [`Thresholds::new`] holds is the one the sheet
/// guarantees and the model cannot do without: the negative-going threshold is
/// **strictly below** the positive-going one, so no input voltage can satisfy
/// both crossing rules at once (`SN7414-OPEN-04`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Thresholds {
    vt_minus_uv: Microvolts,
    vt_plus_uv: Microvolts,
}

impl Thresholds {
    /// The pair at a corner of the hysteresis, the one figure the sheet
    /// guarantees about the pair:
    ///
    /// - [`Corner::Min`], the least hysteresis this sheet admits: `VT−` at its
    ///   maximum and `VT+` at its minimum, which is `HYSTERESIS_MIN_TYP_UV.0`
    ///   apart and no more.
    /// - [`Corner::Typ`], the typical part: `VT−` 0.9 V and `VT+` 1.7 V
    ///   (`SN7414-009`, `SN7414-010`).
    /// - [`Corner::Max`], the most hysteresis this sheet admits: `VT−` at its
    ///   minimum and `VT+` at its maximum. The sheet prints no maximum for the
    ///   hysteresis row itself, which is `SN7414-020`; this is what its two
    ///   threshold rows imply.
    #[must_use]
    pub const fn at_corner(corner: Corner) -> Self {
        match corner {
            Corner::Min => Self {
                vt_minus_uv: VT_MINUS_MIN_TYP_MAX_UV.2,
                vt_plus_uv: VT_PLUS_MIN_TYP_MAX_UV.0,
            },
            Corner::Typ => Self {
                vt_minus_uv: VT_MINUS_MIN_TYP_MAX_UV.1,
                vt_plus_uv: VT_PLUS_MIN_TYP_MAX_UV.1,
            },
            Corner::Max => Self {
                vt_minus_uv: VT_MINUS_MIN_TYP_MAX_UV.0,
                vt_plus_uv: VT_PLUS_MIN_TYP_MAX_UV.2,
            },
        }
    }

    /// One device's pair of thresholds, or `None` if the hysteresis would not be
    /// positive.
    ///
    /// Nothing here checks the pair against the guaranteed band: a caller with a
    /// specimen in front of it knows better than this crate what its part does,
    /// and [`Thresholds::is_guaranteed`] is the question asked
    /// separately. What is refused is a pair no Schmitt trigger has, which
    /// `SN7414-011` guarantees never arrives from this sheet.
    #[must_use]
    pub const fn new(vt_minus_uv: Microvolts, vt_plus_uv: Microvolts) -> Option<Self> {
        if vt_minus_uv.0 >= vt_plus_uv.0 {
            return None;
        }
        Some(Self {
            vt_minus_uv,
            vt_plus_uv,
        })
    }

    /// `VT−`, the negative-going threshold.
    #[must_use]
    pub const fn vt_minus_uv(self) -> Microvolts {
        self.vt_minus_uv
    }

    /// `VT+`, the positive-going threshold.
    #[must_use]
    pub const fn vt_plus_uv(self) -> Microvolts {
        self.vt_plus_uv
    }

    /// The separation of the two, which the sheet names `VT+ − VT−` and gives
    /// a row of its own (`SN7414-011`), saturating at `i32::MAX` microvolts
    /// for a pair more than 2.1 kV apart.
    #[must_use]
    pub const fn hysteresis_uv(self) -> Microvolts {
        Microvolts(self.vt_plus_uv.0.saturating_sub(self.vt_minus_uv.0))
    }

    /// Whether the sheet guarantees a pair like this one: both thresholds
    /// inside the bands `SDLS049C` gives for the `SNx414`, and their
    /// separation inside the hysteresis row's own minimum.
    ///
    /// The third test is redundant on this sheet and is made anyway: it is
    /// implied by the two bands exactly (`SN7414-020`), and a printing whose
    /// rows stopped agreeing is what would make it bite.
    #[must_use]
    pub const fn is_guaranteed(self) -> bool {
        self.vt_plus_uv.0 >= VT_PLUS_MIN_TYP_MAX_UV.0.0
            && self.vt_plus_uv.0 <= VT_PLUS_MIN_TYP_MAX_UV.2.0
            && self.vt_minus_uv.0 >= VT_MINUS_MIN_TYP_MAX_UV.0.0
            && self.vt_minus_uv.0 <= VT_MINUS_MIN_TYP_MAX_UV.2.0
            && self.hysteresis_uv().0 >= HYSTERESIS_MIN_TYP_UV.0.0
    }
}

/// The voltage the outside world holds one inverter's `A` pin at.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// The `A` pin. `Default` holds it at 0 V.
    pub a_uv: Microvolts,
}

impl Inputs {
    /// The pin idle: `A` held at 0 V.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            a_uv: Microvolts(0),
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// The level an inverter drives its `Y` pin to, which is the branch of the
/// transfer characteristic it is on (`SN7414-012`).
///
/// The third value is not a level and is not a voltage: it is a part whose input
/// has crossed neither threshold, which `SDLS049C` gives no state for at all
/// (`SN7414-OPEN-03`).
///
/// `High` is the upper branch, reached by an input falling through `VT−`;
/// `Low` the lower, reached by an input rising through `VT+`.
pub use signal::Logic;

/// What one inverter drives: its `Y` pin.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Outputs {
    /// The `Y` pin. `Default` is [`Logic::Undetermined`], power-up.
    pub y: Logic,
}

/// One of the six Schmitt-trigger inverters (`SN7414-001`), with its thresholds
/// and the branch it is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inverter {
    thresholds: Thresholds,
    level: Logic,
}

impl Inverter {
    /// An inverter on neither branch, which is where `SDLS049C` leaves a part it
    /// has not said anything about yet (`SN7414-OPEN-03`).
    #[must_use]
    pub const fn new(thresholds: Thresholds) -> Self {
        Self {
            thresholds,
            level: Logic::Undetermined,
        }
    }

    /// An inverter placed on a branch, for a caller that knows what its board
    /// has been holding the input pin at.
    #[must_use]
    pub const fn with_state(thresholds: Thresholds, level: Logic) -> Self {
        Self { thresholds, level }
    }

    /// This inverter's two thresholds.
    #[must_use]
    pub const fn thresholds(self) -> Thresholds {
        self.thresholds
    }

    /// The branch it is on, which is what it drives its output pin to:
    /// [`Logic::Undetermined`] on neither.
    #[must_use]
    pub const fn outputs(self) -> Outputs {
        Outputs { y: self.level }
    }

    /// The branch this input voltage would leave it on, without moving it.
    ///
    /// This is the whole of `SN7414-012` and `SN7414-013`: at or above `VT+` the
    /// low branch, at or below `VT−` the high branch, and between them whatever
    /// it was already on. The two rules cannot both fire, because
    /// [`Thresholds::new`] refuses a pair that would let them.
    #[must_use]
    pub const fn held(self, inputs: Inputs) -> Logic {
        if inputs.a_uv.0 >= self.thresholds.vt_plus_uv.0 {
            Logic::Low
        } else if inputs.a_uv.0 <= self.thresholds.vt_minus_uv.0 {
            Logic::High
        } else {
            self.level
        }
    }

    /// Hold the input pin at this voltage, and read the output pin.
    ///
    /// The whole of what the part does is here. There is no time in it: the
    /// sheet's own transfer characteristic draws both transitions vertical
    /// (`SN7414-012`), and what an output does between the crossing and the
    /// settled level is on no page of it.
    pub const fn step(&mut self, inputs: Inputs) -> Outputs {
        self.level = self.held(inputs);
        self.outputs()
    }
}

/// The package: six inverters sharing nothing but the two supply pins
/// (`SN7414-001`, `SN7414-019`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Sn7414 {
    inverters: [Inverter; INVERTER_COUNT],
}

impl Sn7414 {
    /// A package of six inverters given one at a time, each with its own
    /// thresholds and its own branch. Nothing on the sheet says the six share
    /// a threshold and nothing says they do not (`SN7414-008`):
    /// `[Inverter::new(thresholds); INVERTER_COUNT]` is six that do.
    #[must_use]
    pub const fn with_inverters(inverters: [Inverter; INVERTER_COUNT]) -> Self {
        Self { inverters }
    }

    /// Inverter `index`, numbered from 0 for the channel the sheet calls 1.
    ///
    /// # Panics
    ///
    /// If `index` is not below [`INVERTER_COUNT`]. The package has six and a
    /// board cannot reach a seventh.
    #[must_use]
    pub fn inverter(&self, index: usize) -> Inverter {
        self.inverters[index]
    }

    /// What the six output pins are driven to now.
    #[must_use]
    pub fn outputs(&self) -> [Outputs; INVERTER_COUNT] {
        self.inverters.map(Inverter::outputs)
    }

    /// Hold all six input pins at these voltages, and read all six output pins.
    pub fn step(&mut self, inputs: [Inputs; INVERTER_COUNT]) -> [Outputs; INVERTER_COUNT] {
        for (inverter, inputs) in self.inverters.iter_mut().zip(inputs) {
            let _ = inverter.step(inputs);
        }
        self.outputs()
    }

    /// Hold one input pin at this voltage, and read its own output pin.
    ///
    /// The other five are not touched, which is `SN7414-019` and is what makes
    /// this method a different thing from [`Sn7414::step`] rather than a
    /// convenience over it.
    ///
    /// # Panics
    ///
    /// If `index` is not below [`INVERTER_COUNT`].
    pub fn step_one(&mut self, index: usize, inputs: Inputs) -> Outputs {
        self.inverters[index].step(inputs)
    }
}
