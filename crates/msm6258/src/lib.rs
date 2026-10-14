//! The OKI MSM6258V ADPCM speech processor, the MPU interface version, at its
//! pins, playing back.
//!
//! Every item below names the claim it implements.
//!
//! **One [`Msm6258::tick`] is one oscillator cycle** at `XT` (`MSM6258-006`).
//! The consumer shows the part its strapped and driven pins on every call,
//! makes each bus access as one [`Msm6258::write`] or [`Msm6258::read`]
//! between ticks, and reads the outputs back. Nothing here knows a wall clock.
//!
//! **`VCK` is the oscillator over 1024, 768 or 512** by `SAM1` and `SAM2`
//! (`MSM6258-005`), High for the first half of the period (`MSM6258-007`).
//! Every event is timed from its rise. A `PLAY ST` command is taken at the
//! next rise (`MSM6258-014`), which puts the word at mid-scale
//! (`MSM6258-016`); from then on `MCK` rises [`TVM_CYCLES`] after every
//! other rise and stays High [`TMH_CYCLES`] (`MSM6258-008`), the byte
//! last written on the data port is taken at its fall, and the two rises that
//! follow play its `D3`–`D0` nibble and then its `D7`–`D4` nibble
//! (`MSM6258-012`). Where exactly in that chain the part takes the byte and
//! presents each word is the crate's reading of a drawing
//! (`MSM6258-OPEN-05`, `MSM6258-OPEN-07`).
//!
//! **A byte not replaced by the next take is played again** — the crate's
//! answer to a host that runs dry, and nothing the sheet says
//! (`MSM6258-OPEN-06`).
//!
//! **The synthesis stage** is Oki's `(Ln + ½)Δn` on a 12-bit word
//! (`MSM6258-017`) with `msm5205`'s widths and adaptation
//! (`MSM6258-OPEN-01`), and the word holds at its ends rather than wrapping
//! (`MSM6258-018`, `MSM6258-OPEN-03`); a start puts the width at its
//! smallest (`MSM6258-OPEN-04`).
//!
//! **The output** is one converter code, 0 to 1023, 512 at `½VDD`
//! (`MSM6258-019`, `MSM6258-OPEN-02`): 0 while `AC` is High, a ramp to 512
//! once it falls (`MSM6258-015`), and the word's upper ten bits from then on
//! — held through a `SP` command (`MSM6258-016`). `OVF` goes High at 80 % of
//! half the range (`MSM6258-OPEN-15`).
//!
//! **What the crate also chooses**, each an open item: the printed times as
//! the cycle counts at 4.096 MHz (`MSM6258-OPEN-08`); the pointers stored and
//! read back and nothing more (`MSM6258-OPEN-11`); both selects High holding
//! `VCK` Low, and a select change continuing the counter (`MSM6258-OPEN-12`);
//! `VCK` and `MCK` Low and writes ignored while `AC` is High, with a start
//! before `tAS` ignored (`MSM6258-OPEN-14`); and `DAOUT` spanning 0 V to
//! `VDD` across the codes (`MSM6258-OPEN-16`).
//!
//! **Not run**: recording. `REC ST` is ignored and an ADPCM data read returns
//! 0 (`MSM6258-OPEN-09`, `MSM6258-OPEN-10`); the analysis stage takes an analogue
//! input and this crate covers playback only.
//!
//! # Examples
//!
//! A byte of two rising codes, played at 8 kHz from a 4.096 MHz clock: the
//! word climbs from mid-scale.
//!
//! ```
//! use msm6258::{Inputs, Level, Msm6258, command};
//! use units::Microvolts;
//!
//! let mut part = Msm6258::new();
//! let pins = Inputs {
//!     sam2: Level::High,
//!     four_b_three_b: Level::High,
//!     ..Inputs::default()
//! };
//! part.write(Level::High, 0x77);
//! part.write(Level::Low, command::PLAY);
//! for _ in 0..4 * 512 {
//!     part.tick(pins);
//! }
//! assert!(part.playm().is_high());
//! assert!(part.pcm() > 0);
//! assert!(part.daout_uv(Microvolts(5_000_000)) > Microvolts(2_500_000));
//! ```

#![no_std]

mod adpcm;

use adpcm::{LAST_STEP_INDEX, STEP_ADJUST, STEP_TABLE};
use core::num::NonZeroUsize;

use units::{Audio, Hertz, Microvolts, Ohms};

/// How many pins the 40-pin DIP presents (`MSM6258-002`).
pub const DIP_PIN_COUNT: usize = 40;

/// How many pins the 44-pin flat package presents (`MSM6258-003`).
pub const QFP_PIN_COUNT: usize = 44;

/// The width of the PCM word the synthesis stage accumulates (`MSM6258-017`).
pub const PCM_BIT_COUNT: usize = 12;

/// The width of the internal converter (`MSM6258-019`).
pub const DAC_BIT_COUNT: usize = 10;

/// The PCM word at mid-scale, `½VDD` (`MSM6258-016`).
pub const PCM_MID: u16 = 1 << (PCM_BIT_COUNT - 1);

/// The converter code at `½VDD`.
pub const DAC_MID: u16 = 1 << (DAC_BIT_COUNT - 1);

/// A pin's level, as a getter named for the pin returns it (`vck()`).
pub use signal::Level;

/// The largest magnitude [`Msm6258::audio`] returns: the converter's
/// mid-scale code.
pub const AUDIO_FULL_SCALE: i32 = DAC_MID as i32;

/// The lowest and highest master clock the sheet allows, in hertz, as
/// `(min, max)` (`MSM6258-006`).
pub const OSC_RANGE_HZ: (Hertz, Hertz) = (Hertz(4_000_000), Hertz(8_000_000));

/// Oscillator cycles from a `VCK` rise to `MCK`'s rise: the sheet's `tVM`,
/// 19.5 µs at 4.096 MHz and proportional to the clock (`MSM6258-008`,
/// `MSM6258-OPEN-08`).
pub const TVM_CYCLES: usize = 80;

/// Oscillator cycles `MCK` stays High: the sheet's `tMH`, 39.1 µs at
/// 4.096 MHz (`MSM6258-008`, `MSM6258-OPEN-08`).
pub const TMH_CYCLES: usize = 160;

/// Oscillator cycles from `AC` falling to `DAOUT` reaching `½VDD`: the
/// sheet's `tDAS`, 15.6 ms at 4.096 MHz (`MSM6258-015`, `MSM6258-OPEN-08`).
pub const TDAS_CYCLES: usize = 64_000;

/// Oscillator cycles from `AC` falling before a start is taken: the sheet's
/// `tAS`, 16 ms at 4.096 MHz (`MSM6258-015`, `MSM6258-OPEN-08`).
pub const TAS_CYCLES: usize = 65_536;

/// How far the word may stand from mid-scale before `OVF` goes High, in
/// steps of the 12-bit word's least significant bit: 80 % of half the range,
/// truncated (`MSM6258-020`, `MSM6258-OPEN-15`).
pub const OVF_THRESHOLD_STEPS: u32 = 1638;

/// `DAOUT`'s output impedance, in ohms, as `(min, max)` (`MSM6258-019`).
pub const DAOUT_IMPEDANCE_RANGE_OHMS: (Ohms, Ohms) = (Ohms(12_000), Ohms(22_000));

/// `DAOUT`'s relative error unloaded, at most, in microvolts (`MSM6258-019`).
pub const DAOUT_ACCURACY_MAX_UV: Microvolts = Microvolts(40_000);

/// The bits of the command register, written with `D/C` Low (`MSM6258-010`).
pub mod command {
    /// `D0`, `SP`: stops playback.
    pub const STOP: u8 = 0x01;
    /// `D1`, `PLAY ST`: starts playback.
    pub const PLAY: u8 = 0x02;
    /// `D2`, `REC ST`: starts recording, which the crate does not run
    /// (`MSM6258-OPEN-09`).
    pub const RECORD: u8 = 0x04;
}

/// The bits of the status register, read with `D/C` Low (`MSM6258-011`,
/// `MSM6258-OPEN-10`).
pub mod status {
    /// `D6`, `VOICE`.
    pub const VOICE: u8 = 0x40;
    /// `D7`, `REC/PLAY`.
    pub const RECORD: u8 = 0x80;
}

/// The sampling-rate setting on `SAM1` and `SAM2` (`MSM6258-005`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Rate {
    /// `SAM1` Low, `SAM2` Low: the clock over 1024 — 4.0 kHz at 4.096 MHz.
    Div1024,
    /// `SAM1` High, `SAM2` Low: over 768 — 5.3 kHz at 4.096 MHz.
    Div768,
    /// `SAM1` Low, `SAM2` High: over 512 — 8.0 kHz at 4.096 MHz.
    Div512,
    /// `SAM1` High, `SAM2` High: the sheet calls it invalid
    /// (`MSM6258-OPEN-12`).
    Prohibited,
}

impl Rate {
    /// The setting the two select pins name.
    #[must_use]
    const fn from_pins(sam1: Level, sam2: Level) -> Self {
        match (sam1, sam2) {
            (Level::Low, Level::Low) => Self::Div1024,
            (Level::High, Level::Low) => Self::Div768,
            (Level::Low, Level::High) => Self::Div512,
            (Level::High, Level::High) => Self::Prohibited,
        }
    }

    /// Oscillator cycles per `VCK` period, or `None` for the prohibited
    /// setting.
    #[must_use]
    const fn cycles_per_sample(self) -> Option<NonZeroUsize> {
        match self {
            Self::Div1024 => NonZeroUsize::new(1024),
            Self::Div768 => NonZeroUsize::new(768),
            Self::Div512 => NonZeroUsize::new(512),
            Self::Prohibited => None,
        }
    }
}

/// The input pins that are not the bus, as the part sees them on one
/// oscillator cycle.
///
/// `Default` is every pin Low: `AC` released, the rate over 1024, and
/// `4B/3B` Low, which selects 3-bit codes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `AC`, active High: holds the whole part in reset (`MSM6258-015`).
    pub ac: Level,
    /// `SAM1`.
    pub sam1: Level,
    /// `SAM2`.
    pub sam2: Level,
    /// `4B/3B`: High selects 4-bit codes, Low 3-bit (`MSM6258-013`).
    pub four_b_three_b: Level,
}

impl Inputs {
    /// The part idle; see [`Inputs`].
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            ac: Level::Low,
            sam1: Level::Low,
            sam2: Level::Low,
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
        Rate::from_pins(self.sam1, self.sam2)
    }
}

/// The part.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Msm6258 {
    /// Position within the `VCK` period, 0 at the rise.
    counter: usize,
    /// The rate `SAM1` and `SAM2` named on the last tick.
    rate: Rate,
    /// `VCK` as last made.
    vck: bool,
    /// `AC` as last seen.
    in_reset: bool,
    /// Cycles since `AC` fell, held at [`TAS_CYCLES`].
    since_ac: usize,
    /// A `PLAY ST` written and waiting for the next rise.
    start_pending: bool,
    /// Playback running.
    playing: bool,
    /// Rises since the start was taken: 0 at the start, then 1, 2, 3, and
    /// 2 and 3 alternately from there. Odd rises schedule `MCK`; 2 plays a
    /// byte's first nibble and 3 its second.
    rise: u8,
    /// An `MCK` pulse scheduled in this period.
    mck_due: bool,
    /// `MCK` as last made.
    mck: bool,
    /// The byte last written on the data port.
    latch: u8,
    /// The byte taken at `MCK`'s last fall, being played.
    played: u8,
    /// The 12-bit PCM word, unsigned, mid-scale at [`PCM_MID`].
    word: u16,
    /// The adaptation index into `STEP_TABLE`.
    index: u8,
    /// `P4`–`P0` as the last command wrote them, `D3` in bit 0.
    pointers: u8,
}

impl Default for Msm6258 {
    fn default() -> Self {
        Self::new()
    }
}

impl Msm6258 {
    /// A part whose `AC` fell long ago: idle, `DAOUT` at `½VDD`, `VCK` about
    /// to rise, the data latch clear (`MSM6258-OPEN-06`, `MSM6258-OPEN-14`).
    /// The sheet fixes no level at power-up, so this crate starts the part in
    /// the state a reset on `AC` leaves once `TAS_CYCLES` have passed; a
    /// board that needs a known start holds `AC` High.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            counter: 0,
            rate: Rate::Div1024,
            vck: false,
            in_reset: false,
            since_ac: TAS_CYCLES,
            start_pending: false,
            playing: false,
            rise: 0,
            mck_due: false,
            mck: false,
            latch: 0,
            played: 0,
            word: PCM_MID,
            index: 0,
            pointers: 0,
        }
    }

    /// One oscillator cycle with the pins as given.
    pub fn tick(&mut self, pins: Inputs) {
        if pins.ac.is_high() {
            *self = Self {
                rate: pins.rate(),
                in_reset: true,
                since_ac: 0,
                ..Self::new()
            };
            return;
        }
        if self.in_reset {
            self.in_reset = false;
        }
        if self.since_ac < TAS_CYCLES {
            self.since_ac += 1;
        }
        self.rate = pins.rate();
        let Some(period) = self.rate.cycles_per_sample() else {
            self.vck = false;
            return;
        };
        let period = period.get();
        if self.counter >= period {
            self.counter = 0;
        }
        if self.counter == 0 {
            self.vck_rise(pins.four_b_three_b.is_high());
        }
        self.vck = self.counter < period / 2;
        if self.mck_due {
            if self.counter == TVM_CYCLES {
                self.mck = true;
            } else if self.counter == TVM_CYCLES + TMH_CYCLES {
                self.mck = false;
                self.mck_due = false;
                self.played = self.latch;
            }
        }
        self.counter += 1;
    }

    /// A rise of `VCK`: the playback sequence (`MSM6258-014`,
    /// `MSM6258-OPEN-07`).
    fn vck_rise(&mut self, four_b_three_b: bool) {
        if self.playing {
            self.rise = if self.rise >= 3 { 2 } else { self.rise + 1 };
            match self.rise {
                2 => self.synthesise(self.played & 0x0F, four_b_three_b),
                3 => self.synthesise(self.played >> 4, four_b_three_b),
                _ => {}
            }
            if self.rise % 2 == 1 {
                self.mck_due = true;
            }
        } else if self.start_pending {
            self.start_pending = false;
            self.playing = true;
            self.rise = 0;
            self.word = PCM_MID;
            self.index = 0;
        }
    }

    /// The synthesis stage on one nibble: `MSM6258-017`, `MSM6258-018`.
    fn synthesise(&mut self, nibble: u8, four_bit: bool) {
        let step = u32::from(STEP_TABLE[usize::from(self.index)]);
        let negative = nibble & 0b1000 != 0;
        let magnitude = if four_bit {
            nibble & 0b0111
        } else {
            nibble & 0b0110
        };
        let mut delta = if four_bit { step / 8 } else { step / 4 };
        if magnitude & 0b100 != 0 {
            delta += step;
        }
        if magnitude & 0b010 != 0 {
            delta += step / 2;
        }
        if magnitude & 0b001 != 0 {
            delta += step / 4;
        }
        let word = i32::from(self.word);
        let delta = delta as i32;
        let next = if negative { word - delta } else { word + delta };
        self.word = next.clamp(0, (1 << PCM_BIT_COUNT) - 1) as u16;
        let index = i16::from(self.index) + i16::from(STEP_ADJUST[usize::from(magnitude)]);
        self.index = index.clamp(0, i16::from(LAST_STEP_INDEX)) as u8;
    }

    /// A write of `d` with `CS` Low, `RD` High and `WR` rising, `D/C` at
    /// `dc` (`MSM6258-009`): High writes an ADPCM byte to the data latch,
    /// Low a command, which acts at once (`MSM6258-010`, `MSM6258-014`).
    /// Ignored while `AC` is High (`MSM6258-OPEN-14`).
    pub fn write(&mut self, dc: Level, d: u8) {
        if self.in_reset {
            return;
        }
        if dc.is_high() {
            self.latch = d;
            return;
        }
        self.pointers = d >> 3;
        if d & command::STOP != 0 {
            self.start_pending = false;
            self.playing = false;
            self.mck_due = false;
            self.mck = false;
        } else if d & command::PLAY != 0 && !self.playing && self.since_ac >= TAS_CYCLES {
            self.start_pending = true;
        }
    }

    /// A read with `CS` Low and `RD` Low, `D/C` at `dc` (`MSM6258-009`):
    /// High reads ADPCM data, which is recording's and 0 here; Low reads
    /// status, `P0`–`P4` on `D0`–`D4` as the last command wrote them, and
    /// `VOICE` and `REC/PLAY` Low (`MSM6258-011`, `MSM6258-OPEN-10`,
    /// `MSM6258-OPEN-11`).
    #[must_use]
    pub const fn read(&self, dc: Level) -> u8 {
        if dc.is_high() {
            return 0;
        }
        let p = self.pointers;
        ((p >> 4) & 1)
            | (((p >> 3) & 1) << 1)
            | (((p >> 2) & 1) << 2)
            | (((p >> 1) & 1) << 3)
            | ((p & 1) << 4)
    }

    /// Oscillator cycles in one `VCK` period at the rate `SAM1` and `SAM2`
    /// named on the last tick — 1024, 768 or 512, and `None` for the setting
    /// the sheet calls invalid, under which `VCK` holds Low and there is no
    /// period (`MSM6258-005`, `MSM6258-OPEN-12`). Before the first tick, 1024,
    /// the rate of [`Inputs::default`].
    #[must_use]
    pub const fn cycles_per_sample(&self) -> Option<NonZeroUsize> {
        self.rate.cycles_per_sample()
    }

    /// `VCK` (`MSM6258-007`).
    #[must_use]
    pub const fn vck(&self) -> Level {
        Level::from_bool(self.vck)
    }

    /// `MCK` (`MSM6258-008`): a host feeding one byte per pulse writes it
    /// while this is High.
    #[must_use]
    pub const fn mck(&self) -> Level {
        Level::from_bool(self.mck)
    }

    /// `PLAYM`: High from a `PLAY ST` command until a `SP`
    /// (`MSM6258-021`).
    #[must_use]
    pub const fn playm(&self) -> Level {
        Level::from_bool(self.playing || self.start_pending)
    }

    /// `OVF`: High while playing with the word more than [`OVF_THRESHOLD_STEPS`]
    /// from mid-scale (`MSM6258-020`, `MSM6258-OPEN-15`).
    #[must_use]
    pub const fn ovf(&self) -> Level {
        Level::from_bool(self.playing && self.word.abs_diff(PCM_MID) as u32 > OVF_THRESHOLD_STEPS)
    }

    /// The converter's code, 0 to 1023, 512 at `½VDD`: 0 while `AC` is High,
    /// a ramp to 512 over [`TDAS_CYCLES`] once it falls, then the word's
    /// upper ten bits (`MSM6258-015`, `MSM6258-019`, `MSM6258-OPEN-02`).
    #[must_use]
    pub const fn dac(&self) -> u16 {
        if self.in_reset {
            0
        } else if self.since_ac < TDAS_CYCLES {
            (DAC_MID as usize * self.since_ac / TDAS_CYCLES) as u16
        } else {
            self.word >> (PCM_BIT_COUNT - DAC_BIT_COUNT)
        }
    }

    /// The word as a signed level about mid-scale, −2048 to 2047, which the
    /// part shifts out at `DASO` for an external converter (`MSM6258-019`).
    #[must_use]
    pub const fn pcm(&self) -> i32 {
        self.word as i32 - PCM_MID as i32
    }

    /// `DAOUT` as a signed sample at rest 0: [`Msm6258::dac`] less
    /// [`DAC_MID`], in converter steps, −[`AUDIO_FULL_SCALE`] to 511 — so the
    /// drop to 0 while `AC` is High and the ramp after it are heard
    /// (`MSM6258-015`, `MSM6258-019`).
    #[must_use]
    pub const fn audio(&self) -> Audio<AUDIO_FULL_SCALE> {
        Audio(self.dac() as i32 - AUDIO_FULL_SCALE)
    }

    /// `DAOUT` unloaded for a supply of `vdd_uv`: `VDD × code / 1024`
    /// (`MSM6258-OPEN-16`).
    #[must_use]
    pub const fn daout_uv(&self, vdd_uv: Microvolts) -> Microvolts {
        Microvolts(((vdd_uv.0 as i64 * self.dac() as i64) >> DAC_BIT_COUNT) as i32)
    }

    /// The adaptation index the next nibble will use, 0 to 48.
    #[must_use]
    pub const fn step_index(&self) -> usize {
        self.index as usize
    }

    /// Whether `AC` held the part on the last tick.
    #[must_use]
    pub const fn is_in_reset(&self) -> bool {
        self.in_reset
    }

    /// The byte being played, as `MCK`'s last fall took it.
    #[must_use]
    pub const fn played_byte(&self) -> u8 {
        self.played
    }
}
