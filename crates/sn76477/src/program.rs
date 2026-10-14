//! What the caller fits outside the package, and what the part makes of it.
//!
//! [`Components`] is the board's: eight resistors and five capacitors, in ohms
//! and picofarads, on the far side of thirteen pins. [`Timing`] is the same
//! programming expressed the way the part's counters see it — whole ticks of a
//! caller-declared time base — and [`Components::timing`] is seven of the
//! data sheet's eight equations, once each. The eighth is Equation 3, the VCO's
//! duty cycle, which is not a programming constant at all: it is read every tick
//! off two pin voltages as [`crate::Sn76477::sample`] runs.
//!
//! **No board's values are here.** Every field is the caller's to fill and
//! nothing in this crate has a default set of them, because a consumer that
//! cannot state its own R and C has a board problem and not a part problem.

use core::num::NonZeroUsize;

use units::{Hertz, Microvolts, Ohms, Picofarads};

/// The external components, in ohms and picofarads.
///
/// Picofarads because the part's capacitors run from a few hundred pF to tens of
/// microfarads and one unit has to hold both; 10 µF is `Picofarads(10_000_000)`
/// here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Components {
    /// `RSLF`, pin 20. 7.5 kΩ minimum (`SN76477-008`).
    pub r_slf_ohms: Ohms,
    /// `CSLF`, pin 21.
    pub c_slf_pf: Picofarads,
    /// `RVCO`, pin 18. 7.5 kΩ minimum (`SN76477-012`).
    pub r_vco_ohms: Ohms,
    /// `CVCO`, pin 17.
    pub c_vco_pf: Picofarads,
    /// `RNF`, pin 5. 7.5 kΩ minimum (`SN76477-021`).
    pub r_noise_filter_ohms: Ohms,
    /// `CNF`, pin 6.
    pub c_noise_filter_pf: Picofarads,
    /// `ROS`, pin 24. 7.5 kΩ minimum (`SN76477-026`); `None` left open
    /// (`SN76477-046`).
    pub r_one_shot_ohms: Option<Ohms>,
    /// `COS`, pin 23; `None` left open (`SN76477-046`).
    pub c_one_shot_pf: Option<Picofarads>,
    /// `RA`, pin 10. 7.5 kΩ minimum (`SN76477-034`).
    pub r_attack_ohms: Ohms,
    /// `RD`, pin 7. 7.5 kΩ minimum (`SN76477-034`).
    pub r_decay_ohms: Ohms,
    /// `CA/D`, pin 8, shared by the attack and the decay (`SN76477-034`).
    pub c_attack_decay_pf: Picofarads,
    /// `RG`, pin 11. 47 kΩ to 220 kΩ (`SN76477-039`).
    pub r_amplitude_ohms: Ohms,
    /// `RF`, pin 12 to pin 13 (`SN76477-039`).
    pub r_feedback_ohms: Ohms,
}

/// The two voltages pin 13 swings between at full amplitude, in microvolts
/// (`SN76477-039`, `SN76477-040`).
///
/// The output rests at [`crate::OUTPUT_DC_UV`]; a high mixer output drives it
/// towards the high limit and a low one towards the low limit, each by the
/// envelope's fraction of the way. A limit on the wrong side of the DC level
/// gives that half no swing at all.
pub use units::Swing;

/// The part's programming in whole ticks, and its output swing.
///
/// A tick is one period of a time base the caller chooses and states to
/// [`Components::timing`] in hertz. The part has no clock input of its
/// own — its rates are set by its components and by an internal ring
/// oscillator whose frequency is `SN76477-OPEN-03` — so nothing in the part
/// fixes it.
///
/// Every field is public, because a consumer that has measured its own board is
/// better placed than an equation to say what a period is. What
/// [`Components::timing`] writes into each is the data sheet's equation
/// for that block and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Timing {
    /// One whole cycle of the SLF, charging half and discharging half
    /// (`SN76477-009`), or `None` for an SLF that does not run.
    pub slf_period_ticks: Option<NonZeroUsize>,
    /// One whole cycle of the VCO at the top of its control range, which is the
    /// slowest it runs (`SN76477-012`), or `None` for a VCO that does not run.
    pub vco_max_period_ticks: Option<NonZeroUsize>,
    /// One whole cycle of the internal noise clock.
    ///
    /// **No equation writes this**: `SN76477-019` is that the ring's rate is set
    /// by the operating current pin 4's resistor sets, and the rate for a
    /// given resistor is `SN76477-OPEN-03`.
    /// [`Components::timing`] leaves it `None`, the ring not running; a
    /// caller that wants the internal clock sets it, and a caller with an
    /// external one drives pin 3 instead.
    pub noise_clock_period_ticks: Option<NonZeroUsize>,
    /// How long the noise filter's capacitor takes to cross its comparator's
    /// whole hysteresis window (`SN76477-023`).
    pub noise_filter_traverse_ticks: usize,
    /// The one-shot pulse (`SN76477-027`), or `None`, untimed, when `ROS`
    /// and `COS` are not fitted and pin 23 is the only end the pulse has
    /// (`SN76477-046`, `SN76477-OPEN-11`). `Some(0)` is a pulse high on no
    /// tick.
    pub one_shot_ticks: Option<usize>,
    /// A full attack ramp, silence to full amplitude (`SN76477-035`).
    pub attack_ticks: usize,
    /// A full decay ramp, full amplitude to silence (`SN76477-035`).
    pub decay_ticks: usize,
    /// Where pin 13 goes at full amplitude (`SN76477-039`), or `None` for an
    /// output that does not move from [`crate::OUTPUT_DC_UV`].
    pub output_swing: Option<Swing>,
}

/// `round(rate · r · c · num / (den · 10^12))` — a time in ticks from a
/// resistance in ohms, a capacitance in picofarads and the equation's constant
/// as a fraction.
///
/// The whole product is formed before anything is divided, so the result is the
/// exact rational rounded once. `u128` because `rate · r · c` alone reaches
/// about 4 × 10^24 at the extremes the part allows; a product past `u128` is
/// held at its top, which the count saturates from anyway.
const fn ticks(rate: u32, r_ohm: u64, c_pf: u32, num: u64, den: u64) -> usize {
    let product = (rate as u128 * r_ohm as u128)
        .saturating_mul(c_pf as u128)
        .saturating_mul(num as u128);
    let divisor = den as u128 * 1_000_000_000_000;
    let rounded = product.saturating_add(divisor / 2) / divisor;
    if rounded > u32::MAX as u128 {
        u32::MAX as usize
    } else {
        rounded as usize
    }
}

/// Equation 8's swing: `3.4 · RF / RG` volts either side of the DC level,
/// the peak held to the most that fits an `i32` above it, so the swing stays
/// centred, and held there too where `RG` is 0 and `RF` is not; `None` where
/// the peak is zero.
const fn equation_8(r_feedback_ohms: u64, r_amplitude_ohms: u64) -> Option<Swing> {
    let dc = crate::OUTPUT_DC_UV.0;
    let top = (i32::MAX - dc) as u128;
    let uv = match (3_400_000u128 * r_feedback_ohms as u128).checked_div(r_amplitude_ohms as u128) {
        Some(uv) => uv,
        None if r_feedback_ohms == 0 => 0,
        None => top,
    };
    let peak = if uv > top { top } else { uv } as i32;
    Swing::declared(Microvolts(dc - peak), Microvolts(dc + peak))
}

impl Components {
    /// The data sheet's equations, applied to these components at one time
    /// base of `tick_rate_hz` ticks a second: the [`Timing`] the caller passes
    /// to [`crate::Sn76477::new`].
    ///
    /// Seven of the eight fields are written by one equation each of the data
    /// sheet's Table 1, at `Vreg = 5 V` as that table's footnote requires, and
    /// a period that rounds to no tick is `None`. The eighth,
    /// [`Timing::noise_clock_period_ticks`], has no equation in it and is left
    /// `None` — see its own note.
    ///
    /// Equation 8 gives a peak, and the swing is that peak either side of
    /// [`crate::OUTPUT_DC_UV`], held to `i32::MAX - OUTPUT_DC_UV` so both
    /// limits fit (an `RF / RG` above 630.88), which an amplitude resistor of
    /// zero reaches; a peak of zero is no output. A
    /// one-shot resistor or capacitor left open (`None`) is a programming the
    /// data sheet allows and Equation 5 does not cover: the pulse is untimed,
    /// `None`.
    #[must_use]
    pub const fn timing(&self, tick_rate_hz: Hertz) -> Timing {
        let rate = tick_rate_hz.0;
        let parts = self;
        Timing {
            slf_period_ticks: NonZeroUsize::new(ticks(
                rate,
                parts.r_slf_ohms.0,
                parts.c_slf_pf.0,
                25,
                16,
            )),
            vco_max_period_ticks: NonZeroUsize::new(ticks(
                rate,
                parts.r_vco_ohms.0,
                parts.c_vco_pf.0,
                25,
                16,
            )),
            noise_clock_period_ticks: None,
            noise_filter_traverse_ticks: ticks(
                rate,
                parts.r_noise_filter_ohms.0,
                parts.c_noise_filter_pf.0,
                25,
                64,
            ),
            one_shot_ticks: match (parts.r_one_shot_ohms, parts.c_one_shot_pf) {
                (Some(r), Some(c)) => Some(ticks(rate, r.0, c.0, 4, 5)),
                _ => None,
            },
            attack_ticks: ticks(rate, parts.r_attack_ohms.0, parts.c_attack_decay_pf.0, 1, 1),
            decay_ticks: ticks(rate, parts.r_decay_ohms.0, parts.c_attack_decay_pf.0, 1, 1),
            output_swing: equation_8(parts.r_feedback_ohms.0, parts.r_amplitude_ohms.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swing_refuses_a_low_limit_not_below_the_high() {
        assert!(Swing::declared(Microvolts(1), Microvolts(1)).is_none());
        assert!(Swing::declared(Microvolts(2), Microvolts(1)).is_none());
        assert!(Swing::declared(Microvolts(i32::MAX), Microvolts(i32::MIN)).is_none());
        let swing = Swing::declared(Microvolts(i32::MIN), Microvolts(i32::MAX));
        assert_eq!(
            swing.map(Swing::range_uv),
            Some((Microvolts(i32::MIN), Microvolts(i32::MAX)))
        );
    }

    #[test]
    fn equation_8_is_a_peak_either_side_of_the_dc_level() {
        let dc = crate::OUTPUT_DC_UV.0;
        let swing = equation_8(200_000, 100_000).map(Swing::range_uv);
        assert_eq!(
            swing,
            Some((Microvolts(dc - 6_800_000), Microvolts(dc + 6_800_000)))
        );
        assert_eq!(equation_8(200_000, 0), equation_8(u64::MAX, 1));
        assert_eq!(equation_8(0, 0), None);
        assert_eq!(equation_8(0, 100_000), None);
        let widest = equation_8(u64::MAX, 1).map(Swing::range_uv);
        let top = i32::MAX - dc;
        assert_eq!(widest, Some((Microvolts(dc - top), Microvolts(i32::MAX))));
        assert_eq!(equation_8(631, 1).map(Swing::range_uv), widest);
        assert_eq!(equation_8(6_309, 10).map(Swing::range_uv), widest);
        assert_eq!(
            equation_8(630, 1).map(Swing::range_uv),
            Some((
                Microvolts(dc - 2_142_000_000),
                Microvolts(dc + 2_142_000_000)
            ))
        );
    }
}
