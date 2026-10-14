//! The one-shot.
//!
//! A latch set by a high-to-low edge at pin 9, a current source charging `COS`
//! while it is set, and a comparator that resets the latch when the capacitor
//! reaches its reference — `SN76477-026` to `SN76477-029`. It generates no
//! sound; it is one of the four envelopes the mixer output can be given.

/// The comparator's reference, in millivolts — `SN76477-028`.
#[cfg(test)]
const TRIP_MV: u32 = 2_500;

/// The latch and the capacitor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct OneShot {
    running: bool,
    elapsed: usize,
    inhibit_was_high: bool,
}

impl OneShot {
    /// The comparator's reference, in microvolts — `SN76477-028`.
    #[cfg(test)]
    const TRIP_UV: i32 = TRIP_MV as i32 * 1000;

    /// A reset one-shot: latch clear, capacitor empty, pin 9 taken as low so
    /// that a first edge has to be a real one.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            running: false,
            elapsed: 0,
            inhibit_was_high: false,
        }
    }

    /// One tick.
    ///
    /// `inhibit` is pin 9 and its falling edge is the trigger; `terminate` is
    /// pin 23 taken high, which ends the pulse without waiting for the
    /// capacitor (`SN76477-029`). `duration_ticks` is
    /// [`crate::Timing::one_shot_ticks`]; untimed, `None`, when no `ROS` and
    /// no `COS` are fitted: the mirror has no reference current, nothing
    /// charges the capacitor, and `terminate` is the only end (`SN76477-046`).
    /// What a pin 23 that nobody drives does to such a pulse is
    /// `SN76477-OPEN-11`, and this reading is that it does nothing.
    ///
    /// Taking pin 9 back high does **not** end the timing: the data sheet says
    /// the one-shot must be allowed to end for the latch to reset before
    /// another can be triggered, so the counter here runs on while the sound it
    /// was gating is already silent.
    pub(crate) const fn tick(
        &mut self,
        inhibit: bool,
        terminate: bool,
        duration_ticks: Option<usize>,
    ) {
        let falling = self.inhibit_was_high && !inhibit;
        self.inhibit_was_high = inhibit;

        if falling && !self.running {
            if matches!(duration_ticks, Some(0)) {
                return;
            }
            self.running = true;
            self.elapsed = 0;
            return;
        }
        if !self.running {
            return;
        }
        if terminate {
            self.running = false;
            return;
        }
        let Some(duration_ticks) = duration_ticks else {
            return;
        };
        self.elapsed += 1;
        if self.elapsed >= duration_ticks {
            self.running = false;
        }
    }

    /// The pulse, high for the duration.
    #[must_use]
    pub(crate) const fn level(self) -> bool {
        self.running
    }

    /// The capacitor voltage in microvolts, zero to [`OneShot::TRIP_UV`] in
    /// whole millivolts; zero throughout an untimed pulse, there being no
    /// capacitor and no current.
    #[cfg(test)]
    const fn capacitor_uv(self, duration_ticks: Option<usize>) -> i32 {
        self.capacitor_mv(duration_ticks) as i32 * 1000
    }

    /// [`OneShot::capacitor_uv`] in millivolts, the resolution it is computed
    /// at.
    #[cfg(test)]
    const fn capacitor_mv(self, duration_ticks: Option<usize>) -> u32 {
        let Some(duration_ticks) = duration_ticks else {
            return 0;
        };
        if !self.running || duration_ticks == 0 {
            return 0;
        }
        (TRIP_MV as u128 * self.elapsed as u128 / duration_ticks as u128) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::OneShot;

    /// `SN76477-027`, `SN76477-028`: the capacitor climbs to its reference
    /// over the duration and no further.
    #[test]
    fn the_capacitor_climbs_to_its_reference() {
        const DURATION: usize = 500;
        let mut shot = OneShot::default();
        shot.tick(true, false, Some(DURATION));
        let mut peak = 0;
        for _ in 0..DURATION {
            shot.tick(false, false, Some(DURATION));
            peak = peak.max(shot.capacitor_uv(Some(DURATION)));
        }
        assert!(peak < OneShot::TRIP_UV);
        assert!(peak / 1000 * 100 > OneShot::TRIP_UV / 1000 * 99);
    }

    /// `SN76477-046`: with neither `ROS` nor `COS` the capacitor never rises.
    #[test]
    fn an_untimed_pulse_charges_nothing() {
        let mut shot = OneShot::default();
        shot.tick(true, false, None);
        for _ in 0..50_000 {
            shot.tick(false, false, None);
            assert!(shot.level());
            assert_eq!(shot.capacitor_uv(None), 0);
        }
    }
}
