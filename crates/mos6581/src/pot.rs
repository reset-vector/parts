//! One potentiometer-to-digital converter.
//!
//! `MOS6581-046` and `MOS6581-047` give a 512-cycle update, a trigger at
//! `Vcc / 2`, zero at minimum resistance and 255 at maximum. How the window
//! divides between sinking the capacitor and counting, and when the register
//! loads, are `MOS6581-065`: a nine-bit counter whose bit 8 sinks the pin, a
//! comparator that loads bits 7–0 at the crossing, and 255 at the rollover when
//! nothing crossed. Which half follows `RES` is this crate's choice: it sinks
//! first.

use units::Microvolts;

/// The conversion window, in `Ø2` cycles (`MOS6581-046`).
pub const POTS_PERIOD_CYCLES: usize = 512;

/// The cycles at the start of each window during which the pin is sunk
/// (`MOS6581-065`).
pub const SINK_CYCLES: usize = 256;

/// The trigger at a `Vcc` of `vcc_uv`, in microvolts: `Vcc / 2`
/// (`MOS6581-047`). It scales with the supply; this crate's converter runs at
/// `Vcc = 5 V`, where it is 2.5 V.
#[must_use]
pub const fn trigger_uv(vcc_uv: Microvolts) -> Microvolts {
    Microvolts(vcc_uv.0 / 2)
}

const VCC_SUPPLY_TYP_UV: Microvolts = Microvolts(5_000_000);

/// [`trigger_uv`] at the supply the converter is modelled at.
const TRIGGER_UV: i32 = trigger_uv(VCC_SUPPLY_TYP_UV).0;

/// One converter.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Pot {
    /// The nine-bit counter: where in the window this cycle is.
    phase: usize,
    /// Whether the pin has crossed the trigger in this window's count.
    crossed: bool,
    /// The value presented in the register.
    value: u8,
}

impl Pot {
    /// As `RES` leaves it: the window at its start, the register zero.
    #[must_use]
    pub const fn reset() -> Self {
        Self {
            phase: 0,
            crossed: false,
            value: 0,
        }
    }

    /// The register value (`MOS6581-046`).
    #[must_use]
    pub const fn value(&self) -> u8 {
        self.value
    }

    /// One `Ø2` cycle with the pin at `pin_uv`; whether the part sinks the
    /// pin in this cycle.
    ///
    /// For the first [`SINK_CYCLES`] of a window the pin is sunk. For the
    /// rest, the cycle the pin is first seen at [`trigger_uv`] of 5 V loads the
    /// register there and then (`MOS6581-065`); if it never is, the rollover
    /// loads 255.
    ///
    /// `pin_uv` is the level the pin reached in answer to the cycle before,
    /// so a crossing seen on counting cycle `k` happened while the counter's
    /// low bits read `k − 1`, and that is the value loaded. A pin at minimum
    /// resistance is first seen high on counting cycle 1 and reads 0, as
    /// `MOS6581-046` requires; a crossing on the last counting cycle is seen
    /// only after the rollover, which has loaded the 255 it would have.
    pub const fn step(&mut self, pin_uv: i32) -> bool {
        let sinking = self.phase < SINK_CYCLES;
        if !sinking && !self.crossed && pin_uv >= TRIGGER_UV {
            self.crossed = true;
            self.value = (self.phase - SINK_CYCLES).saturating_sub(1) as u8;
        }
        self.phase += 1;
        if self.phase == POTS_PERIOD_CYCLES {
            if !self.crossed {
                self.value = u8::MAX;
            }
            self.phase = 0;
            self.crossed = false;
        }
        sinking
    }
}

#[cfg(test)]
mod tests {
    use super::{POTS_PERIOD_CYCLES, Pot, SINK_CYCLES, TRIGGER_UV};

    /// `MOS6581-065`: a crossing first seen on counting cycle `k` loads
    /// `k − 1` there and then, not at the rollover; no crossing loads 255 at
    /// the rollover and not before.
    #[test]
    fn the_crossing_loads_the_counter_when_it_is_seen() {
        for k in 1..=u8::MAX {
            let mut pot = Pot::reset();
            let seen = SINK_CYCLES + usize::from(k);
            for phase in 0..=seen {
                pot.step(if phase < seen { 0 } else { TRIGGER_UV });
            }
            assert_eq!(pot.value(), k - 1, "loaded at the crossing");
            for _ in seen + 1..POTS_PERIOD_CYCLES {
                pot.step(TRIGGER_UV);
            }
            assert_eq!(pot.value(), k - 1, "kept over the rollover");
        }
        let mut pot = Pot::reset();
        for _ in 0..POTS_PERIOD_CYCLES - 1 {
            pot.step(0);
        }
        assert_eq!(pot.value(), 0, "nothing loaded before the rollover");
        pot.step(0);
        assert_eq!(pot.value(), u8::MAX, "the rollover without a crossing");
    }
}
