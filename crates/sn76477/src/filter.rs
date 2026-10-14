//! The noise filter.
//!
//! Not an RC network: the shift register's output steers an `I`/`2I` pair on
//! `CNF`, so the capacitor ramps at a fixed slope in whichever direction the
//! register last asked for, and a comparator with hysteresis reads it back out
//! as a digital signal (`SN76477-022`). What that does to the noise is slew
//! limiting — a run of the register too short to carry the capacitor across the
//! window never reaches the comparator at all.

/// The comparator's upper threshold, in millivolts — `SN76477-022`.
#[cfg(test)]
const HIGH_MV: u32 = 2_500;

/// The comparator's lower threshold, in millivolts — `SN76477-022`.
#[cfg(test)]
const LOW_MV: u32 = 450;

/// The filter's state: the capacitor, and what the comparator last said.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct NoiseFilter {
    /// Ticks of ramp travelled up from the lower threshold, out of `traverse_ticks`.
    position: usize,
    output: bool,
}

impl NoiseFilter {
    /// The comparator's lower and upper thresholds, in microvolts, as
    /// `(min, max)` — `SN76477-022`.
    #[cfg(test)]
    const THRESHOLD_RANGE_UV: (i32, i32) = (LOW_MV as i32 * 1000, HIGH_MV as i32 * 1000);

    /// A reset filter: the capacitor at the bottom of the window, output low.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            position: 0,
            output: false,
        }
    }

    /// One tick. `noise` is the shift register's output and `traverse_ticks` is
    /// [`crate::Timing::noise_filter_traverse_ticks`].
    ///
    /// The register's output low charges the capacitor and high discharges it,
    /// which is the applications paper's own polarity and the reason this block
    /// inverts (`SN76477-022`).
    pub(crate) const fn tick(&mut self, noise: bool, traverse_ticks: usize) {
        if traverse_ticks == 0 {
            self.output = !noise;
            return;
        }
        if noise {
            self.position = self.position.saturating_sub(1);
        } else if self.position < traverse_ticks {
            self.position += 1;
        }
        if self.position >= traverse_ticks {
            self.output = true;
        } else if self.position == 0 {
            self.output = false;
        }
    }

    /// The filtered noise the mixer sees.
    #[must_use]
    pub(crate) const fn level(self) -> bool {
        self.output
    }

    /// The capacitor voltage in microvolts, within
    /// [`NoiseFilter::THRESHOLD_RANGE_UV`], in whole millivolts.
    #[cfg(test)]
    const fn capacitor_uv(self, traverse_ticks: usize) -> i32 {
        self.capacitor_mv(traverse_ticks) as i32 * 1000
    }

    /// [`NoiseFilter::capacitor_uv`] in millivolts, the resolution it is
    /// computed at.
    #[cfg(test)]
    const fn capacitor_mv(self, traverse_ticks: usize) -> u32 {
        if traverse_ticks == 0 {
            return LOW_MV;
        }
        let span = (HIGH_MV - LOW_MV) as u64;
        LOW_MV + (span as u128 * self.position as u128 / traverse_ticks as u128) as u32
    }
}

impl Default for NoiseFilter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::NoiseFilter;

    /// `SN76477-022`, `SN76477-023`: a run shorter than the traverse never
    /// leaves the block, a longer one arrives whole, and the capacitor stays
    /// inside the comparator's window.
    #[test]
    fn a_slew_limit() {
        const TRAVERSE: usize = 100;
        let mut filter = NoiseFilter::new();
        for _ in 0..TRAVERSE * 4 {
            filter.tick(true, TRAVERSE);
        }
        assert!(!filter.level());
        for _ in 0..TRAVERSE - 1 {
            filter.tick(false, TRAVERSE);
        }
        assert!(!filter.level(), "a short run reached the mixer");
        filter.tick(false, TRAVERSE);
        assert!(filter.level(), "a long enough run did not");

        let transitions = |period: usize, high: usize| {
            let mut filter = NoiseFilter::new();
            let mut count = 0u32;
            let mut previous = filter.level();
            for tick in 0..10_000usize {
                filter.tick(tick % period < high, TRAVERSE);
                if filter.level() != previous {
                    count += 1;
                    previous = filter.level();
                }
            }
            count
        };
        assert_eq!(transitions(8, 4), 0);
        assert!(transitions(TRAVERSE * 4, TRAVERSE * 2) > 20);

        let mut filter = NoiseFilter::new();
        let (low, high) = NoiseFilter::THRESHOLD_RANGE_UV;
        for tick in 0..10_000u32 {
            filter.tick(tick % 37 < 18, TRAVERSE);
            assert!((low..=high).contains(&filter.capacitor_uv(TRAVERSE)));
        }
    }
}
