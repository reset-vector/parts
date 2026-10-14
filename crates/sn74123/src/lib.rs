//! The Texas Instruments SN74123 dual retriggerable monostable multivibrator,
//! at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The timing components are the consumer's, as figures.** A half's pulse
//! is set by a resistor to `Vcc` and a capacitor across `Cext` and
//! `Rext/Cext`, and the sheet gives the width as an equation in the two
//! (`SN74123-010`) — with a second coefficient when the diode of its Figure 2
//! is fitted (`SN74123-011`). [`pulse_width_ps`] is that equation, and
//! [`Timing::new`] takes its width or one the consumer has from elsewhere,
//! because the sheet gives no equation at or below 1000 pF (`SN74123-012`).
//!
//! **Time is integer picoseconds.** An ohm times a picofarad is a picosecond,
//! so the equation needs no unit conversion and no float. A half holds how
//! much of its pulse is left and the instant it was last shown its inputs;
//! [`OneShot::step`] takes the host's time with the levels and spends what
//! has passed since, [`OneShot::outputs`] reads what is left, and
//! [`OneShot::remaining_ps`] tells a caller when the output will fall.
//!
//! **What starts a pulse is one gate's rising edge.** Each half ANDs `A`
//! inverted, `B` and `CLR` and fires on that product going High
//! (`SN74123-003`), which is every trigger row of the function table at once,
//! the clear-release row included (`SN74123-004`, `SN74123-005`).
//!
//! **What the sheet does not say, this crate does not say either.** A half
//! built by [`OneShot::new`] takes the first levels it is shown without
//! triggering (`SN74123-OPEN-05`); the retrigger lock-out is measured from
//! the most recent accepted trigger (`SN74123-OPEN-06`); two trigger inputs
//! that move on one call fire once if the gate rises across it
//! (`SN74123-OPEN-08`); there is no width at or below 1000 pF
//! (`SN74123-OPEN-04`), and one outside the recommended components is
//! computed all the same, [`Connection::is_in_rext_range`] saying which
//! (`SN74123-OPEN-03`); and there is no propagation delay in it
//! (`SN74123-OPEN-07`).
//!
//! # Examples
//!
//! ```
//! use sn74123::{Connection, Inputs, Level, OneShot, Timing, lockout_ps, pulse_width_ps};
//! use units::{Ohms, Picofarads, Picoseconds};
//!
//! let cext_pf = Picofarads(10_000);
//! let width_ps = pulse_width_ps(Ohms(10_000), cext_pf, Connection::Across).ok_or("no equation")?;
//! let mut half = OneShot::with_state(Timing::new(width_ps, lockout_ps(cext_pf)), Level::Low);
//! // `B` rising with `A` Low and `CLR` High fires the pulse.
//! let fired = Inputs { a_n: Level::Low, b: Level::High, clr_n: Level::High };
//! assert_eq!(half.step(Picoseconds(0), fired).q, Level::High);
//! // The pulse ends once its width has passed.
//! assert_eq!(half.step(width_ps, fired).q, Level::Low);
//! # Ok::<(), &str>(())
//! ```

#![no_std]

use units::{Microvolts, Ohms, PerMille, Picofarads, Picoseconds, PicosecondsPerPicofarad};

/// A pin's level, in a pin word's field, an argument and a result.
pub use signal::Level;

/// How many pins the package presents (`SN74123-001`).
pub const PIN_COUNT: usize = 16;

/// The pin names, pin 1 first (`SN74123-001`). `_N` is the sheet's bar over
/// an input, `_BAR` over an output, and `/` joins the two functions of one
/// pin.
pub const PIN_NAMES: [&str; PIN_COUNT] = [
    "1A",
    "1B",
    "1CLR_N",
    "1Q_BAR",
    "2Q",
    "2Cext",
    "2Rext/Cext",
    "GND",
    "2A",
    "2B",
    "2CLR_N",
    "2Q_BAR",
    "1Q",
    "1Cext",
    "1Rext/Cext",
    "VCC",
];

/// The recommended supply, `(min, max)` (`SN74123-002`).
pub const SUPPLY_UV: (Microvolts, Microvolts) = (Microvolts(4_750_000), Microvolts(5_250_000));

/// The recommended external timing resistance, minimum and maximum
/// (`SN74123-002`).
pub const REXT_RANGE_OHMS: (Ohms, Ohms) = (Ohms(5_000), Ohms(50_000));

/// The most wiring capacitance the `Rext/Cext` terminal may carry
/// (`SN74123-002`).
pub const WIRING_CAPACITANCE_MAX_PF: Picofarads = Picofarads(50);

/// `tw`, the shortest input pulse the sheet recommends: 40 ns
/// (`SN74123-002`).
pub const TW_MIN_PS: Picoseconds = Picoseconds(40_000);

/// The capacitance at and below which the sheet gives no equation, only a
/// curve (`SN74123-010`, `SN74123-012`).
pub const EQUATION_CEXT_ABOVE_PF: Picofarads = Picofarads(1_000);

/// The equation's coefficient `K`: 0.28 (`SN74123-010`).
pub const K_PER_MILLE: PerMille = PerMille(280);

/// The coefficient `KD` with Figure 2's diode fitted: 0.25 (`SN74123-011`).
pub const KD_PER_MILLE: PerMille = PerMille(250);

/// The equation's additive resistance: the `0.7` of `1 + 0.7/RT` with `RT`
/// in kilohms, which is 700 ohms added to `RT` (`SN74123-010`).
pub const EQUATION_OFFSET_OHMS: Ohms = Ohms(700);

/// The retrigger lock-out, in picoseconds per picofarad of `Cext`: the note's
/// `0.22 Cext` nanoseconds (`SN74123-007`).
pub const LOCKOUT_PS_PER_PF: PicosecondsPerPicofarad = PicosecondsPerPicofarad(220);

/// The supply the switching table is stated at, at 25 °C (`SN74123-012`,
/// `SN74123-013`, `SN74123-014`).
pub const TEST_SUPPLY_UV: Microvolts = Microvolts(5_000_000);

/// The switching table's `twQ` row: the timing components it is stated at,
/// and the output pulse width there, minimum, typical and maximum
/// (`SN74123-013`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct TwqRow {
    /// `Cext`.
    pub timing_pf: Picofarads,
    /// `Rext`.
    pub rext_ohms: Ohms,
    /// The width.
    pub min_typ_max_ps: (Picoseconds, Picoseconds, Picoseconds),
}

/// The switching table's `twQ(min)` row: the timing components it is stated
/// at, with no timing capacitor, and the output pulse width there, typical
/// and maximum, the maximum as SDLS043 prints it (`SN74123-012`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct TwqShortestRow {
    /// `Cext`.
    pub timing_pf: Picofarads,
    /// `Rext`.
    pub rext_ohms: Ohms,
    /// The width.
    pub typ_max_ps: (Picoseconds, Picoseconds),
}

/// The output pulse width at `Cext` = 1000 pF and `Rext` = 10 kΩ
/// (`SN74123-013`).
pub const TWQ: TwqRow = TwqRow {
    timing_pf: Picofarads(1_000),
    rext_ohms: Ohms(10_000),
    min_typ_max_ps: (
        Picoseconds(2_760_000),
        Picoseconds(3_030_000),
        Picoseconds(3_370_000),
    ),
};

/// The shortest output pulse: no timing capacitor and `Rext` = 5 kΩ
/// (`SN74123-012`).
pub const TWQ_SHORTEST: TwqShortestRow = TwqShortestRow {
    timing_pf: Picofarads(0),
    rext_ohms: Ohms(5_000),
    typ_max_ps: (Picoseconds(45_000), Picoseconds(76_000)),
};

/// `A` to `Q`, `tPLH`, typical and maximum, at `Cext` = 0 and `Rext` = 5 kΩ,
/// as every delay below (`SN74123-014`).
pub const A_TO_Q_TPLH_TYP_MAX_PS: (Picoseconds, Picoseconds) =
    (Picoseconds(22_000), Picoseconds(33_000));
/// `B` to `Q`, `tPLH`, typical and maximum (`SN74123-014`).
pub const B_TO_Q_TPLH_TYP_MAX_PS: (Picoseconds, Picoseconds) =
    (Picoseconds(19_000), Picoseconds(28_000));
/// `A` to `Q̄`, `tPHL`, typical and maximum (`SN74123-014`).
pub const A_TO_Q_BAR_TPHL_TYP_MAX_PS: (Picoseconds, Picoseconds) =
    (Picoseconds(30_000), Picoseconds(40_000));
/// `B` to `Q̄`, `tPHL`, typical and maximum (`SN74123-014`).
pub const B_TO_Q_BAR_TPHL_TYP_MAX_PS: (Picoseconds, Picoseconds) =
    (Picoseconds(27_000), Picoseconds(36_000));
/// `CLR` to `Q`, `tPHL`, typical and maximum (`SN74123-014`).
pub const CLR_N_TO_Q_TPHL_TYP_MAX_PS: (Picoseconds, Picoseconds) =
    (Picoseconds(18_000), Picoseconds(27_000));
/// `CLR` to `Q̄`, `tPLH`, typical and maximum (`SN74123-014`).
pub const CLR_N_TO_Q_BAR_TPLH_TYP_MAX_PS: (Picoseconds, Picoseconds) =
    (Picoseconds(30_000), Picoseconds(40_000));

/// How the timing capacitor is connected (`SN74123-010`, `SN74123-011`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Connection {
    /// Figure 3: `Cext` straight across the two terminals, coefficient `K`.
    Across,
    /// Figure 2: a silicon switching diode from the `Rext` junction to
    /// `Rext/Cext`, which the sheet recommends with an electrolytic `Cext` and
    /// wherever the clear is used; coefficient `KD`, and `Rext` no more than
    /// 0.6 of its recommended maximum.
    Diode,
}

impl Connection {
    /// The coefficient.
    #[must_use]
    pub const fn k_per_mille(self) -> PerMille {
        match self {
            Self::Across => K_PER_MILLE,
            Self::Diode => KD_PER_MILLE,
        }
    }

    /// The `Rext` the sheet recommends for this connection, `(min, max)`:
    /// 5 kΩ to 50 kΩ, the largest 0.6 of that with the diode (`SN74123-002`,
    /// `SN74123-011`).
    #[must_use]
    pub const fn rext_range_ohms(self) -> (Ohms, Ohms) {
        match self {
            Self::Across => REXT_RANGE_OHMS,
            Self::Diode => (REXT_RANGE_OHMS.0, Ohms(REXT_RANGE_OHMS.1.0 * 6 / 10)),
        }
    }

    /// Whether this `Rext` is inside what the sheet recommends for this
    /// connection. The equation is printed without that condition, so a
    /// figure outside it is still computed; this says whether the sheet
    /// stands behind it (`SN74123-OPEN-03`).
    #[must_use]
    pub const fn is_in_rext_range(self, rext_ohms: Ohms) -> bool {
        let (min, max) = self.rext_range_ohms();
        rext_ohms.0 >= min.0 && rext_ohms.0 <= max.0
    }
}

/// The sheet's equation, `K · RT · Cext · (1 + 0.7/RT)`, with no condition
/// on `Cext` (`SN74123-010`, `SN74123-011`).
///
/// With `RT` in kilohms and `Cext` in picofarads the sheet's result is in
/// nanoseconds; the same product with `RT` in ohms is `K · Cext · (Rext +
/// 700)` picoseconds, which is what this returns, rounded down, and
/// saturating at `u64::MAX` picoseconds for components no board fits.
#[must_use]
pub const fn equation_width_ps(
    rext_ohms: Ohms,
    cext_pf: Picofarads,
    connection: Connection,
) -> Picoseconds {
    let width = (cext_pf.0 as u128)
        .saturating_mul(rext_ohms.0 as u128 + EQUATION_OFFSET_OHMS.0 as u128)
        .saturating_mul(connection.k_per_mille().0 as u128)
        / 1000;
    Picoseconds(if width > u64::MAX as u128 {
        u64::MAX
    } else {
        width as u64
    })
}

/// The output pulse width, where the sheet gives one by equation: `Cext`
/// above 1000 pF (`SN74123-010`, `SN74123-011`). `None` at or below
/// 1000 pF, where the sheet gives Figure 4 and no equation (`SN74123-012`,
/// `SN74123-OPEN-04`).
#[must_use]
pub const fn pulse_width_ps(
    rext_ohms: Ohms,
    cext_pf: Picofarads,
    connection: Connection,
) -> Option<Picoseconds> {
    if cext_pf.0 > EQUATION_CEXT_ABOVE_PF.0 {
        Some(equation_width_ps(rext_ohms, cext_pf, connection))
    } else {
        None
    }
}

/// The time after a trigger during which a retrigger is ignored: `0.22 Cext`
/// nanoseconds (`SN74123-007`).
#[must_use]
pub const fn lockout_ps(cext_pf: Picofarads) -> Picoseconds {
    Picoseconds(cext_pf.0 as u64 * LOCKOUT_PS_PER_PF.0)
}

/// One half's timing: its pulse width and its retrigger lock-out.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Timing {
    width_ps: Picoseconds,
    lockout_ps: Picoseconds,
}

impl Timing {
    /// A timing whose width the caller supplies — by the sheet's equation,
    /// [`pulse_width_ps`], read off Figure 4, taken from a specimen, or put at
    /// a corner of a spread — and whose lock-out it supplies likewise, the
    /// sheet's being [`lockout_ps`] of its `Cext`.
    #[must_use]
    pub const fn new(width_ps: Picoseconds, lockout_ps: Picoseconds) -> Self {
        Self {
            width_ps,
            lockout_ps,
        }
    }

    /// The pulse width.
    #[must_use]
    pub const fn width_ps(self) -> Picoseconds {
        self.width_ps
    }

    /// The retrigger lock-out.
    #[must_use]
    pub const fn lockout_ps(self) -> Picoseconds {
        self.lockout_ps
    }
}

/// One half's three inputs, as levels.
///
/// [`Inputs::default`] is [`Inputs::idle`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `A`, the low-active trigger input.
    pub a_n: Level,
    /// `B`, the high-active trigger input.
    pub b: Level,
    /// `CLR`, the low-active clear.
    pub clr_n: Level,
}

impl Default for Inputs {
    /// [`Inputs::idle`].
    fn default() -> Self {
        Self::idle()
    }
}

impl Inputs {
    /// The levels at which nothing triggers and nothing clears: `A` High,
    /// `B` Low, `CLR` High.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            a_n: Level::High,
            b: Level::Low,
            clr_n: Level::High,
        }
    }

    /// The input gate: `A` inverted, `B` and `CLR` (`SN74123-003`).
    const fn gate(self) -> Level {
        Level::from_bool(self.a_n.is_low() && self.b.is_high() && self.clr_n.is_high())
    }
}

/// One half's two outputs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `Q`, High for the pulse.
    pub q: Level,
    /// `Q̄`, its complement.
    pub q_bar: Level,
}

/// One multivibrator: its timing, the input gate's last level, what is left
/// of the pulse, and the instant it was last shown its inputs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct OneShot {
    timing: Timing,
    gate: Option<Level>,
    remaining_ps: u64,
    since_trigger_ps: u64,
    at_ps: u64,
}

impl OneShot {
    /// A half that has seen no input yet: the first levels it is shown are
    /// taken as they stand and trigger nothing (`SN74123-OPEN-05`).
    ///
    /// The sheet fixes no level for `Q` at power-up. This crate starts the
    /// half at rest, as a clear leaves it (`Q` Low, no pulse running); a board
    /// that needs a known level asserts `CLR`. Its clock starts at 0 ps.
    #[must_use]
    pub const fn new(timing: Timing) -> Self {
        Self {
            timing,
            gate: None,
            remaining_ps: 0,
            since_trigger_ps: u64::MAX,
            at_ps: 0,
        }
    }

    /// A half at rest, as [`OneShot::new`] starts one, whose input gate
    /// (`A` inverted, `B` and `CLR`) the caller says stood at this level
    /// before the first call, so that a first call raising it triggers.
    ///
    /// `gate` is the level of the gate's product, which no pin shows: Low
    /// where any of `A` High, `B` Low or `CLR` Low holds.
    #[must_use]
    pub const fn with_state(timing: Timing, gate: Level) -> Self {
        Self {
            timing,
            gate: Some(gate),
            remaining_ps: 0,
            since_trigger_ps: u64::MAX,
            at_ps: 0,
        }
    }

    /// The timing this half was built with.
    #[must_use]
    pub const fn timing(self) -> Timing {
        self.timing
    }

    /// How much of the pulse is left at the instant of the last
    /// [`OneShot::step`]; zero at rest.
    #[must_use]
    pub const fn remaining_ps(self) -> Picoseconds {
        Picoseconds(self.remaining_ps)
    }

    /// The outputs at the instant of the last [`OneShot::step`].
    #[must_use]
    pub const fn outputs(self) -> Outputs {
        let pulse = self.remaining_ps > 0;
        Outputs {
            q: Level::from_bool(pulse),
            q_bar: Level::from_bool(!pulse),
        }
    }

    /// The inputs change to these levels at `now_ps`, on the host's own
    /// clock; returns where the outputs then stand. The time since
    /// the last call passes first with the earlier levels held. Times must
    /// not go backwards; one that does passes no time. A caller that wants to
    /// see a pulse end shows the half the same levels at a later instant.
    ///
    /// A clear Low ends any pulse (`SN74123-008`). A rise of the input gate
    /// starts a pulse from rest, or restarts the one running unless it falls
    /// inside the lock-out after the last accepted trigger (`SN74123-006`,
    /// `SN74123-007`). Nothing else ends a pulse (`SN74123-009`).
    pub const fn step(&mut self, now_ps: Picoseconds, inputs: Inputs) -> Outputs {
        let now_ps = now_ps.0;
        let dt_ps = now_ps.saturating_sub(self.at_ps);
        self.remaining_ps = self.remaining_ps.saturating_sub(dt_ps);
        self.since_trigger_ps = self.since_trigger_ps.saturating_add(dt_ps);
        if now_ps > self.at_ps {
            self.at_ps = now_ps;
        }
        if inputs.clr_n.is_low() {
            self.remaining_ps = 0;
        }
        let gate = inputs.gate();
        let rose = matches!(self.gate, Some(Level::Low)) && gate.is_high();
        if rose {
            let running = self.remaining_ps > 0;
            if !running || self.since_trigger_ps >= self.timing.lockout_ps.0 {
                self.remaining_ps = self.timing.width_ps.0;
                self.since_trigger_ps = 0;
            }
        }
        self.gate = Some(gate);
        self.outputs()
    }
}

/// How many halves the package carries (`SN74123-001`).
pub const ONE_SHOT_COUNT: usize = 2;

/// The package: two halves that share nothing but the supply (`SN74123-001`,
/// `SN74123-003`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Sn74123 {
    /// Index 0 is half 1: pins 1, 2, 3 in, 13 and 4 out, 14 and 15 timing.
    /// Index 1 is half 2: pins 9, 10, 11 in, 5 and 12 out, 6 and 7 timing.
    one_shots: [OneShot; ONE_SHOT_COUNT],
}

impl Sn74123 {
    /// A package built from its two halves, index 0 the one the sheet
    /// numbers 1; [`OneShot::new`] builds a half from its timing.
    #[must_use]
    pub const fn with_one_shots(one_shots: [OneShot; ONE_SHOT_COUNT]) -> Self {
        Self { one_shots }
    }

    /// One of the two halves, index 0 being the one the sheet numbers 1.
    ///
    /// # Panics
    ///
    /// If `index` is not 0 or 1.
    #[must_use]
    pub const fn one_shot(self, index: usize) -> OneShot {
        self.one_shots[index]
    }

    /// Both halves' outputs now.
    #[must_use]
    pub const fn outputs(self) -> [Outputs; ONE_SHOT_COUNT] {
        [self.one_shots[0].outputs(), self.one_shots[1].outputs()]
    }

    /// Both halves' inputs change to these levels at `now_ps`, on the host's
    /// own clock, as [`OneShot::step`] takes them.
    pub const fn step(
        &mut self,
        now_ps: Picoseconds,
        inputs: [Inputs; ONE_SHOT_COUNT],
    ) -> [Outputs; ONE_SHOT_COUNT] {
        [
            self.one_shots[0].step(now_ps, inputs[0]),
            self.one_shots[1].step(now_ps, inputs[1]),
        ]
    }
}
