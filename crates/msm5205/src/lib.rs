//! The OKI MSM5205 ADPCM speech synthesis LSI, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **One [`Msm5205::tick`] is one oscillator cycle** at `XT` (`MSM5205-004`).
//! The consumer calls it at 384 kHz or whatever it clocks the part at, shows
//! the part its input pins on every call and reads the outputs back. Nothing
//! here knows a wall clock.
//!
//! **`VCK` is the oscillator over 96, 64 or 48** by `S1` and `S2`
//! (`MSM5205-005`), High for the first half of the period and Low for the
//! second — the halves are the crate's definition, `MSM5205-OPEN-04`. Every
//! internal event is timed from its fall (`MSM5205-006`): `RESET` is sampled
//! there (`MSM5205-010`); the code on `D3`–`D0` is captured
//! [`CAPTURE_DELAY_CYCLES`] cycles after it (`MSM5205-008`); and the word the
//! previous period's code made reaches the converter [`OUTPUT_DELAY_CYCLES`]
//! cycles after it (`MSM5205-009`), so the output is one period behind the
//! input.
//!
//! **The synthesis stage** is Oki's `(Ln + ½)Δn` on a 12-bit word
//! (`MSM5205-012`) with the width walked through a table of 49 widths,
//! `⌊16 × 1.1^k⌋`, by an adjustment for each code (`MSM5205-013`,
//! `MSM5205-014`). The table and the adjustments are Dialogic's. The word
//! wraps (`MSM5205-017`): the part has no overflow protection and the sheet
//! says what happens instead.
//!
//! **The output** is the word's upper ten bits as a converter code
//! (`MSM5205-018`), 512 at mid-scale, which a consumer scales by its supply
//! through [`Msm5205::daout_uv`] and loads through
//! [`DAOUT_IMPEDANCE_TYP_OHMS`] (`MSM5205-019`).
//!
//! **What the crate defines**, each an open item: the width table as
//! `⌊16 × 1.1^k⌋` (`MSM5205-OPEN-01`); term-by-term truncation of the
//! fractions (`MSM5205-OPEN-02`); the 3-bit width adaptation
//! (`MSM5205-OPEN-03`); `VCK`'s halves and phase (`MSM5205-OPEN-04`); the
//! prohibited setting holding `VCK` Low (`MSM5205-OPEN-05`); a select change
//! mid-stream continuing the counter (`MSM5205-OPEN-06`); the two delays as
//! cycle counts (`MSM5205-OPEN-07`); the reset asserting at the first fall
//! after `RESET` rises (`MSM5205-OPEN-08`); and mid-scale on the output
//! through a reset and from power-up (`MSM5205-OPEN-09`).
//!
//! # Examples
//!
//! Three 4-bit codes at 8 kHz from a 384 kHz oscillator: the largest rising
//! code lifts the output above mid-scale a period after it is taken.
//!
//! ```
//! use msm5205::{Inputs, Level, Msm5205};
//! use units::{Microvolts, Audio};
//!
//! let mut part = Msm5205::new();
//! let pins = Inputs {
//!     d: 0x7,
//!     s1: Level::High,
//!     four_b_three_b: Level::High,
//!     ..Inputs::default()
//! };
//! assert_eq!(part.sample(pins), Some(Audio(0)), "the first code is not out yet");
//! let level = part.sample(pins).ok_or("a prohibited rate")?;
//! assert!(level > Audio(0));
//! assert!(part.daout_uv(Microvolts(5_000_000)) > Microvolts(2_500_000));
//! # Ok::<(), &str>(())
//! ```

#![no_std]

mod adpcm;

use adpcm::{LAST_STEP_INDEX, STEP_ADJUST, STEP_TABLE};
use core::num::NonZeroUsize;

use units::{Audio, Hertz, Microvolts, Ohms};

/// How many pins the package presents (`MSM5205-002`).
pub const PIN_COUNT: usize = 18;

/// The width of the PCM word the synthesis stage accumulates (`MSM5205-001`).
pub const PCM_BIT_COUNT: usize = 12;

/// The width of the converter (`MSM5205-001`, `MSM5205-018`).
pub const DAC_BIT_COUNT: usize = 10;

/// The PCM word at mid-scale — what a reset leaves and what `½VDD` is
/// (`MSM5205-011`, `MSM5205-018`).
pub const PCM_MID: u16 = 1 << (PCM_BIT_COUNT - 1);

/// The converter code at mid-scale, `½VDD`, where a reset leaves it
/// (`MSM5205-011`, `MSM5205-018`).
pub const DAC_MID: u16 = 1 << (DAC_BIT_COUNT - 1);

/// A pin's level, as a getter named for the pin returns it (`vck()`).
pub use signal::Level;

/// The largest magnitude [`Msm5205::audio`] returns: the converter's
/// mid-scale code.
pub const AUDIO_FULL_SCALE: i32 = DAC_MID as i32;

/// The lowest and highest oscillator frequency the sheet allows, in hertz,
/// as `(min, max)` (`MSM5205-004`).
pub const OSC_RANGE_HZ: (Hertz, Hertz) = (Hertz(384_000), Hertz(768_000));

/// Oscillator cycles from `VCK`'s fall to the capture of `D3`–`D0`: the
/// sheet's 15.6 µs at 384 kHz (`MSM5205-008`, `MSM5205-OPEN-07`).
pub const CAPTURE_DELAY_CYCLES: usize = 6;

/// Oscillator cycles from `VCK`'s fall to the output taking the previous
/// period's word: the sheet's 5.2 µs at 384 kHz (`MSM5205-009`,
/// `MSM5205-OPEN-07`).
pub const OUTPUT_DELAY_CYCLES: usize = 2;

/// `DAOUT`'s output impedance, typical, in ohms (`MSM5205-019`).
pub const DAOUT_IMPEDANCE_TYP_OHMS: Ohms = Ohms(100_000);

/// The sampling-rate setting on `S1` and `S2` (`MSM5205-005`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Rate {
    /// `S1` Low, `S2` Low: the oscillator over 96 — 4 kHz from 384 kHz.
    Div96,
    /// `S1` Low, `S2` High: over 64 — 6 kHz from 384 kHz.
    Div64,
    /// `S1` High, `S2` Low: over 48 — 8 kHz from 384 kHz.
    Div48,
    /// `S1` High, `S2` High: prohibited by the sheet (`MSM5205-OPEN-05`).
    Prohibited,
}

impl Rate {
    /// The setting the two select pins name.
    #[must_use]
    const fn from_pins(s1: Level, s2: Level) -> Self {
        match (s1, s2) {
            (Level::Low, Level::Low) => Self::Div96,
            (Level::Low, Level::High) => Self::Div64,
            (Level::High, Level::Low) => Self::Div48,
            (Level::High, Level::High) => Self::Prohibited,
        }
    }

    /// Oscillator cycles per `VCK` period, or `None` for the prohibited
    /// setting.
    #[must_use]
    const fn cycles_per_sample(self) -> Option<NonZeroUsize> {
        match self {
            Self::Div96 => NonZeroUsize::new(96),
            Self::Div64 => NonZeroUsize::new(64),
            Self::Div48 => NonZeroUsize::new(48),
            Self::Prohibited => None,
        }
    }
}

/// The input pins, as the part sees them on one oscillator cycle.
///
/// `Default` is every pin Low: `RESET` released, the rate over 96, and
/// `4B/3B` Low, which selects 3-bit codes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `D3`–`D0` in bits 3 to 0; a 3-bit code is `D3`–`D1` and bit 0 is
    /// ignored (`MSM5205-007`).
    pub d: u8,
    /// `RESET`, active High (`MSM5205-010`).
    pub reset: Level,
    /// `S1`, pin 1.
    pub s1: Level,
    /// `S2`, pin 2.
    pub s2: Level,
    /// `4B/3B`: High selects 4-bit codes, Low 3-bit (`MSM5205-007`).
    pub four_b_three_b: Level,
}

impl Inputs {
    /// The part idle; see [`Inputs`].
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            d: 0,
            reset: Level::Low,
            s1: Level::Low,
            s2: Level::Low,
            four_b_three_b: Level::Low,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

impl Inputs {
    /// The rate the selects name.
    #[must_use]
    const fn rate(self) -> Rate {
        Rate::from_pins(self.s1, self.s2)
    }
}

/// The part.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Msm5205 {
    /// Position within the `VCK` period, 0 at the start of the High half.
    counter: usize,
    /// The rate `S1` and `S2` named on the last tick.
    rate: Rate,
    /// `VCK` as last made.
    vck: bool,
    /// The internal reset, sampled from `RESET` at each fall.
    in_reset: bool,
    /// Whether a capture may take place this period: false in the period
    /// the reset releases in (`MSM5205-010`).
    armed: bool,
    /// The 12-bit PCM word, unsigned, mid-scale at [`PCM_MID`].
    word: u16,
    /// The adaptation index into `STEP_TABLE`.
    index: u8,
    /// The word made by this period's capture, waiting for the next
    /// period's output instant (`MSM5205-009`).
    pending: Option<u16>,
    /// The word the converter is presenting.
    output: u16,
}

impl Default for Msm5205 {
    fn default() -> Self {
        Self::new()
    }
}

impl Msm5205 {
    /// A part in its reset state: smallest width, mid-scale, `VCK` at the
    /// start of a High half (`MSM5205-011`, `MSM5205-OPEN-04`,
    /// `MSM5205-OPEN-09`). The sheet fixes no level at power-up, for `VCK` or
    /// the stage, so this crate starts the part in the state a reset leaves; a
    /// board that needs a known start holds `RESET` High over a fall of `VCK`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            counter: 0,
            rate: Rate::Div96,
            vck: true,
            in_reset: false,
            armed: true,
            word: PCM_MID,
            index: 0,
            pending: None,
            output: PCM_MID,
        }
    }

    /// One oscillator cycle with the pins as given.
    pub fn tick(&mut self, pins: Inputs) {
        self.rate = pins.rate();
        let Some(period) = self.rate.cycles_per_sample() else {
            self.vck = false;
            return;
        };
        let period = period.get();
        let half = period / 2;
        if self.counter >= period {
            self.counter = 0;
        }
        self.vck = self.counter < half;
        if self.counter == half {
            let was_reset = self.in_reset;
            self.in_reset = pins.reset.is_high();
            self.armed = !was_reset && !self.in_reset;
            if self.in_reset {
                self.word = PCM_MID;
                self.index = 0;
                self.pending = None;
                self.output = PCM_MID;
            }
        }
        if self.counter == half + OUTPUT_DELAY_CYCLES
            && let Some(word) = self.pending.take()
        {
            self.output = word;
        }
        if self.counter == half + CAPTURE_DELAY_CYCLES && self.armed {
            self.synthesise(pins.d, pins.four_b_three_b.is_high());
            self.pending = Some(self.word);
        }
        self.counter += 1;
    }

    /// The synthesis stage on one code: `MSM5205-012` to `MSM5205-017`.
    fn synthesise(&mut self, data: u8, four_bit: bool) {
        let step = u32::from(STEP_TABLE[usize::from(self.index)]);
        let negative = data & 0b1000 != 0;
        let (magnitude, delta) = if four_bit {
            let magnitude = data & 0b0111;
            let mut delta = step / 8;
            if magnitude & 0b100 != 0 {
                delta += step;
            }
            if magnitude & 0b010 != 0 {
                delta += step / 2;
            }
            if magnitude & 0b001 != 0 {
                delta += step / 4;
            }
            (magnitude, delta)
        } else {
            let magnitude = data & 0b0110;
            let mut delta = step / 4;
            if magnitude & 0b100 != 0 {
                delta += step;
            }
            if magnitude & 0b010 != 0 {
                delta += step / 2;
            }
            (magnitude, delta)
        };
        let mask = (1_u32 << PCM_BIT_COUNT) - 1;
        let word = u32::from(self.word);
        self.word = if negative {
            (word.wrapping_sub(delta) & mask) as u16
        } else {
            ((word + delta) & mask) as u16
        };
        let index = i16::from(self.index) + i16::from(STEP_ADJUST[usize::from(magnitude)]);
        self.index = index.clamp(0, i16::from(LAST_STEP_INDEX)) as u8;
    }

    /// Oscillator cycles in one `VCK` period at the rate `S1` and `S2` named
    /// on the last tick — 96, 64 or 48, and `None` for the prohibited
    /// setting, under which `VCK` holds Low and there is no period
    /// (`MSM5205-005`, `MSM5205-OPEN-05`). Before the first tick, 96, the rate
    /// of [`Inputs::default`].
    #[must_use]
    pub const fn cycles_per_sample(&self) -> Option<NonZeroUsize> {
        self.rate.cycles_per_sample()
    }

    /// `VCK`, pin 14 (`MSM5205-006`).
    #[must_use]
    pub const fn vck(&self) -> Level {
        Level::from_bool(self.vck)
    }

    /// The converter's code: the presented word's upper ten bits, 0 to 1023,
    /// 512 at mid-scale (`MSM5205-018`).
    #[must_use]
    pub const fn dac(&self) -> u16 {
        self.output >> (PCM_BIT_COUNT - DAC_BIT_COUNT)
    }

    /// The presented word as a signed level about mid-scale, −2048 to 2047
    /// (`MSM5205-018`); the two low bits do not reach the converter.
    #[must_use]
    pub const fn pcm(&self) -> i32 {
        self.output as i32 - PCM_MID as i32
    }

    /// `DAOUT` as a signed sample at rest 0: the converter's code less
    /// [`DAC_MID`], in converter steps, −[`AUDIO_FULL_SCALE`] to 511
    /// (`MSM5205-018`).
    #[must_use]
    pub const fn audio(&self) -> Audio<AUDIO_FULL_SCALE> {
        Audio(self.dac() as i32 - AUDIO_FULL_SCALE)
    }

    /// `DAOUT` unloaded for a supply of `vdd_uv`: `VDD × code / 1024`
    /// (`MSM5205-018`).
    #[must_use]
    pub const fn daout_uv(&self, vdd_uv: Microvolts) -> Microvolts {
        Microvolts(((vdd_uv.0 as i64 * self.dac() as i64) >> DAC_BIT_COUNT) as i32)
    }

    /// The synthesis stage's word as it stands after the last capture,
    /// before the output instant, −2048 to 2047.
    #[must_use]
    pub const fn stage_pcm(&self) -> i32 {
        self.word as i32 - PCM_MID as i32
    }

    /// The adaptation index the next code will use, 0 to 48 (`MSM5205-014`).
    #[must_use]
    pub const fn step_index(&self) -> usize {
        self.index as usize
    }

    /// Whether the internal reset stands (`MSM5205-010`).
    #[must_use]
    pub const fn is_in_reset(&self) -> bool {
        self.in_reset
    }

    /// Run the part with the pins held to the end of the present `VCK`
    /// period — a whole period from its start — and return
    /// [`Msm5205::audio`], for a caller feeding one code a sample. The code
    /// is captured in this period and reaches the output in the next
    /// (`MSM5205-009`). Under the prohibited setting nothing runs and the
    /// answer is `None` (`MSM5205-OPEN-05`).
    #[must_use]
    pub fn sample(&mut self, pins: Inputs) -> Option<Audio<AUDIO_FULL_SCALE>> {
        let period = pins.rate().cycles_per_sample()?.get();
        let ticks = if self.counter == 0 || self.counter >= period {
            period
        } else {
            period - self.counter
        };
        for _ in 0..ticks {
            self.tick(pins);
        }
        Some(self.audio())
    }
}
