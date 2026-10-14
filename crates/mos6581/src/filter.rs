//! The programmable filter.
//!
//! An eleven-bit cutoff, a maximum set by the capacitor with nine octaves
//! below it, three outputs at stated slopes that add, and sixteen resonance
//! settings (`MOS6581-036` to `MOS6581-041`). The structure that yields those
//! slopes at once is a two-pole state-variable filter, run once per `Ø2` in
//! sixteen-bit fixed point; the resonance mapping, the shape near the cutoff
//! and the register-to-frequency map are this crate's choice under
//! `MOS6581-OPEN-09` to `-12`.

use units::{Millihertz, Picofarads};

/// `FILT 1`, `RES/Filt` bit 0 (`MOS6581-039`).
pub const FILT1: u8 = 0x01;
/// `FILT 2`, `RES/Filt` bit 1.
pub const FILT2: u8 = 0x02;
/// `FILT 3`, `RES/Filt` bit 2.
pub const FILT3: u8 = 0x04;
/// `FILTEX`, `RES/Filt` bit 3.
pub const FILTEX: u8 = 0x08;
/// `LP`, `Mode/Vol` bit 4 (`MOS6581-040`).
pub const LP: u8 = 0x10;
/// `BP`, `Mode/Vol` bit 5.
pub const BP: u8 = 0x20;
/// `HP`, `Mode/Vol` bit 6.
pub const HP: u8 = 0x40;
/// `3 OFF`, `Mode/Vol` bit 7 (`MOS6581-042`).
pub const VOICE3_OFF: u8 = 0x80;

/// The fixed-point scale of the two coefficients.
const Q: u32 = 16;

/// `2π · 2¹⁶`, rounded.
const TWO_PI_Q16: u64 = 411_775;

/// `√2 · 2¹⁶`, rounded: the damping at which a two-pole low-pass shows no
/// peak, which is this crate's reading of *no resonance* (`MOS6581-OPEN-10`).
const SQRT2_Q16: i64 = 92_682;

/// The sheet's constant in `FCmax = 2.6E-5 / C`, scaled so that dividing by a
/// capacitance in picofarads gives millihertz (`MOS6581-037`).
const FCMAX_NUMERATOR_MHZ_PF: u64 = 26_000_000_000;

/// The eleven-bit register's top value (`MOS6581-036`).
const FC_MAX: u64 = 2047;

/// The octaves the range extends below the maximum, as a ratio: nine octaves
/// is 2⁹ (`MOS6581-037`).
const RANGE_RATIO: u64 = 512;

/// The maximum cutoff for capacitors of `capacitor_pf` picofarads, in millihertz
/// (`MOS6581-037`), or `None` for a zero capacitance, which no board has and
/// [`crate::Mos6581::new`] refuses.
#[must_use]
pub const fn fcmax_millihertz(capacitor_pf: Picofarads) -> Option<Millihertz> {
    match FCMAX_NUMERATOR_MHZ_PF.checked_div(capacitor_pf.0 as u64) {
        Some(fcmax) => Some(Millihertz(fcmax)),
        None => None,
    }
}

/// [`fcmax_millihertz`] on the bare integers the filter computes in, for a
/// capacitance [`crate::Mos6581::new`] has refused at zero.
const fn fcmax(capacitor_pf: u32) -> u64 {
    FCMAX_NUMERATOR_MHZ_PF / capacitor_pf as u64
}

/// The cutoff a register value `fc` selects, in millihertz: linear in `fc`
/// from `FCmax / 512` at 0 to `FCmax` at 2047 (`MOS6581-036`, `MOS6581-037`,
/// `MOS6581-OPEN-12`). A zero capacitance never reaches it:
/// [`crate::Mos6581::new`] refuses one.
#[must_use]
pub(crate) const fn cutoff_millihertz(fc: u16, capacitor_pf: u32) -> u64 {
    let fc = fc as u64 & FC_MAX;
    fcmax(capacitor_pf) * (FC_MAX + (RANGE_RATIO - 1) * fc) / (FC_MAX * RANGE_RATIO)
}

/// The level each integrator and the filter's output saturate at, in the
/// units of [`Filter::run`]'s input, 32 V at
/// the pin, at `2 · 2048 · 255` units to the volt (`MOS6581-OPEN-11`,
/// `MOS6581-OPEN-13`). At 1 MHz and 2200 pF three voices through the filter
/// stay inside it; a cutoff too near the clock makes the recursion diverge,
/// and it stops here, the same in every build.
pub(crate) const SATURATION: i64 = 33_423_360;

/// `value` held to `±SATURATION`.
const fn saturate(value: i128) -> i64 {
    if value > SATURATION as i128 {
        SATURATION
    } else if value < -(SATURATION as i128) {
        -SATURATION
    } else {
        value as i64
    }
}

/// The filter's state and its two coefficients.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Filter {
    lp: i64,
    bp: i64,
    /// `2π · fc / Fclk`, in Q16.
    f: i64,
    /// The damping, in Q16 (`MOS6581-OPEN-09`).
    d: i64,
}

impl Filter {
    /// As `RES` leaves it: no state, the register value 0 and resonance 0.
    #[must_use]
    pub const fn reset(clock_hz: u32, capacitor_pf: u32) -> Self {
        let mut filter = Self {
            lp: 0,
            bp: 0,
            f: 0,
            d: 0,
        };
        filter.set(0, 0, clock_hz, capacitor_pf);
        filter
    }

    /// Take the register value `fc` and resonance setting `res` at clock
    /// `clock_hz` with capacitors of `capacitor_pf`.
    ///
    /// The damping falls linearly from `√2` at resonance 0 to `√2 / 16` at 15
    /// (`MOS6581-038`, `MOS6581-OPEN-09`).
    pub const fn set(&mut self, fc: u16, res: u8, clock_hz: u32, capacitor_pf: u32) {
        let f = cutoff_millihertz(fc, capacitor_pf) * TWO_PI_Q16 / (clock_hz as u64 * 1000);
        self.f = f as i64;
        self.d = SQRT2_Q16 * (16 - (res & 0x0f) as i64) / 16;
    }

    /// One `Ø2` cycle with `input` at the filter's input; the sum of the
    /// outputs `mode` selects, which is nothing when it selects none
    /// (`MOS6581-040`, `MOS6581-041`).
    ///
    /// Each integrator and the sum saturate at [`SATURATION`], and the
    /// arithmetic is wide enough that nothing short of that wraps.
    pub const fn run(&mut self, input: i64, mode: u8) -> i64 {
        let (f, d) = (self.f as i128, self.d as i128);
        self.lp = saturate(self.lp as i128 + ((f * self.bp as i128) >> Q));
        let hp = input as i128 - self.lp as i128 - ((d * self.bp as i128) >> Q);
        self.bp = saturate(self.bp as i128 + ((f * hp) >> Q));
        let mut out = 0;
        if mode & LP != 0 {
            out += self.lp as i128;
        }
        if mode & BP != 0 {
            out += self.bp as i128;
        }
        if mode & HP != 0 {
            out += hp;
        }
        saturate(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recommended_capacitor_puts_the_maximum_near_twelve_kilohertz() {
        assert_eq!(
            fcmax_millihertz(Picofarads(2200)).map(|f| f.0 / 1000),
            Some(11_818)
        );
        assert_eq!(cutoff_millihertz(2047, 2200), fcmax(2200));
        assert_eq!(cutoff_millihertz(0, 2200) / 1000, 23);
    }

    #[test]
    fn a_zero_capacitance_has_no_maximum() {
        assert_eq!(fcmax_millihertz(Picofarads(0)), None);
    }

    /// The largest output for an input bounded by one is the sum of the
    /// impulse response's magnitudes; it peaks at the top cutoff.
    #[test]
    fn three_full_voices_through_the_filter_stay_inside_the_saturation() {
        let mut gain = 0f64;
        for res in 0..16 {
            for fc in (1984..2048).step_by(4).chain([2047]) {
                let mut filter = Filter::reset(1_000_000, 2200);
                filter.set(fc, res, 1_000_000, 2200);
                let (f, d) = (filter.f as f64 / 65536.0, filter.d as f64 / 65536.0);
                let (mut lp, mut bp) = (0f64, 0f64);
                let mut sums = [0f64; 8];
                for n in 0..20_000 {
                    lp += f * bp;
                    let hp = f64::from(u8::from(n == 0)) - lp - d * bp;
                    bp += f * hp;
                    for (mode, sum) in sums.iter_mut().enumerate() {
                        let pick = |bit: usize, v: f64| if mode & bit != 0 { v } else { 0.0 };
                        *sum += (pick(1, lp) + pick(2, bp) + pick(4, hp)).abs();
                    }
                }
                gain = sums.iter().fold(gain, |g, &s| g.max(s));
            }
        }
        let voice_uv = 500_000.0;
        let bound = 3.0 * voice_uv * gain;
        let saturation_uv = 32_000_000.0;
        assert!(bound <= saturation_uv, "{bound}");
        assert!(bound > 0.98 * saturation_uv, "{bound}");
    }
}
