//! The Texas Instruments CD4006B 18-stage static shift register, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The part is four registers behind one clock** (`CD4006B-001`): two of four
//! stages and two of five, each with its own data pin and its own output pins,
//! sharing the clock and the supplies and nothing else. So [`Cd4006b`] holds
//! four small registers and the clock level it last saw, and [`Cd4006b::step`]
//! is the whole of the part.
//!
//! **Data move on the clock's fall** (`CD4006B-003`). A stage is a master that
//! is open while the clock is high and a slave that takes the master on the
//! fall (`CD4006B-004`), so what enters a stage on an edge is the input as it
//! stands at that edge; a part at its pins is only ever shown one level at a
//! time, and an edge is the change between two calls.
//!
//! **Pin 2 is pin 13 half a clock late** (`CD4006B-006`): a latch open while
//! the clock is high. The register's fourth stage moves on the fall, so the
//! latch sees the new value on the next rise and holds it through the low
//! half; here the latch takes `D1+4` on the rise, which is the same thing at
//! every instant a caller can observe, because nothing changes `D1+4` while
//! the clock stands high.
//!
//! **Fully static** (`CD4006B-007`): with the clock at either level nothing
//! moves, however many calls pass. There is no clear pin and no parallel load.
//!
//! **No power-on state is given** (`CD4006B-OPEN-01`). `Default` is power-up:
//! every stage and the latch [`Logic::Undetermined`], with the clock
//! remembered Low. A stage takes a level from the first fall that shifts one
//! into it, so a register's fourth stage is determined after four falls and
//! its fifth after five.
//!
//! **An input between `VIL` and `VIH` is [`Logic::Undetermined`]**
//! (`CD4006B-OPEN-04`): [`Supply::input_level`] picks neither side where the
//! sheet gives no level.
//!
//! **There are no nanoseconds here.** The setup, hold, pulse-width and edge-rate
//! figures of `SCHS017` (`CD4006B-009`) are conditions a board meets so that a
//! fall means what `CD4006B-003` says it means, and this crate assumes they are
//! met; a consumer that wants to check its own clock against them reads them
//! off [`Supply`].
//!
//! # Examples
//!
//! ```
//! use cd4006b::{Cd4006b, Inputs, Level, Logic};
//!
//! let mut part = Cd4006b::default();
//! let d1_high = Inputs { d1: Level::High, clock: Level::High, ..Inputs::default() };
//! // Four periods carry D1's level to D1+4.
//! for _ in 0..4 {
//!     part.tick(d1_high);
//! }
//! assert_eq!(part.outputs().d1_4, Logic::High);
//! assert_eq!(part.outputs().d2_5, Logic::Undetermined);
//! ```

#![no_std]

use units::{Hertz, Microvolts, Picoamps, Picoseconds, TenthPicofarads};

/// How many stages the package holds, over its four registers (`CD4006B-001`).
pub const STAGE_COUNT: usize = 18;

/// How many shift-register sections the package holds (`CD4006B-001`).
pub const SECTION_COUNT: usize = 4;

/// How many stages each section holds, `D1`'s first (`CD4006B-001`, `CD4006B-005`).
pub const STAGES_PER_SECTION: [usize; SECTION_COUNT] = [4, 5, 4, 5];

/// How many pins the package presents (`CD4006B-002`).
pub const PIN_COUNT: usize = 14;

/// The recommended supply range, `(min, max)`: 3 V to 18 V (`CD4006B-012`).
pub const SUPPLY_UV: (Microvolts, Microvolts) = (Microvolts(3_000_000), Microvolts(18_000_000));

/// The absolute maximum supply: 20 V (`CD4006B-012`).
pub const SUPPLY_ABSOLUTE_MAX_UV: Microvolts = Microvolts(20_000_000);

/// `tr`, `tf`, the longest clock rise or fall the sheet allows, one figure for
/// both edges: 15 μs at every supply (`CD4006B-009`).
pub const CLOCK_TT_MAX_PS: Picoseconds = Picoseconds(15_000_000);

/// `CIN`, the input capacitance, typical and maximum: 5 pF and 7.5 pF
/// (`CD4006B-014`).
pub const CIN_TYP_MAX_TENTH_PF: (TenthPicofarads, TenthPicofarads) =
    (TenthPicofarads(50), TenthPicofarads(75));

/// `IIN`, the input current at 18 V, typical at 25 °C and maximum over
/// temperature: ±10 pA and ±1 μA (`CD4006B-014`).
pub const IIN_TYP_MAX_PA: (Picoamps, Picoamps) = (Picoamps(10), Picoamps(1_000_000));

/// `VOL`, the highest voltage an output at a low level sits at, above `VSS`:
/// 0.05 V at every supply (`CD4006B-011`).
pub const VOL_MAX_UV: Microvolts = Microvolts(50_000);

/// The three supplies the tables are stated at (`CD4006B-009`, `CD4006B-010`,
/// `CD4006B-011`, `CD4006B-013`).
///
/// Every timing and level figure on the sheet is a row per supply. The crate
/// spends none of them — a caller drives the clock and a level is `High` or
/// `Low` — and carries them so a consumer can hold its own clock and its own
/// drivers to the conditions under which the register is specified.
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

    /// `tW`, the shortest clock pulse the sheet allows (`CD4006B-009`).
    #[must_use]
    pub const fn tw_min_ps(self) -> Picoseconds {
        Picoseconds(match self {
            Self::Vdd5V => 180_000,
            Self::Vdd10V => 80_000,
            Self::Vdd15V => 50_000,
        })
    }

    /// `tS`, the shortest data setup before the fall the sheet allows
    /// (`CD4006B-009`).
    #[must_use]
    pub const fn ts_min_ps(self) -> Picoseconds {
        Picoseconds(match self {
            Self::Vdd5V => 100_000,
            Self::Vdd10V => 50_000,
            Self::Vdd15V => 40_000,
        })
    }

    /// `tH`, the shortest data hold after the fall the sheet allows
    /// (`CD4006B-009`).
    #[must_use]
    pub const fn th_min_ps(self) -> Picoseconds {
        Picoseconds(match self {
            Self::Vdd5V => 60_000,
            Self::Vdd10V => 40_000,
            Self::Vdd15V => 30_000,
        })
    }

    /// The highest clock frequency, `fCL` max, as the sheet's minimum (what it
    /// guarantees) and typical columns (`CD4006B-009`).
    #[must_use]
    pub const fn fcl_min_typ_hz(self) -> (Hertz, Hertz) {
        let (min, typ) = match self {
            Self::Vdd5V => (2_500_000, 5_000_000),
            Self::Vdd10V => (5_000_000, 10_000_000),
            Self::Vdd15V => (7_000_000, 14_000_000),
        };
        (Hertz(min), Hertz(typ))
    }

    /// `VIL`, the highest input voltage that is a low level, above `VSS`
    /// (`CD4006B-010`). A method because it scales with the supply.
    #[must_use]
    pub const fn vil_max_uv(self) -> Microvolts {
        Microvolts(match self {
            Self::Vdd5V => 1_500_000,
            Self::Vdd10V => 3_000_000,
            Self::Vdd15V => 4_000_000,
        })
    }

    /// `VIH`, the lowest input voltage that is a high level, above `VSS`
    /// (`CD4006B-010`). A method because it scales with the supply.
    #[must_use]
    pub const fn vih_min_uv(self) -> Microvolts {
        Microvolts(match self {
            Self::Vdd5V => 3_500_000,
            Self::Vdd10V => 7_000_000,
            Self::Vdd15V => 11_000_000,
        })
    }

    /// `VOH`, the lowest voltage an output at a high level sits at, above
    /// `VSS`: 0.05 V below the supply (`CD4006B-011`). A method because it
    /// scales with the supply; `VOL` does not, and is [`VOL_MAX_UV`].
    #[must_use]
    pub const fn voh_min_uv(self) -> Microvolts {
        Microvolts(self.supply_uv().0 - 50_000)
    }

    /// `tPHL`, `tPLH`, one figure for both edges: the propagation delay from
    /// the clock's fall to an output, typical and maximum at 25 °C into 50 pF
    /// (`CD4006B-013`).
    #[must_use]
    pub const fn tpd_typ_max_ps(self) -> (Picoseconds, Picoseconds) {
        let (typ, max) = match self {
            Self::Vdd5V => (200_000, 400_000),
            Self::Vdd10V => (100_000, 200_000),
            Self::Vdd15V => (80_000, 160_000),
        };
        (Picoseconds(typ), Picoseconds(max))
    }

    /// The level a voltage on an input pin resolves to: High at or above
    /// `VIH`, Low at or below `VIL`, and [`Logic::Undetermined`] between them,
    /// where the sheet says nothing (`CD4006B-010`, `CD4006B-OPEN-04`).
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

/// A pin's level where the part always determines it: an input pin, in a pin
/// word, in [`Cd4006b::with_state`] and from a getter.
pub use signal::Level;
/// A level: what an input voltage resolves to ([`Supply::input_level`]), and
/// what an output pin stands at ([`Outputs`]). `Low` is an input at or below
/// `VIL` and `High` one at or above `VIH`; `Undetermined` is an input
/// between the two, where the sheet gives no level, and an output whose stage
/// has held nothing the sheet gives since power-up (`CD4006B-OPEN-01`).
pub use signal::Logic;

/// The levels the outside world holds the part's five input pins at.
///
/// **A field is a level, not a condition.** No pin of this part is drawn with a
/// bar (`CD4006B-002`). There is no clear and no enable: the package has
/// none.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `D1`, pin 1 — the first four-stage register's data.
    pub d1: Level,
    /// `D2`, pin 4 — the first five-stage register's data.
    pub d2: Level,
    /// `D3`, pin 5 — the second four-stage register's data.
    pub d3: Level,
    /// `D4`, pin 6 — the second five-stage register's data.
    pub d4: Level,
    /// `CLOCK`, pin 3. What moves every register is this pin
    /// going from High to Low since the last call (`CD4006B-003`); what moves
    /// pin 2 is it going from Low to High (`CD4006B-006`).
    pub clock: Level,
}

impl Inputs {
    /// Every pin Low.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            d1: Level::Low,
            d2: Level::Low,
            d3: Level::Low,
            d4: Level::Low,
            clock: Level::Low,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

impl Inputs {
    /// The four data pins as one nibble, `D1` in bit 0, with the clock level
    /// given.
    #[cfg(test)]
    const fn from_nibble(data: u8, clock: Level) -> Self {
        Self {
            d1: Level::from_bool(data & 1 != 0),
            d2: Level::from_bool(data & 2 != 0),
            d3: Level::from_bool(data & 4 != 0),
            d4: Level::from_bool(data & 8 != 0),
            clock,
        }
    }

    /// The four data pins, in register order.
    const fn data(self) -> [Level; SECTION_COUNT] {
        [self.d1, self.d2, self.d3, self.d4]
    }
}

/// What the part drives on its seven output pins (`CD4006B-002`,
/// `CD4006B-005`, `CD4006B-006`).
///
/// Every output is a stage's own contents brought to a pin, except
/// `d1_4_delayed`, which is the latch's. `Default` is power-up, every pin
/// [`Logic::Undetermined`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Outputs {
    /// `D1+4`, pin 13 — the first register's fourth stage.
    pub d1_4: Logic,
    /// `D1+4'`, pin 2 — `D1+4` delayed one half clock cycle (`CD4006B-006`).
    pub d1_4_delayed: Logic,
    /// `D2+4`, pin 11 — the second register's fourth stage.
    pub d2_4: Logic,
    /// `D2+5`, pin 12 — the second register's fifth stage.
    pub d2_5: Logic,
    /// `D3+4`, pin 10 — the third register's fourth stage.
    pub d3_4: Logic,
    /// `D4+4`, pin 8 — the fourth register's fourth stage.
    pub d4_4: Logic,
    /// `D4+5`, pin 9 — the fourth register's fifth stage.
    pub d4_5: Logic,
}

/// What the four registers' stages hold, as [`Cd4006b::q`] reads them and
/// [`Cd4006b::with_state`] takes them: stage 1 first, each register as long
/// as the sheet's (`CD4006B-001`). A stage nothing has been shifted into since
/// power-up is [`Logic::Undetermined`]; `Default` is every stage so.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Stages {
    /// `D1`'s four stages.
    pub d1: [Logic; 4],
    /// `D2`'s five.
    pub d2: [Logic; 5],
    /// `D3`'s four.
    pub d3: [Logic; 4],
    /// `D4`'s five.
    pub d4: [Logic; 5],
}

/// One register's stages as the part packs them: the levels, stage 1 in bit
/// 0, and which of them hold one.
const fn pack(stages: &[Logic]) -> (u8, u8) {
    let (mut levels, mut known) = (0, 0);
    let mut stage = 0;
    while stage < stages.len() {
        match stages[stage] {
            Logic::High => {
                levels |= 1 << stage;
                known |= 1 << stage;
            }
            Logic::Low => known |= 1 << stage,
            Logic::Undetermined => {}
        }
        stage += 1;
    }
    (levels, known)
}

/// The part.
///
/// `Default` is power-up: every stage and the latch
/// [`Logic::Undetermined`] and the clock remembered Low, because the sheet
/// gives no power-on state (`CD4006B-OPEN-01`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Cd4006b {
    /// The four registers, `D1`'s first: stage 1 in bit 0, stage `n` in bit
    /// `n − 1`, nothing above the register's length (`CD4006B-001`).
    registers: [u8; SECTION_COUNT],
    /// Which stages of each register hold a level, laid out as `registers`; a
    /// clear bit is a stage nothing has been shifted into since power-up.
    known: [u8; SECTION_COUNT],
    /// The `D1+4'` latch (`CD4006B-006`).
    latch: Logic,
    /// The level of `CLOCK` this part was last shown.
    ///
    /// A part at its pins cannot see an edge, only two levels in succession;
    /// the edge is the change between them. It is not a nineteenth stage and
    /// no pin reports it.
    clock: bool,
}

impl Cd4006b {
    /// The part with given stages, a given latch and a given clock level
    /// remembered.
    ///
    /// For a caller that has to reach a state rather than clock its way to
    /// one. `with_state(p.q(), p.outputs().d1_4_delayed, p.clock_level())` is
    /// `p`.
    #[must_use]
    pub const fn with_state(stages: Stages, latch: Logic, clock: Level) -> Self {
        let packed = [
            pack(&stages.d1),
            pack(&stages.d2),
            pack(&stages.d3),
            pack(&stages.d4),
        ];
        Self {
            registers: [packed[0].0, packed[1].0, packed[2].0, packed[3].0],
            known: [packed[0].1, packed[1].1, packed[2].1, packed[3].1],
            latch,
            clock: clock.is_high(),
        }
    }

    /// The bits a register of this index can hold.
    const fn mask(index: usize) -> u8 {
        (1 << STAGES_PER_SECTION[index]) - 1
    }

    /// What the four registers' stages hold.
    #[must_use]
    pub const fn q(&self) -> Stages {
        let mut stages = Stages {
            d1: [Logic::Undetermined; 4],
            d2: [Logic::Undetermined; 5],
            d3: [Logic::Undetermined; 4],
            d4: [Logic::Undetermined; 5],
        };
        let mut stage = 1;
        while stage <= 5 {
            if stage <= 4 {
                stages.d1[stage - 1] = self.stage(0, stage);
                stages.d3[stage - 1] = self.stage(2, stage);
            }
            stages.d2[stage - 1] = self.stage(1, stage);
            stages.d4[stage - 1] = self.stage(3, stage);
            stage += 1;
        }
        stages
    }

    /// Stage `stage` (from 1) of register `index`.
    const fn stage(&self, index: usize, stage: usize) -> Logic {
        let bit = 1 << (stage - 1);
        if self.known[index] & bit == 0 {
            Logic::Undetermined
        } else {
            Logic::from_bool(self.registers[index] & bit != 0)
        }
    }

    /// The `CLOCK` level the part was last shown.
    #[must_use]
    pub const fn clock_level(&self) -> Level {
        Level::from_bool(self.clock)
    }

    /// What the part drives on its seven output pins, which is what its stages
    /// and its latch hold and no computation (`CD4006B-005`, `CD4006B-006`).
    #[must_use]
    pub const fn outputs(&self) -> Outputs {
        Outputs {
            d1_4: self.stage(0, 4),
            d1_4_delayed: self.latch,
            d2_4: self.stage(1, 4),
            d2_5: self.stage(1, 5),
            d3_4: self.stage(2, 4),
            d4_4: self.stage(3, 4),
            d4_5: self.stage(3, 5),
        }
    }

    /// Show the part one set of input levels, and read back what it drives.
    ///
    /// - A **fall** on `CLOCK` shifts every register one stage, entering that
    ///   register's data pin as it stands in this call (`CD4006B-003`,
    ///   `CD4006B-004`) and dropping what its last stage held.
    /// - A **rise** on `CLOCK` opens the `D1+4'` latch onto `D1+4`
    ///   (`CD4006B-006`).
    /// - The clock at either level holds everything (`CD4006B-007`).
    pub fn step(&mut self, inputs: Inputs) -> Outputs {
        let falling = self.clock && inputs.clock.is_low();
        let rising = !self.clock && inputs.clock.is_high();
        self.clock = inputs.clock.is_high();
        if rising {
            self.latch = self.outputs().d1_4;
        }
        if falling {
            let data = inputs.data();
            for (index, register) in self.registers.iter_mut().enumerate() {
                *register =
                    ((*register << 1) | u8::from(data[index].is_high())) & Self::mask(index);
                self.known[index] = ((self.known[index] << 1) | 1) & Self::mask(index);
            }
        }
        self.outputs()
    }

    /// One full period of `CLOCK` from wherever it stands: the other level,
    /// then this one again, with the data pins held throughout. `inputs.clock`
    /// is not read: the remembered level decides both halves.
    ///
    /// A convenience for a caller counting periods rather than levels; it is
    /// two calls of [`Cd4006b::step`] and nothing else.
    pub fn tick(&mut self, inputs: Inputs) -> Outputs {
        let other = Inputs {
            clock: Level::from_bool(!self.clock),
            ..inputs
        };
        let back = Inputs {
            clock: !other.clock,
            ..inputs
        };
        let _ = self.step(other);
        self.step(back)
    }
}

#[cfg(test)]
mod tests {
    use super::{Cd4006b, Inputs, Level, Logic};

    /// Power-up is undetermined at every pin, and a register's fourth and fifth
    /// stages take a level on the fourth and fifth falls (`CD4006B-OPEN-01`).
    #[test]
    fn power_up_is_undetermined_until_shifted_in() {
        let mut part = Cd4006b::default();
        assert_eq!(part.outputs(), super::Outputs::default());
        let high = Inputs::from_nibble(0b1111, Level::High);
        let low = Inputs::from_nibble(0b1111, Level::Low);
        for _ in 0..3 {
            let _ = part.step(high);
            let _ = part.step(low);
        }
        assert_eq!(part.outputs().d1_4, Logic::Undetermined);
        let _ = part.step(high);
        assert_eq!(part.outputs().d1_4_delayed, Logic::Undetermined);
        let out = part.step(low);
        assert_eq!(out.d1_4, Logic::High);
        assert_eq!(out.d2_5, Logic::Undetermined);
        let _ = part.step(high);
        let out = part.step(low);
        assert_eq!((out.d1_4_delayed, out.d2_5), (Logic::High, Logic::High));
    }

    /// A part part-way out of power-up, some stages undetermined, is rebuilt
    /// from its own getters.
    #[test]
    fn with_state_rebuilds_an_undetermined_part() {
        let mut part = Cd4006b::default();
        let _ = part.step(Inputs::from_nibble(0b0101, Level::High));
        let _ = part.step(Inputs::from_nibble(0b0101, Level::Low));
        assert_eq!(part.q().d2[1], Logic::Undetermined);
        let rebuilt =
            Cd4006b::with_state(part.q(), part.outputs().d1_4_delayed, part.clock_level());
        assert_eq!(rebuilt, part);
    }
}
