//! The noise generator's shift register.
//!
//! Thirty-two stages, each a two-latch flip-flop on the die, with a **nonlinear**
//! feedback function — `SN76477-001` and `SN76477-003`. Stages are numbered
//! `r0`..`r31`; the feedback bit enters at `r0` and bits move towards `r31`,
//! which is the register's output.

/// The thirty-two latches, `r0` in bit 0 and `r31` in bit 31.
///
/// `Default` is the all-zero state, the crate's reset — `SN76477-004`. The sequence is **not** periodic from there for a
/// long while: 69 708 clocks pass before the register enters its cycle, and a
/// consumer that wants the repeating sequence has to run that tail off first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct NoiseShiftRegister(u32);

impl NoiseShiftRegister {
    /// The cycle the register reaches from reset, in clocks — `SN76477-004`.
    #[cfg(test)]
    const PERIOD_FROM_RESET_CYCLES: u32 = 56_883;

    /// Clocks from the all-zero reset state before that cycle is entered.
    #[cfg(test)]
    const TAIL_FROM_RESET_CYCLES: u32 = 69_708;

    /// A register holding `bits`, `r0` in bit 0.
    #[must_use]
    pub(crate) const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// The thirty-two latches, `r0` in bit 0.
    #[must_use]
    pub(crate) const fn bits(self) -> u32 {
        self.0
    }

    /// The noise output: the last stage, `r31` — `SN76477-002`.
    #[must_use]
    pub(crate) const fn level(self) -> bool {
        self.0 >> 31 & 1 == 1
    }

    /// The bit the feedback logic presents to `r0` for the next edge.
    ///
    /// `NOT ((r2 XOR r30) OR (r2 AND r26 AND r27 AND r28 AND r29 AND r30))` —
    /// `SN76477-003`.
    #[must_use]
    pub(crate) const fn feedback(self) -> bool {
        let s = self.0;
        let r2 = s >> 2 & 1;
        let r30 = s >> 30 & 1;
        let run = s >> 26 & s >> 27 & s >> 28 & s >> 29 & r30 & 1;
        (r2 ^ r30 | r2 & run) & 1 == 0
    }

    /// One rising clock edge: every stage takes its neighbour, `r0` takes the
    /// feedback bit.
    pub(crate) const fn shift(&mut self) {
        let feedback = self.feedback() as u32;
        self.0 = self.0 << 1 | feedback;
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::NoiseShiftRegister;
    use std::vec::Vec;

    /// The register after `n` clocks.
    fn shifted(mut reg: NoiseShiftRegister, n: u32) -> NoiseShiftRegister {
        for _ in 0..n {
            reg.shift();
        }
        reg
    }

    fn feedback(bits: u32) -> bool {
        NoiseShiftRegister::from_bits(bits).feedback()
    }

    /// Brent's cycle detection: `(cycle length, tail length, one state on the
    /// cycle)`.
    fn orbit(start: u32) -> (u32, u32, u32) {
        let step = |s: u32| shifted(NoiseShiftRegister::from_bits(s), 1).bits();

        let (mut power, mut lambda) = (1u32, 1u32);
        let mut tortoise = start;
        let mut hare = step(start);
        while tortoise != hare {
            if power == lambda {
                tortoise = hare;
                power *= 2;
                lambda = 0;
            }
            hare = step(hare);
            lambda += 1;
        }

        tortoise = start;
        hare = start;
        for _ in 0..lambda {
            hare = step(hare);
        }
        let mut mu = 0;
        while tortoise != hare {
            tortoise = step(tortoise);
            hare = step(hare);
            mu += 1;
        }
        (lambda, mu, tortoise)
    }

    /// The smallest state on a cycle, a stable name for the attractor.
    fn attractor_name(on_cycle: u32, length: u32) -> u32 {
        let mut s = on_cycle;
        let mut best = s;
        for _ in 0..length {
            s = shifted(NoiseShiftRegister::from_bits(s), 1).bits();
            best = best.min(s);
        }
        best
    }

    /// `SN76477-003`: the feedback expression, against a second statement of
    /// it, at the states that tell the stage numberings apart.
    #[test]
    fn feedback_is_the_nonlinear_expression() {
        fn expected(bits: u32) -> bool {
            let r = |i: u32| bits >> i & 1 == 1;
            let run = r(2) && r(26) && r(27) && r(28) && r(29) && r(30);
            !((r(2) != r(30)) || run)
        }

        for probe in [
            0x0000_0000,
            0xFFFF_FFFF,
            0x0000_0004,
            0x4000_0000,
            0x7C00_0004,
            0x7800_0004,
            0x0023_4A1E,
        ] {
            assert_eq!(feedback(probe), expected(probe), "at {probe:#010x}");
        }

        assert!(!feedback(0x7C00_0004));
        assert!(feedback(0x7800_0004));

        let mut reg = NoiseShiftRegister::from_bits(0);
        assert!(feedback(reg.bits()));
        reg.shift();
        assert_eq!(reg.bits(), 1);
    }

    /// `SN76477-001`, `SN76477-004`: the cycle reached from reset, which a
    /// register of any other width misses.
    #[test]
    fn the_cycle_from_reset() {
        let (cycle, tail, _) = orbit(0);
        assert_eq!(cycle, NoiseShiftRegister::PERIOD_FROM_RESET_CYCLES);
        assert_eq!(tail, NoiseShiftRegister::TAIL_FROM_RESET_CYCLES);

        let entry = shifted(NoiseShiftRegister::from_bits(0), tail);
        assert_eq!(shifted(entry, cycle), entry);
        assert_ne!(shifted(entry, cycle - 1), entry);

        let (ones_cycle, _, ones_on) = orbit(u32::MAX);
        assert_eq!(ones_cycle, cycle);
        assert_eq!(
            attractor_name(ones_on, ones_cycle),
            attractor_name(entry.bits(), cycle)
        );
    }

    /// `SN76477-005`: the register is not globally convergent, so a seed other
    /// than the reset state can sit on a shorter sequence.
    #[test]
    fn other_attractors() {
        let reset_cycle = orbit(0);
        let reset_name = attractor_name(reset_cycle.2, reset_cycle.0);

        let mut x: u32 = 0x1234_5678;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };

        let mut names: Vec<(u32, u32)> = Vec::new();
        for _ in 0..200 {
            let (length, _, on) = orbit(next());
            names.push((attractor_name(on, length), length));
        }
        names.sort_unstable();
        names.dedup();

        assert!(names.iter().any(|&(name, _)| name == reset_name));
        assert!(names.len() >= 2);

        let lengths: Vec<u32> = names.iter().map(|&(_, length)| length).collect();
        for expected in [56_883, 9_760, 26_688, 32_648, 32_178] {
            assert!(lengths.contains(&expected), "{expected} not in {lengths:?}");
        }
    }
}
