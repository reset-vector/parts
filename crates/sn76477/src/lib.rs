//! The Texas Instruments `SN76477` complex sound generator at its pins.
//!
//! One verb, [`Sn76477::sample`]: the part has no clock input and no bus, so a
//! caller advances it by one tick of a time base it chose itself and reads what
//! pin 13 is being driven with.
//!
//! **The package is the boundary.** Every resistor and capacitor this part is
//! programmed with is on the far side of a pin and is the caller's:
//! [`Components`] is where they go, [`Components::timing`] turns them into
//! whole ticks by the data sheet's own equations, and nothing here holds any
//! board's values.
//!
//! **Integer only, and the part earns it.** Each timing block is a current
//! mirror charging a capacitor at a constant rate into a comparator
//! (`SN76477-042`), so every ramp in the part is a straight line and a counter
//! is an exact model of one rather than an approximation. Nothing here is a
//! filter in the signal-processing sense; the one block called a filter is a
//! slew limiter and is modelled as the ramp it is.
//!
//! **What is not modelled.** The internal noise clock's frequency
//! (`SN76477-OPEN-03`): [`Timing::noise_clock_period_ticks`] is the caller's.
//! What happens to the signal on
//! the far side of pin 13 is the board's subject and is nobody's here.
//!
//! **What the crate chooses**, each an open item: a reset with every capacitor
//! empty (`SN76477-OPEN-02`); Equation 8's peak at pin 13, clipped nowhere
//! (`SN76477-OPEN-05`); the noise filter's window traversed in half the period
//! of its 3 dB frequency (`SN76477-OPEN-08`); the VCO's control voltage
//! clamped at a tenth of its range (`SN76477-OPEN-09`); the mixer's `INHIBIT`
//! row as a mixer output held low (`SN76477-OPEN-10`); a one-shot with no
//! `ROS` and no `COS` that lasts until pin 23 is taken high
//! (`SN76477-OPEN-11`); the SLF and one-shot comparators at the documents'
//! thresholds (`SN76477-OPEN-06`); the noise filter's comparator at the
//! applications paper's window (`SN76477-OPEN-07`); and pin 13 resting at the
//! output op amp's reference (`SN76477-OPEN-12`).
//!
//! # Examples
//!
//! The SLF's square wave through the mixer, with no envelope:
//!
//! ```
//! use sn76477::{Components, Inputs, Level, Sn76477};
//! use units::{Hertz, Ohms, Picofarads};
//!
//! let parts = Components {
//!     r_slf_ohms: Ohms(47_000),
//!     c_slf_pf: Picofarads(1_000),
//! #   r_vco_ohms: Ohms(10_000),
//! #   c_vco_pf: Picofarads(39_000),
//! #   r_noise_filter_ohms: Ohms(47_000),
//! #   c_noise_filter_pf: Picofarads(330),
//! #   r_one_shot_ohms: Some(Ohms(100_000)),
//! #   c_one_shot_pf: Some(Picofarads(1_000_000)),
//! #   r_attack_ohms: Ohms(10_000),
//! #   r_decay_ohms: Ohms(100_000),
//! #   c_attack_decay_pf: Picofarads(1_000),
//!     r_amplitude_ohms: Ohms(100_000),
//!     r_feedback_ohms: Ohms(50_000),
//! };
//! let mut part = Sn76477::new(parts.timing(Hertz(1_000_000)));
//! let pins = Inputs {
//!     mixer_a: Level::High,
//!     envelope_select_2: Level::High,
//!     ..Inputs::default()
//! };
//! let (mut above, mut below) = (0, 0);
//! for _ in 0..100_000 {
//!     let out = part.sample(pins);
//!     above += usize::from(out.0 > 0);
//!     below += usize::from(out.0 < 0);
//! }
//! assert!(above > 0 && below > 0);
//! assert!(part.audio().0.abs() <= 1_700_000);
//! ```

#![no_std]

mod envelope;
mod filter;
mod mixer;
mod noise;
mod one_shot;
mod program;
mod slf;
mod vco;

use core::num::NonZeroUsize;

use noise::NoiseShiftRegister;
pub use program::{Components, Swing, Timing};
use units::Microvolts;

use envelope::{Envelope, EnvelopeGenerator};
use filter::NoiseFilter;
use mixer::Mix;
use one_shot::OneShot;
use slf::Slf;
use vco::Vco;

/// The control voltage at which the VCO's period is
/// [`Timing::vco_max_period_ticks`], in microvolts: the top of the external
/// control range (`SN76477-014`).
pub const VCO_CONTROL_FULL_UV: Microvolts = Microvolts(Vco::CONTROL_FULL_UV);

/// A block's period in ticks, 0 for one that does not run, which is how the
/// blocks take it.
const fn period(ticks: Option<NonZeroUsize>) -> usize {
    match ticks {
        Some(t) => t.get(),
        None => 0,
    }
}

/// Full envelope amplitude, in the steps [`Sn76477::envelope_steps`] counts;
/// one step is 1/65 536 of it, and what it is worth at the pin is
/// [`Timing::output_swing`].
pub const ENVELOPE_FULL_STEPS: u32 = EnvelopeGenerator::FULL_STEPS;

/// Pin 13's DC level with no signal, in microvolts: the output op amp's reference, which
/// its feedback through `RF` holds the pin to when the output stage's current is
/// zero (`SN76477-047`). Every microvolt [`Sn76477::sample`] returns is measured
/// from here; the level at the pin itself is `SN76477-OPEN-12`.
pub const OUTPUT_DC_UV: Microvolts = Microvolts(2_500_000);

/// Half the 3.0 V peak-to-peak swing the sheet gives the output, in microvolts: the
/// scale [`Sn76477::audio`] is normalised against. What the part reaches is
/// [`Timing::output_swing`], which Equation 8 sets from `RF` and `RG`
/// (`SN76477-039`); nothing here clips it, and the sheet's own two bounds on
/// the swing disagree (`SN76477-OPEN-05`).
pub const AUDIO_FULL_SCALE_UV: Microvolts = Microvolts(1_500_000);

/// A pin's logic level, or a
/// block's present bit.
pub use signal::Level;

/// The part's logic and analogue inputs, as levels at pins.
///
/// On the logic inputs a low level and an open pin are the same thing to this
/// part, on all of them but pin 4, which is a resistor pin that a high level
/// overrides.
///
/// `Default` is [`Inputs::idle`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Inputs {
    /// Pin 1, `ENVELOPE SELECT 1`.
    pub envelope_select_1: Level,
    /// Pin 28, `ENVELOPE SELECT 2`.
    pub envelope_select_2: Level,
    /// Pin 26, `MIXER SELECT A`.
    pub mixer_a: Level,
    /// Pin 25, `MIXER SELECT B`.
    pub mixer_b: Level,
    /// Pin 27, `MIXER SELECT C`.
    pub mixer_c: Level,
    /// Pin 22, `VCO SELECT`. High takes the VCO's control voltage from the SLF,
    /// low from pin 16 (`SN76477-014`).
    pub vco_select: Level,
    /// Pin 9, `SYSTEM INHIBIT`. High stops the sound; its falling edge triggers
    /// the one-shot (`SN76477-030`).
    pub system_inhibit: Level,
    /// Pin 4 at a high logic level, which stops the internal noise clock and
    /// hands the register to pin 3 (`SN76477-020`); Low is the resistor
    /// pin left to its resistor.
    pub noise_clock_control: Level,
    /// Pin 3, `EXTERNAL NOISE CLOCK`. Read only while pin 4 is high; the
    /// register shifts on its rising edge.
    pub external_noise_clock: Level,
    /// Pin 23 driven to a high logic level, which ends a one-shot early without
    /// waiting for the capacitor (`SN76477-029`).
    pub one_shot_terminate: Level,
    /// Pin 16, `EXTERNAL VCO CONTROL`, in microvolts.
    pub external_vco_control_uv: Microvolts,
    /// Pin 19, `PITCH CONTROL`, in microvolts. Leave it at or above the control
    /// voltage for a 50% duty cycle, which is what the data sheet means by
    /// leaving it high.
    pub pitch_control_uv: Microvolts,
}

impl Inputs {
    /// Every logic input low, pin 16 at 0 V, and pin 19 left high: at the top
    /// of the control range, [`VCO_CONTROL_FULL_UV`], so the duty cycle is 50%
    /// at any control voltage in range (`SN76477-016`).
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            envelope_select_1: Level::Low,
            envelope_select_2: Level::Low,
            mixer_a: Level::Low,
            mixer_b: Level::Low,
            mixer_c: Level::Low,
            vco_select: Level::Low,
            system_inhibit: Level::Low,
            noise_clock_control: Level::Low,
            external_noise_clock: Level::Low,
            one_shot_terminate: Level::Low,
            external_vco_control_uv: Microvolts(0),
            pitch_control_uv: Microvolts(Vco::CONTROL_FULL_UV),
        }
    }
}

impl Default for Inputs {
    /// [`Inputs::idle`].
    fn default() -> Self {
        Self::idle()
    }
}

/// The whole part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Sn76477 {
    timing: Timing,
    slf: Slf,
    vco: Vco,
    noise_clock: usize,
    external_clock_was_high: bool,
    noise: NoiseShiftRegister,
    noise_filter: NoiseFilter,
    one_shot: OneShot,
    envelope: EnvelopeGenerator,
    mixer_level: bool,
    disabled: bool,
}

impl Sn76477 {
    /// A part with its components fitted and its state at reset.
    ///
    /// **Reset is this crate's, not the part's.** What an `SN76477` holds at
    /// power-up, in the noise register or anywhere else, is this crate's
    /// choice under `SN76477-OPEN-02`: every capacitor empty, the noise
    /// register all zeros, and the oscillators at the bottom of their ramps.
    #[must_use]
    pub const fn new(timing: Timing) -> Self {
        Self {
            timing,
            slf: Slf::new(),
            vco: Vco::new(),
            noise_clock: 0,
            external_clock_was_high: false,
            noise: NoiseShiftRegister::from_bits(0),
            noise_filter: NoiseFilter::new(),
            one_shot: OneShot::new(),
            envelope: EnvelopeGenerator::new(),
            mixer_level: false,
            disabled: true,
        }
    }

    /// The programming, in ticks, that the part was built with.
    #[must_use]
    pub const fn timing(&self) -> Timing {
        self.timing
    }

    /// One tick. Returns what pin 13 is driven with, in microvolts either side
    /// of the output's DC level, [`OUTPUT_DC_UV`].
    ///
    /// Positive while the mixer output is high and negative while it is low
    /// (`SN76477-040`), with a magnitude the envelope scales between zero and
    /// the [`Timing::output_swing`] limit on that side. The same value as
    /// [`Sn76477::audio`].
    #[must_use]
    pub const fn sample(&mut self, pins: Inputs) -> Microvolts {
        self.slf.tick(period(self.timing.slf_period_ticks));
        let slf_out = self.slf.level(period(self.timing.slf_period_ticks));

        let control_uv = if pins.vco_select.is_high() {
            self.slf.triangle_uv(period(self.timing.slf_period_ticks))
        } else {
            pins.external_vco_control_uv.0
        };
        self.vco
            .tick(period(self.timing.vco_max_period_ticks), control_uv);
        let vco_out = self.vco.level(
            period(self.timing.vco_max_period_ticks),
            control_uv,
            pins.pitch_control_uv.0,
        );

        self.tick_noise(pins);
        let noise_out = self.noise_filter.level();

        let mix = Mix::select(
            pins.mixer_a.is_high(),
            pins.mixer_b.is_high(),
            pins.mixer_c.is_high(),
        );
        self.mixer_level = mix.level(slf_out, vco_out, noise_out);

        self.one_shot.tick(
            pins.system_inhibit.is_high(),
            pins.one_shot_terminate.is_high(),
            self.timing.one_shot_ticks,
        );

        let cutoff =
            pins.vco_select.is_low() && pins.external_vco_control_uv.0 > Vco::CONTROL_FULL_UV;
        self.disabled = pins.system_inhibit.is_high() || cutoff;

        let selection = Envelope::select(
            pins.envelope_select_1.is_high(),
            pins.envelope_select_2.is_high(),
        );
        let gate = selection.gate(vco_out, self.vco.alternate(), self.one_shot.level());
        self.envelope.tick(
            self.disabled,
            gate,
            selection.decays(),
            self.timing.attack_ticks,
            self.timing.decay_ticks,
        );

        self.audio()
    }

    /// The noise clock and everything it drives.
    const fn tick_noise(&mut self, pins: Inputs) {
        let edge = if pins.noise_clock_control.is_high() {
            let rising = !self.external_clock_was_high && pins.external_noise_clock.is_high();
            self.external_clock_was_high = pins.external_noise_clock.is_high();
            rising
        } else if let Some(period) = self.timing.noise_clock_period_ticks {
            self.noise_clock += 1;
            if self.noise_clock >= period.get() {
                self.noise_clock = 0;
                true
            } else {
                false
            }
        } else {
            false
        };
        if edge {
            self.noise.shift();
        }
        self.noise_filter
            .tick(self.noise.level(), self.timing.noise_filter_traverse_ticks);
    }

    /// Pin 13's level, in microvolts, without advancing anything: the swing
    /// [`Sn76477::audio`] gives, about [`OUTPUT_DC_UV`], and that rest
    /// itself while the part is inhibited.
    #[must_use]
    pub const fn output_uv(&self) -> Microvolts {
        Microvolts(OUTPUT_DC_UV.0.saturating_add(self.audio().0))
    }

    /// Pin 13 as a signed sample: microvolts either side of
    /// [`OUTPUT_DC_UV`], 0 at rest and while inhibited, full scale the
    /// [`Timing::output_swing`] limit's distance from [`OUTPUT_DC_UV`] on the
    /// mixer's side, which at the sheet's 3.0 V peak-to-peak swing is
    /// [`AUDIO_FULL_SCALE_UV`]. [`Sn76477::sample`] returns it too.
    #[must_use]
    pub const fn audio(&self) -> Microvolts {
        if self.disabled {
            return Microvolts(0);
        }
        let Some(swing) = self.timing.output_swing else {
            return Microvolts(0);
        };
        let (low, high) = swing.range_uv();
        let dc = OUTPUT_DC_UV.0 as i64;
        let half = if self.mixer_level {
            high.0 as i64 - dc
        } else {
            dc - low.0 as i64
        };
        let peak = if half < 0 { 0 } else { half as u64 };
        let magnitude = (peak * self.envelope.amplitude_steps() as u64
            / EnvelopeGenerator::FULL_STEPS as u64) as i64;
        let signed = if self.mixer_level {
            magnitude
        } else {
            -magnitude
        };
        Microvolts(if signed > i32::MAX as i64 {
            i32::MAX
        } else if signed < i32::MIN as i64 {
            i32::MIN
        } else {
            signed as i32
        })
    }

    /// The mixer's output, before any envelope.
    #[must_use]
    pub const fn mixer_level(&self) -> Level {
        Level::from_bool(self.mixer_level)
    }

    /// The envelope's amplitude, `0..=`[`ENVELOPE_FULL_STEPS`].
    #[must_use]
    pub const fn envelope_steps(&self) -> u32 {
        self.envelope.amplitude_steps()
    }

    /// The noise generator's shift register, its thirty-two latches.
    #[must_use]
    pub const fn noise_shift_register(&self) -> u32 {
        self.noise.bits()
    }

    /// The one-shot pulse.
    #[must_use]
    pub const fn one_shot_level(&self) -> Level {
        Level::from_bool(self.one_shot.level())
    }
}

#[cfg(test)]
mod tests {
    use super::{Inputs, Level, OUTPUT_DC_UV, Sn76477, Swing, Timing};
    use core::num::NonZeroUsize;
    use units::Microvolts;

    const fn quiet() -> Timing {
        Timing {
            slf_period_ticks: None,
            vco_max_period_ticks: None,
            noise_clock_period_ticks: None,
            noise_filter_traverse_ticks: 0,
            one_shot_ticks: Some(0),
            attack_ticks: 1,
            decay_ticks: 1,
            output_swing: None,
        }
    }

    const fn from_mv(mv: i32) -> Microvolts {
        Microvolts(mv * 1000)
    }

    /// `SN76477-040` with a swing that is not centred: each half of the output
    /// reaches its own limit, and a limit on the wrong side of the DC level gives
    /// that half nothing.
    #[test]
    fn an_uncentred_swing_sets_each_half() {
        let pins = Inputs {
            mixer_a: Level::High,
            envelope_select_2: Level::High,
            ..Inputs::default()
        };
        let run = |swing| {
            let mut part = Sn76477::new(Timing {
                slf_period_ticks: NonZeroUsize::new(400),
                output_swing: swing,
                ..quiet()
            });
            let (mut lowest, mut highest) = (OUTPUT_DC_UV, OUTPUT_DC_UV);
            for _ in 0..4_000 {
                let _ = part.sample(pins);
                lowest = lowest.min(part.output_uv());
                highest = highest.max(part.output_uv());
            }
            (lowest, highest)
        };
        let swing = Swing::declared(from_mv(2_000), from_mv(4_000));
        assert_eq!(run(swing), (from_mv(2_000), from_mv(4_000)));
        let above = Swing::declared(from_mv(3_000), from_mv(4_000));
        assert_eq!(run(above), (OUTPUT_DC_UV, from_mv(4_000)));
        assert_eq!(run(None), (OUTPUT_DC_UV, OUTPUT_DC_UV));
    }
}
