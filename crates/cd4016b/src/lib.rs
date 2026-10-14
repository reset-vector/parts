//! The Texas Instruments CD4016B quad bilateral switch, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **A switch is a resistance or an open circuit** (`CD4016B-004`,
//! `CD4016B-006`, `CD4016B-009`). The sheet states the part as an on-state
//! resistance into a stated load and an off-state leakage, never as a
//! voltage, because the voltage across a closed switch belongs to the network
//! on both sides of it. So [`Switch::drive`] returns [`State::Off`] or
//! [`State::On`] with an on-resistance in ohms, and nothing here returns a
//! voltage.
//!
//! **The on-resistance depends on where the signal sits** (`CD4016B-007`),
//! which is what separates this part from the `CD4066B` in the `cd4066b` crate:
//! at 10 V and 15 V the sheet prints one maximum with the signal at a rail
//! and a larger one with it in a band about mid-supply, and at 5 V one
//! figure with no qualifier. A [`Switch`] is built with a rail figure and,
//! where it has one, a mid-band figure; [`Switch::drive`] returns the rail
//! figure whatever the signal, and [`Switch::drive_at_signal`] picks the rail
//! or the mid-band figure by where the signal is, and returns
//! [`State::Undetermined`] between them, where the sheet has a curve and no
//! number (`CD4016B-OPEN-03`).
//!
//! **The control pin is a voltage that resolves to a level** (`CD4016B-005`),
//! against a low maximum and a high minimum the sheet gives. Between the two
//! the sheet says nothing (`CD4016B-OPEN-02`) and [`Switch::control_level`]
//! returns [`Logic::Undetermined`] there rather than pick a side.
//!
//! **There is no state.** Every method here is a function of its arguments
//! and there is no `tick`; the package is evaluated by [`Cd4016b::drive`].
//!
//! **Microvolts and ohms, and no float anywhere.**
//!
//! # Examples
//!
//! ```
//! use cd4016b::{Cd4016b, Corner, Inputs, State, Supply, Switch, SWITCH_COUNT};
//! use units::{Microvolts, Ohms};
//!
//! let switch = Switch::at_corner(Supply::Vdd10V, Corner::Max);
//! let package = Cd4016b::with_switches([switch; SWITCH_COUNT]);
//! let mut inputs = Inputs::default();
//! inputs.control_uv[0] = Microvolts(10_000_000);
//! assert_eq!(package.drive(inputs)[0], State::On { r_on_ohms: Ohms(660) });
//! // With the signal in the mid-supply band, the band's maximum.
//! let mid = switch.drive_at_signal(Microvolts(10_000_000), Microvolts(5_000_000));
//! assert_eq!(mid, State::On { r_on_ohms: Ohms(2000) });
//! ```

#![no_std]

use units::{Celsius, Microvolts, Ohms, Picoamps, Picoseconds, TenthPicofarads};

/// How many switches the package carries (`CD4016B-001`).
pub const SWITCH_COUNT: usize = 4;

/// How many pins the package presents (`CD4016B-002`).
pub const PIN_COUNT: usize = 14;

/// `Iis`, the off-state leakage through one switch, `(typical, maximum)`, at
/// 25 °C with [`IIS_TEST_SUPPLY_UV`] across it: the maximum is ±0.1 μA
/// (`CD4016B-009`).
pub const IIS_TYP_MAX_PA: (Picoamps, Picoamps) = (Picoamps(100), Picoamps(100_000));

/// The supply the two `Iis` figures are stated at, with it all across the
/// switch (`CD4016B-009`).
pub const IIS_TEST_SUPPLY_UV: Microvolts = Microvolts(18_000_000);

/// The control pin's input impedance, typical: 10¹² Ω (`CD4016B-012`).
pub const CONTROL_INPUT_IMPEDANCE_TYP_OHMS: Ohms = Ohms(1_000_000_000_000);

/// `CIN`, the control pin's input capacitance, `(typical, maximum)`: 5 pF and
/// 7.5 pF (`CD4016B-012`).
pub const CIN_TYP_MAX_TENTH_PF: (TenthPicofarads, TenthPicofarads) =
    (TenthPicofarads(50), TenthPicofarads(75));

/// One column of the `VILC` row: the highest control voltage that is a low
/// level at a temperature, one figure for every supply (`CD4016B-005`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct VilcRow {
    /// The temperature the figure is stated at.
    pub temperature_c: Celsius,
    /// `VILC` maximum there, above `VSS`.
    pub max_uv: Microvolts,
}

/// The `VILC` row, coldest column first (`CD4016B-005`). [`Switch::control_level`]
/// uses the 25 °C figure, 0.7 V; the others are here for a consumer whose
/// cabinet runs cold or hot.
pub const VILC_ROWS: [VilcRow; 5] = [
    VilcRow {
        temperature_c: Celsius(-55),
        max_uv: Microvolts(900_000),
    },
    VilcRow {
        temperature_c: Celsius(-40),
        max_uv: Microvolts(900_000),
    },
    VilcRow {
        temperature_c: Celsius(25),
        max_uv: Microvolts(700_000),
    },
    VilcRow {
        temperature_c: Celsius(85),
        max_uv: Microvolts(400_000),
    },
    VilcRow {
        temperature_c: Celsius(125),
        max_uv: Microvolts(400_000),
    },
];

/// The `VILC` figure [`Switch::control_level`] uses: the 25 °C column.
const VILC_MAX_UV: i32 = VILC_ROWS[2].max_uv.0;

/// The recommended supply range, `(min, max)`: 3 V to 18 V (`CD4016B-011`).
pub const SUPPLY_UV: (Microvolts, Microvolts) = (Microvolts(3_000_000), Microvolts(18_000_000));

/// The absolute maximum supply: 20 V (`CD4016B-011`).
pub const SUPPLY_ABSOLUTE_MAX_UV: Microvolts = Microvolts(20_000_000);

/// The three single supplies the electrical tables are stated at
/// (`CD4016B-005` to `CD4016B-008`, `CD4016B-013`).
///
/// Every figure in the sheet's tables is a row per supply, so the supply is
/// the first thing a switch is built at. The split-supply rows the sheet also
/// prints (±2.5 V, ±5 V, ±7.5 V) are [`Supply::split_r_on_typ_max_ohms`], and
/// no switch is built at them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Supply {
    /// `VDD − VSS` = 5 V.
    Vdd5V,
    /// `VDD − VSS` = 10 V.
    Vdd10V,
    /// `VDD − VSS` = 15 V.
    Vdd15V,
}

impl Supply {
    /// The three, in the order the tables print them.
    pub const ALL: [Self; 3] = [Self::Vdd5V, Self::Vdd10V, Self::Vdd15V];

    /// `VDD − VSS`.
    #[must_use]
    pub const fn supply_uv(self) -> Microvolts {
        Microvolts(match self {
            Self::Vdd5V => 5_000_000,
            Self::Vdd10V => 10_000_000,
            Self::Vdd15V => 15_000_000,
        })
    }

    /// The on-resistance with the signal at a rail, `(typical, maximum)` at
    /// 25 °C (`CD4016B-006`).
    #[must_use]
    pub const fn r_on_typ_max_ohms(self) -> (Ohms, Ohms) {
        let (typ, max) = match self {
            Self::Vdd5V => (580, 7000),
            Self::Vdd10V => (250, 660),
            Self::Vdd15V => (200, 400),
        };
        (Ohms(typ), Ohms(max))
    }

    /// The mid-supply-band row: the on-resistance with the signal in the band,
    /// maximum at 25 °C, and the band itself (`CD4016B-007`). `None` at 5 V,
    /// where the sheet prints no such row.
    #[must_use]
    pub const fn mid_signal_row(self) -> Option<MidSignalRow> {
        match self {
            Self::Vdd5V => None,
            Self::Vdd10V => Some(MidSignalRow {
                r_on_max_ohms: Ohms(2000),
                band_range_uv: (Microvolts(4_750_000), Microvolts(5_750_000)),
            }),
            Self::Vdd15V => Some(MidSignalRow {
                r_on_max_ohms: Ohms(850),
                band_range_uv: (Microvolts(7_250_000), Microvolts(7_750_000)),
            }),
        }
    }

    /// `ΔRON`, the on-resistance difference between any two switches of one
    /// package, typical (`CD4016B-008`).
    #[must_use]
    pub const fn delta_r_on_typ_ohms(self) -> Ohms {
        Ohms(match self {
            Self::Vdd5V => 15,
            Self::Vdd10V => 10,
            Self::Vdd15V => 5,
        })
    }

    /// The on-resistance the sheet prints for the split supply of the same
    /// total, `(typical, maximum)` at 25 °C (`CD4016B-006`): ±2.5 V, ±5 V and
    /// ±7.5 V for 5 V, 10 V and 15 V. No switch here is built at a split
    /// supply.
    ///
    /// The ±5 V row's maximum is 580 Ω where the 10 V single-supply row's is
    /// 660 Ω; the sheet does not say why.
    #[must_use]
    pub const fn split_r_on_typ_max_ohms(self) -> (Ohms, Ohms) {
        let (typ, max) = match self {
            Self::Vdd5V => (520, 30_000),
            Self::Vdd10V => (250, 580),
            Self::Vdd15V => (200, 280),
        };
        (Ohms(typ), Ohms(max))
    }

    /// `VIHC`, the lowest control voltage that is a high level, above `VSS`
    /// (`CD4016B-005`). A method because it scales with the supply; the low
    /// level's figure does not, and is [`VILC_ROWS`].
    #[must_use]
    pub const fn vihc_min_uv(self) -> Microvolts {
        Microvolts(match self {
            Self::Vdd5V => 3_500_000,
            Self::Vdd10V => 7_000_000,
            Self::Vdd15V => 11_000_000,
        })
    }

    /// The turn-on propagation delay from the control edge, `(typical,
    /// maximum)` at 25 °C (`CD4016B-013`). The crate does not spend it; it is
    /// here for a caller that wants to know what is being treated as
    /// instantaneous.
    #[must_use]
    pub const fn control_to_output_tpd_typ_max_ps(self) -> (Picoseconds, Picoseconds) {
        let (typ, max) = match self {
            Self::Vdd5V => (35_000, 70_000),
            Self::Vdd10V => (20_000, 40_000),
            Self::Vdd15V => (15_000, 30_000),
        };
        (Picoseconds(typ), Picoseconds(max))
    }

    /// `tpd`, the signal-input-to-output propagation delay with the switch on,
    /// `(typical, maximum)` at 25 °C (`CD4016B-013`).
    #[must_use]
    pub const fn signal_to_output_tpd_typ_max_ps(self) -> (Picoseconds, Picoseconds) {
        let (typ, max) = match self {
            Self::Vdd5V => (40_000, 100_000),
            Self::Vdd10V => (20_000, 40_000),
            Self::Vdd15V => (15_000, 30_000),
        };
        (Picoseconds(typ), Picoseconds(max))
    }
}

/// The sheet's mid-supply-band row at one supply ([`Supply::mid_signal_row`],
/// `CD4016B-007`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct MidSignalRow {
    /// The on-resistance with the signal inside the band, maximum at 25 °C.
    pub r_on_max_ohms: Ohms,
    /// The band's lower and upper edges above `VSS`, `(min, max)`.
    pub band_range_uv: (Microvolts, Microvolts),
}

/// Which of the sheet's figures a switch is built at ([`Switch::at_corner`]):
/// the rail figure of `CD4016B-006` at this corner, and the mid-band figure of
/// `CD4016B-007` at the same corner where the sheet prints one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Corner {
    /// The 25 °C typical. The sheet prints no mid-band typical.
    Typ,
    /// The 25 °C maximum.
    Max,
}

/// What a control voltage resolves to ([`Switch::control_level`]): `Low` at
/// or below `VILC`, `High` at or above `VIHC`, and `Undetermined` between the
/// two, where the sheet gives no level.
pub use signal::Logic;

/// The levels the outside world holds the four control pins at.
///
/// `Default` holds every control pin at `VSS`, every switch off.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// The control pins, `A` first, each above `VSS` (`CD4016B-005`).
    pub control_uv: [Microvolts; SWITCH_COUNT],
}

impl Inputs {
    /// Every control at `VSS`, every switch off.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            control_uv: [Microvolts(0); SWITCH_COUNT],
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// What one switch is between its two signal pins (`CD4016B-004`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum State {
    /// Open: nothing passes but the leakage of `CD4016B-009`, which is not
    /// returned because the sheet gives it as a bound and not a path.
    Off,
    /// Closed: a resistance between the two signal pins.
    On {
        /// The on-resistance.
        r_on_ohms: Ohms,
    },
    /// A state the sheet does not determine.
    Undetermined,
}

impl State {
    /// The resistance between the signal pins: `None` for an open switch,
    /// and for one the sheet does not determine.
    #[must_use]
    pub const fn resistance_ohms(self) -> Option<Ohms> {
        match self {
            Self::Off | Self::Undetermined => None,
            Self::On { r_on_ohms } => Some(r_on_ohms),
        }
    }

    /// Whether the switch conducts.
    #[must_use]
    pub const fn is_on(self) -> bool {
        matches!(self, Self::On { .. })
    }
}

/// One of the four bilateral switches (`CD4016B-001`): a supply it is run at
/// and the on-resistances it has there, with the signal at a rail and in the
/// mid-supply band.
///
/// The two signal pins are not named here, because the sheet names them
/// `IN/OUT` and `OUT/IN` and calls the path bilateral (`CD4016B-003`): the
/// resistance is between them and has no direction.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Switch {
    supply: Supply,
    r_on_ohms: Ohms,
    mid_band_r_on_ohms: Option<Ohms>,
}

impl Switch {
    /// A switch at one of the sheet's supplies, at one of its corners: the
    /// rail figure at that corner (`CD4016B-006`), and the mid-band figure at
    /// that corner where the sheet prints one (`CD4016B-007`). The sheet
    /// prints a mid-band maximum and no typical, so a [`Corner::Typ`] switch
    /// has no mid-band figure and [`Switch::drive_at_signal`] does not
    /// determine it in the band.
    #[must_use]
    pub const fn at_corner(supply: Supply, corner: Corner) -> Self {
        let (typ, max) = supply.r_on_typ_max_ohms();
        let (r_on_ohms, mid_band_r_on_ohms) = match (corner, supply.mid_signal_row()) {
            (Corner::Typ, _) => (typ, None),
            (Corner::Max, None) => (max, None),
            (Corner::Max, Some(row)) => (max, Some(row.r_on_max_ohms)),
        };
        Self {
            supply,
            r_on_ohms,
            mid_band_r_on_ohms,
        }
    }

    /// A switch whose figures a caller with a specimen sets itself: its
    /// on-resistance with the signal at a rail, and with it in the supply's
    /// mid-band ([`Supply::mid_signal_row`]), `None` where the caller has no
    /// such figure. At 5 V, where the sheet prints no band, the second is not
    /// consulted.
    #[must_use]
    pub const fn new(supply: Supply, r_on_ohms: Ohms, mid_band_r_on_ohms: Option<Ohms>) -> Self {
        Self {
            supply,
            r_on_ohms,
            mid_band_r_on_ohms,
        }
    }

    /// The supply this switch is built at.
    #[must_use]
    pub const fn supply(self) -> Supply {
        self.supply
    }

    /// The on-resistance this switch has when on with the signal at a rail,
    /// and the one [`Switch::drive`] returns.
    #[must_use]
    pub const fn r_on_ohms(self) -> Ohms {
        self.r_on_ohms
    }

    /// The on-resistance this switch has when on with the signal in the
    /// mid-supply band, `None` where it was built with none.
    #[must_use]
    pub const fn mid_band_r_on_ohms(self) -> Option<Ohms> {
        self.mid_band_r_on_ohms
    }

    /// The level a control voltage resolves to: High at or above `VIHC`,
    /// Low at or below `VILC`, and [`Logic::Undetermined`] between them
    /// (`CD4016B-005`, `CD4016B-OPEN-02`).
    ///
    /// The voltage is measured from `VSS`, as the sheet's tables measure it.
    #[must_use]
    pub const fn control_level(self, control_uv: Microvolts) -> Logic {
        if control_uv.0 >= self.supply.vihc_min_uv().0 {
            Logic::High
        } else if control_uv.0 <= VILC_MAX_UV {
            Logic::Low
        } else {
            Logic::Undetermined
        }
    }

    /// What the switch is between its signal pins with its control pin held
    /// at this voltage: on at a high level, with its rail figure; off at a low
    /// one; and [`State::Undetermined`] where the sheet does not say
    /// (`CD4016B-004`, `CD4016B-005`).
    #[must_use]
    pub const fn drive(self, control_uv: Microvolts) -> State {
        match self.control_level(control_uv) {
            Logic::High => State::On {
                r_on_ohms: self.r_on_ohms,
            },
            Logic::Low => State::Off,
            Logic::Undetermined => State::Undetermined,
        }
    }

    /// As [`Switch::drive`], but with the on-resistance chosen by where the
    /// signal sits: this switch's rail figure at either rail, its mid-band
    /// figure inside the mid-band, and [`State::Undetermined`] for a signal
    /// between the two, or inside the band on a switch with no mid-band
    /// figure, at a supply where the sheet prints a mid-band row
    /// (`CD4016B-007`, `CD4016B-OPEN-03`).
    ///
    /// At 5 V there is no mid-band row and the rail figure stands over the
    /// whole range. A signal outside `VSS` to `VDD` is
    /// [`State::Undetermined`] (`CD4016B-010`).
    #[must_use]
    pub const fn drive_at_signal(self, control_uv: Microvolts, signal_uv: Microvolts) -> State {
        if !self.is_in_signal_range(signal_uv) {
            return State::Undetermined;
        }
        match self.control_level(control_uv) {
            Logic::Low => State::Off,
            Logic::Undetermined => State::Undetermined,
            Logic::High => {
                let rail = State::On {
                    r_on_ohms: self.r_on_ohms,
                };
                let Some(row) = self.supply.mid_signal_row() else {
                    return rail;
                };
                let signal = signal_uv.0;
                if signal == 0 || signal == self.supply.supply_uv().0 {
                    rail
                } else if signal >= row.band_range_uv.0.0 && signal <= row.band_range_uv.1.0 {
                    match self.mid_band_r_on_ohms {
                        Some(r_on_ohms) => State::On { r_on_ohms },
                        None => State::Undetermined,
                    }
                } else {
                    State::Undetermined
                }
            }
        }
    }

    /// Whether a signal pin at this voltage above `VSS` is inside the range the
    /// sheet specifies the switch over, `VSS` to `VDD` inclusive
    /// (`CD4016B-010`).
    #[must_use]
    pub const fn is_in_signal_range(self, signal_uv: Microvolts) -> bool {
        signal_uv.0 >= 0 && signal_uv.0 <= self.supply.supply_uv().0
    }
}

/// The package: four switches sharing nothing but the two supply pins
/// (`CD4016B-001`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Cd4016b {
    switches: [Switch; SWITCH_COUNT],
}

impl Cd4016b {
    /// A package of these four switches, `A` first. The sheet says two
    /// switches of a package differ by 5 to 15 Ω typical (`CD4016B-008`), so
    /// each is built on its own ([`Switch::new`], [`Switch::at_corner`]).
    #[must_use]
    pub const fn with_switches(switches: [Switch; SWITCH_COUNT]) -> Self {
        Self { switches }
    }

    /// Switch `index`, numbered from 0 for the switch the sheet letters `A`.
    ///
    /// # Panics
    ///
    /// If `index` is not below [`SWITCH_COUNT`].
    #[must_use]
    pub const fn switch(&self, index: usize) -> Switch {
        self.switches[index]
    }

    /// What the four switches are with their control pins held at these
    /// levels, `A` first.
    #[must_use]
    pub const fn drive(&self, inputs: Inputs) -> [State; SWITCH_COUNT] {
        let [a, b, c, d] = inputs.control_uv;
        [
            self.switches[0].drive(a),
            self.switches[1].drive(b),
            self.switches[2].drive(c),
            self.switches[3].drive(d),
        ]
    }
}
