//! One voice's envelope generator.
//!
//! An eight-bit counter stepped by a free-running rate counter, a divider that
//! stretches the step down the fall, a sustain that is an equality, and the
//! part's edges (`MOS6581-059` to `-062`).

/// The rate counter's count for each rate value: the counter steps the
/// level when it reaches this, so one step every count-plus-one cycles
/// (`MOS6581-059`).
pub(crate) const RATE_COUNTS: [u16; 16] = [
    8, 31, 62, 94, 148, 219, 266, 312, 391, 976, 1953, 3125, 3906, 11719, 19531, 31250,
];

/// The rate counter's period: it counts modulo this, so a rate whose count
/// it has passed waits this many cycles at most (`MOS6581-062`).
pub(crate) const RATE_COUNTER_PERIOD: u16 = 32767;

/// The full level, in steps: eight bits, as `ENV 3` reads them
/// (`MOS6581-016`, `MOS6581-045`).
pub(crate) const FULL: u8 = 255;

/// The cycles one step takes at rate value `rate` (`MOS6581-059`).
#[must_use]
#[cfg(test)]
pub(crate) const fn step_cycles(rate: u8) -> usize {
    RATE_COUNTS[(rate & 0x0f) as usize] as usize + 1
}

/// The cycles an attack from zero to the peak takes at rate value `rate`,
/// with the rate counter starting from zero (`MOS6581-059`).
#[must_use]
#[cfg(test)]
pub(crate) const fn attack_cycles(rate: u8) -> usize {
    FULL as usize * step_cycles(rate)
}

/// The divider applied to the step period at level `level` on the way down
/// (`MOS6581-060`).
#[must_use]
pub(crate) const fn fall_divider(level: u8) -> u8 {
    match level {
        0x5e..=0xff => 1,
        0x37..=0x5d => 2,
        0x1b..=0x36 => 4,
        0x0f..=0x1a => 8,
        0x07..=0x0e => 16,
        0x00..=0x06 => 30,
    }
}

/// The cycles a fall from the peak to zero takes at rate value `rate`
/// (`MOS6581-060`): one step per level, each stretched by its divider.
#[must_use]
#[cfg(test)]
pub(crate) const fn fall_cycles(rate: u8) -> usize {
    let mut cycles = 0;
    let mut level = FULL;
    while level > 0 {
        cycles += step_cycles(rate) * fall_divider(level) as usize;
        level -= 1;
    }
    cycles
}

/// Where in its cycle a generator is.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum EnvelopeStage {
    /// Rising to the peak (`MOS6581-029`).
    Attack,
    /// Falling from the peak towards the sustain level (`MOS6581-030`).
    Decay,
    /// Holding at the sustain level while gated (`MOS6581-031`).
    Sustain,
    /// Falling to zero, and resting there (`MOS6581-032`).
    Release,
}

/// One envelope generator.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Envelope {
    level: u8,
    /// Rising, or falling; the sustain hold is the falling state with the
    /// level at the sustain value.
    rising: bool,
    /// Whether the gate is set, as last seen (`MOS6581-034`).
    gate: bool,
    /// The free-running rate counter (`MOS6581-059`, `MOS6581-062`).
    rate: u16,
    /// Steps of the rate counter since the level last fell (`MOS6581-060`).
    divider: u8,
    /// The counter wrapped from the peak to zero under a gate set at the
    /// peak, and holds there (`MOS6581-062`).
    hold_zero: bool,
}

impl Envelope {
    /// The power-up state this crate starts from: zero, released, ungated.
    #[must_use]
    pub const fn reset() -> Self {
        Self {
            level: 0,
            rising: false,
            gate: false,
            rate: 0,
            divider: 0,
            hold_zero: false,
        }
    }

    /// What `RES` does to a generator (`MOS6581-063`): the level stays, the
    /// gate is gone, and the generator releases from where it stands.
    pub const fn on_reset(&mut self) {
        self.rising = false;
        self.gate = false;
        self.rate = 0;
        self.divider = 0;
        self.hold_zero = false;
    }

    /// The level, `0` to [`FULL`].
    #[must_use]
    pub const fn level(&self) -> u8 {
        self.level
    }

    /// The stage, as [`EnvelopeStage`] names it, for the sustain value in `sr`.
    #[must_use]
    pub const fn stage(&self, sr: u8) -> EnvelopeStage {
        if self.rising {
            EnvelopeStage::Attack
        } else if !self.gate {
            EnvelopeStage::Release
        } else if self.level == Self::sustain_level(sr) {
            EnvelopeStage::Sustain
        } else {
            EnvelopeStage::Decay
        }
    }

    /// The sustain level a `Sustain/Release` byte selects: the value in
    /// both nibbles (`MOS6581-031`, `MOS6581-061`).
    #[must_use]
    pub const fn sustain_level(sr: u8) -> u8 {
        (sr >> 4) * 0x11
    }

    /// One `Ø2` cycle with the gate at `gate` and the two rate registers at
    /// `ad` and `sr`.
    ///
    /// A gate edge changes direction at once, from whatever level stands
    /// (`MOS6581-028`, `MOS6581-034`), and the rate counter runs on
    /// regardless (`MOS6581-062`). Each time it reaches the current rate's
    /// count the level takes a step: up by one while rising, the peak
    /// turning it round; down by one otherwise, every divider'th step, and
    /// not at all at the sustain level while gated, at zero, or after a
    /// wrap from the peak (`MOS6581-059` to `-062`).
    pub const fn step(&mut self, gate: bool, ad: u8, sr: u8) {
        if gate != self.gate {
            self.gate = gate;
            self.rising = gate;
            self.divider = 0;
            if gate {
                self.hold_zero = false;
            }
        }
        let rate = if self.rising {
            ad >> 4
        } else if gate {
            ad & 0x0f
        } else {
            sr & 0x0f
        };
        if self.rate != RATE_COUNTS[rate as usize] {
            self.rate = (self.rate + 1) % RATE_COUNTER_PERIOD;
            return;
        }
        self.rate = 0;
        if self.rising {
            if self.hold_zero {
                return;
            }
            self.level = self.level.wrapping_add(1);
            if self.level == FULL {
                self.rising = false;
                self.divider = 0;
            } else if self.level == 0 {
                self.hold_zero = true;
            }
            return;
        }
        if self.level == 0 || (gate && self.level == Self::sustain_level(sr)) {
            return;
        }
        self.divider += 1;
        if self.divider >= fall_divider(self.level) {
            self.divider = 0;
            self.level -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_until(
        env: &mut Envelope,
        gate: bool,
        ad: u8,
        sr: u8,
        limit: usize,
        done: impl Fn(u8) -> bool,
    ) -> usize {
        for cycle in 1..=limit {
            env.step(gate, ad, sr);
            if done(env.level()) {
                return cycle;
            }
        }
        panic!("not reached in {limit} cycles, level {}", env.level());
    }

    #[test]
    fn an_attack_at_rate_zero_takes_255_steps_of_nine_cycles() {
        let mut env = Envelope::reset();
        let cycles = run_until(&mut env, true, 0x00, 0xf0, 3_000, |l| l == FULL);
        assert_eq!(cycles, attack_cycles(0));
        assert_eq!(attack_cycles(0), 2295);
    }

    #[test]
    fn a_fall_at_rate_zero_takes_6804_cycles() {
        assert_eq!(fall_cycles(0), 6804);
        let mut env = Envelope::reset();
        run_until(&mut env, true, 0x00, 0x00, 3_000, |l| l == FULL);
        let cycles = run_until(&mut env, true, 0x00, 0x00, 10_000, |l| l == 0);
        assert_eq!(cycles, fall_cycles(0));
    }

    #[test]
    fn a_decay_stops_at_the_sustain_level_and_a_raised_sustain_falls_to_zero() {
        let mut env = Envelope::reset();
        run_until(&mut env, true, 0x00, 0x80, 3_000, |l| l == FULL);
        run_until(&mut env, true, 0x00, 0x80, 10_000, |l| l == 0x88);
        for _ in 0..5_000 {
            env.step(true, 0x00, 0x80);
        }
        assert_eq!(env.level(), 0x88);
        assert_eq!(env.stage(0x80), EnvelopeStage::Sustain);
        run_until(&mut env, true, 0x00, 0xc0, 10_000, |l| l == 0);
        for _ in 0..5_000 {
            env.step(true, 0x00, 0xc0);
        }
        assert_eq!(env.level(), 0);
    }

    #[test]
    fn a_gate_set_at_the_peak_wraps_the_counter_to_zero() {
        let mut env = Envelope::reset();
        run_until(&mut env, true, 0x00, 0xf0, 3_000, |l| l == FULL);
        env.step(false, 0x00, 0xf0);
        for _ in 0..100 {
            env.step(true, 0x00, 0xf0);
        }
        assert_eq!(env.level(), 0);
        for _ in 0..3_000 {
            env.step(true, 0x00, 0xf0);
        }
        assert_eq!(env.level(), 0, "and holds");
    }
}
