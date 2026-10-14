//! The Texas Instruments TMS5220C voice synthesis processor, at its pins —
//! the CPU side and the synthesizer.
//!
//! Every item below names the claim it implements.
//!
//! **One [`Tms5220c::tick`] is one period of the oscillator at `OSC`**, the
//! 640 kHz RC clock of an 8 kHz part (`TMS5220C-016`): four ticks are one
//! `ROMCLK` cycle, [`CYCLES_PER_SAMPLE`] are one sample, twenty-five samples
//! are one interpolation interval and eight, six, four or two intervals are
//! one frame (`TMS5220C-018`). The consumer shows the part `/RS`, `/WS` and
//! `D7`–`D0` on every call and reads back the data bus, `/READY`, `/INT` and
//! the audio output.
//!
//! **The bus** (`TMS5220C-003` to `TMS5220C-006`): `/WS` low is a write and
//! `/RS` low a read; `/READY` goes high the tick after a select falls and low
//! again when the transfer is done — a write is latched three or four
//! `ROMCLK` cycles after `/WS` fell (`TMS5220C-004`), a read's data is driven
//! one or two `ROMCLK` cycles after `/RS` fell. A write goes to the FIFO while Speak External
//! runs and to the command register otherwise (`TMS5220C-005`); a read
//! returns the data register after a Read Byte and the status register
//! otherwise. A command is busy for its wait time and a select during it holds
//! `/READY` high until it is done (`TMS5220C-006`). Both selects low is the
//! hardware clear (`TMS5220C-007`).
//!
//! **Speak External** (`TMS5220C-010`, `TMS5220C-013`): the FIFO is purged,
//! writes go to it, and when nine bytes are in, Talk Status sets and the
//! synthesizer starts fetching frames from it, LSB of the first-in byte
//! first. `BL` says fewer than nine bytes remain and `BE` that it ran dry,
//! which ends speech at once. A write while sixteen bytes are in holds
//! `/READY` high until one has left (`TMS5220C-OPEN-11`). The stop code ends it at the next frame
//! boundary after the energy has interpolated to zero (`TMS5220C-021`).
//!
//! **The synthesizer** decodes a frame (`TMS5220C-019`, `TMS5220C-020`) into
//! targets for energy, pitch and `K1`–`K10` and interpolates the present
//! values toward them once an interval, excites a ten-stage lattice with a
//! chirp on a pitch period or a random-signed constant, and clips the result
//! into an 8-bit D/A code once a sample (`TMS5220C-024`). The coefficient
//! tables (`TMS5220C-OPEN-01`), the arithmetic (`TMS5220C-OPEN-02`), the
//! chirp and the noise source (`TMS5220C-OPEN-03`) and the interpolation
//! fractions (`TMS5220C-OPEN-04`) are **the crate's definitions under open
//! items**, from the TMS 5220 manual's coding table and the interpolator
//! patent.
//!
//! **The output** ([`Tms5220c::dac`], [`Tms5220c::speaker_pa`],
//! [`Tms5220c::output`]): the D/A input byte, the current it stands
//! for on the sheet's 0 to 1500 µA scale, and the signed 10-bit value the
//! `I/O` pin carries serially (`TMS5220C-025`). Not speaking, the part drives
//! 750 µA, code [`DAC_MID`] (`TMS5220C-OPEN-09`). [`Tms5220c::audio`] is the
//! D/A's code less [`DAC_MID`], so the part not speaking reads 0.
//!
//! **What the crate chooses**, each an open item: each byte shifted out LSB
//! first and each field filled MSB first (`TMS5220C-OPEN-05`); where in the
//! `/READY` window a transfer lands (`TMS5220C-OPEN-06`); a read's
//! unspecified bits as 0 (`TMS5220C-OPEN-07`); the counters restarted as Talk
//! Status sets (`TMS5220C-OPEN-08`); the frame rate left alone by Reset
//! (`TMS5220C-OPEN-10`); and the output at rest and the lattice cleared at
//! once on an abnormal halt (`TMS5220C-OPEN-13`).
//!
//! **Withheld**: the VSM path — Load Address, Speak, Read Byte and Read and
//! Branch are latched and timed and drive no memory (`TMS5220C-012`,
//! `TMS5220C-027`); the `I/O` pin's bit-serial framing.
//!
//! # Examples
//!
//! ```
//! use tms5220c::{CYCLES_PER_SAMPLE, Inputs, Tms5220c, command, status};
//!
//! let mut vsp = Tms5220c::new();
//! // Speak External: the command, held on the bus until `/READY` falls.
//! vsp.tick(Inputs::write_access(command::SPEAK_EXTERNAL));
//! while vsp.ready_n().is_high() {
//!     vsp.tick(Inputs::write_access(command::SPEAK_EXTERNAL));
//! }
//! vsp.tick(Inputs::idle());
//! for _ in 0..CYCLES_PER_SAMPLE {
//!     vsp.tick(Inputs::idle());
//! }
//! assert!(vsp.is_speak_external_executing());
//! // The FIFO is empty and the part is silent until nine bytes are in.
//! assert_eq!(vsp.status() & status::BE, status::BE);
//! assert_eq!(vsp.audio().0, 0);
//! ```

#![no_std]

mod tables;

use core::num::NonZeroUsize;

use units::{Audio, Hertz, PartsPerBillion, Picoamps};

use tables::{
    CHIRP, ENERGY, FIELD_BITS, INTERPOLATION_DIVISORS, K1, K2, K3, K4, K5, K6, K7, K8, K9, K10,
    PITCH,
};

/// How many pins the package presents (`TMS5220C-002`).
pub const PIN_COUNT: usize = 28;

/// The RC oscillator of an 8 kHz part, typical (`TMS5220C-016`).
pub const OSC_TYP_HZ: Hertz = Hertz(640_000);

/// The oscillator's operational range, minimum and maximum, external RC
/// (`TMS5220C-016`).
pub const OSC_RANGE_HZ: (Hertz, Hertz) = (Hertz(620_000), Hertz(825_000));

/// Oscillator periods per `ROMCLK` period, the part's bit time
/// (`TMS5220C-016`).
pub const OSC_CYCLES_PER_ROMCLK_CYCLE: usize = 4;

/// `ROMCLK` periods per sample (`TMS5220C-016`, `TMS5220C-025`).
pub const ROMCLK_CYCLES_PER_SAMPLE: usize = 20;

/// Oscillator periods per sample: 80, so 640 kHz is 8 kHz (`TMS5220C-016`).
pub const CYCLES_PER_SAMPLE: usize = OSC_CYCLES_PER_ROMCLK_CYCLE * ROMCLK_CYCLES_PER_SAMPLE;

/// Samples per interpolation interval (`TMS5220C-018`).
pub const SAMPLES_PER_INTERVAL: usize = 25;

/// The external square-wave clock that stands in for the 640 kHz RC
/// oscillator, typical: half its rate (`TMS5220C-017`). A consumer driving
/// `OSC` that way ticks the crate twice per input period.
pub const EXTERNAL_CLOCK_TYP_HZ: Hertz = Hertz(320_000);

/// The FIFO's depth in bytes (`TMS5220C-013`).
pub const FIFO_BYTES: usize = 16;

/// Speak External starts, and `BL` clears, when this many bytes are in the
/// FIFO (`TMS5220C-010`, `TMS5220C-013`).
pub const FIFO_START_BYTE_COUNT: usize = 9;

/// The D/A's full-scale current: 1500 µA (`TMS5220C-024`).
pub const DAC_FULL_SCALE_PA: Picoamps = Picoamps(1_500_000_000);

/// The same full scale in microamps, the unit the D/A's steps are computed in.
const DAC_FULL_SCALE_UA: u32 = 1500;

/// Picoamps in a microamp.
const PA_PER_UA: i64 = 1_000_000;

/// The D/A code at half the D/A's full-scale current, 750 µA: the code the
/// part drives while not speaking, the word −1's (`TMS5220C-OPEN-09`).
pub const DAC_MID: u8 = 0x7F;

/// The `SPEAKER` pin's DC current with no signal, while not speaking:
/// 750 µA (`TMS5220C-OPEN-09`), code [`DAC_MID`].
pub const SPEAKER_DC_PA: Picoamps = Picoamps(750_000_000);

/// A pin's level, as a getter named for the pin returns it (`ready_n()`, `int_n()`).
pub use signal::Level;

/// The largest magnitude [`Tms5220c::audio`] returns: the D/A's top code
/// above [`DAC_MID`].
pub const AUDIO_FULL_SCALE: i32 = 128;

/// Oscillator cycles from `/WS` falling to the earliest write latch: three `ROMCLK` cycles
/// (`TMS5220C-004`, the sheet's 18 µs).
pub const WRITE_LATCH_MIN_CYCLES: usize = 12;

/// Oscillator cycles from `/RS` falling to the earliest read data: one `ROMCLK` cycle
/// (`TMS5220C-004`, the sheet's 6 µs).
pub const READ_DATA_MIN_CYCLES: usize = 4;

/// Wait time, in oscillator cycles at 640 kHz (`TMS5220C-006`): Load Address: 42 µs.
pub const WAIT_LOAD_ADDRESS_CYCLES: usize = 27;

/// Wait time, in oscillator cycles at 640 kHz (`TMS5220C-006`): Speak, not preceded by a Load Address: 56 µs.
pub const WAIT_SPEAK_CYCLES: usize = 36;

/// Wait time, in oscillator cycles at 640 kHz (`TMS5220C-006`): Speak, preceded by a Load Address: 287 µs.
pub const WAIT_SPEAK_AFTER_LOAD_ADDRESS_CYCLES: usize = 184;

/// Wait time, in oscillator cycles at 640 kHz (`TMS5220C-006`): Speak External: 42 µs.
pub const WAIT_SPEAK_EXTERNAL_CYCLES: usize = 27;

/// Wait time, in oscillator cycles at 640 kHz (`TMS5220C-006`): Reset: 300 µs.
pub const WAIT_RESET_CYCLES: usize = 192;

/// Wait time, in oscillator cycles at 640 kHz (`TMS5220C-006`): Read and Branch: 596 µs.
pub const WAIT_READ_AND_BRANCH_CYCLES: usize = 382;

/// Wait time, in oscillator cycles at 640 kHz (`TMS5220C-006`): Read Byte, until the byte can be read: 320 µs (§2.4.4).
pub const WAIT_READ_BYTE_CYCLES: usize = 205;

/// Wait time, in oscillator cycles at 640 kHz (`TMS5220C-006`): Read Byte preceded by a Load Address: 440 µs.
pub const WAIT_READ_BYTE_AFTER_LOAD_ADDRESS_CYCLES: usize = 282;

/// Wait time, in oscillator cycles at 640 kHz (`TMS5220C-006`): Load Frame Rate: not in the table; the Load Address figure is taken
/// (`TMS5220C-OPEN-06`).
pub const WAIT_LOAD_FRAME_RATE_CYCLES: usize = 27;

/// Wait time, in oscillator cycles at 640 kHz (`TMS5220C-006`): A Speak External data byte: 10 µs (§2.4.2).
pub const WAIT_SPEAK_EXTERNAL_DATA_CYCLES: usize = 7;

/// Wait time, in oscillator cycles at 640 kHz (`TMS5220C-006`): A status read: 12 µs (§2.4.3).
pub const WAIT_STATUS_READ_CYCLES: usize = 8;

/// The command codes, `D7` the MSB, `D7` don't care (`TMS5220C-008`).
pub mod command {
    /// `X100AAAA`.
    pub const LOAD_ADDRESS: u8 = 0x40;
    /// `X101XXXX`.
    pub const SPEAK: u8 = 0x50;
    /// `X110XXXX`.
    pub const SPEAK_EXTERNAL: u8 = 0x60;
    /// `X001XXXX`.
    pub const READ_BYTE: u8 = 0x10;
    /// `X011XXXX`.
    pub const READ_AND_BRANCH: u8 = 0x30;
    /// `X0X0XBCC`.
    pub const LOAD_FRAME_RATE: u8 = 0x00;
    /// `X111XXXX`.
    pub const RESET: u8 = 0x70;
    /// Load Frame Rate's `B` bit: 1 takes the rate from each frame
    /// (`TMS5220C-011`).
    pub const FRAME_RATE_VARIABLE: u8 = 0x04;
}

/// The status register's bits on the data bus (`TMS5220C-014`).
pub mod status {
    /// Talk Status, `D7`.
    pub const TS: u8 = 0x80;
    /// Buffer Low, `D6`.
    pub const BL: u8 = 0x40;
    /// Buffer Empty, `D5`.
    pub const BE: u8 = 0x20;
}

/// The four frame lengths in samples, indexed by the two control bits
/// (`TMS5220C-011`).
pub const FRAME_SAMPLES: [usize; 4] = [200, 150, 100, 50];

/// The unvoiced excitation's magnitude in steps of the chirp's signed
/// eight-bit word, 128 steps full scale: 0.5 of full scale (`TMS5220C-OPEN-03`).
pub const UNVOICED_MAGNITUDE_STEPS: u32 = 64;

/// The pins the consumer drives on a tick. `Default` is [`Inputs::idle`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Inputs {
    /// `/RS`, pin 28: Low is a read.
    pub rs_n: Level,
    /// `/WS`, pin 27: Low is a write.
    pub ws_n: Level,
    /// `D7`–`D0`, as the CPU drives them; ignored while the part is not
    /// being written.
    pub d: u8,
}

impl Inputs {
    /// Neither select active.
    #[must_use]
    pub const fn idle() -> Self {
        Inputs {
            rs_n: Level::High,
            ws_n: Level::High,
            d: 0xFF,
        }
    }

    /// `/WS` low with `d` on the bus.
    #[must_use]
    pub const fn write_access(d: u8) -> Self {
        Inputs {
            rs_n: Level::High,
            ws_n: Level::Low,
            d,
        }
    }

    /// `/RS` low.
    #[must_use]
    pub const fn read_access() -> Self {
        Inputs {
            rs_n: Level::Low,
            ws_n: Level::High,
            d: 0xFF,
        }
    }

    /// Both selects low: the hardware clear (`TMS5220C-003`).
    #[must_use]
    pub const fn clear_access() -> Self {
        Inputs {
            rs_n: Level::Low,
            ws_n: Level::Low,
            d: 0xFF,
        }
    }
}

impl Default for Inputs {
    /// [`Inputs::idle`].
    fn default() -> Self {
        Self::idle()
    }
}

/// The FIFO of §2.2.2: sixteen bytes, parallel in, serial out
/// (`TMS5220C-013`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Fifo {
    bytes: [u8; FIFO_BYTES],
    /// Index of the first-in byte.
    head: usize,
    /// Bytes not yet fully shifted out, the partly consumed one included.
    len: usize,
    /// Bits already shifted out of the head byte.
    bit: u8,
}

impl Fifo {
    const fn new() -> Self {
        Fifo {
            bytes: [0; FIFO_BYTES],
            head: 0,
            len: 0,
            bit: 0,
        }
    }

    fn purge(&mut self) {
        *self = Fifo::new();
    }

    /// Loads a byte above the last-in one. The bus never offers a
    /// seventeenth: the write cycle waits on a full buffer (`TMS5220C-OPEN-11`).
    fn push(&mut self, byte: u8) -> bool {
        if self.len >= FIFO_BYTES {
            return false;
        }
        self.bytes[(self.head + self.len) % FIFO_BYTES] = byte;
        self.len += 1;
        true
    }

    /// The next bit, LSB of the first-in byte first (`TMS5220C-OPEN-05`), or `None`
    /// on an empty buffer.
    fn pop_bit(&mut self) -> Option<bool> {
        if self.len == 0 {
            return None;
        }
        let bit = (self.bytes[self.head] >> self.bit) & 1 == 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.head = (self.head + 1) % FIFO_BYTES;
            self.len -= 1;
        }
        Some(bit)
    }
}

/// A bus transfer in flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Cycle {
    None,
    /// `/WS` fell `ticks` ago; `data` is what `/WS`'s rise captured, if it
    /// has risen.
    Write {
        ticks: usize,
        data: Option<u8>,
    },
    /// `/RS` fell `ticks` ago; `done` once the data is on the bus.
    Read {
        ticks: usize,
        done: bool,
    },
}

/// One set of synthesis parameters: what a frame decodes to, or what the
/// synthesizer presently runs on (`TMS5220C-019`, `TMS5220C-020`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Parameters {
    /// Energy, in steps of the lattice's 14-bit word: the unvoiced
    /// excitation's magnitude; 0 is silence.
    pub energy_steps: u32,
    /// Pitch period in samples; `None` is unvoiced, which has no period.
    pub pitch_samples: Option<NonZeroUsize>,
    /// `K1`–`K10`, in parts per billion; the synthesizer holds each in
    /// steps of 1/512, so every value is a multiple of 1 953 125.
    pub k_ppb: [PartsPerBillion; 10],
}

/// Parts per billion in one 1/512 step of a coefficient.
const PPB_PER_K_STEP: i64 = 1_000_000_000 / 512;

/// The synthesizer's own parameters: energy in lattice steps, pitch in
/// samples, the coefficients in steps of 1/512 (`TMS5220C-OPEN-02`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Lpc {
    energy: i32,
    pitch_samples: i32,
    k: [i32; 10],
}

impl Lpc {
    const ZERO: Self = Self {
        energy: 0,
        pitch_samples: 0,
        k: [0; 10],
    };

    fn parameters(self) -> Parameters {
        debug_assert!(
            self.energy >= 0 && self.pitch_samples >= 0,
            "an interpolated parameter went below zero: {self:?}"
        );
        Parameters {
            energy_steps: self.energy.unsigned_abs(),
            pitch_samples: usize::try_from(self.pitch_samples.unsigned_abs())
                .ok()
                .and_then(NonZeroUsize::new),
            k_ppb: self
                .k
                .map(|k| PartsPerBillion(i64::from(k) * PPB_PER_K_STEP)),
        }
    }
}

/// What the part drives on `D7`–`D0` during a read (`TMS5220C-003`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bus {
    /// The byte, once it is stable (`TMS5220C-004`).
    Stable(u8),
    /// Driven, before the byte is stable: a level the part does not
    /// determine.
    Undetermined,
}

/// The part.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Tms5220c {
    prev_rs_n: bool,
    prev_ws_n: bool,
    cycle: Cycle,
    ready_n: bool,
    int_n: bool,
    /// An `/INT` assertion held back until a read cycle in progress ends.
    int_pending: bool,
    d_out: Option<u8>,
    /// Ticks until the present command has finished executing.
    busy: usize,
    /// The read that follows a Read Byte returns the data register.
    next_read_is_data: bool,
    data_register: u8,
    last_was_load_address: bool,
    fifo: Fifo,
    talk: bool,
    buffer_low: bool,
    buffer_empty: bool,
    speak_external: bool,
    /// Load Frame Rate's `B` bit.
    frame_rate_variable: bool,
    /// The fixed rate's two control bits.
    frame_rate_code: u8,
    tick_in_bit_time: usize,
    bit_time_in_sample: usize,
    sample_in_interval: usize,
    interval_in_frame: usize,
    intervals_per_frame: usize,
    /// The interval count the frame fetched during `IC0` asks for; taken at
    /// the boundary.
    next_intervals: usize,
    /// The stop code's frame has run; Talk Status clears at the boundary.
    stop_at_boundary: bool,
    target: Lpc,
    present: Lpc,
    old_energy_zero: bool,
    old_unvoiced: bool,
    inhibit: bool,
    /// The stop code has been fetched; Talk Status clears at the next
    /// frame boundary.
    stopping: bool,
    chirp_index: i32,
    lfsr: u16,
    lattice: [i32; 11],
    y_latch: i32,
}

impl Default for Tms5220c {
    fn default() -> Self {
        Self::new()
    }
}

impl Tms5220c {
    /// A part just after the hardware clear (`TMS5220C-007`).
    #[must_use]
    pub const fn new() -> Self {
        Tms5220c {
            prev_rs_n: true,
            prev_ws_n: true,
            cycle: Cycle::None,
            ready_n: false,
            int_n: true,
            int_pending: false,
            d_out: None,
            busy: 0,
            next_read_is_data: false,
            data_register: 0,
            last_was_load_address: false,
            fifo: Fifo::new(),
            talk: false,
            buffer_low: true,
            buffer_empty: true,
            speak_external: false,
            frame_rate_variable: false,
            frame_rate_code: 0,
            tick_in_bit_time: 0,
            bit_time_in_sample: 0,
            sample_in_interval: 0,
            interval_in_frame: 0,
            intervals_per_frame: 8,
            next_intervals: 8,
            stop_at_boundary: false,
            target: Lpc::ZERO,
            present: Lpc::ZERO,
            old_energy_zero: true,
            old_unvoiced: true,
            inhibit: false,
            stopping: false,
            chirp_index: 0,
            lfsr: 0x7FFF,
            lattice: [0; 11],
            y_latch: -1,
        }
    }

    /// `/READY`, pin 18: High while the CPU must wait, Low when the transfer
    /// is complete (`TMS5220C-004`).
    #[must_use]
    pub const fn ready_n(&self) -> Level {
        Level::from_bool(self.ready_n)
    }

    /// `/INT`, pin 17: Low while an interrupt stands (`TMS5220C-015`).
    #[must_use]
    pub const fn int_n(&self) -> Level {
        Level::from_bool(self.int_n)
    }

    /// What the part drives on `D7`–`D0` this tick: `None` while it
    /// releases them, and while `/RS` is Low and `/WS` High, the byte once it
    /// is stable and [`Bus::Undetermined`] before (`TMS5220C-003`,
    /// `TMS5220C-004`).
    #[must_use]
    pub const fn d(&self) -> Option<Bus> {
        if self.prev_rs_n || !self.prev_ws_n {
            return None;
        }
        Some(match self.d_out {
            Some(byte) => Bus::Stable(byte),
            None => Bus::Undetermined,
        })
    }

    /// The D/A's input byte this sample, offset binary: the 10-bit sample
    /// clamped to −128..=127 plus 128, so `0xFF` is 0 µA and `0x00` the top
    /// step, 1494 µA of the 1500 µA full scale (`TMS5220C-024`, `TMS5220C-OPEN-09`).
    #[must_use]
    pub fn dac(&self) -> u8 {
        let y10 = self.output();
        let [byte, ..] = (y10.clamp(-128, 127) + 128).to_le_bytes();
        byte
    }

    /// The `SPEAKER` pin's current, in picoamps, on the sheet's scale of
    /// 5.86 µA a step, rounded down to the microamp (`TMS5220C-024`).
    #[must_use]
    pub fn speaker_pa(&self) -> Picoamps {
        let ua = (255 - u32::from(self.dac())) * DAC_FULL_SCALE_UA / 256;
        Picoamps(i64::from(ua) * PA_PER_UA)
    }

    /// The sample as `SPEAKER` presents it: the 10-bit sample clamped to the
    /// D/A's eight bits, −128 to 127, measured from the word −1, the code
    /// [`DAC_MID`], so the part not speaking reads 0 (`TMS5220C-024`,
    /// `TMS5220C-OPEN-09`). One step is one D/A
    /// step, 5.86 µA; the sign is the sample's, which the pin's current
    /// inverts. Full scale is −127 to [`AUDIO_FULL_SCALE`].
    #[must_use]
    pub fn audio(&self) -> Audio<AUDIO_FULL_SCALE> {
        Audio(self.output().clamp(-128, 127) + 1)
    }

    /// The 10-bit word the `I/O` pin carries, as its bits: the most
    /// significant ten bits of the lattice's 14-bit output, two's complement
    /// (`TMS5220C-025`, `TMS5220C-OPEN-09`). [`Tms5220c::output`] is its level.
    #[must_use]
    pub const fn serial_word(&self) -> u16 {
        (self.y_latch >> 4) as u16 & 0x3FF
    }

    /// [`Tms5220c::serial_word`] decoded to its signed level, −512 to 511.
    #[must_use]
    pub const fn output(&self) -> i32 {
        self.y_latch >> 4
    }

    /// The lattice's 14-bit output word (`TMS5220C-OPEN-02`); `−1` at rest.
    #[must_use]
    pub const fn y_latch(&self) -> i32 {
        self.y_latch
    }

    /// The status register as a read returns it (`TMS5220C-014`); the
    /// unspecified bits read 0 (`TMS5220C-OPEN-07`).
    #[must_use]
    pub const fn status(&self) -> u8 {
        (if self.talk { status::TS } else { 0 })
            | (if self.buffer_low { status::BL } else { 0 })
            | (if self.buffer_empty { status::BE } else { 0 })
    }

    /// Talk Status (`TMS5220C-014`).
    #[must_use]
    pub const fn is_talking(&self) -> bool {
        self.talk
    }

    /// Buffer Low, `BL`: whether the FIFO is half empty, set when it has
    /// emptied to eight bytes and cleared when a load makes nine
    /// (`TMS5220C-013`).
    #[must_use]
    pub const fn is_buffer_half_empty(&self) -> bool {
        self.buffer_low
    }

    /// Buffer Empty, `BE` (`TMS5220C-013`).
    #[must_use]
    pub const fn is_buffer_empty(&self) -> bool {
        self.buffer_empty
    }

    /// Bytes in the FIFO not yet fully shifted out.
    #[must_use]
    pub const fn fifo_byte_count(&self) -> usize {
        self.fifo.len
    }

    /// Whether writes are going to the FIFO (`TMS5220C-005`).
    #[must_use]
    pub const fn is_speak_external_executing(&self) -> bool {
        self.speak_external
    }

    /// The frame length in samples the next fixed-rate frame takes
    /// (`TMS5220C-011`).
    #[must_use]
    pub const fn frame_samples(&self) -> usize {
        FRAME_SAMPLES[self.frame_rate_code as usize & 3]
    }

    /// Whether the frame rate is taken from each frame (`TMS5220C-011`).
    #[must_use]
    pub const fn is_frame_rate_variable(&self) -> bool {
        self.frame_rate_variable
    }

    /// Oscillator cycles until the present command has finished (`TMS5220C-006`).
    #[must_use]
    pub const fn busy_remaining_cycles(&self) -> usize {
        self.busy
    }

    /// The present (interpolated) energy, in steps of the lattice's 14-bit
    /// word, as [`Parameters::energy_steps`].
    #[must_use]
    pub fn energy_steps(&self) -> u32 {
        self.present.parameters().energy_steps
    }

    /// The present pitch period in samples; `None` is unvoiced, which has no
    /// period.
    #[must_use]
    pub fn pitch_samples(&self) -> Option<NonZeroUsize> {
        self.present.parameters().pitch_samples
    }

    /// The present `K1`–`K10`, in parts per billion, as
    /// [`Parameters::k_ppb`].
    #[must_use]
    pub fn k_ppb(&self) -> [PartsPerBillion; 10] {
        self.present.parameters().k_ppb
    }

    /// The targets the last fetched frame set.
    #[must_use]
    pub fn frame_parameters(&self) -> Parameters {
        self.target.parameters()
    }

    /// Whether interpolation is inhibited this frame (`TMS5220C-OPEN-04`).
    #[must_use]
    pub const fn is_inhibited(&self) -> bool {
        self.inhibit
    }

    /// Which interpolation interval of the frame is running, `0` the first.
    #[must_use]
    pub const fn interval(&self) -> usize {
        self.interval_in_frame
    }

    /// Intervals in the frame now running.
    #[must_use]
    pub const fn intervals_per_frame(&self) -> usize {
        self.intervals_per_frame
    }

    /// The cycle within the sample frame: the tick, `0` to
    /// `CYCLES_PER_SAMPLE − 1`.
    #[must_use]
    pub const fn frame_cycle(&self) -> usize {
        self.tick_in_bit_time + OSC_CYCLES_PER_ROMCLK_CYCLE * self.bit_time_in_sample
    }

    /// One period of `OSC`.
    pub fn tick(&mut self, pins: Inputs) {
        let (rs_n, ws_n) = (pins.rs_n.is_high(), pins.ws_n.is_high());
        if !rs_n && !ws_n {
            if self.prev_rs_n || self.prev_ws_n {
                self.hardware_clear();
            }
            self.prev_rs_n = false;
            self.prev_ws_n = false;
            return;
        }

        self.tick_in_bit_time += 1;
        let bit_boundary = self.tick_in_bit_time == OSC_CYCLES_PER_ROMCLK_CYCLE;
        if bit_boundary {
            self.tick_in_bit_time = 0;
            self.bit_time_in_sample += 1;
            if self.bit_time_in_sample == ROMCLK_CYCLES_PER_SAMPLE {
                self.bit_time_in_sample = 0;
                self.sample();
            }
        }
        if self.busy > 0 {
            self.busy -= 1;
        }

        if !ws_n && self.prev_ws_n && self.cycle == Cycle::None {
            self.cycle = Cycle::Write {
                ticks: 0,
                data: None,
            };
            self.ready_n = true;
        }
        if ws_n
            && !self.prev_ws_n
            && let Cycle::Write {
                data: data @ None, ..
            } = &mut self.cycle
        {
            *data = Some(pins.d);
        }
        if !rs_n && self.prev_rs_n && self.cycle == Cycle::None {
            self.cycle = Cycle::Read {
                ticks: 0,
                done: false,
            };
            self.ready_n = true;
            self.d_out = None;
        }

        match self.cycle {
            Cycle::None => {}
            Cycle::Write { ticks, data } => {
                let ticks = ticks + 1;
                let fifo_full = self.speak_external && self.fifo.len >= FIFO_BYTES;
                if self.busy == 0 && !fifo_full && ticks >= WRITE_LATCH_MIN_CYCLES && bit_boundary {
                    let byte = data.unwrap_or(pins.d);
                    self.cycle = Cycle::None;
                    self.ready_n = false;
                    self.write(byte);
                } else {
                    self.cycle = Cycle::Write { ticks, data };
                }
            }
            Cycle::Read { ticks, done } => {
                let ticks = ticks + 1;
                if !done && self.busy == 0 && ticks >= READ_DATA_MIN_CYCLES && bit_boundary {
                    let byte = if self.next_read_is_data {
                        self.next_read_is_data = false;
                        self.data_register
                    } else {
                        self.int_n = true;
                        self.status()
                    };
                    self.d_out = Some(byte);
                    self.ready_n = false;
                    self.busy = WAIT_STATUS_READ_CYCLES;
                    self.cycle = Cycle::Read { ticks, done: true };
                } else {
                    self.cycle = Cycle::Read { ticks, done };
                }
                if rs_n {
                    self.d_out = None;
                    let done = matches!(self.cycle, Cycle::Read { done: true, .. });
                    if done {
                        self.cycle = Cycle::None;
                        if self.int_pending {
                            self.int_pending = false;
                            self.int_n = false;
                        }
                    } else {
                        self.cycle = Cycle::None;
                        self.ready_n = false;
                        if self.int_pending {
                            self.int_pending = false;
                            self.int_n = false;
                        }
                    }
                }
            }
        }

        self.prev_rs_n = rs_n;
        self.prev_ws_n = ws_n;
    }

    /// Asserts `/INT`, or holds it until a read cycle in progress ends
    /// (`TMS5220C-015`).
    fn interrupt(&mut self) {
        if matches!(self.cycle, Cycle::Read { .. }) {
            self.int_pending = true;
        } else {
            self.int_n = false;
        }
    }

    /// A byte latched off the bus (`TMS5220C-005`).
    fn write(&mut self, byte: u8) {
        if self.speak_external {
            self.fifo.push(byte);
            self.buffer_empty = false;
            if self.fifo.len >= FIFO_START_BYTE_COUNT {
                self.buffer_low = false;
                if !self.talk {
                    self.start_talking();
                }
            }
            self.busy = WAIT_SPEAK_EXTERNAL_DATA_CYCLES;
            return;
        }
        let was_load_address = self.last_was_load_address;
        self.last_was_load_address = false;
        match byte & 0x70 {
            0x40 => {
                self.last_was_load_address = true;
                self.busy = WAIT_LOAD_ADDRESS_CYCLES;
            }
            0x50 => {
                self.busy = if was_load_address {
                    WAIT_SPEAK_AFTER_LOAD_ADDRESS_CYCLES
                } else {
                    WAIT_SPEAK_CYCLES
                };
            }
            0x60 => {
                self.speak_external_command();
                self.busy = WAIT_SPEAK_EXTERNAL_CYCLES;
            }
            0x10 => {
                self.next_read_is_data = true;
                self.busy = if was_load_address {
                    WAIT_READ_BYTE_AFTER_LOAD_ADDRESS_CYCLES
                } else {
                    WAIT_READ_BYTE_CYCLES
                };
            }
            0x30 => {
                self.busy = WAIT_READ_AND_BRANCH_CYCLES;
            }
            0x70 => {
                self.reset_command();
                self.busy = WAIT_RESET_CYCLES;
            }
            _ => {
                self.frame_rate_variable = byte & command::FRAME_RATE_VARIABLE != 0;
                if !self.frame_rate_variable {
                    self.frame_rate_code = byte & 3;
                }
                self.busy = WAIT_LOAD_FRAME_RATE_CYCLES;
            }
        }
    }

    /// Speak External (`TMS5220C-010`).
    fn speak_external_command(&mut self) {
        self.stop_talking();
        self.fifo.purge();
        self.speak_external = true;
        self.buffer_low = true;
        self.buffer_empty = true;
        self.interrupt();
    }

    /// Reset (`TMS5220C-009`).
    fn reset_command(&mut self) {
        self.stop_talking();
        self.speak_external = false;
        self.fifo.purge();
        self.buffer_low = true;
        self.buffer_empty = true;
        self.next_read_is_data = false;
        self.int_n = true;
        self.int_pending = false;
    }

    /// The hardware clear (`TMS5220C-007`): Reset, the frame period back
    /// to 200 samples, the T state counter reset.
    fn hardware_clear(&mut self) {
        let bl_was_low = !self.buffer_low;
        self.reset_command();
        self.frame_rate_variable = false;
        self.frame_rate_code = 0;
        self.tick_in_bit_time = 0;
        self.bit_time_in_sample = 0;
        self.sample_in_interval = 0;
        self.interval_in_frame = 0;
        self.cycle = Cycle::None;
        self.ready_n = false;
        self.d_out = None;
        self.busy = 0;
        self.last_was_load_address = false;
        if bl_was_low {
            self.interrupt();
        }
    }

    /// Talk Status sets: the counters restart and the first frame is fetched
    /// (`TMS5220C-OPEN-08`).
    fn start_talking(&mut self) {
        self.talk = true;
        self.stopping = false;
        self.present = Lpc::ZERO;
        self.target = Lpc::ZERO;
        self.old_energy_zero = true;
        self.old_unvoiced = true;
        self.lattice = [0; 11];
        self.chirp_index = 0;
        self.sample_in_interval = 0;
        self.interval_in_frame = 0;
        self.stop_at_boundary = false;
        self.fetch_frame();
        self.intervals_per_frame = self.next_intervals;
    }

    /// Talk Status clears and the output rests (`TMS5220C-OPEN-13`).
    fn stop_talking(&mut self) {
        if self.talk {
            self.talk = false;
            self.interrupt();
        }
        self.stopping = false;
        self.stop_at_boundary = false;
        self.speak_external = false;
        self.y_latch = -1;
        self.lattice = [0; 11];
        self.present = Lpc::ZERO;
    }

    /// Speech ends because the FIFO ran dry (`TMS5220C-013`).
    fn buffer_ran_dry(&mut self) {
        self.buffer_empty = true;
        self.stop_talking();
    }

    fn pop_bits(&mut self, n: u8) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            let bit = self.fifo.pop_bit()?;
            v = (v << 1) | bit as u32;
            if self.fifo.len == 8 && self.fifo.bit == 0 && !self.buffer_low {
                self.buffer_low = true;
                self.interrupt();
            }
        }
        Some(v)
    }

    /// Reads one frame off the FIFO into the targets (`TMS5220C-019`,
    /// `TMS5220C-020`, `TMS5220C-OPEN-04`, `TMS5220C-OPEN-05`).
    fn fetch_frame(&mut self) {
        let mut targets = self.target;
        let fetched = self.parse_frame(&mut targets);
        if fetched.is_none() || self.fifo.len == 0 {
            self.buffer_ran_dry();
            return;
        }
        let Some((energy_code, unvoiced)) = fetched else {
            return;
        };
        if energy_code == 15 {
            self.stopping = true;
            targets.energy = 0;
            self.inhibit = false;
        } else {
            let energy_zero = energy_code == 0;
            self.inhibit = (energy_zero != self.old_energy_zero && !energy_zero)
                || (unvoiced != self.old_unvoiced && !energy_zero);
            self.old_energy_zero = energy_zero;
            if !energy_zero {
                self.old_unvoiced = unvoiced;
            }
        }
        self.target = targets;
    }

    /// The frame data string (`TMS5220C-020`). Returns the energy code and
    /// whether the frame is unvoiced, or `None` when the FIFO ran out.
    fn parse_frame(&mut self, t: &mut Lpc) -> Option<(u32, bool)> {
        let rate = if self.frame_rate_variable {
            self.pop_bits(2)? as u8
        } else {
            self.frame_rate_code
        };
        self.next_intervals = FRAME_SAMPLES[rate as usize & 3] / SAMPLES_PER_INTERVAL;
        let energy_code = self.pop_bits(FIELD_BITS[0])?;
        if energy_code == 0 {
            t.energy = 0;
            return Some((0, self.old_unvoiced));
        }
        if energy_code == 15 {
            return Some((15, self.old_unvoiced));
        }
        t.energy = i32::from(ENERGY[energy_code as usize]);
        let repeat = self.pop_bits(1)? == 1;
        let pitch_code = self.pop_bits(FIELD_BITS[1])?;
        t.pitch_samples = i32::from(PITCH[pitch_code as usize]);
        let unvoiced = pitch_code == 0;
        if repeat {
            return Some((energy_code, unvoiced));
        }
        t.k[0] = i32::from(K1[self.pop_bits(FIELD_BITS[2])? as usize]);
        t.k[1] = i32::from(K2[self.pop_bits(FIELD_BITS[3])? as usize]);
        t.k[2] = i32::from(K3[self.pop_bits(FIELD_BITS[4])? as usize]);
        t.k[3] = i32::from(K4[self.pop_bits(FIELD_BITS[5])? as usize]);
        if unvoiced {
            t.k[4..].fill(0);
            return Some((energy_code, true));
        }
        t.k[4] = i32::from(K5[self.pop_bits(FIELD_BITS[6])? as usize]);
        t.k[5] = i32::from(K6[self.pop_bits(FIELD_BITS[7])? as usize]);
        t.k[6] = i32::from(K7[self.pop_bits(FIELD_BITS[8])? as usize]);
        t.k[7] = i32::from(K8[self.pop_bits(FIELD_BITS[9])? as usize]);
        t.k[8] = i32::from(K9[self.pop_bits(FIELD_BITS[10])? as usize]);
        t.k[9] = i32::from(K10[self.pop_bits(FIELD_BITS[11])? as usize]);
        Some((energy_code, false))
    }

    /// One interpolation step at the start of an interval (`TMS5220C-OPEN-04`).
    fn interpolate(&mut self) {
        let n = self.intervals_per_frame;
        let last = self.interval_in_frame == n - 1;
        let divisor = INTERPOLATION_DIVISORS[8 - n + self.interval_in_frame];
        let step = |present: &mut i32, target: i32| {
            *present += (target - *present) / divisor;
        };
        if self.inhibit && !last {
            return;
        }
        step(&mut self.present.energy, self.target.energy);
        step(&mut self.present.pitch_samples, self.target.pitch_samples);
        for i in 0..10 {
            step(&mut self.present.k[i], self.target.k[i]);
        }
    }

    /// One sample period (`TMS5220C-024`).
    fn sample(&mut self) {
        if !self.talk {
            self.y_latch = -1;
            return;
        }
        if self.sample_in_interval == 0 {
            self.interpolate();
            if self.interval_in_frame + 1 == self.intervals_per_frame {
                if self.stopping {
                    self.stop_at_boundary = true;
                } else {
                    self.fetch_frame();
                    if !self.talk {
                        return;
                    }
                }
            }
        }
        self.sample_in_interval += 1;
        if self.sample_in_interval == SAMPLES_PER_INTERVAL {
            self.sample_in_interval = 0;
            self.interval_in_frame += 1;
            if self.interval_in_frame == self.intervals_per_frame {
                self.interval_in_frame = 0;
                self.intervals_per_frame = self.next_intervals;
                if self.stop_at_boundary {
                    self.stop_at_boundary = false;
                    self.stop_talking();
                    return;
                }
            }
        }

        let energy = self.present.energy;
        let excitation = if self.present.pitch_samples == 0 {
            let bit = (self.lfsr ^ (self.lfsr >> 1)) & 1;
            self.lfsr = (self.lfsr >> 1) | (bit << 14);
            let sign = if bit == 1 { 1 } else { -1 };
            (sign * UNVOICED_MAGNITUDE_STEPS as i32 * energy) >> 6
        } else {
            let chirp = CHIRP.get(self.chirp_index as usize).copied().unwrap_or(0);
            self.chirp_index += 1;
            if self.chirp_index >= self.present.pitch_samples {
                self.chirp_index = 0;
            }
            (i32::from(chirp) * energy) >> 6
        };

        let k = self.present.k;
        let mut u = excitation;
        for i in (1..=10).rev() {
            u -= (k[i - 1] * self.lattice[i - 1]) >> 9;
            self.lattice[i] = self.lattice[i - 1] + ((k[i - 1] * u) >> 9);
        }
        self.lattice[0] = u;
        for stage in &mut self.lattice {
            *stage = (*stage).clamp(-8192, 8191);
        }
        self.y_latch = u.clamp(-8192, 8191);
    }
}
