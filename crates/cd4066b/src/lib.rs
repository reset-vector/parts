//! The Texas Instruments CD4066B quad bilateral switch, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **A switch is a resistance or an open circuit** (`CD4066B-004`,
//! `CD4066B-006`, `CD4066B-008`). The sheet states the part as an on-state
//! resistance into a stated load and an off-state leakage, never as a voltage,
//! because the voltage across a closed switch belongs to the network on both
//! sides of it. So [`Switch::drive`] returns [`State::Off`] or [`State::On`]
//! with an on-resistance in ohms, and nothing here returns a voltage.
//!
//! **The control pin is a voltage that resolves to a level** (`CD4066B-005`),
//! against a low maximum and a high minimum the sheet gives per supply. Between
//! the two the sheet says nothing (`CD4066B-OPEN-02`) and [`Switch::control_level`]
//! returns [`Logic::Undetermined`] there rather than pick a side.
//!
//! **There is no state.** The sheet's truth table — which Hitachi prints and TI
//! puts in a note — is two rows of a control level against `ON` or `OFF`, so
//! every method here is a function of its arguments and there is no `tick`;
//! the package is evaluated by [`Cd4066b::drive`].
//!
//! **Microvolts and ohms, and no float anywhere.** The sheet gives its levels
//! in volts and its resistances in ohms; a microvolt carries the levels with
//! precision to spare, an `i32` reaches ±2.1 kV, and a resistance is an
//! integer already.
//!
//! # Examples
//!
//! ```
//! use cd4066b::{Cd4066b, Corner, Inputs, State, Supply, Switch, SWITCH_COUNT};
//! use units::{Microvolts, Ohms};
//!
//! let switch = Switch::at_corner(Supply::Vdd10V, Corner::Typ);
//! let package = Cd4066b::with_switches([switch; SWITCH_COUNT]);
//! let mut inputs = Inputs::default();
//! inputs.control_uv[0] = Microvolts(10_000_000);
//! let states = package.drive(inputs);
//! assert_eq!(states[0], State::On { r_on_ohms: Ohms(180) });
//! assert_eq!(states[1], State::Off);
//! ```

#![no_std]

use units::{Microvolts, Ohms, Picoamps, Picoseconds, TenthPicofarads};

/// How many switches the package carries (`CD4066B-001`).
pub const SWITCH_COUNT: usize = 4;

/// How many pins the package presents (`CD4066B-002`).
pub const PIN_COUNT: usize = 14;

/// `Iis`, the off-state leakage through one switch, `(typical, maximum)`, at
/// 25 °C with [`IIS_TEST_SUPPLY_UV`] across it: the maximum is ±0.1 μA
/// (`CD4066B-008`).
pub const IIS_TYP_MAX_PA: (Picoamps, Picoamps) = (Picoamps(10), Picoamps(100_000));

/// The supply the two `Iis` figures are stated at, with it all across the
/// switch (`CD4066B-008`).
pub const IIS_TEST_SUPPLY_UV: Microvolts = Microvolts(18_000_000);

/// The control pin's input impedance, typical: 10¹² Ω (`CD4066B-011`).
pub const CONTROL_INPUT_IMPEDANCE_TYP_OHMS: Ohms = Ohms(1_000_000_000_000);

/// `CIN`, the control pin's input capacitance, typical: 7.5 pF
/// (`CD4066B-011`).
pub const CIN_TYP_TENTH_PF: TenthPicofarads = TenthPicofarads(75);

/// The recommended supply range, `(min, max)`: 3 V to 18 V (`CD4066B-010`).
pub const SUPPLY_UV: (Microvolts, Microvolts) = (Microvolts(3_000_000), Microvolts(18_000_000));

/// The absolute maximum supply: 20 V (`CD4066B-010`).
pub const SUPPLY_ABSOLUTE_MAX_UV: Microvolts = Microvolts(20_000_000);

/// The three supplies the electrical tables are stated at (`CD4066B-005`,
/// `CD4066B-006`, `CD4066B-007`).
///
/// Every figure in the sheet's tables is a row per supply, so the supply is
/// the first thing a switch is built at. A supply between the rows is
/// `CD4066B-OPEN-03` and no row here interpolates.
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

    /// The on-resistance, `(typical, maximum)` at 25 °C (`CD4066B-006`).
    #[must_use]
    pub const fn r_on_typ_max_ohms(self) -> (Ohms, Ohms) {
        let (typ, max) = match self {
            Self::Vdd5V => (470, 1050),
            Self::Vdd10V => (180, 400),
            Self::Vdd15V => (125, 240),
        };
        (Ohms(typ), Ohms(max))
    }

    /// `Δron`, the on-resistance difference between any two switches of one
    /// package, typical (`CD4066B-007`).
    #[must_use]
    pub const fn delta_r_on_typ_ohms(self) -> Ohms {
        Ohms(match self {
            Self::Vdd5V => 15,
            Self::Vdd10V => 10,
            Self::Vdd15V => 5,
        })
    }

    /// `VILC`, the highest control voltage that is a low level, above `VSS`
    /// (`CD4066B-005`). A method because it scales with the supply.
    #[must_use]
    pub const fn vilc_max_uv(self) -> Microvolts {
        Microvolts(match self {
            Self::Vdd5V => 1_000_000,
            Self::Vdd10V | Self::Vdd15V => 2_000_000,
        })
    }

    /// `VIHC`, the lowest control voltage that is a high level, above `VSS`
    /// (`CD4066B-005`). A method because it scales with the supply.
    #[must_use]
    pub const fn vihc_min_uv(self) -> Microvolts {
        Microvolts(match self {
            Self::Vdd5V => 3_500_000,
            Self::Vdd10V => 7_000_000,
            Self::Vdd15V => 11_000_000,
        })
    }

    /// The turn-on propagation delay from the control edge, typical
    /// (`CD4066B-013`). The sheet prints one row for turn-on and turn-off, so
    /// this is the turn-off delay as well. The crate does not spend it; it is
    /// here for a caller that wants to know what is being treated as
    /// instantaneous.
    #[must_use]
    pub const fn control_to_output_tpd_typ_ps(self) -> Picoseconds {
        Picoseconds(match self {
            Self::Vdd5V => 35_000,
            Self::Vdd10V => 20_000,
            Self::Vdd15V => 15_000,
        })
    }

    /// `tpd`, the signal-input-to-output propagation delay with the switch on,
    /// typical (`CD4066B-013`).
    #[must_use]
    pub const fn signal_to_output_tpd_typ_ps(self) -> Picoseconds {
        Picoseconds(match self {
            Self::Vdd5V => 20_000,
            Self::Vdd10V => 10_000,
            Self::Vdd15V => 7_000,
        })
    }
}

/// Which of the sheet's two figures a switch is built at (`CD4066B-006`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Corner {
    /// The 25 °C typical.
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
    /// The control pins, `A` first, each above `VSS` (`CD4066B-005`).
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

/// What one switch is between its two signal pins (`CD4066B-004`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum State {
    /// Open: nothing passes but the leakage of `CD4066B-008`, which is not
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

/// One of the four bilateral switches (`CD4066B-001`): a supply it is run at
/// and the on-resistance it has there.
///
/// The two signal pins are not named here, because the sheet names them
/// `IN/OUT` and `OUT/IN` and calls the path bilateral (`CD4066B-003`): the
/// resistance is between them and has no direction.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Switch {
    supply: Supply,
    r_on_ohms: Ohms,
}

impl Switch {
    /// A switch at one of the sheet's supplies, at one of its two figures.
    /// Both are printed at every supply, so this cannot fail.
    #[must_use]
    pub const fn at_corner(supply: Supply, corner: Corner) -> Self {
        let (typ, max) = supply.r_on_typ_max_ohms();
        let r_on_ohms = match corner {
            Corner::Typ => typ,
            Corner::Max => max,
        };
        Self { supply, r_on_ohms }
    }

    /// A switch whose on-resistance a caller with a specimen, or with the
    /// other maker's sheet (`CD4066B-SIB-02`), sets itself.
    #[must_use]
    pub const fn new(supply: Supply, r_on_ohms: Ohms) -> Self {
        Self { supply, r_on_ohms }
    }

    /// The supply this switch is built at.
    #[must_use]
    pub const fn supply(self) -> Supply {
        self.supply
    }

    /// The on-resistance this switch has when on.
    #[must_use]
    pub const fn r_on_ohms(self) -> Ohms {
        self.r_on_ohms
    }

    /// The level a control voltage resolves to: High at or above `VIHC`,
    /// Low at or below `VILC`, and [`Logic::Undetermined`] between them
    /// (`CD4066B-005`, `CD4066B-OPEN-02`).
    ///
    /// The voltage is measured from `VSS`, as the sheet's tables measure it.
    #[must_use]
    pub const fn control_level(self, control_uv: Microvolts) -> Logic {
        if control_uv.0 >= self.supply.vihc_min_uv().0 {
            Logic::High
        } else if control_uv.0 <= self.supply.vilc_max_uv().0 {
            Logic::Low
        } else {
            Logic::Undetermined
        }
    }

    /// What the switch is between its signal pins with its control pin held
    /// at this voltage: on at a high level, off at a low one, and
    /// [`State::Undetermined`] where the sheet does not say (`CD4066B-004`, `CD4066B-005`).
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

    /// Whether a signal pin at this voltage above `VSS` is inside the range the
    /// sheet specifies the switch over, `VSS` to `VDD` inclusive
    /// (`CD4066B-009`).
    #[must_use]
    pub const fn is_in_signal_range(self, signal_uv: Microvolts) -> bool {
        signal_uv.0 >= 0 && signal_uv.0 <= self.supply.supply_uv().0
    }
}

/// The package: four switches sharing nothing but the two supply pins
/// (`CD4066B-001`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Cd4066b {
    switches: [Switch; SWITCH_COUNT],
}

impl Cd4066b {
    /// A package of these four switches, `A` first. The sheet says two
    /// switches of a package differ by 5 to 15 Ω typical (`CD4066B-007`), so
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
