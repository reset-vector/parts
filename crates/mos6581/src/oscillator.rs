//! One voice's tone oscillator and waveform generator.
//!
//! A 24-bit accumulator, twelve-bit outputs, a pulse comparator one cycle
//! behind, a 23-bit noise register clocked off accumulator bit 19, and the
//! sync and ring terms (`MOS6581-052` to `-058`). What two selected waveforms
//! produce together is this crate's choice under `MOS6581-OPEN-18`: the
//! sheet's AND.

/// `GATE`, control bit 0 (`MOS6581-014`, `MOS6581-028`).
pub const GATE: u8 = 0x01;
/// `SYNC`, control bit 1 (`MOS6581-026`).
pub const SYNC: u8 = 0x02;
/// `RING MOD`, control bit 2 (`MOS6581-027`).
pub const RING_MOD: u8 = 0x04;
/// `TEST`, control bit 3 (`MOS6581-025`).
pub const TEST: u8 = 0x08;
/// The triangle waveform, control bit 4 (`MOS6581-021`).
pub const TRIANGLE: u8 = 0x10;
/// The sawtooth waveform, control bit 5 (`MOS6581-021`).
pub const SAWTOOTH: u8 = 0x20;
/// The pulse waveform, control bit 6 (`MOS6581-021`).
pub const PULSE: u8 = 0x40;
/// The noise waveform, control bit 7 (`MOS6581-021`).
pub const NOISE: u8 = 0x80;

/// The four waveform bits together.
const WAVEFORMS: u8 = TRIANGLE | SAWTOOTH | PULSE | NOISE;

/// The phase accumulator's width (`MOS6581-052`).
pub const PHASE_BIT_COUNT: usize = 24;
const ACCUMULATOR_MASK: u32 = (1 << PHASE_BIT_COUNT) - 1;

/// The width of a waveform output (`MOS6581-053`): twelve bits, of which
/// `OSC 3` reads the top eight (`MOS6581-044`).
pub const OUTPUT_BIT_COUNT: usize = 12;
const OUTPUT_MASK: u16 = (1 << OUTPUT_BIT_COUNT) - 1;

/// The noise register's width (`MOS6581-055`).
pub const NOISE_BIT_COUNT: usize = 23;
const NOISE_MASK: u32 = (1 << NOISE_BIT_COUNT) - 1;

/// The noise register's starting state, this crate's choice. `RES` does not
/// clear the register (`MOS6581-063`); any non-zero value is as good as
/// another here.
const NOISE_SEED: u32 = 0x5E_C7A3;

/// The accumulator bit whose rising edge clocks the noise register: bit 19,
/// so the register shifts at `Fn · Fclk / 2²⁰`, sixteen times per
/// accumulator wrap (`MOS6581-023`, `MOS6581-055`).
pub const NOISE_CLOCK_BIT: usize = 19;
const NOISE_CLOCK: u32 = 1 << NOISE_CLOCK_BIT;

/// The cycles between the rising edge of bit 19 and the shift it clocks
/// (`MOS6581-055`).
pub const NOISE_SHIFT_LAG_CYCLES: usize = 2;

/// The register bits that are the output's bits 11 down to 4
/// (`MOS6581-055`).
const NOISE_TAPS: [u32; 8] = [20, 18, 14, 11, 9, 5, 2, 0];

/// One oscillator: a phase accumulator, the pulse comparator's view of it a
/// cycle behind, and a noise register with its clock pipeline.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Oscillator {
    /// The 24-bit phase accumulator (`MOS6581-052`).
    acc: u32,
    /// The accumulator's top twelve bits as they stood a cycle ago, which is
    /// what the pulse compares (`MOS6581-054`).
    pulse_top: u16,
    /// The 23-bit noise shift register (`MOS6581-055`).
    noise: u32,
    /// Rising edges of bit 19 on their way to the register, one bit per
    /// cycle of lag (`MOS6581-055`).
    noise_pending: u8,
    /// Whether `TEST` was set in the last cycle, so that the cycle it clears
    /// in shifts once with the inverted input (`MOS6581-056`).
    test_latched: bool,
}

impl Oscillator {
    /// The power-up state this crate starts from: phase zero, the noise
    /// register seeded. `RES` itself leaves both alone (`MOS6581-063`).
    #[must_use]
    pub const fn reset() -> Self {
        Self {
            acc: 0,
            pulse_top: 0,
            noise: NOISE_SEED,
            noise_pending: 0,
            test_latched: false,
        }
    }

    /// One `Ø2` cycle at frequency value `freq`; whether the noise register
    /// shifted in this cycle.
    ///
    /// With `test` set the accumulator is held at zero and the noise
    /// register holds (`MOS6581-025`, `MOS6581-056`); the cycle after `test`
    /// clears, the register shifts once with bit 0 the complement of its
    /// feedback. Otherwise a rising edge of accumulator bit
    /// [`NOISE_CLOCK_BIT`] shifts the register [`NOISE_SHIFT_LAG_CYCLES`] cycles
    /// later (`MOS6581-055`).
    pub const fn step(&mut self, freq: u16, test: bool) -> bool {
        self.pulse_top = (self.acc >> (PHASE_BIT_COUNT - OUTPUT_BIT_COUNT)) as u16;
        if test {
            self.acc = 0;
            self.noise_pending = 0;
            self.test_latched = true;
            return false;
        }
        let next = (self.acc + freq as u32) & ACCUMULATOR_MASK;
        let edge = !self.acc & next & NOISE_CLOCK != 0;
        self.acc = next;
        if self.test_latched {
            self.test_latched = false;
            self.noise = shift(self.noise, feedback(self.noise) ^ 1);
            return true;
        }
        let fire = self.noise_pending & (1 << (NOISE_SHIFT_LAG_CYCLES - 1)) != 0;
        self.noise_pending =
            ((self.noise_pending << 1) & ((1 << NOISE_SHIFT_LAG_CYCLES) - 1)) | edge as u8;
        if fire {
            self.noise = shift(self.noise, feedback(self.noise));
        }
        fire
    }

    /// The neighbour's bit 23 rose this cycle: the accumulator returns to
    /// zero (`MOS6581-057`).
    pub const fn sync(&mut self) {
        self.acc = 0;
    }

    /// The accumulator's top bit, which a neighbour's sync and ring
    /// modulation read (`MOS6581-057`, `MOS6581-058`).
    #[must_use]
    pub const fn msb(&self) -> bool {
        self.acc & (1 << (PHASE_BIT_COUNT - 1)) != 0
    }

    /// The accumulator, for the debugger; not a pin.
    #[must_use]
    pub const fn phase(&self) -> u32 {
        self.acc
    }

    /// The noise register, for the debugger; not a pin.
    #[must_use]
    pub const fn noise_register(&self) -> u32 {
        self.noise
    }

    /// The twelve-bit waveform output for control byte `ctrl` and pulse width
    /// `pw`, with `neighbour_msb` the neighbouring oscillator's top bit.
    ///
    /// No waveform selected is zero (`MOS6581-021`); more than one is their
    /// bitwise AND, the sheet's word and `MOS6581-OPEN-18`'s choice. The
    /// sawtooth is the accumulator's top twelve bits; the triangle is bits
    /// 22–12 one bit up with bit 0 zero, inverted by bit 23 XOR (`RING MOD`
    /// AND NOT the neighbour's bit 23), the term forced off while the
    /// sawtooth is selected (`MOS6581-053`, `MOS6581-058`); the pulse is all
    /// ones while last cycle's top twelve exceed `pw`, or under `TEST`
    /// (`MOS6581-054`); the noise is eight register bits at bits 11–4.
    #[must_use]
    pub const fn waveform(&self, ctrl: u8, pw: u16, neighbour_msb: bool) -> u16 {
        if ctrl & WAVEFORMS == 0 {
            return 0;
        }
        let top = (self.acc >> (PHASE_BIT_COUNT - OUTPUT_BIT_COUNT)) as u16;
        let mut out = OUTPUT_MASK;
        if ctrl & TRIANGLE != 0 {
            let invert =
                ctrl & SAWTOOTH == 0 && (self.msb() ^ (ctrl & RING_MOD != 0 && !neighbour_msb));
            let tri =
                ((self.acc >> (PHASE_BIT_COUNT - OUTPUT_BIT_COUNT - 1)) as u16) & (OUTPUT_MASK - 1);
            out &= if invert {
                !tri & (OUTPUT_MASK - 1)
            } else {
                tri
            };
        }
        if ctrl & SAWTOOTH != 0 {
            out &= top;
        }
        if ctrl & PULSE != 0 {
            out &= if ctrl & TEST != 0 || self.pulse_top > (pw & OUTPUT_MASK) {
                OUTPUT_MASK
            } else {
                0
            };
        }
        if ctrl & NOISE != 0 {
            out &= noise_bits(self.noise);
        }
        out
    }

    /// The shared output lines written back into the register at a shift
    /// (`MOS6581-056`): with another waveform selected beside noise, every
    /// register bit whose output line `out` holds low is cleared. Called by
    /// the part in a cycle [`Oscillator::step`] reported a shift.
    pub const fn noise_writeback(&mut self, out: u16) {
        let mut i = 0;
        while i < NOISE_TAPS.len() {
            if out & (1 << (OUTPUT_BIT_COUNT - 1 - i)) == 0 {
                self.noise &= !(1 << NOISE_TAPS[i]);
            }
            i += 1;
        }
    }
}

/// Bit 22 XOR bit 17 (`MOS6581-055`).
const fn feedback(n: u32) -> u32 {
    ((n >> 22) ^ (n >> 17)) & 1
}

/// The register shifted up one with `input` as its new bit 0.
const fn shift(n: u32, input: u32) -> u32 {
    ((n << 1) | input) & NOISE_MASK
}

/// Register bits 20, 18, 14, 11, 9, 5, 2, 0 as output bits 11 down to 4
/// (`MOS6581-055`).
const fn noise_bits(n: u32) -> u16 {
    let mut out = 0u16;
    let mut i = 0;
    while i < NOISE_TAPS.len() {
        out |= (((n >> NOISE_TAPS[i]) & 1) as u16) << (OUTPUT_BIT_COUNT - 1 - i);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    #[test]
    fn the_noise_register_is_maximal_length() {
        let mut n = NOISE_SEED;
        let mut period = 0u32;
        loop {
            n = shift(n, feedback(n));
            period += 1;
            if n == NOISE_SEED {
                break;
            }
        }
        assert_eq!(period, (1 << NOISE_BIT_COUNT) - 1);
    }

    #[test]
    fn the_noise_shifts_sixteen_times_per_wrap_two_cycles_after_the_edge() {
        let mut osc = Oscillator::reset();
        let mut edges = Vec::new();
        let mut shifts = Vec::new();
        for cycle in 0..(4096 * 4) {
            let before = osc.phase();
            let shifted = osc.step(0x1000, false);
            if !before & osc.phase() & NOISE_CLOCK != 0 {
                edges.push(cycle);
            }
            if shifted {
                shifts.push(cycle);
            }
        }
        assert_eq!(edges.len(), 16 * 4);
        assert_eq!(shifts.len(), edges.len());
        for (e, s) in edges.iter().zip(&shifts) {
            assert_eq!(s - e, NOISE_SHIFT_LAG_CYCLES);
        }
    }

    #[test]
    fn a_pulse_at_half_width_is_square_and_a_cycle_behind() {
        let mut osc = Oscillator::reset();
        let mut high = 0;
        let mut first_high = None;
        for cycle in 0..4096 {
            osc.step(0x1000, false);
            if osc.waveform(PULSE, 2048, false) == OUTPUT_MASK {
                high += 1;
                first_high.get_or_insert(cycle);
            }
        }
        assert_eq!(high, 2047, "PWn = 2048: ones while last cycle's top > 2048");
        assert_eq!(first_high, Some(2049));
    }

    #[test]
    fn the_triangle_has_a_zero_bit_0_and_the_sawtooth_disables_its_inversion() {
        let mut osc = Oscillator::reset();
        for _ in 0..3000 {
            osc.step(0x1000, false);
            assert_eq!(osc.waveform(TRIANGLE, 0, false) & 1, 0);
        }
        assert!(osc.msb());
        let tri = ((osc.phase() >> 11) as u16) & 0xffe;
        assert_eq!(osc.waveform(TRIANGLE, 0, false), !tri & 0xffe);
        assert_eq!(
            osc.waveform(TRIANGLE | SAWTOOTH, 0, false),
            tri & (osc.phase() >> 12) as u16
        );
    }
}
