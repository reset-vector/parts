//! The Texas Instruments CD4030B quad exclusive-OR gate, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part holds no state** (`CD4030B-001`, `CD4030B-003`), so the whole of
//! it is [`drive`]: a function from the levels the outside world holds the
//! eight input pins at to the four levels the part drives back. There is no
//! clock here and no `tick`.
//!
//! **This crate is a reading of the truth table.** `SCHS035C` prints the
//! function as the word *Exclusive-OR* in the description, the symbol `⊕` in
//! the functional diagram, and a four-row truth table for one gate. [`gate`]
//! is that table's four rows written as a `match`. The transistor schematic of
//! Fig. 1 (`CD4030B-005`) is not modelled.
//!
//! **The four gates are lettered as the sheet letters them.** Gate `J` takes
//! `A` and `B`, `K` takes `C` and `D`, `L` takes `E` and `F`, `M` takes `G` and
//! `H` (`CD4030B-002`); here they are gates 0 to 3 in that order, and each
//! gate's first-named input is its `a` and its second its `b`.
//!
//! **An input between `VIL` and `VIH` is [`Logic::Undetermined`]**
//! (`CD4030B-OPEN-02`): [`Supply::input_level`] picks neither side where the
//! sheet gives no level.
//!
//! # Examples
//!
//! ```
//! use cd4030b::{drive, Inputs, Logic, Supply};
//! use units::Microvolts;
//!
//! // Gate J with A High and B Low, gate K with both High.
//! let out = drive(Inputs { aceg: 0b0011, bdfh: 0b0010 });
//! assert_eq!(out, 0b0001);
//! assert_eq!(Supply::Vdd5V.input_level(Microvolts(4_000_000)), Logic::High);
//! ```

#![no_std]

use units::{Microvolts, Picoamps, Picoseconds, TenthPicofarads};

/// How many gates the package carries (`CD4030B-001`).
pub const GATE_COUNT: usize = 4;

/// How many pins the package presents (`CD4030B-002`).
pub const PIN_COUNT: usize = 14;

/// The four gates of the package, in the low four bits of a byte.
///
/// Bit `n` is gate `n`'s pin: on the inputs bit 0 is `A` (or `B`), bit 1 `C`
/// (or `D`), bit 2 `E` (or `F`), bit 3 `G` (or `H`); on the outputs `J`, `K`,
/// `L`, `M`, `J` at bit 0. That is a numbering this crate imposes for driving
/// four gates at once; it is **not** the order the pins run round the package,
/// which is `CD4030B-002`. Nothing above bit 3 leaves a pin.
pub const GATE_MASK: u8 = 0x0F;

/// The recommended supply range, `(min, max)`: 3 V to 18 V (`CD4030B-010`).
pub const SUPPLY_UV: (Microvolts, Microvolts) = (Microvolts(3_000_000), Microvolts(18_000_000));

/// The absolute maximum supply: 20 V (`CD4030B-010`).
pub const SUPPLY_ABSOLUTE_MAX_UV: Microvolts = Microvolts(20_000_000);

/// `CIN`, the input capacitance, typical and maximum: 5 pF and 7.5 pF
/// (`CD4030B-011`).
pub const CIN_TYP_MAX_TENTH_PF: (TenthPicofarads, TenthPicofarads) =
    (TenthPicofarads(50), TenthPicofarads(75));

/// `IIN`, the input current at 18 V, typical at 25 °C and maximum over
/// temperature: ±10 pA and ±1 μA (`CD4030B-011`).
pub const IIN_TYP_MAX_PA: (Picoamps, Picoamps) = (Picoamps(10), Picoamps(1_000_000));

/// `VOL`, the highest voltage an output at a low level sits at, above `VSS`:
/// 0.05 V at every supply (`CD4030B-007`).
pub const VOL_MAX_UV: Microvolts = Microvolts(50_000);

/// The three supplies the tables are stated at (`CD4030B-006`, `CD4030B-007`,
/// `CD4030B-008`, `CD4030B-009`).
///
/// The crate spends none of these — a level is `High` or `Low` and the delay is
/// not modelled — and carries them so a consumer can hold its own drivers and
/// its own timing to the conditions under which the table holds.
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

    /// `VIL`, the highest input voltage that is a low level, above `VSS`
    /// (`CD4030B-006`). A method because it scales with the supply.
    #[must_use]
    pub const fn vil_max_uv(self) -> Microvolts {
        Microvolts(match self {
            Self::Vdd5V => 1_500_000,
            Self::Vdd10V => 3_000_000,
            Self::Vdd15V => 4_000_000,
        })
    }

    /// `VIH`, the lowest input voltage that is a high level, above `VSS`
    /// (`CD4030B-006`). A method because it scales with the supply.
    #[must_use]
    pub const fn vih_min_uv(self) -> Microvolts {
        Microvolts(match self {
            Self::Vdd5V => 3_500_000,
            Self::Vdd10V => 7_000_000,
            Self::Vdd15V => 11_000_000,
        })
    }

    /// `VOH`, the lowest voltage an output at a high level sits at, above
    /// `VSS`: 0.05 V below the supply (`CD4030B-007`). A method because it
    /// scales with the supply; `VOL` does not, and is [`VOL_MAX_UV`].
    #[must_use]
    pub const fn voh_min_uv(self) -> Microvolts {
        Microvolts(self.supply_uv().0 - 50_000)
    }

    /// `tPHL`, `tPLH`, one figure for both edges: the propagation delay,
    /// typical and maximum at 25 °C into 50 pF (`CD4030B-008`).
    #[must_use]
    pub const fn tpd_typ_max_ps(self) -> (Picoseconds, Picoseconds) {
        let (typ, max) = match self {
            Self::Vdd5V => (140_000, 280_000),
            Self::Vdd10V => (65_000, 130_000),
            Self::Vdd15V => (50_000, 100_000),
        };
        (Picoseconds(typ), Picoseconds(max))
    }

    /// `tTHL`, `tTLH`, one figure for both edges: the output transition time,
    /// typical and maximum at 25 °C into 50 pF (`CD4030B-009`).
    #[must_use]
    pub const fn tt_typ_max_ps(self) -> (Picoseconds, Picoseconds) {
        let (typ, max) = match self {
            Self::Vdd5V => (100_000, 200_000),
            Self::Vdd10V => (50_000, 100_000),
            Self::Vdd15V => (40_000, 80_000),
        };
        (Picoseconds(typ), Picoseconds(max))
    }

    /// The level a voltage on an input pin resolves to: High at or above
    /// `VIH`, Low at or below `VIL`, and [`Logic::Undetermined`] between them,
    /// where the sheet says nothing (`CD4030B-006`, `CD4030B-OPEN-02`).
    #[must_use]
    pub const fn input_level(self, input_uv: Microvolts) -> Logic {
        if input_uv.0 >= self.vih_min_uv().0 {
            Logic::High
        } else if input_uv.0 <= self.vil_max_uv().0 {
            Logic::Low
        } else {
            Logic::Undetermined
        }
    }
}

/// A pin's level where the part always determines it: a pin it was shown, as
/// [`gate`] takes its inputs, or a gate's output, as [`gate`] returns it.
pub use signal::Level;
/// A level: what an input voltage resolves to ([`Supply::input_level`]).
/// `Low` is an input at or below `VIL` and `High` one at or above `VIH`;
/// `Undetermined` is an input between the two, where the sheet gives no level.
pub use signal::Logic;

/// The levels the outside world holds the part's eight input pins at.
///
/// **A field is a level, not a condition.** No input of this part is drawn
/// with a bar (`CD4030B-002`), so a set bit is simply High on the pin. There
/// is no enable and no strobe: the package has none, and the two supplies are
/// the only pins the four gates share (`CD4030B-001`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// The first input of each gate — `A`, `C`, `E`, `G` — bit `n` for gate
    /// `n`, per [`GATE_MASK`].
    pub aceg: u8,
    /// The second input of each gate — `B`, `D`, `F`, `H`.
    pub bdfh: u8,
}

impl Inputs {
    /// Every input pin Low.
    #[must_use]
    pub const fn idle() -> Self {
        Self { aceg: 0, bdfh: 0 }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// What the part drives on its four outputs, one bit per gate per [`GATE_MASK`].
///
/// A set bit is a High pin. Each gate reads its own two pins and nothing else,
/// so this is [`gate`] four times and no gate's inputs reach another's output
/// (`CD4030B-001`).
#[must_use]
pub fn drive(inputs: Inputs) -> u8 {
    let mut out = 0;
    for n in 0..GATE_COUNT {
        if gate(bit(inputs.aceg, n), bit(inputs.bdfh, n)).is_high() {
            out |= 1 << n;
        }
    }
    out
}

/// One exclusive-OR gate: the level its output pin is driven to.
///
/// This is the truth table of `SCHS035C` printed 3-90, four rows of `A`, `B`
/// against `J`, in the table's own order (`CD4030B-003`), under its key of
/// `1` for a high level and `0` for a low one (`CD4030B-004`).
#[must_use]
pub const fn gate(a: Level, b: Level) -> Level {
    match (a, b) {
        (Level::Low, Level::Low) => Level::Low,
        (Level::Low, Level::High) => Level::High,
        (Level::High, Level::Low) => Level::High,
        (Level::High, Level::High) => Level::Low,
    }
}

/// The level of one bit of a nibble, bit 0 at index 0, a set bit High.
#[must_use]
pub(crate) const fn bit(nibble: u8, index: usize) -> Level {
    Level::from_bool((nibble >> index) & 1 == 1)
}
