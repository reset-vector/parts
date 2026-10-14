//! The OKI MSM6295 four-channel ADPCM voice synthesis LSI, at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **One [`Msm6295::tick`] is one oscillator cycle** at `XT` (`MSM6295-005`).
//! A sample point comes every 33 × n cycles, n = 4 with `SS` High and 5 with it
//! Low (`MSM6295-004`); every channel that plays takes one nibble there.
//! Nothing here knows a wall clock.
//!
//! **The command bus** is [`Msm6295::write`], one call per byte latched on
//! `WR`'s rise with `CS` Low (`MSM6295-007`), and [`Msm6295::read`], the busy
//! nibble a read drives on `I0`–`I3` (`MSM6295-013`). A byte with `I7` set
//! names a phrase and the next byte is its channel and attenuation
//! (`MSM6295-008`); a byte with `I7` clear in its place stops the channels it
//! names at the next sample point (`MSM6295-011`).
//!
//! **The ROM is the consumer's**, read through [`Rom`] at 18-bit addresses:
//! the phrase's eight bytes of address data at 8n (`MSM6295-009`) and then one
//! byte for every two samples, high nibble first, through the low nibble of the
//! stop address's byte (`MSM6295-010`).
//!
//! **Each channel** is Oki's `(Ln + ½)Δn` on a 12-bit word (`MSM6295-015`),
//! scaled by its attenuation (`MSM6295-012`); the four are summed onto one
//! 12-bit converter code, mid-scale when nothing sounds (`MSM6295-016`).
//!
//! **What the crate defines**, each an open item: the widths and their
//! adaptation, `msm5205`'s (`MSM6295-OPEN-01`); a phrase starting at the
//! smallest width and mid-scale (`MSM6295-OPEN-02`); a channel's word clamped
//! at its ends (`MSM6295-OPEN-03`); the attenuation as ratios over 32
//! (`MSM6295-OPEN-04`), codes 9 to 15 silent (`MSM6295-OPEN-05`); the
//! channels summed and the sum clamped (`MSM6295-OPEN-06`); a start on a
//! playing channel ignored (`MSM6295-OPEN-07`), and one on several channels
//! or none (`MSM6295-OPEN-08`) or of phrase 0 (`MSM6295-OPEN-09`); the sample
//! point moved to a lone start (`MSM6295-OPEN-10`); a stop address below the
//! start playing nothing (`MSM6295-OPEN-11`); what a reset clears
//! (`MSM6295-OPEN-12`); and the address data read when the channel starts
//! (`MSM6295-OPEN-13`).
//!
//! # Examples
//!
//! Phrase 1 started on channel 1 at full level: a lone start sounds 48 × n
//! cycles after its second byte, its first code `0111` at the smallest width.
//!
//! ```
//! use msm6295::{Inputs, Msm6295};
//! use units::Microvolts;
//!
//! let mut rom = [0xFF_u8; 0x440];
//! // Phrase 1's address data at 8: start 0x400, stop 0x43F.
//! rom[8..14].copy_from_slice(&[0x00, 0x04, 0x00, 0x00, 0x04, 0x3F]);
//! rom[0x400..].fill(0x77);
//! let mut part = Msm6295::new();
//! part.write(0x81);
//! part.write(0x10);
//! for _ in 0..48 * 4 {
//!     part.tick(Inputs::default(), &mut rom[..]);
//! }
//! assert_eq!(part.read(), 0b0001, "channel 1 busy");
//! assert_eq!(part.channel_pcm(0), 16 + 8 + 4 + 2);
//! assert!(part.dao_uv(Microvolts(5_000_000)) > Microvolts(2_500_000));
//! ```

#![no_std]

mod adpcm;
mod attenuation;

use adpcm::{LAST_STEP_INDEX, STEP_ADJUST, STEP_TABLE};
use attenuation::{ATTENUATION_GAIN, GAIN_UNITY};
use core::num::NonZeroUsize;

use units::{Audio, Centibels, Hertz, Microvolts, Ohms};

/// A pin's logic level.
pub use signal::Level;

/// How many channels play at once (`MSM6295-001`).
pub const CHANNEL_COUNT: usize = 4;

/// The address outputs `A0`–`A17` (`MSM6295-001`).
pub const ADDRESS_BIT_COUNT: usize = 18;

/// Every address the part can drive.
pub const ADDRESS_MASK: u32 = (1 << ADDRESS_BIT_COUNT) - 1;

/// The highest phrase number (`MSM6295-001`, `MSM6295-008`).
pub const LAST_PHRASE: u8 = 127;

/// The width of the converter (`MSM6295-016`).
pub const DAC_BIT_COUNT: usize = 12;

/// The converter's code at `½VDD` (`MSM6295-016`).
pub const DAC_MID: u16 = 1 << (DAC_BIT_COUNT - 1);

/// The largest magnitude [`Msm6295::audio`] returns: the converter's
/// mid-scale code.
pub const AUDIO_FULL_SCALE: i32 = DAC_MID as i32;

/// The pins of the 44-pin QFP and the 42-pin DIP (`MSM6295-002`,
/// `MSM6295-003`).
pub const QFP_PIN_COUNT: usize = 44;
/// See [`QFP_PIN_COUNT`].
pub const DIP_PIN_COUNT: usize = 42;

/// The oscillator range, in hertz, as `(min, max)` (`MSM6295-005`).
pub const OSC_RANGE_HZ: (Hertz, Hertz) = (Hertz(1_000_000), Hertz(5_000_000));

/// `DAO`'s output impedance, typical, in ohms (`MSM6295-006`).
pub const DAO_IMPEDANCE_TYP_OHMS: Ohms = Ohms(15_000);

/// The converter's largest error unloaded, in microvolts (`MSM6295-006`).
pub const DAO_ACCURACY_MAX_UV: Microvolts = Microvolts(20_000);

/// One sample is this many cycles times n (`MSM6295-004`).
pub const CYCLES_PER_SAMPLE_PER_N: usize = 33;

/// A lone channel's voice is out this many cycles times n after its second
/// byte (`MSM6295-014`).
pub const START_CYCLES_PER_N: usize = 48;

/// A started channel's busy rises this many cycles times n after its second
/// byte (`MSM6295-014`).
pub const BUSY_CYCLES_PER_N: usize = 15;

/// The attenuation of codes 0 to 8: a loss, so the sheet's −3.2 dB is 32 cB
/// (`MSM6295-012`).
pub const ATTENUATION_CB: [Centibels; 9] = [
    Centibels(0),
    Centibels(32),
    Centibels(60),
    Centibels(92),
    Centibels(120),
    Centibels(145),
    Centibels(180),
    Centibels(205),
    Centibels(240),
];

/// A channel's word at its ends, in steps of its least significant bit, as
/// `(min, max)` (`MSM6295-OPEN-03`).
pub const PCM_RANGE_STEPS: (i32, i32) = (-2048, 2047);

/// The sampling-rate select, `SS` (`MSM6295-004`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Rate {
    /// `SS` High: n = 4, 132 cycles a sample — 8 kHz from 1.056 MHz.
    High,
    /// `SS` Low: n = 5, 165 cycles a sample — 6.4 kHz from 1.056 MHz.
    Low,
}

impl Rate {
    /// The setting `SS` at `ss` selects.
    #[must_use]
    const fn from_pin(ss: Level) -> Self {
        match ss {
            Level::High => Self::High,
            Level::Low => Self::Low,
        }
    }

    /// The printed n.
    #[must_use]
    const fn n(self) -> NonZeroUsize {
        match self {
            Self::High => const { NonZeroUsize::new(4).unwrap() },
            Self::Low => const { NonZeroUsize::new(5).unwrap() },
        }
    }

    /// Oscillator cycles in one sample.
    #[must_use]
    const fn cycles_per_sample(self) -> NonZeroUsize {
        self.n()
            .saturating_mul(const { NonZeroUsize::new(CYCLES_PER_SAMPLE_PER_N).unwrap() })
    }
}

/// The pins that are neither the bus nor the ROM, as the part sees them on one
/// oscillator cycle.
///
/// `Default` is `RESET` released and `SS` High, 132 cycles a sample.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Inputs {
    /// `SS`, the sampling-rate select (`MSM6295-004`).
    pub ss: Level,
    /// `RESET`, active Low: held Low it suspends every function
    /// (`MSM6295-007`).
    pub reset_n: Level,
}

impl Inputs {
    /// The rate `SS` selects.
    #[must_use]
    const fn rate(self) -> Rate {
        Rate::from_pin(self.ss)
    }
}

impl Inputs {
    /// The part idle; see [`Inputs`].
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            ss: Level::High,
            reset_n: Level::High,
        }
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

/// The external ROM, read at an 18-bit address on `A0`–`A17` and answering on
/// `D0`–`D7`. The part holds no ROM of its own; banking is the board's.
pub trait Rom {
    /// The byte at `address`, which is always below `1 << 18`.
    #[must_use]
    fn read(&mut self, address: u32) -> u8;
}

impl<F: FnMut(u32) -> u8> Rom for F {
    fn read(&mut self, address: u32) -> u8 {
        self(address)
    }
}

impl Rom for [u8] {
    /// Reads past the end answer `0xFF`, an erased ROM.
    fn read(&mut self, address: u32) -> u8 {
        usize::try_from(address)
            .ok()
            .and_then(|a| self.get(a).copied())
            .unwrap_or(0xFF)
    }
}

/// A start written and not yet sounding.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Pending {
    phrase: u8,
    gain: u8,
    /// Cycles since the second byte; busy rises at 15 × n and a lone start
    /// sounds at 48 × n, n the rate's.
    elapsed: usize,
    /// Sample points passed with another channel playing.
    seen: u8,
}

/// Where a channel stands.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Phase {
    Idle,
    Pending(Pending),
    /// Starting at this sample point, its address data read.
    Armed {
        start: u32,
        stop: u32,
        gain: u8,
    },
    Playing {
        /// The byte the next nibble comes from.
        address: u32,
        stop: u32,
        /// The next nibble is the byte's low one.
        low: bool,
        /// The stop byte's low nibble has been taken.
        finished: bool,
    },
}

/// One of the four channels.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Channel {
    phase: Phase,
    busy: bool,
    /// A stop byte named it; acted on at the next sample point.
    stop_requested: bool,
    word: i16,
    index: u8,
    gain: u8,
}

impl Channel {
    const IDLE: Self = Self {
        phase: Phase::Idle,
        busy: false,
        stop_requested: false,
        word: 0,
        index: 0,
        gain: GAIN_UNITY,
    };

    const fn silence(&mut self) {
        self.phase = Phase::Idle;
        self.busy = false;
        self.stop_requested = false;
        self.word = 0;
    }

    /// Oki's reproduction on one code (`MSM6295-015`), with `msm5205`'s widths
    /// and fractions truncated term by term (`MSM6295-OPEN-01`).
    fn synthesise(&mut self, code: u8) {
        let step = STEP_TABLE[usize::from(self.index)];
        let magnitude = code & 0b0111;
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
        let delta = delta as i16;
        let word = if code & 0b1000 != 0 {
            self.word.saturating_sub(delta)
        } else {
            self.word.saturating_add(delta)
        };
        self.word = word.clamp(PCM_RANGE_STEPS.0 as i16, PCM_RANGE_STEPS.1 as i16);
        let index = i16::from(self.index) + i16::from(STEP_ADJUST[usize::from(magnitude)]);
        self.index = index.clamp(0, i16::from(LAST_STEP_INDEX)) as u8;
    }
}

/// The part.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Msm6295 {
    /// Cycles since the last sample point.
    divider: usize,
    /// A phrase byte awaiting its second.
    first: Option<u8>,
    channels: [Channel; CHANNEL_COUNT],
    /// `RESET` as last seen.
    reset: bool,
    /// `SS` as last seen.
    ss: Level,
}

impl Default for Msm6295 {
    fn default() -> Self {
        Self::new()
    }
}

impl Msm6295 {
    /// A part at power-up: nothing playing, `DAO` at mid-scale
    /// (`MSM6295-016`).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            divider: 0,
            first: None,
            channels: [Channel::IDLE; CHANNEL_COUNT],
            reset: false,
            ss: Level::High,
        }
    }

    /// Oscillator cycles in one sample at the rate `SS` named on the last
    /// cycle, 33 × n: 132 with `SS` High and 165 with it Low (`MSM6295-004`).
    /// Before the first cycle, 132, the rate of [`Inputs::default`].
    #[must_use]
    pub const fn cycles_per_sample(&self) -> NonZeroUsize {
        Rate::from_pin(self.ss).cycles_per_sample()
    }

    /// Whether `RESET` held Low on the last cycle (`MSM6295-007`).
    #[must_use]
    pub const fn is_in_reset(&self) -> bool {
        self.reset
    }

    /// One byte latched from `I0`–`I7` (`MSM6295-007`), ignored while
    /// [`Msm6295::is_in_reset`].
    pub fn write(&mut self, d: u8) {
        if self.reset {
            return;
        }
        if let Some(phrase) = self.first.take() {
            self.second_byte(phrase, d);
        } else if d & 0x80 != 0 {
            self.first = Some(d & 0x7F);
        } else {
            let named = (d >> 3) & 0x0F;
            for (i, channel) in self.channels.iter_mut().enumerate() {
                if named & (1 << i) != 0 {
                    channel.stop_requested = true;
                }
            }
        }
    }

    /// A phrase's second byte: one channel in `I7`–`I4`, the attenuation in
    /// `I3`–`I0` (`MSM6295-008`).
    fn second_byte(&mut self, phrase: u8, byte: u8) {
        let named = byte >> 4;
        if phrase == 0 || named.count_ones() != 1 {
            return;
        }
        let channel = &mut self.channels[named.trailing_zeros() as usize];
        if channel.phase != Phase::Idle || channel.busy {
            return;
        }
        channel.phase = Phase::Pending(Pending {
            phrase,
            gain: ATTENUATION_GAIN[usize::from(byte & 0x0F)],
            elapsed: 0,
            seen: 0,
        });
    }

    /// A read, `CS` and `RD` Low: the busy nibble the part drives on
    /// `I0`–`I3`, channel 1 in bit 0, set while the channel plays
    /// (`MSM6295-013`). `I4`–`I7` are not driven and read here as 0. The part
    /// has no select pin, and the read has no side effect.
    #[must_use]
    pub fn read(&self) -> u8 {
        self.channels
            .iter()
            .enumerate()
            .fold(0, |acc, (i, c)| acc | (u8::from(c.busy) << i))
    }

    /// Advances one oscillator cycle with the pins as given, reading `rom` where
    /// the part would.
    ///
    /// `RESET` held Low suspends every function (`MSM6295-007`): it stops every
    /// channel and forgets a phrase byte awaiting its second
    /// (`MSM6295-OPEN-12`). A change of `SS` mid-stream continues the divider,
    /// compared with the new length.
    pub fn tick<R: Rom + ?Sized>(&mut self, pins: Inputs, rom: &mut R) {
        self.reset = pins.reset_n.is_low();
        self.ss = pins.ss;
        if self.reset {
            self.divider = 0;
            self.first = None;
            for channel in &mut self.channels {
                channel.silence();
            }
            return;
        }
        let rate = pins.rate();
        let n = rate.n().get();
        let playing = self
            .channels
            .iter()
            .any(|c| matches!(c.phase, Phase::Playing { .. }));
        let mut lone_start = false;
        for channel in &mut self.channels {
            if let Phase::Pending(p) = &mut channel.phase {
                p.elapsed = p.elapsed.saturating_add(1);
                if p.elapsed >= BUSY_CYCLES_PER_N * n {
                    channel.busy = true;
                }
                lone_start |= p.elapsed >= START_CYCLES_PER_N * n && !playing;
            }
        }
        self.divider += 1;
        if self.divider >= rate.cycles_per_sample().get() || lone_start {
            self.divider = 0;
            for channel in &mut self.channels {
                if let Phase::Pending(p) = &mut channel.phase {
                    if playing {
                        p.seen += 1;
                    }
                    if p.seen >= 2 || p.elapsed >= START_CYCLES_PER_N * n {
                        let (start, stop) = address_data(rom, p.phrase);
                        channel.phase = Phase::Armed {
                            start,
                            stop,
                            gain: p.gain,
                        };
                    }
                }
                sample_point(channel, rom);
            }
        }
    }

    /// Channel `i`'s word, before its attenuation, 0 when silent.
    ///
    /// # Panics
    ///
    /// If `i` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn channel_pcm(&self, i: usize) -> i32 {
        self.channels[i].word as i32
    }

    /// The four channels' attenuated words summed, unclamped (`MSM6295-OPEN-06`).
    #[must_use]
    pub fn accumulator(&self) -> i32 {
        self.channels
            .iter()
            .map(|c| i32::from(c.word) * i32::from(c.gain) / i32::from(GAIN_UNITY))
            .sum()
    }

    /// `DAO` as a signed sample at rest 0: [`Msm6295::dac`] less [`DAC_MID`],
    /// the accumulator clamped to the converter's twelve bits, −[`AUDIO_FULL_SCALE`]
    /// to 2047 (`MSM6295-016`, `MSM6295-OPEN-06`).
    #[must_use]
    pub fn audio(&self) -> Audio<AUDIO_FULL_SCALE> {
        Audio(i32::from(self.dac()) - AUDIO_FULL_SCALE)
    }

    /// The converter's code, 0 to 4095, [`DAC_MID`] at rest: the accumulator clamped
    /// to twelve bits (`MSM6295-016`, `MSM6295-OPEN-06`).
    #[must_use]
    pub fn dac(&self) -> u16 {
        (self.accumulator() + i32::from(DAC_MID)).clamp(0, (1 << DAC_BIT_COUNT) - 1) as u16
    }

    /// `DAO` for a supply of `vdd_uv`, the code over 4096 of it.
    #[must_use]
    pub fn dao_uv(&self, vdd_uv: Microvolts) -> Microvolts {
        Microvolts(((i64::from(vdd_uv.0) * i64::from(self.dac())) >> DAC_BIT_COUNT) as i32)
    }
}

/// Phrase `phrase`'s start and stop addresses, from the eight bytes at 8n
/// (`MSM6295-009`).
fn address_data<R: Rom + ?Sized>(rom: &mut R, phrase: u8) -> (u32, u32) {
    let base = u32::from(phrase) << 3;
    let mut three = |at: u32| {
        (u32::from(rom.read(at) & 0x03) << 16)
            | (u32::from(rom.read(at + 1)) << 8)
            | u32::from(rom.read(at + 2))
    };
    let start = three(base);
    let stop = three(base + 3);
    (start, stop)
}

/// One channel at a sample point.
fn sample_point<R: Rom + ?Sized>(channel: &mut Channel, rom: &mut R) {
    if channel.stop_requested {
        channel.silence();
        return;
    }
    match channel.phase {
        Phase::Idle | Phase::Pending(_) => {}
        Phase::Armed { start, stop, gain } => {
            channel.busy = true;
            channel.gain = gain;
            channel.word = 0;
            channel.index = 0;
            if stop < start {
                channel.silence();
                return;
            }
            channel.phase = Phase::Playing {
                address: start,
                stop,
                low: false,
                finished: false,
            };
            take_nibble(channel, rom);
        }
        Phase::Playing { finished: true, .. } => channel.silence(),
        Phase::Playing { .. } => take_nibble(channel, rom),
    }
}

/// The next nibble, high first, through the stop byte's low nibble
/// (`MSM6295-010`).
fn take_nibble<R: Rom + ?Sized>(channel: &mut Channel, rom: &mut R) {
    let Phase::Playing {
        address, stop, low, ..
    } = channel.phase
    else {
        return;
    };
    let byte = rom.read(address & ADDRESS_MASK);
    let code = if low { byte & 0x0F } else { byte >> 4 };
    channel.synthesise(code);
    channel.phase = if low {
        Phase::Playing {
            address: (address + 1) & ADDRESS_MASK,
            stop,
            low: false,
            finished: address == stop,
        }
    } else {
        Phase::Playing {
            address,
            stop,
            low: true,
            finished: false,
        }
    };
}
