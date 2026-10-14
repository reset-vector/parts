//! The super-low frequency oscillator.
//!
//! A current mirror charges `CSLF` at `I` and a doubled mirror discharges it at
//! a net `−I`, so the two sides of the triangle have the same slope and the
//! comparator's output has equal on and off times — `SN76477-009` and
//! `SN76477-010`. The block supplies two signals: a square wave to the mixer,
//! high while the capacitor charges, and the triangle itself to the VCO.

/// The comparator's lower limit, in millivolts — `SN76477-010`.
const LOW_MV: u32 = 350;

/// The comparator's upper limit, in millivolts — `SN76477-010`.
const HIGH_MV: u32 = 2_500;

/// The oscillator's state: where it is in the current cycle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Slf {
    /// Ticks into the current cycle, `0` at the bottom of the triangle.
    elapsed: usize,
}

impl Slf {
    /// The comparator's lower and upper limits, in microvolts, as
    /// `(min, max)` — `SN76477-010`.
    #[cfg(test)]
    const THRESHOLD_RANGE_UV: (i32, i32) = (LOW_MV as i32 * 1000, HIGH_MV as i32 * 1000);

    /// A reset oscillator, at the bottom of the triangle.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self { elapsed: 0 }
    }

    /// One tick. `period_ticks` is [`crate::Timing::slf_period_ticks`]; a period of zero
    /// leaves the oscillator where it is, there being no such part.
    pub(crate) const fn tick(&mut self, period_ticks: usize) {
        if period_ticks == 0 {
            return;
        }
        self.elapsed += 1;
        if self.elapsed >= period_ticks {
            self.elapsed = 0;
        }
    }

    /// The square wave the mixer sees: high over the charging half.
    #[must_use]
    pub(crate) const fn level(self, period_ticks: usize) -> bool {
        period_ticks != 0 && self.elapsed < period_ticks.div_ceil(2)
    }

    /// The capacitor voltage in microvolts, which is what the VCO sees when
    /// `VCO SELECT` is high.
    ///
    /// A straight line across the comparator's window and back,
    /// because the charge and discharge currents have the same magnitude; it
    /// moves in whole millivolts.
    #[must_use]
    pub(crate) const fn triangle_uv(self, period_ticks: usize) -> i32 {
        self.triangle_mv(period_ticks) as i32 * 1000
    }

    /// [`Slf::triangle_uv`] in millivolts, the resolution it is computed at.
    const fn triangle_mv(self, period_ticks: usize) -> u32 {
        if period_ticks == 0 {
            return LOW_MV;
        }
        let half = period_ticks.div_ceil(2);
        let (into_ramp, ramp) = if self.elapsed < half {
            (self.elapsed, half)
        } else {
            (period_ticks - self.elapsed, period_ticks - half)
        };
        if ramp == 0 {
            return LOW_MV;
        }
        let span = (HIGH_MV - LOW_MV) as u64;
        LOW_MV + (span as u128 * into_ramp as u128 / ramp as u128) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::Slf;

    /// `SN76477-010`, `SN76477-011`: equal halves, and a triangle that is a
    /// straight line between the comparator's two limits.
    #[test]
    fn halves_and_triangle() {
        const PERIOD: usize = 4_000;
        let mut slf = Slf::new();

        let mut high = 0usize;
        let mut lowest = i32::MAX;
        let mut highest = 0i32;
        let mut previous = slf.triangle_uv(PERIOD);
        let (mut steps_up, mut steps_down) = (0u32, 0u32);
        for _ in 0..PERIOD {
            slf.tick(PERIOD);
            if slf.level(PERIOD) {
                high += 1;
            }
            let level = slf.triangle_uv(PERIOD);
            assert_eq!(level % 1000, 0, "not a whole millivolt");
            lowest = lowest.min(level);
            highest = highest.max(level);
            match level.cmp(&previous) {
                core::cmp::Ordering::Greater => steps_up += 1,
                core::cmp::Ordering::Less => steps_down += 1,
                core::cmp::Ordering::Equal => {}
            }
            previous = level;
        }

        assert_eq!(high, PERIOD / 2);
        assert_eq!((lowest, highest), Slf::THRESHOLD_RANGE_UV);
        assert_eq!(steps_up, steps_down);

        let mut stopped = Slf::new();
        stopped.tick(0);
        assert!(!stopped.level(0));
        assert_eq!(stopped.triangle_uv(0), Slf::THRESHOLD_RANGE_UV.0);
    }
}
