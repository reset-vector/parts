//! The Texas Instruments SN74191 and SN74LS191 synchronous up/down 4-bit binary
//! counter, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **The two grades compute one function** (`SN74191-001`): one logic diagram
//! is drawn for both, and they part company only in currents and voltages,
//! which this crate does not model. [`Sn74191`] is either.
//!
//! **The count moves on a rising edge of `CLK` and a load moves it at once**
//! (`SN74191-006`, `SN74191-009`), so the part holds its four bits and the
//! clock level it last saw: an edge is a change between two levels, and a part
//! at its pins is only ever shown one at a time. The part has no clear, and
//! its count is [`Count::Undetermined`] from power-up until a load
//! (`SN74191-OPEN-03`).
//!
//! **`MAX/MIN` and `RCO` are not state.** They are a decode of the four bits,
//! `D/U`, `CTEN` and `CLK` (`SN74191-013`, `SN74191-015`), so
//! [`Sn74191::outputs`] reads them off the levels the part was last shown, which
//! it remembers as it remembers the clock's.
//!
//! **The two conditions the data sheet places on the board are not modelled**
//! (`SN74191-018`, `SN74191-019`): `CTEN` and `D/U` are taken as they stand at
//! the rising edge, whenever they last moved. `SN74191-OPEN-02` is what the
//! sheet does not say about a board that breaks them.
//!
//! **There are no nanoseconds here.** The setup, hold and pulse-width figures
//! are conditions a board meets so that an edge means what `SN74191-009` says
//! it means, and this crate assumes they are met: an edge shown with `LOAD`
//! already High counts (`SN74191-OPEN-04`).
//!
//! # Examples
//!
//! A load fixes the count, and a rising clock edge counts it up by one.
//!
//! ```
//! use sn74191::{Count, Inputs, Level, Nibble, Sn74191};
//!
//! let mut part = Sn74191::default();
//! assert_eq!(part.q(), Count::Undetermined);
//! let up = Inputs { d_u: Level::Low, ..Inputs::default() };
//! part.step(Inputs { abcd: 14, load_n: Level::Low, ..up });
//! part.step(up);
//! part.step(Inputs { clk: Level::High, ..up });
//! assert_eq!(part.q(), Count::Known(Nibble::new(15)));
//! ```

#![no_std]

/// How many pins the package presents (`SN74191-002`).
pub const PIN_COUNT: usize = 16;

/// The counter's four outputs, in the low four bits of a byte.
///
/// Bit 0 is the output the data sheet calls `QA`, the least significant
/// (`SN74191-001`). Nothing above bit 3 leaves a pin, and every value this
/// crate produces is masked with this.
pub const Q_MASK: u8 = 0x0F;

/// The highest count, which `MAX/MIN` decodes counting up (`SN74191-013`).
pub const LAST_STATE: u8 = 0x0F;

/// The levels the outside world holds the part's inputs at.
///
/// **A field is a level, not a condition**, the pin's own. `LOAD` and `CTEN`
/// are drawn with a bar over them on the package (`SN74191-002`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `A`–`D`, in the low four bits, `A` at bit 0.
    pub abcd: u8,
    /// The `LOAD` pin; High is the part *not* being loaded
    /// (`SN74191-006`).
    pub load_n: Level,
    /// The `CTEN` pin; High inhibits counting (`SN74191-007`).
    pub cten_n: Level,
    /// The `D/U` pin; High counts down and Low counts up
    /// (`SN74191-008`).
    pub d_u: Level,
    /// The `CLK` pin.
    pub clk: Level,
}

impl Inputs {
    /// `LOAD` High, `CTEN` Low and `D/U` High: the part counting, down, with the
    /// clock Low and nothing on the data inputs.
    ///
    /// `CTEN` defaults to its enabling level and `D/U`, a direction pin, High.
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            abcd: 0,
            load_n: Level::High,
            cten_n: Level::Low,
            d_u: Level::High,
            clk: Level::Low,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// What the part drives, on all six output pins at once.
///
/// Every field is always driven: the part has no output control and no
/// high-impedance state. A field is [`Logic::Undetermined`] only while the
/// count is (`SN74191-OPEN-03`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Outputs {
    /// `QA`–`QD`, `QA` at index 0.
    pub q: [Logic; 4],
    /// The `MAX/MIN` pin (`SN74191-013`).
    pub max_min: Logic,
    /// The `RCO` pin, Low while it pulses (`SN74191-015`).
    pub rco_n: Logic,
}

/// A pin's level the part always determines, in a pin word's field, an
/// argument and a getter's result.
pub use signal::Level;
/// A level an output pin stands at. `Undetermined` is a level the sheet does
/// not fix: a count held since power-up with no load, and what `MAX/MIN` and
/// `RCO` decode from it (`SN74191-OPEN-03`).
pub use signal::Logic;

/// Four bits, `QA`–`QD` with `QA` at bit 0: a count [`Count::Known`]
/// holds. Every one of the sixteen is a count the part can hold.
pub use signal::Nibble;

/// The count the four flip-flops hold, as [`Sn74191::q`] reads it.
///
/// A count is one value of four bits or none, never some bits known and some
/// not: power-up fixes none of them and a load fixes all four
/// (`SN74191-OPEN-03`, `SN74191-006`). So it is this enum rather than a
/// [`Logic`] per bit, which would admit mixtures the part never holds and
/// repeat [`Outputs::q`], and rather than an `Option`, which on a level means
/// released (`None`) and nothing else.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Count {
    /// The four bits `QA`–`QD`.
    Known(Nibble),
    /// A count held since power-up with no load.
    #[default]
    Undetermined,
}

/// The part.
///
/// `Default` stands at power-up. The data sheet fixes no level there and the
/// part has no clear, so the count is [`Count::Undetermined`], with the levels
/// of [`Inputs::default`] remembered (`SN74191-OPEN-03`), until a board
/// asserts `LOAD` (`SN74191-006`). [`Sn74191::with_state`] builds the part at
/// any count, `with_state(p.q(), p.clk(), p.cten_n(), p.d_u())` being `p`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Sn74191 {
    /// The four bits the package holds, masked to [`Q_MASK`].
    count: Count,
    /// The `CLK` level this part was last shown.
    clk: Level,
    /// The `CTEN` level this part was last shown.
    cten_n: Level,
    /// The `D/U` level this part was last shown.
    d_u: Level,
}

/// The count after `q`, up or down, modulo 16 (`SN74191-011`).
fn next(q: u8, down: bool) -> u8 {
    if down {
        q.wrapping_sub(1) & Q_MASK
    } else {
        q.wrapping_add(1) & Q_MASK
    }
}

impl Default for Sn74191 {
    /// Power-up, which the data sheet does not fix: the count undetermined,
    /// with `CLK` Low, `CTEN` Low and `D/U` High remembered.
    fn default() -> Self {
        Self {
            count: Count::Undetermined,
            clk: Level::Low,
            cten_n: Level::Low,
            d_u: Level::High,
        }
    }
}

impl Sn74191 {
    /// The part with a given count held and given `CLK`, `CTEN` and `D/U`
    /// levels remembered.
    ///
    /// For a caller that has to reach a state rather than clock its way to one.
    /// [`Count::Undetermined`] is the count at power-up.
    #[must_use]
    pub const fn with_state(q: Count, clk: Level, cten_n: Level, d_u: Level) -> Self {
        Self {
            count: q,
            clk,
            cten_n,
            d_u,
        }
    }

    /// The count the package holds, [`Count::Undetermined`] from power-up to
    /// the first load.
    #[must_use]
    pub const fn q(&self) -> Count {
        self.count
    }

    /// The `CLK` level the part was last shown.
    #[must_use]
    pub const fn clk(&self) -> Level {
        self.clk
    }

    /// The `CTEN` level the part was last shown.
    #[must_use]
    pub const fn cten_n(&self) -> Level {
        self.cten_n
    }

    /// The `D/U` level the part was last shown.
    #[must_use]
    pub const fn d_u(&self) -> Level {
        self.d_u
    }

    /// What the part drives, given what it holds and the levels it was last
    /// shown.
    ///
    /// `MAX/MIN` is High at 15 counting up and at 0 counting down
    /// (`SN74191-013`), and follows `D/U` with no edge (`SN74191-014`). `RCO` is
    /// Low exactly while `CLK` is Low, `CTEN` is Low and `MAX/MIN` is High
    /// (`SN74191-015`, `SN74191-016`). While the count is undetermined, so
    /// are `QA`–`QD` and `MAX/MIN`, and `RCO` but where `CLK` or `CTEN` holds
    /// it High.
    #[must_use]
    pub fn outputs(&self) -> Outputs {
        let enabled = self.clk.is_low() && self.cten_n.is_low();
        let Count::Known(nibble) = self.count else {
            return Outputs {
                q: [Logic::Undetermined; 4],
                max_min: Logic::Undetermined,
                rco_n: if enabled {
                    Logic::Undetermined
                } else {
                    Logic::High
                },
            };
        };
        let q = nibble.value();
        let max_min = if self.d_u.is_high() {
            q == 0
        } else {
            q == LAST_STATE
        };
        let bit = |i: u8| Logic::from_bool(q >> i & 1 != 0);
        Outputs {
            q: [bit(0), bit(1), bit(2), bit(3)],
            max_min: Logic::from_bool(max_min),
            rco_n: Logic::from_bool(!(enabled && max_min)),
        }
    }

    /// Show the part one set of input levels, and read back what it drives.
    ///
    /// - `LOAD` Low puts `A`–`D` on the outputs with no edge, whatever the
    ///   clock and the enable are doing (`SN74191-006`, `SN74191-010`); the
    ///   other levels are still taken up, because the pins are what they are
    ///   whatever the part is doing about them;
    /// - otherwise a rising edge of `CLK` with `CTEN` Low counts one step in the
    ///   direction `D/U` gives (`SN74191-007`, `SN74191-008`, `SN74191-009`);
    /// - anything else holds: `CTEN` High, a clock standing at either level, and
    ///   a falling edge; and an undetermined count counted stays undetermined.
    pub fn step(&mut self, inputs: Inputs) -> Outputs {
        let rising = inputs.clk.is_high() && self.clk.is_low();
        self.clk = inputs.clk;
        self.cten_n = inputs.cten_n;
        self.d_u = inputs.d_u;
        if inputs.load_n.is_low() {
            self.count = Count::Known(Nibble::new(inputs.abcd));
        } else if let Count::Known(nibble) = self.count
            && rising
            && inputs.cten_n.is_low()
        {
            self.count = Count::Known(Nibble::new(next(nibble.value(), inputs.d_u.is_high())));
        }
        self.outputs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SN74191-OPEN-03`: no count until a load, and a count counted stays none.
    #[test]
    fn power_up_holds_no_count_until_a_load() {
        let mut part = Sn74191::default();
        let out = part.outputs();
        assert_eq!(out.q, [Logic::Undetermined; 4]);
        assert_eq!(out.max_min, Logic::Undetermined);
        assert_eq!(out.rco_n, Logic::Undetermined);
        let up = Inputs {
            d_u: Level::Low,
            ..Inputs::default()
        };
        let _ = part.step(Inputs {
            clk: Level::High,
            ..up
        });
        let out = part.step(Inputs {
            clk: Level::Low,
            ..up
        });
        assert_eq!(
            out.rco_n,
            Logic::Undetermined,
            "no carry from a count of none"
        );
        assert_eq!(
            part.step(Inputs {
                clk: Level::High,
                ..up
            })
            .rco_n,
            Logic::High
        );
        let out = part.step(Inputs {
            abcd: 5,
            load_n: Level::Low,
            ..up
        });
        assert_eq!(
            out.q,
            [Logic::High, Logic::Low, Logic::High, Logic::Low],
            "a load fixes the count"
        );
        assert_eq!(
            part,
            Sn74191::with_state(
                Count::Known(Nibble::new(5)),
                Level::Low,
                Level::Low,
                Level::Low
            )
        );
    }

    /// A part rebuilt from its getters is the part, at power-up as at a count.
    #[test]
    fn with_state_from_the_getters_rebuilds_the_part() {
        let rebuilt = |p: Sn74191| Sn74191::with_state(p.q(), p.clk(), p.cten_n(), p.d_u());
        let power_up = Sn74191::default();
        assert_eq!(power_up.q(), Count::Undetermined);
        assert_eq!(rebuilt(power_up), power_up);
        assert_eq!(rebuilt(power_up).outputs(), power_up.outputs());
        let counted = Sn74191::with_state(
            Count::Known(Nibble::new(0xF7)),
            Level::High,
            Level::Low,
            Level::Low,
        );
        assert_eq!(
            counted.q(),
            Count::Known(Nibble::new(7)),
            "a nibble is four bits"
        );
        assert_eq!(rebuilt(counted), counted);
    }
}
