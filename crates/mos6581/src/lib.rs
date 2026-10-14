//! The MOS 6581 Sound Interface Device, emulated at its pins.
//!
//! The crate depends only on `units`, allocates nothing, does no I/O, uses no
//! floating point and is `no_std`: a [`Mos6581`] is a plain value that
//! advances one `Ø2` cycle at a time and shows the outside world its [`Pins`].
//!
//! # Examples
//!
//! ```
//! use mos6581::control::{GATE, SAWTOOTH};
//! use mos6581::{AUDIO_OUT_DC_UV, Mos6581, Pins, Select};
//! use units::{Hertz, Picofarads};
//!
//! // A 1 MHz clock and the sheet's recommended 2200 pF at the filter pins.
//! let mut sid = Mos6581::new(Hertz(1_000_000), Picofarads(2200))?;
//! // Voice 1: concert A (`MOS6581-018`), sawtooth, attack 0, sustain 15.
//! let _ = sid.tick(Pins::write_access(Select::new(0x00), 0xd6));
//! let _ = sid.tick(Pins::write_access(Select::new(0x01), 0x1c));
//! let _ = sid.tick(Pins::write_access(Select::new(0x05), 0x00));
//! let _ = sid.tick(Pins::write_access(Select::new(0x06), 0xf0));
//! let _ = sid.tick(Pins::write_access(Select::new(0x18), 0x0f)); // volume 15
//! let _ = sid.tick(Pins::write_access(Select::new(0x04), SAWTOOTH | GATE));
//! // The level at pin 27, once per cycle, in microvolts.
//! let mut lowest = AUDIO_OUT_DC_UV;
//! let mut highest = AUDIO_OUT_DC_UV;
//! for _ in 0..5_000 {
//!     let out = sid.tick(Pins::idle());
//!     lowest = lowest.min(out.audio_out_uv);
//!     highest = highest.max(out.audio_out_uv);
//! }
//! assert!(lowest < AUDIO_OUT_DC_UV && highest > AUDIO_OUT_DC_UV);
//! assert!(sid.audio().0.abs() <= mos6581::AUDIO_FULL_SCALE_UV.0);
//! # Ok::<(), mos6581::Mos6581Error>(())
//! ```
//!
//! # The part, not the machine
//!
//! Twenty-nine registers behind `CS`, `R/W` and `A4`–`A0`, and nothing about
//! where a machine puts them (`MOS6581-005`). The two filter capacitors, the
//! potentiometers and their capacitors, the source resistor at `AUDIO OUT`
//! and whatever drives `EXT IN` are a board's, and the two numbers the part
//! cannot work without — the clock rate and the filter capacitance — are
//! taken at construction.
//!
//! # One tick is one `Ø2` cycle, and the bus access rides in the pins
//!
//! [`Mos6581::tick`] takes the pins for one cycle and returns them. A read or
//! write the part is selected for in that cycle is carried in the same word —
//! [`Pins::read_access`] and [`Pins::write_access`] build one — and the byte a read returns
//! is in the returned word's [`Pins::d`], **resolved in the cycle it is
//! taken**. The level at `AUDIO OUT` is in [`Pins::audio_out_uv`] every
//! cycle, in microvolts into the sheet's 1 kΩ, so a consumer takes one sample
//! per cycle and resamples on its own side. Within one cycle the part
//! **advances its state first and serves the bus after** (`MOS6581-052`).
//!
//! # What this crate chooses
//!
//! Seven behaviours here are the crate's choice, each against an open item,
//! and each is marked where it is written: the numbers in the filter — what a
//! resonance setting is worth (`MOS6581-OPEN-09`), *no resonance* as the
//! damping at which the low-pass shows no peak (`MOS6581-OPEN-10`), one
//! structure with no ceiling above the designed range (`MOS6581-OPEN-11`),
//! and a state-variable filter with the cutoff linear in the register value
//! and unity gain in the pass bands (`MOS6581-OPEN-12`); one voice at
//! full swing is 1 V peak to peak at the pin (`MOS6581-OPEN-13`); two waveforms
//! selected together are the sheet's bitwise AND (`MOS6581-OPEN-18`); and a
//! byte left on the internal bus is held for ever (`MOS6581-OPEN-19`).
//! Everything digital — the accumulator, the twelve-bit outputs, the pulse
//! comparator, the noise register, the envelope's counters and edges — is
//! `MOS6581-052` to `-064`.

#![no_std]

#[cfg(test)]
extern crate std;

mod envelope;
mod filter;
mod oscillator;
mod pins;
mod pot;

pub use envelope::EnvelopeStage;

/// An envelope's full level, in the steps [`Mos6581::envelope_steps`]
/// counts: eight bits, as `ENV 3` reads them (`MOS6581-016`,
/// `MOS6581-045`). One step is 1/255 of a voice's full swing.
pub const ENVELOPE_FULL_STEPS: u32 = envelope::FULL as u32;
pub use filter::fcmax_millihertz;
pub use oscillator::{
    NOISE_BIT_COUNT, NOISE_CLOCK_BIT, NOISE_SHIFT_LAG_CYCLES, OUTPUT_BIT_COUNT, PHASE_BIT_COUNT,
};
pub use pins::{AUDIO_OUT_DC_UV, Ctrl, EXT_IN_DC_UV, Pins, Select};
pub use pot::{POTS_PERIOD_CYCLES, SINK_CYCLES, trigger_uv};

use control::{GATE, NOISE, PULSE, SAWTOOTH, SYNC, TEST, TRIANGLE};
use envelope::Envelope;
use filter::Filter;
use mode_vol::VOICE3_OFF;
use oscillator::Oscillator;
use pot::Pot;
use reg::{ENV3, FC_LOWER, FC_UPPER, MODE_VOL, OSC3, POTX, POTY, RES_FILT};
use res_filt::{FILT1, FILTEX};
use units::{Hertz, Microvolts, Picofarads};

/// The bits of a voice's control register, at `$04`, `$0B` and `$12`
/// (`MOS6581-014`).
pub mod control {
    pub use crate::oscillator::{GATE, NOISE, PULSE, RING_MOD, SAWTOOTH, SYNC, TEST, TRIANGLE};
}

/// The bits of `RES/Filt` ([`reg::RES_FILT`]) that route a source through the
/// filter (`MOS6581-039`); bits 7–4 are the resonance.
pub mod res_filt {
    pub use crate::filter::{FILT1, FILT2, FILT3, FILTEX};
}

/// The bits of `Mode/Vol` ([`reg::MODE_VOL`]) that choose the filter's outputs and
/// cut voice 3 (`MOS6581-040`, `MOS6581-042`); bits 3–0 are the volume.
pub mod mode_vol {
    pub use crate::filter::{BP, HP, LP, VOICE3_OFF};
}

/// The register addresses the part's logic names (`MOS6581-013`); a voice's
/// seven registers start at `$00`, `$07` and `$0E`.
pub mod reg {
    /// `FC Lo`, register `$15`.
    pub const FC_LOWER: u8 = 0x15;
    /// `FC Hi`, register `$16`.
    pub const FC_UPPER: u8 = 0x16;
    /// `RES/Filt`, register `$17`.
    pub const RES_FILT: u8 = 0x17;
    /// `Mode/Vol`, register `$18`.
    pub const MODE_VOL: u8 = 0x18;
    /// `POTX`, register `$19`.
    pub const POTX: u8 = 0x19;
    /// `POTY`, register `$1A`.
    pub const POTY: u8 = 0x1a;
    /// `OSC 3/RANDOM`, register `$1B`.
    pub const OSC3: u8 = 0x1b;
    /// `ENV 3`, register `$1C`.
    pub const ENV3: u8 = 0x1c;
}

/// The write-only registers, `$00` to `$18` (`MOS6581-013`).
pub const WRITE_ONLY_REGISTER_COUNT: usize = 0x19;

/// [`Mos6581::accumulator`]'s units in a volt at the pin (`MOS6581-OPEN-13`):
/// one voice's full swing — a twelve-bit waveform around its midpoint times
/// an eight-bit envelope — is `2 · 2048 · 255` units, and it is 1 V peak to
/// peak at the pin.
const UNITS_PER_VOLT: i64 = 2 * 2048 * 255;

/// The three voices' largest magnitude in the composite: each a twelve-bit
/// waveform's lowest step below its midpoint at a full envelope.
const VOICES_UNITS: i64 = 3 * 2048 * 255;

/// `EXT IN`'s largest departure from [`EXT_IN_DC_UV`] the sheet admits, in
/// microvolts: half of 3 V peak to peak (`MOS6581-048`).
const EXT_IN_PEAK_UV: i64 = 1_500_000;

/// The largest magnitude [`Mos6581::audio`] returns, in microvolts, at every
/// register setting, clock and capacitance, with `EXT IN` inside the 3 V
/// peak to peak `MOS6581-048` admits: the filter's output at its
/// saturation, 32 V at the pin (`MOS6581-OPEN-11`), the three voices taken
/// direct, 1.5 V, and `EXT IN` taken direct, 1.5 V; volume 15 passes them
/// whole.
///
/// The filter reaches its saturation inside the sheet's own figures: at
/// 1 MHz and 2200 pF, resonance 15 and the top cutoff, band-pass and
/// high-pass together pass just over 21 times one input's swing
/// (`MOS6581-OPEN-09`). At a clock nearer the sheet's slowest the top cutoff
/// is too near the clock for the once-a-cycle recursion, which then rises to
/// the saturation from any input and holds there, at low resonance and not
/// at high: at 2200 pF, resonances 0 to 9 at 50 kHz, 0 to 4 at 60 kHz and 0
/// alone at 70 kHz, while resonance 15 settles at all three. Three voices
/// direct alone reach 1.5 V, about a twenty-third of this. A signal at
/// `EXT IN` past the sheet's swing carries the level further, and the crate
/// does not clamp it.
pub const AUDIO_FULL_SCALE_UV: Microvolts = Microvolts(
    ((filter::SATURATION + VOICES_UNITS) * 1_000_000 / UNITS_PER_VOLT + EXT_IN_PEAK_UV) as i32,
);

/// Why [`Mos6581::new`] refuses a part.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum Mos6581Error {
    /// A clock of zero hertz: the filter's coefficient divides by it.
    ZeroClock,
    /// A filter capacitance of zero picofarads: the cutoff divides by it.
    ZeroCapacitance,
}

impl core::fmt::Display for Mos6581Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::ZeroClock => "a clock of zero hertz",
            Self::ZeroCapacitance => "a filter capacitance of zero",
        })
    }
}

impl core::error::Error for Mos6581Error {}

/// The whole part.
///
/// [`Mos6581::new`] is the part as `RES` leaves it (`MOS6581-009`) with its
/// accumulators and envelope counters, which `RES` does not touch
/// (`MOS6581-063`), at zero. There is no other constructor.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Mos6581 {
    /// The twenty-five write-only registers (`MOS6581-013`).
    regs: [u8; WRITE_ONLY_REGISTER_COUNT],
    osc: [Oscillator; 3],
    env: [Envelope; 3],
    filter: Filter,
    pots: [Pot; 2],
    /// Each oscillator's twelve-bit output this cycle, for `OSC 3`.
    wave: [u16; 3],
    /// The composite this cycle, in units, before the volume control.
    composite: i64,
    /// The byte the internal data bus last carried, which a read of an
    /// address with no register behind it returns (`MOS6581-064`).
    bus: u8,
    /// The clock at `Ø2`, in hertz — needed only by the filter.
    clock_hz: u32,
    /// The capacitance at each pair of filter pins, in picofarads.
    capacitor_pf: u32,
}

impl Mos6581 {
    /// The part as `RES` leaves it (`MOS6581-009`), clocked at `clock_hz`
    /// with `capacitor_pf` at each pair of filter pins (`MOS6581-037`).
    ///
    /// The sheet's clock is 1.0 MHz nominal, 1 to 20 µs per cycle
    /// (`MOS6581-004`), and its recommended capacitance 2200 pF. Neither is
    /// enforced: a consumer's clock outside the range is the consumer's, and
    /// the part has no ceiling on the filter (`MOS6581-OPEN-11`).
    ///
    /// # Errors
    ///
    /// [`Mos6581Error::ZeroClock`] on a zero clock and
    /// [`Mos6581Error::ZeroCapacitance`] on a zero capacitance, which no
    /// board has.
    ///
    /// # Examples
    ///
    /// ```
    /// use mos6581::{Mos6581, Mos6581Error};
    /// use units::{Hertz, Picofarads};
    ///
    /// // A 1 MHz clock and the sheet's recommended 2200 pF.
    /// let sid = Mos6581::new(Hertz(1_000_000), Picofarads(2200))?;
    /// # let _ = sid;
    ///
    /// assert_eq!(
    ///     Mos6581::new(Hertz(0), Picofarads(2200)),
    ///     Err(Mos6581Error::ZeroClock)
    /// );
    /// # Ok::<(), Mos6581Error>(())
    /// ```
    pub const fn new(clock_hz: Hertz, capacitor_pf: Picofarads) -> Result<Self, Mos6581Error> {
        let (clock_hz, capacitor_pf) = (clock_hz.0, capacitor_pf.0);
        if clock_hz == 0 {
            return Err(Mos6581Error::ZeroClock);
        }
        if capacitor_pf == 0 {
            return Err(Mos6581Error::ZeroCapacitance);
        }
        Ok(Self {
            regs: [0; WRITE_ONLY_REGISTER_COUNT],
            osc: [Oscillator::reset(); 3],
            env: [Envelope::reset(); 3],
            filter: Filter::reset(clock_hz, capacitor_pf),
            pots: [Pot::reset(); 2],
            wave: [0; 3],
            composite: 0,
            bus: 0,
            clock_hz,
            capacitor_pf,
        })
    }

    /// One `Ø2` cycle.
    ///
    /// See the crate documentation for what rides in `pins` and what comes
    /// back. `RES` Low zeroes the registers in this cycle, leaves the
    /// accumulators and envelope counters where they stand (`MOS6581-009`,
    /// `MOS6581-063`), and drives the DC level and nothing else.
    #[must_use]
    pub fn tick(&mut self, pins: Pins) -> Pins {
        if pins.ctrl.contains(Ctrl::RES) {
            self.regs = [0; WRITE_ONLY_REGISTER_COUNT];
            for env in &mut self.env {
                env.on_reset();
            }
            self.filter = Filter::reset(self.clock_hz, self.capacitor_pf);
            self.pots = [Pot::reset(); 2];
            self.wave = [0; 3];
            self.composite = 0;
            self.bus = 0;
            return self.outputs(pins, None, [false; 2]);
        }

        let msb_before = [self.osc[0].msb(), self.osc[1].msb(), self.osc[2].msb()];
        let mut shifted = [false; 3];
        for (v, shift) in shifted.iter_mut().enumerate() {
            let (freq, _, ctrl) = self.voice_regs(v);
            *shift = self.osc[v].step(freq, ctrl & TEST != 0);
        }
        let rose: [bool; 3] = core::array::from_fn(|n| !msb_before[n] && self.osc[n].msb());
        for v in 0..3 {
            let (_, _, ctrl) = self.voice_regs(v);
            if ctrl & SYNC != 0 && rose[Self::neighbour(v)] {
                self.osc[v].sync();
            }
        }
        for (v, &shifted) in shifted.iter().enumerate() {
            let (_, pw, ctrl) = self.voice_regs(v);
            let ring_msb = self.osc[Self::neighbour(v)].msb();
            self.wave[v] = self.osc[v].waveform(ctrl, pw, ring_msb);
            if shifted && ctrl & NOISE != 0 && ctrl & (TRIANGLE | SAWTOOTH | PULSE) != 0 {
                self.osc[v].noise_writeback(self.wave[v]);
                self.wave[v] = self.osc[v].waveform(ctrl, pw, ring_msb);
            }
        }
        for v in 0..3 {
            let base = v * 7;
            let ctrl = self.regs[base + 4];
            self.env[v].step(ctrl & GATE != 0, self.regs[base + 5], self.regs[base + 6]);
        }

        let routing = self.regs[RES_FILT as usize];
        let mode_vol = self.regs[MODE_VOL as usize];
        let mut direct = 0i64;
        let mut through = 0i64;
        for v in 0..3 {
            let sample = (self.wave[v] as i64 - 2048) * self.env[v].level() as i64;
            if routing & (FILT1 << v) != 0 {
                through += sample;
            } else if v != 2 || mode_vol & VOICE3_OFF == 0 {
                direct += sample;
            }
        }
        let ext = (pins.ext_in_uv.0 as i64 - EXT_IN_DC_UV.0 as i64) * UNITS_PER_VOLT / 1_000_000;
        if routing & FILTEX != 0 {
            through += ext;
        } else {
            direct += ext;
        }
        let filtered = self.filter.run(through, mode_vol);
        self.composite = direct + filtered;

        let sinking = [
            self.pots[0].step(pins.potx_uv.0),
            self.pots[1].step(pins.poty_uv.0),
        ];

        let mut d = None;
        if pins.ctrl.contains(Ctrl::CS) {
            let a = pins.a & 0x1f;
            if pins.ctrl.contains(Ctrl::WRITE) {
                self.write(a, pins.d.unwrap_or(0));
            } else {
                d = Some(self.read(a));
            }
        }

        self.outputs(pins, d, sinking)
    }

    /// Voice `v`'s frequency, pulse width and control bytes.
    const fn voice_regs(&self, v: usize) -> (u16, u16, u8) {
        let base = v * 7;
        let freq = u16::from_le_bytes([self.regs[base], self.regs[base + 1]]);
        let pw = u16::from_le_bytes([self.regs[base + 2], self.regs[base + 3] & 0x0f]);
        (freq, pw, self.regs[base + 4])
    }

    /// The oscillator voice `v` syncs to and ring-modulates with
    /// (`MOS6581-026`, `MOS6581-027`): 3 for voice 1, 1 for 2, 2 for 3.
    const fn neighbour(v: usize) -> usize {
        (v + 2) % 3
    }

    /// `MOS6581-007`, `MOS6581-013`: a write lands in a write-only register
    /// and nowhere else, and leaves its byte on the bus (`MOS6581-064`).
    fn write(&mut self, addr: u8, value: u8) {
        self.bus = value;
        if (addr as usize) < WRITE_ONLY_REGISTER_COUNT {
            self.regs[addr as usize] = value;
        }
        if matches!(addr, FC_LOWER | FC_UPPER | RES_FILT) {
            let fc = ((self.regs[FC_UPPER as usize] as u16) << 3)
                | (self.regs[FC_LOWER as usize] as u16 & 0x07);
            self.filter.set(
                fc,
                self.regs[RES_FILT as usize] >> 4,
                self.clock_hz,
                self.capacitor_pf,
            );
        }
    }

    /// `MOS6581-016`, `MOS6581-044` to `-046`: the four read-only registers;
    /// everything else reads what the bus last carried (`MOS6581-064`), held
    /// for ever (`MOS6581-OPEN-19`).
    const fn read(&mut self, addr: u8) -> u8 {
        self.bus = match addr {
            POTX => self.pots[0].value(),
            POTY => self.pots[1].value(),
            OSC3 => (self.wave[2] >> (OUTPUT_BIT_COUNT - 8)) as u8,
            ENV3 => self.env[2].level(),
            _ => self.bus,
        };
        self.bus
    }

    /// The returned pin word: the caller's inputs, the read byte, the level
    /// at `AUDIO OUT` and the two sink flags.
    const fn outputs(&self, pins: Pins, d: Option<u8>, sinking: [bool; 2]) -> Pins {
        let mut out = pins;
        out.d = d;
        out.audio_out_uv = Microvolts(self.audio_out_uv());
        out.ctrl = pins.ctrl.without(Ctrl::POTX_LOW.with(Ctrl::POTY_LOW));
        if sinking[0] {
            out.ctrl = out.ctrl.with(Ctrl::POTX_LOW);
        }
        if sinking[1] {
            out.ctrl = out.ctrl.with(Ctrl::POTY_LOW);
        }
        out
    }

    /// `MOS6581-043`, `MOS6581-049`: the composite through the volume
    /// control, scaled onto the pin's volts around the DC level
    /// (`MOS6581-OPEN-13`).
    const fn audio_out_uv(&self) -> i32 {
        let vol = (self.regs[MODE_VOL as usize] & 0x0f) as i64;
        let uv = self.composite * vol / 15 * 1_000_000 / UNITS_PER_VOLT + AUDIO_OUT_DC_UV.0 as i64;
        if uv > i32::MAX as i64 {
            i32::MAX
        } else if uv < i32::MIN as i64 {
            i32::MIN
        } else {
            uv as i32
        }
    }

    /// The sound at `AUDIO OUT` as a signed sample at rest 0: the pin's level
    /// in microvolts, [`Pins::audio_out_uv`], less its DC level
    /// [`AUDIO_OUT_DC_UV`]. One voice at full swing and volume 15 is 1 V
    /// peak to peak, −500 000 µV to +500 000 µV (`MOS6581-OPEN-13`), and three
    /// taken direct 3 V. The filter's gain carries it further, up to
    /// [`AUDIO_FULL_SCALE_UV`], and nothing here clips it.
    #[must_use]
    pub const fn audio(&self) -> Microvolts {
        Microvolts(self.audio_out_uv().saturating_sub(AUDIO_OUT_DC_UV.0))
    }

    /// The sum before the output stage, the volume control: every voice
    /// and `EXT IN`, direct or through the filter, this cycle. One voice at
    /// full swing spans `2 · 2048 · 255` units, which is 1 V peak to peak at
    /// the pin (`MOS6581-OPEN-13`); not a pin. Held to the `i32` range, which
    /// only a signal at `EXT IN` far past the supply reaches.
    #[must_use]
    pub const fn accumulator(&self) -> i32 {
        if self.composite > i32::MAX as i64 {
            i32::MAX
        } else if self.composite < i32::MIN as i64 {
            i32::MIN
        } else {
            self.composite as i32
        }
    }

    /// Write-only register `i`'s contents, `$00` to `$18`; reading it here
    /// touches nothing.
    ///
    /// # Panics
    /// If `i` is not below [`WRITE_ONLY_REGISTER_COUNT`].
    #[must_use]
    pub const fn register(&self, i: usize) -> u8 {
        self.regs[i]
    }

    /// Oscillator `v`'s phase: its phase accumulator, [`PHASE_BIT_COUNT`]
    /// bits, `v` being 0 to 2.
    ///
    /// # Panics
    /// If `v` is not `0..=2`.
    #[must_use]
    pub const fn phase(&self, v: usize) -> u32 {
        self.osc[v].phase()
    }

    /// Oscillator `v`'s twelve-bit output this cycle.
    ///
    /// # Panics
    /// If `v` is not `0..=2`.
    #[must_use]
    pub const fn waveform(&self, v: usize) -> u16 {
        self.wave[v]
    }

    /// Envelope `v`'s level, `0` to [`ENVELOPE_FULL_STEPS`].
    ///
    /// # Panics
    /// If `v` is not `0..=2`.
    #[must_use]
    pub const fn envelope_steps(&self, v: usize) -> u32 {
        self.env[v].level() as u32
    }

    /// Envelope `v`'s stage.
    ///
    /// # Panics
    /// If `v` is not `0..=2`.
    #[must_use]
    pub const fn envelope_stage(&self, v: usize) -> EnvelopeStage {
        self.env[v].stage(self.regs[v * 7 + 6])
    }

    /// Oscillator `v`'s noise shift register, [`NOISE_BIT_COUNT`] bits.
    ///
    /// # Panics
    /// If `v` is not `0..=2`.
    #[must_use]
    pub const fn noise_shift_register(&self, v: usize) -> u32 {
        self.osc[v].noise_register()
    }

    /// The clock the part was built for.
    #[must_use]
    pub const fn clock_hz(&self) -> Hertz {
        Hertz(self.clock_hz)
    }

    /// The filter capacitance the part was built for.
    #[must_use]
    pub const fn capacitor_pf(&self) -> Picofarads {
        Picofarads(self.capacitor_pf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use control::{GATE, SAWTOOTH};

    #[test]
    fn new_names_each_refusal() {
        assert_eq!(
            Mos6581::new(Hertz(0), Picofarads(2200)),
            Err(Mos6581Error::ZeroClock)
        );
        assert_eq!(
            Mos6581::new(Hertz(1_000_000), Picofarads(0)),
            Err(Mos6581Error::ZeroCapacitance)
        );
        assert!(Mos6581::new(Hertz(1_000_000), Picofarads(2200)).is_ok());
    }
    use mode_vol::{BP, HP, LP};
    use res_filt::{FILT2, FILT3};

    /// Oscillator 1 rises in the cycle oscillator 3 rises and syncs it to
    /// zero; voice 2, synced to oscillator 1, still sees that edge
    /// (`MOS6581-026`, `MOS6581-057`).
    #[test]
    fn a_sync_edge_is_seen_even_when_its_oscillator_is_synced_the_same_cycle() {
        let mut sid = Mos6581::new(Hertz(1_000_000), Picofarads(2200))
            .expect("a non-zero clock and capacitance");
        let mut write = |a: u8, d: u8| {
            let _ = sid.tick(Pins::write_access(Select::new(a), d));
        };
        write(4, TEST | SYNC);
        write(18, TEST);
        write(0, 0xff);
        write(1, 0xff);
        write(14, 0x10);
        write(15, 0xfc);
        write(7, 0x00);
        write(8, 0x10);
        write(11, SYNC);
        write(18, 0);
        write(4, SYNC);
        let top = 1 << 23;
        for _ in 0..200 {
            let before = [sid.phase(0), sid.phase(1), sid.phase(2)];
            let _ = sid.tick(Pins::idle());
            if before[2] < top && sid.phase(2) >= top {
                assert!(before[0] < top && before[0] + 0xffff >= top, "{before:x?}");
                assert_eq!(sid.phase(0), 0, "oscillator 1 synced by 3");
                assert_eq!(sid.phase(1), 0, "voice 2 saw oscillator 1's edge");
                return;
            }
        }
        panic!("oscillator 3 never rose");
    }

    #[test]
    fn the_full_scale_is_the_saturated_filter_and_everything_direct() {
        assert_eq!(filter::SATURATION, 32 * UNITS_PER_VOLT);
        assert_eq!(VOICES_UNITS * 2, 3 * UNITS_PER_VOLT);
        assert_eq!(AUDIO_FULL_SCALE_UV, Microvolts(35_000_000));
    }

    /// Random writes to every register, resonance 15 at every cutoff, every
    /// waveform and routing, volume 15, clocks across the sheet's range and
    /// `EXT IN` anywhere in its swing: `audio()` never passes the bound, and
    /// passes the three direct voices' 1.5 V.
    #[test]
    fn no_register_setting_carries_audio_past_the_full_scale() {
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut peak = 0;
        for trial in 0..120 {
            let clock_hz = [50_000, 100_000, 250_000, 500_000, 1_000_000][trial % 5];
            let capacitor_pf = [470, 1000, 2200, 4700][(trial / 5) % 4];
            let mut sid = Mos6581::new(Hertz(clock_hz), Picofarads(capacitor_pf))
                .expect("a non-zero clock and capacitance");
            let peak_uv = EXT_IN_PEAK_UV as i32;
            let mut ext = EXT_IN_DC_UV.0;
            for cycle in 0..6_000 {
                let mut pins = Pins::idle();
                if cycle % 64 == 0 {
                    let a = (next() % 0x19) as u8;
                    let mut d = next() as u8;
                    if a == RES_FILT {
                        d |= 0xf0;
                    } else if a == MODE_VOL {
                        d |= 0x0f;
                    }
                    pins = Pins::write_access(Select::new(a), d);
                }
                if cycle % 16 == 0 {
                    ext = match next() % 3 {
                        0 => EXT_IN_DC_UV.0 - peak_uv,
                        1 => EXT_IN_DC_UV.0 + peak_uv,
                        _ => EXT_IN_DC_UV.0 - peak_uv + (next() % 3_000_001) as i32,
                    };
                }
                let _ = sid.tick(pins.with_ext_in_uv(Microvolts(ext)));
                let level = sid.audio().0.abs();
                assert!(
                    level <= AUDIO_FULL_SCALE_UV.0,
                    "trial {trial}, {clock_hz} Hz, {capacitor_pf} pF, cycle {cycle}: {level}"
                );
                peak = peak.max(level);
            }
        }
        assert!(peak > 1_500_000, "{peak}");
    }

    /// The most the saturated filter and three direct voices reach, in
    /// microvolts.
    const SATURATED_UV: i64 = (filter::SATURATION + 3 * 2048 * 255) * 1_000_000 / UNITS_PER_VOLT;

    /// Clocks and capacitors inside the sheet's range whose top cutoff the
    /// once-a-cycle recursion cannot follow: three voices through the filter
    /// at the top of the register stay inside the filter's saturation,
    /// without a panic in a debug build.
    #[test]
    fn a_slow_clock_at_the_top_cutoff_stays_inside_the_saturation() {
        for (clock_hz, capacitor_pf) in [(50_000, 2200), (100_000, 1000), (250_000, 470)] {
            for res in [0x00, 0xf0] {
                for mode in [LP, BP, HP, LP | BP | HP] {
                    let mut sid = Mos6581::new(Hertz(clock_hz), Picofarads(capacitor_pf))
                        .expect("a non-zero clock and capacitance");
                    for v in 0..3u8 {
                        let base = v * 7;
                        let _ = sid.tick(Pins::write_access(Select::new(base), 0xd6));
                        let _ = sid.tick(Pins::write_access(Select::new(base + 1), 0x1c + v));
                        let _ = sid.tick(Pins::write_access(Select::new(base + 6), 0xf0));
                        let _ =
                            sid.tick(Pins::write_access(Select::new(base + 4), SAWTOOTH | GATE));
                    }
                    let _ = sid.tick(Pins::write_access(Select::new(FC_LOWER), 0x07));
                    let _ = sid.tick(Pins::write_access(Select::new(FC_UPPER), 0xff));
                    let _ = sid.tick(Pins::write_access(
                        Select::new(RES_FILT),
                        res | FILT1 | FILT2 | FILT3,
                    ));
                    let _ = sid.tick(Pins::write_access(Select::new(MODE_VOL), mode | 0x0f));
                    let mut peak = 0;
                    for _ in 0..20_000 {
                        let _ = sid.tick(Pins::idle());
                        peak = peak.max(sid.audio().0.abs());
                    }
                    assert!(
                        i64::from(peak) <= SATURATED_UV,
                        "{clock_hz} Hz, {capacitor_pf} pF, res {res:#x}, mode {mode:#x}: {peak}"
                    );
                }
            }
        }
    }

    /// The level band-pass and high-pass reach at the top cutoff and
    /// 2200 pF, `res` the resonance, after a 20-cycle kick at `EXT IN`.
    fn level_after_a_kick(clock_hz: u32, res: u8) -> i32 {
        let mut sid = Mos6581::new(Hertz(clock_hz), Picofarads(2200))
            .expect("a non-zero clock and capacitance");
        let _ = sid.tick(Pins::write_access(Select::new(FC_LOWER), 0x07));
        let _ = sid.tick(Pins::write_access(Select::new(FC_UPPER), 0xff));
        let _ = sid.tick(Pins::write_access(
            Select::new(RES_FILT),
            (res << 4) | FILTEX,
        ));
        let _ = sid.tick(Pins::write_access(Select::new(MODE_VOL), BP | HP | 0x0f));
        let kick = Pins::idle().with_ext_in_uv(Microvolts(EXT_IN_DC_UV.0 + 1_500_000));
        for _ in 0..20 {
            let _ = sid.tick(kick);
        }
        for _ in 0..20_000 {
            let _ = sid.tick(Pins::idle());
        }
        let mut peak = 0;
        for _ in 0..20_000 {
            let _ = sid.tick(Pins::idle());
            peak = peak.max(sid.audio().0.abs());
        }
        peak
    }

    /// [`AUDIO_FULL_SCALE_UV`]'s doc: a slow clock at the top cutoff holds the
    /// filter at its saturation at low resonance, and higher settles to under
    /// a millivolt.
    #[test]
    fn a_slow_clock_saturates_at_low_resonance_and_settles_at_high() {
        let saturated = (filter::SATURATION * 1_000_000 / UNITS_PER_VOLT) as i32;
        for (clock_hz, last_saturating) in [(50_000, 9), (60_000, 4), (70_000, 0)] {
            for res in 0..16u8 {
                let level = level_after_a_kick(clock_hz, res);
                if res <= last_saturating {
                    assert_eq!(level, saturated, "{clock_hz} Hz, res {res}");
                } else {
                    assert!(level < 1_000, "{clock_hz} Hz, res {res}: {level}");
                }
            }
        }
    }
}
