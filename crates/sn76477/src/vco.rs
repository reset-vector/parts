//! The voltage-controlled oscillator.
//!
//! The same charge-discharge pair as the SLF, with one difference that is the
//! whole of the block: the capacitor charges until it reaches **the control
//! voltage** rather than a fixed reference (`SN76477-013`). The charging current
//! is constant, so the time to reach it — and therefore the period — is
//! proportional to that voltage: a higher control voltage gives a lower
//! frequency.
//!
//! Equation 2's minimum frequency is the period at the top of the control
//! range (`SN76477-012`). The duty cycle is the fraction of the charging ramp
//! below pin 19's voltage (`SN76477-017`), which this crate computes as
//! `50 × V19 / V16 %` (`SN76477-016`).

/// The oscillator's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Vco {
    /// Ticks into the current half.
    elapsed: usize,
    /// Whether the capacitor is charging.
    charging: bool,
    /// How long the charging half just ended lasted, which is how long the
    /// discharge back to zero takes at the same slope.
    charge_len: usize,
    /// Which of two consecutive cycles this is, for the alternating envelope.
    alternate: bool,
}

impl Vco {
    /// The control voltage at which the period is
    /// [`crate::Timing::vco_max_period_ticks`], in microvolts — the top of the
    /// external control range (`SN76477-014`).
    pub(crate) const CONTROL_FULL_UV: i32 = 2_350_000;

    /// The control voltage below which the part runs no faster, in
    /// microvolts: a tenth of [`Vco::CONTROL_FULL_UV`], the part running at
    /// most ten times its minimum frequency (`SN76477-015`).
    pub(crate) const CONTROL_FLOOR_UV: i32 = Self::CONTROL_FULL_UV / 10;

    /// A reset oscillator: the capacitor empty and charging.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            elapsed: 0,
            charging: true,
            charge_len: 0,
            alternate: false,
        }
    }

    /// Where on the charging ramp a voltage of `uv` falls, in ticks from the
    /// bottom; a negative voltage is the bottom.
    ///
    /// The ramp's slope is fixed by `RVCO` and `CVCO` and does not depend on
    /// what it is climbing towards, so this is one line for both the ceiling the
    /// capacitor charges to and the pitch voltage the duty-cycle comparator
    /// trips at.
    #[must_use]
    pub(crate) const fn ramp_ticks(min_period_ticks: usize, uv: i32) -> usize {
        let uv = if uv < 0 { 0 } else { uv as u128 };
        (min_period_ticks as u128 * uv / (2 * Self::CONTROL_FULL_UV as u128)) as usize
    }

    /// Ticks the charging half takes at a control voltage of `control_uv`.
    ///
    /// The floor is [`Vco::CONTROL_FLOOR_UV`]: the data sheet puts the frequency range at an
    /// approximate 10:1 determined inside the part, so a control voltage below a
    /// tenth of the range does not go on making the part faster.
    /// This clamp's exact shape is this crate's choice under
    /// `SN76477-OPEN-09`.
    #[must_use]
    pub(crate) const fn half_at(min_period_ticks: usize, control_uv: i32) -> usize {
        let floor = Self::CONTROL_FLOOR_UV;
        let clamped = if control_uv < floor {
            floor
        } else if control_uv > Self::CONTROL_FULL_UV {
            Self::CONTROL_FULL_UV
        } else {
            control_uv
        };
        Self::ramp_ticks(min_period_ticks, clamped)
    }

    /// One tick at a control voltage of `control_uv`.
    ///
    /// The control voltage is read every tick rather than once a cycle, which is
    /// what makes a triangle on it come out as the swept tone of Figure 2 rather
    /// than as a staircase of constant tones.
    pub(crate) const fn tick(&mut self, min_period_ticks: usize, control_uv: i32) {
        if min_period_ticks == 0 {
            return;
        }
        self.elapsed += 1;
        if self.charging {
            if self.elapsed >= Self::half_at(min_period_ticks, control_uv) {
                self.charge_len = self.elapsed;
                self.elapsed = 0;
                self.charging = false;
            }
        } else if self.elapsed >= self.charge_len {
            self.elapsed = 0;
            self.charging = true;
            self.alternate = !self.alternate;
        }
    }

    /// The square wave the mixer and the envelope select logic see.
    ///
    /// High from the bottom of the charging ramp until the capacitor passes the
    /// pitch voltage, and low for the rest of the cycle. With pin 19 above the
    /// control voltage that is the whole charging half and the duty cycle is
    /// 50%, which is what the data sheet says leaving pin 19 high does.
    #[must_use]
    pub(crate) const fn level(
        self,
        min_period_ticks: usize,
        control_uv: i32,
        pitch_uv: i32,
    ) -> bool {
        if min_period_ticks == 0 {
            return false;
        }
        let ceiling = Self::half_at(min_period_ticks, control_uv);
        let trip = Self::ramp_ticks(min_period_ticks, pitch_uv);
        let trip = if trip > ceiling { ceiling } else { trip };
        self.charging && self.elapsed < trip
    }

    /// Whether this is the second of two consecutive cycles, which is what the
    /// `VCO with alternating cycles` envelope divides on (`SN76477-033`).
    #[must_use]
    pub(crate) const fn alternate(self) -> bool {
        self.alternate
    }
}

impl Default for Vco {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::Vco;

    /// `SN76477-015`: the frequency span is held to ten to one, with no change
    /// below the floor or above the top of the range.
    #[test]
    fn span_is_ten_to_one() {
        const MAX_PERIOD: usize = 4_700;
        let full = Vco::half_at(MAX_PERIOD, Vco::CONTROL_FULL_UV);
        let floor = Vco::half_at(MAX_PERIOD, 0);
        assert_eq!(full / floor, 10);
        assert_eq!(Vco::CONTROL_FLOOR_UV * 10, Vco::CONTROL_FULL_UV);
        for uv in [0, 1_000, 100_000, 234_000] {
            assert_eq!(Vco::half_at(MAX_PERIOD, uv), floor);
        }
        for uv in [2_351_000, 5_000_000, 15_000_000] {
            assert_eq!(Vco::half_at(MAX_PERIOD, uv), full);
        }
    }
}
