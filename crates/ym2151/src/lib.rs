//! The Yamaha YM2151 (OPM), at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **One [`Ym2151::tick`] is one cycle of `φM`**, the 3.58 MHz clock at pin
//! 24 (`YM2151-002`). The consumer shows the part its bus on every call —
//! `A0`, `D7`–`D0`, `/CS`, `/WR`, `/RD`, `/IC` — and reads back the data
//! bus, `/IRQ`, `CT1`, `CT2`, `φ1` and the serial output `SO` with its
//! sample-and-hold strobes `SH1` and `SH2`. A write is taken when `/CS` and
//! `/WR` stop being both Low (`YM2151-004`); a read drives the status byte
//! while `/CS` and `/RD` are both Low (`YM2151-005`).
//!
//! **One sample every [`CYCLES_PER_SAMPLE`] cycles** (`YM2151-042`): the
//! thirty-two slots — four operators of eight channels — are computed on
//! the first cycle of the frame, and the frame's sixty-four cycles carry
//! the two channels' serial words out on `SO`, right then left, thirteen
//! bits each of a 10-bit offset-binary mantissa and a 3-bit exponent
//! (`YM2151-041`). [`Ym2151::output`] is the same pair decoded to signed
//! levels on a ±32768 scale, which is what the YM3012 turns into
//! ±¼ `VDD` (`YM2151-SIB-01`).
//!
//! **Inside**: a phase generator per slot in units of `φM / 2^26`, an
//! envelope generator per slot in units of 0.09375 dB ([`Envelope`]), one
//! operator with a log-sin and an exponential table, the eight connections (`YM2151-020`), the feedback path
//! (`YM2151-021`), the LFO with its four waveforms (`YM2151-032` to
//! `YM2151-035`), the noise generator on slot 32 (`YM2151-031`), the two
//! timers with their flags, `/IRQ` and `CSM` (`YM2151-036` to
//! `YM2151-040`), and the accumulator with its left and right enables
//! (`YM2151-022`, `YM2151-041`).
//!
//! **What the crate defines** is each an open item: the state at power-up
//! and what `/IC` resets (`YM2151-OPEN-01`); the sine and exponential tables
//! (`YM2151-OPEN-02`); a write taken at once, busy or not (`YM2151-OPEN-03`);
//! note codes 3, 7, 11 and 15 (`YM2151-OPEN-04`); the frequency table's
//! octave shift and its clamp (`YM2151-OPEN-05`); DT1 4 to 7 as a sign
//! (`YM2151-OPEN-06`); the envelope's step pattern within the manual's printed
//! times (`YM2151-OPEN-07`); a key-on over a sounding slot (`YM2151-OPEN-08`);
//! a register rate of 0 key-scaled to `Rks` rather than held at 0
//! (`YM2151-OPEN-09`); decay times to Fig. 2.11's 90 % to 10 % column
//! (`YM2151-OPEN-10`); the modulation depth (`YM2151-OPEN-11`); the noise
//! generator's polynomial and its restart on a write (`YM2151-OPEN-12`); CSM's
//! one-sample key-on (`YM2151-OPEN-13`); where the bits and `SH` strobes sit
//! on `SO` (`YM2151-OPEN-14`); status bits 2 to 6 and `A0` on a read
//! (`YM2151-OPEN-15`); the LFO's noise waveform (`YM2151-OPEN-16`); `LOAD`'s
//! start and reload (`YM2151-OPEN-17`); the accumulator's width and clamp
//! (`YM2151-OPEN-18`); and where DT1 joins against MUL (`YM2151-OPEN-19`).
//!
//! # Examples
//!
//! Key one carrier at A4 and run a hundred samples:
//!
//! ```
//! use ym2151::{AUDIO_FULL_SCALE, CYCLES_PER_SAMPLE, Inputs, Level, Ym2151, kon, reg};
//!
//! let mut part = Ym2151::default();
//! let mut poke = |address: u8, data: u8| {
//!     part.write(Level::Low, address);
//!     part.write(Level::High, data);
//! };
//! poke(reg::RL_FB_CON, 0xC7); // channel 0: both outputs, connection 7
//! poke(reg::KC, 0x4A); // A4
//! poke(reg::DT1_MUL + 24, 0x01); // C2: MUL 1
//! poke(reg::KS_AR + 24, 0x1F); // C2: AR 31
//! poke(reg::KON, kon::C2);
//! let mut peak = 0;
//! for _ in 0..100 * CYCLES_PER_SAMPLE {
//!     part.tick(Inputs::idle());
//!     let [left, right] = part.audio();
//!     assert_eq!(left, right);
//!     peak = peak.max(left.0.abs());
//! }
//! assert!(peak > 0 && peak <= AUDIO_FULL_SCALE);
//! ```

#![no_std]

#[cfg(test)]
extern crate std;

mod connections;
mod eg;
mod op;
mod pg;

use core::num::NonZeroUsize;
pub use eg::{Envelope, EnvelopeStage, Rates};
pub use op::ModulationDepth;
use pg::Waveform;
pub use pg::{DetunePosition, Lfo};
use units::{Audio, Hertz};

/// How many pins the package presents (`YM2151-001`).
pub const PIN_COUNT: usize = 24;

/// The typical clock at `φM`, which the manual states every frequency at,
/// in hertz (`YM2151-002`).
pub const CLOCK_TYP_HZ: Hertz = Hertz(pg::REFERENCE_CLOCK_HZ);

/// `φM` cycles in one sample: thirty-two slots of two (`YM2151-042`).
pub const CYCLES_PER_SAMPLE: usize = 64;

/// Channels (`YM2151-011`).
pub const CHANNEL_COUNT: usize = 8;
/// Operators per channel (`YM2151-011`).
pub const OPERATOR_COUNT: usize = 4;
/// Slots: every operator of every channel (`YM2151-011`).
pub const SLOT_COUNT: usize = CHANNEL_COUNT * OPERATOR_COUNT;

/// `φM` cycles a data write keeps the busy flag up (`YM2151-006`).
pub const BUSY_CYCLES: usize = 68;

/// Samples in one cycle of timer B's clock, 1024 `φM` cycles; timer A's
/// clock is the sample, 64 (`YM2151-036`, `YM2151-037`).
pub const TIMER_B_SAMPLES_PER_CYCLE: usize = 16;

/// The noise generator's clock period in `φM` cycles is this times
/// `32 − NFRQ` (`YM2151-031`).
const NOISE_PRESCALE_CYCLES: usize = 32;
/// The noise generator's sequence, in cycles of its own clock, whose period
/// in `φM` cycles is `32 × (32 − NFRQ)` (`YM2151-031`).
pub const NOISE_SEQUENCE_CYCLES: usize = (1 << 17) - 1;

/// Bits in a serial word on `SO`: nine of mantissa, the sign, three of
/// exponent (`YM2151-041`).
pub const SERIAL_BIT_COUNT: usize = 13;
/// `φ1` slots in one channel's half of the frame (`YM2151-041`).
pub const SERIAL_SLOTS_PER_CHANNEL: usize = 16;
/// The `φ1` slot within a channel's half at which its first bit appears:
/// the YM3012 discards three bits before the mantissa (`YM2151-OPEN-14`).
pub const SERIAL_FIRST_BIT_SLOT: usize = 3;
/// The `φ1` slot within a channel's half from which its `SH` strobe is High
/// to the end of the half (`YM2151-OPEN-14`).
pub const SERIAL_STROBE_SLOT: usize = 8;

/// The register addresses (`YM2151-010`); per-channel ones take `+ ch`,
/// per-slot ones `+ slot`, with `slot = 8 × operator + ch` for operators
/// `M1, M2, C1, C2` in that order (`YM2151-011`).
pub mod reg {
    /// The LSI test register; bit 1 holds the LFO at its start.
    pub const TEST: u8 = 0x01;
    /// Key on: bits 6–3 `C2 M2 C1 M1`, bits 2–0 the channel.
    pub const KON: u8 = 0x08;
    /// Bit 7 noise enable, bits 4–0 noise frequency.
    pub const NOISE: u8 = 0x0F;
    /// Timer A's top eight bits.
    pub const CLKA1: u8 = 0x10;
    /// Timer A's low two bits, in bits 1–0.
    pub const CLKA2: u8 = 0x11;
    /// Timer B's eight bits.
    pub const CLKB: u8 = 0x12;
    /// Timer control: bit 7 CSM, 5–4 flag reset B A, 3–2 IRQ enable B A,
    /// 1–0 load B A.
    pub const TIMER: u8 = 0x14;
    /// LFO frequency.
    pub const LFRQ: u8 = 0x18;
    /// Bit 7 set: PMD in bits 6–0; clear: AMD.
    pub const PMD_AMD: u8 = 0x19;
    /// Bits 7–6 `CT2 CT1`, bits 1–0 the LFO waveform.
    pub const CT_W: u8 = 0x1B;
    /// Per channel: bit 7 right, 6 left, 5–3 feedback, 2–0 connection.
    pub const RL_FB_CON: u8 = 0x20;
    /// Per channel: bits 6–4 octave, 3–0 note.
    pub const KC: u8 = 0x28;
    /// Per channel: bits 7–2 key fraction.
    pub const KF: u8 = 0x30;
    /// Per channel: bits 6–4 PMS, 1–0 AMS.
    pub const PMS_AMS: u8 = 0x38;
    /// Per slot: bits 6–4 DT1, 3–0 MUL.
    pub const DT1_MUL: u8 = 0x40;
    /// Per slot: bits 6–0 total level.
    pub const TL: u8 = 0x60;
    /// Per slot: bits 7–6 KS, 4–0 AR.
    pub const KS_AR: u8 = 0x80;
    /// Per slot: bit 7 AMS enable, 4–0 D1R.
    pub const AMSEN_D1R: u8 = 0xA0;
    /// Per slot: bits 7–6 DT2, 4–0 D2R.
    pub const DT2_D2R: u8 = 0xC0;
    /// Per slot: bits 7–4 D1L, 3–0 RR.
    pub const D1L_RR: u8 = 0xE0;
}

/// The bits of the timer register `$14` (`YM2151-038`).
pub mod timer {
    /// The manual's `LOAD A`: timer A counts while set, reloaded as it is set.
    pub const LOAD_A: u8 = 0x01;
    /// The manual's `LOAD B`: timer B counts while set, reloaded as it is set.
    pub const LOAD_B: u8 = 0x02;
    /// The manual's `IRQEN A`: timer A's overflow sets its flag.
    pub const ENABLE_A: u8 = 0x04;
    /// The manual's `IRQEN B`: timer B's overflow sets its flag.
    pub const ENABLE_B: u8 = 0x08;
    /// The manual's `F RESET A`: writing it set clears timer A's flag.
    pub const RESET_A: u8 = 0x10;
    /// The manual's `F RESET B`: writing it set clears timer B's flag.
    pub const RESET_B: u8 = 0x20;
    /// `CSM`: timer A's overflow keys on every slot (`YM2151-040`).
    pub const CSM: u8 = 0x80;
}

/// The bits of the status byte (`YM2151-005`, `YM2151-039`).
pub mod status {
    /// Timer A overflowed.
    pub const TIMER_A: u8 = 0x01;
    /// Timer B overflowed.
    pub const TIMER_B: u8 = 0x02;
    /// A write is in progress.
    pub const BUSY: u8 = 0x80;
}

/// The bits of `KON` (`YM2151-012`): the manual's `SN` order is M1, C1,
/// M2, C2 from bit 3 up.
pub mod kon {
    /// Keys operator `M1`.
    pub const M1: u8 = 0x08;
    /// Keys operator `C1`.
    pub const C1: u8 = 0x10;
    /// Keys operator `M2`.
    pub const M2: u8 = 0x20;
    /// Keys operator `C2`.
    pub const C2: u8 = 0x40;
    /// The channel number, bits 2–0.
    pub const CHANNEL_MASK: u8 = 0x07;
}

/// The operators in register order (`YM2151-011`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Operator {
    /// The first modulator, the one with the feedback path; slots 0–7.
    M1 = 0,
    /// The second modulator; slots 8–15.
    M2 = 1,
    /// The first carrier; slots 16–23.
    C1 = 2,
    /// The second carrier; slots 24–31.
    C2 = 3,
}

impl Operator {
    /// Every operator, in register order.
    pub const ALL: [Operator; 4] = [Operator::M1, Operator::M2, Operator::C1, Operator::C2];

    /// The offset this operator's per-slot registers take on a channel,
    /// which is its slot number, `8 × operator + channel` (`YM2151-011`).
    ///
    /// # Panics
    ///
    /// Panics if `channel` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn slot_offset(self, channel: usize) -> u8 {
        assert!(channel < CHANNEL_COUNT, "channel out of range");
        ((self as usize) * CHANNEL_COUNT + channel) as u8
    }

    /// The `KON` bit that keys this operator.
    #[must_use]
    pub const fn key_bit(self) -> u8 {
        match self {
            Operator::M1 => kon::M1,
            Operator::M2 => kon::M2,
            Operator::C1 => kon::C1,
            Operator::C2 => kon::C2,
        }
    }
}

use Operator::{C1, C2, M1, M2};
use connections::CONNECTIONS;

/// The bus pins the consumer shows the part on every cycle (`YM2151-003`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Inputs {
    /// `A0`, pin 4: Low selects the address latch, High the data.
    pub a0: Level,
    /// `D7`–`D0` as driven by the bus master; read on a write.
    pub d: u8,
    /// `/CS`, pin 7.
    pub cs_n: Level,
    /// `/WR`, pin 5.
    pub wr_n: Level,
    /// `/RD`, pin 6.
    pub rd_n: Level,
    /// `/IC`, pin 3: Low holds the part in initial clear.
    pub ic_n: Level,
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

impl Inputs {
    /// The bus idle: nothing selected, `/IC` High.
    #[must_use]
    pub const fn idle() -> Self {
        Inputs {
            a0: Level::Low,
            d: 0,
            cs_n: Level::High,
            wr_n: Level::High,
            rd_n: Level::High,
            ic_n: Level::High,
        }
    }

    /// A write cycle: the address latch when `a0` is Low, data when High.
    #[must_use]
    pub const fn write_access(a0: Level, d: u8) -> Self {
        Inputs {
            a0,
            d,
            cs_n: Level::Low,
            wr_n: Level::Low,
            rd_n: Level::High,
            ic_n: Level::High,
        }
    }

    /// A read cycle at `a0`. The part drives the status byte at either level
    /// (`YM2151-005`, `YM2151-OPEN-15`).
    #[must_use]
    pub const fn read_access(a0: Level) -> Self {
        Inputs {
            a0,
            d: 0,
            cs_n: Level::Low,
            wr_n: Level::High,
            rd_n: Level::Low,
            ic_n: Level::High,
        }
    }

    /// The same pins with `/IC` at `ic_n`; Low holds the part in initial
    /// clear.
    #[must_use]
    pub const fn with_ic_n(self, ic_n: Level) -> Self {
        Inputs { ic_n, ..self }
    }
}

/// A pin's logic level.
pub use signal::Level;

/// The largest magnitude [`Ym2151::audio`] returns on either channel: the
/// decoded serial level's −32768, the YM3012's −¼ `VDD` (`YM2151-SIB-01`).
pub const AUDIO_FULL_SCALE: i32 = 32768;

/// A 13-bit serial word: bits 9–0 the mantissa in offset binary (bit 9 set
/// for zero and above), bits 12–10 the exponent code `S`, 1 to 7: the
/// level is the mantissa times `2^(S − 1)`, which is the YM3012's
/// `2^−N` of full scale with `N = 7 − S` (`YM2151-041`, `YM2151-SIB-01`).
const fn encode_serial(level: i16) -> u16 {
    let v = level as i32;
    let mut s: u32 = 1;
    while s < 7 {
        let m = v >> (s - 1);
        if m >= -512 && m <= 511 {
            break;
        }
        s += 1;
    }
    let m = v >> (s - 1);
    ((m + 512) as u16) | ((s as u16) << 10)
}

/// The level a serial word stands for, on the ±32768 scale.
const fn decode_serial(word: u16) -> i16 {
    let m = (word & 0x3FF) as i32 - 512;
    let s = ((word >> 10) & 7) as u32;
    let s = if s == 0 { 1 } else { s };
    (m << (s - 1)) as i16
}

/// One slot's state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Slot {
    phase: u32,
    envelope: Envelope,
    keyed: bool,
    csm_keyed: bool,
    /// The last two outputs, for the feedback path.
    out_prev: i16,
    out_before: i16,
    /// The latest output.
    out: i16,
    /// The latest increment, for [`Ym2151::increment`].
    increment: u32,
}

/// The part.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ym2151 {
    regs: [u8; 256],
    address: u8,
    pmd: u8,
    amd: u8,
    slots: [Slot; SLOT_COUNT],
    lfo: Lfo,
    /// `φM` cycle within the sample frame, 0..63.
    cycle: usize,
    /// Samples since the last envelope clock.
    eg_divider: usize,
    /// Envelope generator clocks since the last initial clear.
    eg_cycles: u64,
    /// The noise shift register, 17 bits.
    noise: u32,
    /// `φM` cycles until the noise register next steps.
    noise_countdown: usize,
    timer_a: usize,
    timer_b: usize,
    timer_a_running: bool,
    timer_b_running: bool,
    flag_a: bool,
    flag_b: bool,
    busy: usize,
    /// The write in flight: address-or-data and the byte last seen.
    write_pending: Option<(Level, u8)>,
    d_out: Option<u8>,
    /// The latest sample's accumulators before clamping.
    acc_left: i32,
    acc_right: i32,
    /// The latest sample's serial words.
    word_right: u16,
    word_left: u16,
    sample_ready: bool,
    /// Which reading of the connection path's depth the part runs under
    /// (`YM2151-OPEN-11`). A choice of reading, not a pin.
    modulation_depth: ModulationDepth,
    /// Which reading of DT1's place against MUL the part runs under
    /// (`YM2151-OPEN-19`). A choice of reading, not a pin.
    detune_position: DetunePosition,
}

impl Default for Ym2151 {
    /// [`Ym2151::new`] under the default readings, `EightPi` and `Inside`.
    fn default() -> Self {
        Self::new(ModulationDepth::EightPi, DetunePosition::Inside)
    }
}

impl Ym2151 {
    /// The part as `/IC` leaves it (`YM2151-007`, `YM2151-OPEN-01`), run
    /// under two readings of the caller's.
    ///
    /// `modulation_depth` is the connection path's modulation depth
    /// (`YM2151-OPEN-11`): `EightPi` is the default; `FourPi` is the depth
    /// `FL = 7` reaches. `detune_position` is where DT1 joins the increment
    /// against MUL (`YM2151-OPEN-19`): `Inside` is the default; `Outside` is
    /// chapter 1's equation read literally. They differ only where DT1 is set
    /// and MUL is not 1.
    #[must_use]
    pub const fn new(modulation_depth: ModulationDepth, detune_position: DetunePosition) -> Self {
        Ym2151 {
            regs: [0; 256],
            address: 0,
            pmd: 0,
            amd: 0,
            slots: [Slot {
                phase: 0,
                envelope: Envelope::new(),
                keyed: false,
                csm_keyed: false,
                out_prev: 0,
                out_before: 0,
                out: 0,
                increment: 0,
            }; SLOT_COUNT],
            lfo: Lfo::new(),
            cycle: 0,
            eg_divider: 0,
            eg_cycles: 0,
            noise: 1,
            noise_countdown: NOISE_PRESCALE_CYCLES * 32,
            timer_a: 0,
            timer_b: 0,
            timer_a_running: false,
            timer_b_running: false,
            flag_a: false,
            flag_b: false,
            busy: 0,
            write_pending: None,
            d_out: None,
            acc_left: 0,
            acc_right: 0,
            word_right: encode_serial(0),
            word_left: encode_serial(0),
            sample_ready: false,
            modulation_depth,
            detune_position,
        }
    }

    /// Initial clear: every register zero, every circuit at its start
    /// (`YM2151-007`). The two readings are not state and survive it.
    fn initial_clear(&mut self) {
        *self = Self::new(self.modulation_depth, self.detune_position);
    }

    /// The depth reading in force.
    #[must_use]
    pub const fn modulation_depth(&self) -> ModulationDepth {
        self.modulation_depth
    }

    /// The DT1 reading in force.
    #[must_use]
    pub const fn detune_position(&self) -> DetunePosition {
        self.detune_position
    }

    /// One cycle of `φM`.
    pub fn tick(&mut self, pins: Inputs) {
        self.sample_ready = false;
        if pins.ic_n.is_low() {
            self.initial_clear();
            return;
        }

        if self.busy > 0 {
            self.busy -= 1;
        }

        if pins.cs_n.is_low() && pins.wr_n.is_low() {
            self.write_pending = Some((pins.a0, pins.d));
        } else if let Some((a0, d)) = self.write_pending.take() {
            self.write(a0, d);
        }
        self.d_out = if pins.cs_n.is_low() && pins.rd_n.is_low() {
            Some(self.status())
        } else {
            None
        };
        self.noise_countdown -= 1;
        if self.noise_countdown == 0 {
            let nfrq = (self.regs[reg::NOISE as usize] & 0x1F) as usize;
            self.noise_countdown = NOISE_PRESCALE_CYCLES * (32 - nfrq);
            self.step_noise();
        }

        if self.cycle == 0 {
            self.compute_sample();
            self.sample_ready = true;
        }
        self.cycle = (self.cycle + 1) % CYCLES_PER_SAMPLE;
    }

    /// The 17-bit noise register: `x^17 + x^14 + 1`, the crate's choice of a
    /// maximal polynomial (`YM2151-OPEN-12`).
    fn step_noise(&mut self) {
        let bit = (self.noise ^ (self.noise >> 3)) & 1;
        self.noise = (self.noise >> 1) | (bit << 16);
    }

    /// One bus write, as [`Ym2151::tick`] takes it when `/CS` and `/WR` stop
    /// being both Low, without the bus: with `a0` Low, `d` is latched as the
    /// register address the next data write reaches (`YM2151-003`); with
    /// `a0` High, `d` is written to that register (`YM2151-010`) and the
    /// part is busy for [`BUSY_CYCLES`] after it.
    pub fn write(&mut self, a0: Level, d: u8) {
        if a0.is_high() {
            self.write_data(d);
        } else {
            self.address = d;
        }
    }

    /// A data write to the latched address.
    fn write_data(&mut self, data: u8) {
        let address = self.address;
        self.busy = BUSY_CYCLES;
        match address {
            reg::KON => {
                let ch = data & kon::CHANNEL_MASK;
                for operator in Operator::ALL {
                    let on = data & operator.key_bit() != 0;
                    let slot = slot_index(ch as usize, operator);
                    self.key(slot, on);
                }
                self.regs[address as usize] = data;
            }
            reg::NOISE => {
                self.noise_countdown = NOISE_PRESCALE_CYCLES * (32 - (data & 0x1F) as usize);
                self.regs[address as usize] = data;
            }
            reg::TIMER => {
                let was = self.regs[address as usize];
                if data & timer::LOAD_A != 0 && was & timer::LOAD_A == 0 {
                    self.timer_a = self.timer_a_period_cycles().get();
                }
                if data & timer::LOAD_B != 0 && was & timer::LOAD_B == 0 {
                    self.timer_b = self.timer_b_reload();
                }
                self.timer_a_running = data & timer::LOAD_A != 0;
                self.timer_b_running = data & timer::LOAD_B != 0;
                if data & timer::RESET_A != 0 {
                    self.flag_a = false;
                }
                if data & timer::RESET_B != 0 {
                    self.flag_b = false;
                }
                self.regs[address as usize] = data & !(timer::RESET_A | timer::RESET_B);
            }
            reg::PMD_AMD => {
                if data & 0x80 != 0 {
                    self.pmd = data & 0x7F;
                } else {
                    self.amd = data & 0x7F;
                }
                self.regs[address as usize] = data;
            }
            _ => self.regs[address as usize] = data,
        }
    }

    /// The `KON` state of a slot changing.
    fn key(&mut self, slot: usize, on: bool) {
        let was = self.slots[slot].keyed;
        if on && !was {
            let rates = self.rates_of_slot(slot);
            let s = &mut self.slots[slot];
            s.phase = 0;
            s.envelope.key_on(rates.attack);
        } else if !on && was && !self.slots[slot].csm_keyed {
            self.slots[slot].envelope.key_off();
        }
        self.slots[slot].keyed = on;
    }

    /// Timer A's period in cycles of its clock, one a sample (`YM2151-036`).
    #[must_use]
    pub const fn timer_a_period_cycles(&self) -> NonZeroUsize {
        let na = ((self.regs[reg::CLKA1 as usize] as usize) << 2)
            | (self.regs[reg::CLKA2 as usize] as usize & 3);
        NonZeroUsize::MIN.saturating_add(1023 - na)
    }

    /// Timer B's period in cycles of its clock, `256 − CLKB`, each
    /// [`TIMER_B_SAMPLES_PER_CYCLE`] samples (`YM2151-037`).
    #[must_use]
    pub const fn timer_b_period_cycles(&self) -> NonZeroUsize {
        NonZeroUsize::MIN.saturating_add((u8::MAX - self.regs[reg::CLKB as usize]) as usize)
    }

    /// Timer B's period in samples, which the counter is kept in.
    const fn timer_b_reload(&self) -> usize {
        self.timer_b_period_cycles().get() * TIMER_B_SAMPLES_PER_CYCLE
    }

    /// The status byte a read returns (`YM2151-005`, `YM2151-039`).
    #[must_use]
    pub const fn status(&self) -> u8 {
        let mut s = 0;
        if self.busy > 0 {
            s |= status::BUSY;
        }
        if self.flag_a {
            s |= status::TIMER_A;
        }
        if self.flag_b {
            s |= status::TIMER_B;
        }
        s
    }

    /// The key-scaled rates and `D1L` of a slot (`YM2151-025`).
    fn rates_of_slot(&self, slot: usize) -> Rates {
        let ch = slot % CHANNEL_COUNT;
        let kc = self.regs[reg::KC as usize + ch];
        let ks_ar = self.regs[reg::KS_AR as usize + slot];
        let rks = eg::key_scale_offset(kc, ks_ar >> 6);
        let d1l_rr = self.regs[reg::D1L_RR as usize + slot];
        Rates {
            attack: eg::rate(ks_ar & 0x1F, rks),
            decay1: eg::rate(self.regs[reg::AMSEN_D1R as usize + slot] & 0x1F, rks),
            decay2: eg::rate(self.regs[reg::DT2_D2R as usize + slot] & 0x1F, rks),
            release: eg::rate(eg::release_r(d1l_rr & 0xF), rks),
            decay1_level_steps: u32::from(eg::first_decay_level(d1l_rr >> 4)),
        }
    }

    /// A slot's phase increment per sample from its registers and the LFO's
    /// present phase modulation (`YM2151-014` to `YM2151-019`).
    fn increment_now_of_slot(&self, slot: usize) -> u32 {
        let ch = slot % CHANNEL_COUNT;
        let kc = self.regs[reg::KC as usize + ch];
        let kf = self.regs[reg::KF as usize + ch] >> 2;
        let dt1_mul = self.regs[reg::DT1_MUL as usize + slot];
        let dt2 = self.regs[reg::DT2_D2R as usize + slot] >> 6;
        let pms = (self.regs[reg::PMS_AMS as usize + ch] >> 4) & 7;
        let w = Waveform::from_field(self.regs[reg::CT_W as usize]);
        let lfp = pg::lfp(self.lfo.pm_waveform(w), self.pmd);
        let position = pg::key_position(kc, kf) as i32
            + pg::DT2_STEPS[dt2 as usize] as i32
            + pg::pm_offset_steps(lfp, pms);
        pg::increment_at(
            position,
            kc,
            (dt1_mul >> 4) & 7,
            dt1_mul & 0xF,
            self.detune_position,
        )
    }

    /// The thirty-two slots, once a sample.
    fn compute_sample(&mut self) {
        if self.timer_a_running {
            if self.timer_a <= 1 {
                self.timer_a = self.timer_a_period_cycles().get();
                if self.regs[reg::TIMER as usize] & timer::ENABLE_A != 0 {
                    self.flag_a = true;
                }
                if self.regs[reg::TIMER as usize] & timer::CSM != 0 {
                    self.csm_key_on();
                }
            } else {
                self.timer_a -= 1;
            }
        }
        if self.timer_b_running {
            if self.timer_b <= 1 {
                self.timer_b = self.timer_b_reload();
                if self.regs[reg::TIMER as usize] & timer::ENABLE_B != 0 {
                    self.flag_b = true;
                }
            } else {
                self.timer_b -= 1;
            }
        }

        if self.regs[reg::TEST as usize] & 0x02 != 0 {
            self.lfo.reset();
        } else {
            self.lfo
                .advance(self.regs[reg::LFRQ as usize], (self.noise & 0xFF) as u8);
        }
        let w = Waveform::from_field(self.regs[reg::CT_W as usize]);
        let lfp = pg::lfp(self.lfo.pm_waveform(w), self.pmd);
        let lfa = pg::lfa(self.lfo.am_waveform(w), self.amd);

        self.eg_divider += 1;
        let eg_tick = if self.eg_divider == eg::SAMPLES_PER_STEP {
            self.eg_divider = 0;
            self.eg_cycles += 1;
            true
        } else {
            false
        };
        if eg_tick {
            for slot in 0..SLOT_COUNT {
                let rates = self.rates_of_slot(slot);
                self.slots[slot]
                    .envelope
                    .tick(&rates, self.eg_cycles as u32);
            }
        }

        let noise_on = self.regs[reg::NOISE as usize] & 0x80 != 0;
        let noise_bit = self.noise & 1 != 0;
        let depth = self.modulation_depth;
        let mut acc_left: i32 = 0;
        let mut acc_right: i32 = 0;

        for ch in 0..CHANNEL_COUNT {
            let rl_fb_con = self.regs[reg::RL_FB_CON as usize + ch];
            let con = &CONNECTIONS[(rl_fb_con & 7) as usize];
            let fl = (rl_fb_con >> 3) & 7;
            let ams = self.regs[reg::PMS_AMS as usize + ch] & 3;
            let pms = (self.regs[reg::PMS_AMS as usize + ch] >> 4) & 7;
            let kc = self.regs[reg::KC as usize + ch];
            let kf = self.regs[reg::KF as usize + ch] >> 2;
            let pm_steps = pg::pm_offset_steps(lfp, pms);
            let am = pg::am_attenuation(lfa, ams);

            let mut outs = [0i32; OPERATOR_COUNT];
            for operator in [M1, C1, M2, C2] {
                let slot = slot_index(ch, operator);
                let dt1_mul = self.regs[reg::DT1_MUL as usize + slot];
                let dt2 = self.regs[reg::DT2_D2R as usize + slot] >> 6;
                let position =
                    pg::key_position(kc, kf) as i32 + pg::DT2_STEPS[dt2 as usize] as i32 + pm_steps;
                let increment = pg::increment_at(
                    position,
                    kc,
                    (dt1_mul >> 4) & 7,
                    dt1_mul & 0xF,
                    self.detune_position,
                );

                let sources: &[Operator] = match operator {
                    M1 => &[],
                    C1 => con.c1_from,
                    M2 => con.m2_from,
                    C2 => con.c2_from,
                };
                let s = &self.slots[slot];
                let modulation: i32 = if operator == M1 {
                    op::feedback_index(s.out_prev, s.out_before, fl)
                } else {
                    sources
                        .iter()
                        .map(|src| op::modulation_index_at(outs[*src as usize], depth))
                        .sum()
                };

                let tl = eg::total_level(self.regs[reg::TL as usize + slot]);
                let amsen = self.regs[reg::AMSEN_D1R as usize + slot] & 0x80 != 0;
                let envelope =
                    s.envelope.attenuation_steps() + tl as u32 + if amsen { am } else { 0 };
                let index = ((s.phase >> pg::PHASE_FRACTION_BITS) as i32 + modulation) as u32;
                let noise = if noise_on && slot == SLOT_COUNT - 1 {
                    Some(noise_bit)
                } else {
                    None
                };
                let out = op::operator_output(index, envelope, noise);
                outs[operator as usize] = out as i32;

                let s = &mut self.slots[slot];
                s.out_before = s.out_prev;
                s.out_prev = out;
                s.out = out;
                s.increment = increment;
                s.phase = (s.phase + increment) & (pg::PHASE_MODULUS - 1);
            }

            let channel: i32 = con.carriers.iter().map(|c| outs[*c as usize]).sum();
            if rl_fb_con & 0x40 != 0 {
                acc_left += channel;
            }
            if rl_fb_con & 0x80 != 0 {
                acc_right += channel;
            }
        }

        self.acc_left = acc_left;
        self.acc_right = acc_right;
        let clamp = |v: i32| v.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        self.word_left = encode_serial(clamp(acc_left));
        self.word_right = encode_serial(clamp(acc_right));

        for slot in 0..SLOT_COUNT {
            if self.slots[slot].csm_keyed {
                self.slots[slot].csm_keyed = false;
                if !self.slots[slot].keyed {
                    self.slots[slot].envelope.key_off();
                }
            }
        }
    }

    /// Timer A's overflow with `CSM` set keys on every slot (`YM2151-040`).
    fn csm_key_on(&mut self) {
        for slot in 0..SLOT_COUNT {
            let rates = self.rates_of_slot(slot);
            let s = &mut self.slots[slot];
            s.phase = 0;
            s.envelope.key_on(rates.attack);
            s.csm_keyed = true;
        }
    }

    /// What the data bus carries this cycle, if the part drives it.
    #[must_use]
    pub const fn d(&self) -> Option<u8> {
        self.d_out
    }

    /// `/IRQ`, pin 2, open drain: Low while either timer's flag is set
    /// (`YM2151-008`).
    #[must_use]
    pub const fn irq_n(&self) -> Level {
        Level::from_bool(!(self.flag_a || self.flag_b))
    }

    /// `CT1`, pin 8 (`YM2151-013`).
    #[must_use]
    pub const fn ct1(&self) -> Level {
        Level::from_bool(self.regs[reg::CT_W as usize] & 0x40 != 0)
    }

    /// `CT2`, pin 9 (`YM2151-013`).
    #[must_use]
    pub const fn ct2(&self) -> Level {
        Level::from_bool(self.regs[reg::CT_W as usize] & 0x80 != 0)
    }

    /// `φ1`, pin 23: `φM` halved, High on the first cycle of each slot
    /// (`YM2151-002`).
    #[must_use]
    pub const fn phi1(&self) -> Level {
        Level::from_bool(self.cycle & 1 == 0)
    }

    /// The `φ1` slot within the frame, 0..31.
    const fn serial_slot(&self) -> usize {
        self.cycle >> 1
    }

    /// `SO`, pin 21: the serial bit for this `φ1` slot (`YM2151-041`,
    /// `YM2151-OPEN-14`).
    #[must_use]
    pub const fn so(&self) -> Level {
        let slot = self.serial_slot();
        let (word, within) = if slot < SERIAL_SLOTS_PER_CHANNEL {
            (self.word_right, slot)
        } else {
            (self.word_left, slot - SERIAL_SLOTS_PER_CHANNEL)
        };
        if within < SERIAL_FIRST_BIT_SLOT || within >= SERIAL_FIRST_BIT_SLOT + SERIAL_BIT_COUNT {
            return Level::Low;
        }
        Level::from_bool((word >> (within - SERIAL_FIRST_BIT_SLOT)) & 1 != 0)
    }

    /// `SH1`, pin 20: High through the tail of the right word
    /// (`YM2151-041`).
    #[must_use]
    pub const fn sh1(&self) -> Level {
        let slot = self.serial_slot();
        Level::from_bool(slot >= SERIAL_STROBE_SLOT && slot < SERIAL_SLOTS_PER_CHANNEL)
    }

    /// `SH2`, pin 19: High through the tail of the left word
    /// (`YM2151-041`).
    #[must_use]
    pub const fn sh2(&self) -> Level {
        let slot = self.serial_slot();
        Level::from_bool(slot >= SERIAL_SLOTS_PER_CHANNEL + SERIAL_STROBE_SLOT)
    }

    /// Whether this cycle began a frame and computed a sample.
    #[must_use]
    pub const fn is_sample_ready(&self) -> bool {
        self.sample_ready
    }

    /// The latest sample's two serial words decoded to signed levels,
    /// `[left, right]`: full scale −32768 to 32767, rest 0, which is the
    /// YM3012's ±¼ `VDD` (`YM2151-041`, `YM2151-SIB-01`). This is the
    /// converter's scale, not a sum of channel levels.
    #[must_use]
    pub const fn output(&self) -> [i32; 2] {
        [
            decode_serial(self.word_left) as i32,
            decode_serial(self.word_right) as i32,
        ]
    }

    /// The latest sample as a stereo pair, `[left, right]`: the levels of
    /// [`Ym2151::output`], the serial words on `SO` as the YM3012 would take
    /// them, full scale [`AUDIO_FULL_SCALE`], rest 0 (`YM2151-041`,
    /// `YM2151-SIB-01`).
    #[must_use]
    pub const fn audio(&self) -> [Audio<AUDIO_FULL_SCALE>; 2] {
        let [left, right] = self.output();
        [Audio(left), Audio(right)]
    }

    /// The latest sample's 13-bit serial words, `[left, right]`, as `SO`
    /// carries them, right first in time (`YM2151-041`).
    #[must_use]
    pub const fn serial_word(&self) -> [u16; 2] {
        [self.word_left, self.word_right]
    }

    /// Register `i` as written.
    ///
    /// # Panics
    ///
    /// If `i` is not below 256.
    #[must_use]
    pub const fn register(&self, i: usize) -> u8 {
        self.regs[i]
    }

    /// The latched address.
    #[must_use]
    pub const fn address(&self) -> u8 {
        self.address
    }

    /// `PMD` as last written.
    #[must_use]
    pub const fn pmd(&self) -> u8 {
        self.pmd
    }

    /// `AMD` as last written.
    #[must_use]
    pub const fn amd(&self) -> u8 {
        self.amd
    }

    /// The key-scaled rates and `D1L` of channel `ch`'s operator `op`
    /// (`YM2151-025`).
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub fn rates(&self, ch: usize, op: Operator) -> Rates {
        self.rates_of_slot(slot_index(ch, op))
    }

    /// The phase increment per sample channel `ch`'s operator `op` would
    /// advance by now, from its registers and the LFO's present phase
    /// modulation (`YM2151-014` to `YM2151-019`).
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub fn increment_now(&self, ch: usize, op: Operator) -> u32 {
        self.increment_now_of_slot(slot_index(ch, op))
    }

    /// An operator's phase accumulator.
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn phase(&self, ch: usize, op: Operator) -> u32 {
        self.slots[slot_index(ch, op)].phase
    }

    /// The increment an operator last advanced by.
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn increment(&self, ch: usize, op: Operator) -> u32 {
        self.slots[slot_index(ch, op)].increment
    }

    /// An operator's envelope.
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn envelope(&self, ch: usize, op: Operator) -> Envelope {
        self.slots[slot_index(ch, op)].envelope
    }

    /// An operator's latest output, fourteen bits signed.
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn operator_output(&self, ch: usize, op: Operator) -> i32 {
        self.slots[slot_index(ch, op)].out as i32
    }

    /// Whether an operator is keyed by `KON`.
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn is_keyed(&self, ch: usize, op: Operator) -> bool {
        self.slots[slot_index(ch, op)].keyed
    }

    /// The LFO.
    #[must_use]
    pub const fn lfo(&self) -> &Lfo {
        &self.lfo
    }

    /// The noise generator's 17-bit shift register (`YM2151-031`).
    #[must_use]
    pub const fn noise_shift_register(&self) -> u32 {
        self.noise
    }

    /// Timer A's count, in cycles of its clock to its overflow, one a sample.
    #[must_use]
    pub const fn timer_a_remaining_cycles(&self) -> usize {
        self.timer_a
    }

    /// Timer B's count, in cycles of its clock to its overflow, a part-spent cycle
    /// counting whole.
    #[must_use]
    pub const fn timer_b_remaining_cycles(&self) -> usize {
        self.timer_b.div_ceil(TIMER_B_SAMPLES_PER_CYCLE)
    }

    /// `φM` cycles of busy left, of the [`BUSY_CYCLES`] a write starts.
    #[must_use]
    pub const fn busy_remaining_cycles(&self) -> usize {
        self.busy
    }

    /// The accumulators before clamping, `[left, right]`.
    #[must_use]
    pub const fn accumulator(&self) -> [i32; 2] {
        [self.acc_left, self.acc_right]
    }

    /// The envelope generator's clocks since the last initial clear, one
    /// every three samples (`YM2151-029`).
    #[must_use]
    pub const fn eg_cycles(&self) -> u64 {
        self.eg_cycles
    }

    /// The `φM` cycle within the sample frame, 0 to 63.
    #[must_use]
    pub const fn frame_cycle(&self) -> usize {
        self.cycle
    }
}

/// The slot of channel `ch`'s operator `op`, refusing a channel past the
/// last.
const fn slot_index(ch: usize, op: Operator) -> usize {
    op.slot_offset(ch) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_word_encodes_zero_from_power_up() {
        let p = Ym2151::default();
        assert_eq!(decode_serial(p.word_left), 0);
        assert_eq!(decode_serial(p.word_right), 0);
    }

    #[test]
    fn serial_codec() {
        for v in [
            0i16, 1, -1, 511, 512, -512, -513, 1000, -1000, 16384, 32767, -32768,
        ] {
            let w = encode_serial(v);
            let s = (w >> 10) & 7;
            assert!((1..=7).contains(&s), "{v}: S {s}");
            let d = decode_serial(w);
            let shift = s - 1;
            assert_eq!(d, (i32::from(v) >> shift << shift) as i16, "{v}");
            let m = i32::from(v) >> (s - 1);
            assert!((-512..=511).contains(&m));
            if s > 1 {
                let m_finer = i32::from(v) >> (s - 2);
                assert!(!(-512..=511).contains(&m_finer), "{v} fits finer");
            }
        }
        assert_eq!(encode_serial(0) & 0x3FF, 512, "zero is offset binary 512");
        assert_eq!(encode_serial(-1) & 0x3FF, 511);
        assert_eq!(encode_serial(32767) >> 10, 7);
        assert_eq!(encode_serial(5) >> 10, 1);
        assert_eq!(encode_serial(512) >> 10, 2);
    }

    fn run_samples(p: &mut Ym2151, n: usize) {
        while p.frame_cycle() != 0 {
            p.tick(Inputs::idle());
        }
        for _ in 0..n * CYCLES_PER_SAMPLE {
            p.tick(Inputs::idle());
        }
    }

    #[test]
    fn lfo_restarts_from_its_left_edge() {
        let mut p = Ym2151::default();
        p.write(Level::Low, reg::LFRQ);
        p.write(Level::High, 0xFF);
        run_samples(&mut p, 100);
        p.write(Level::Low, reg::TEST);
        p.write(Level::High, 0x02);
        run_samples(&mut p, 5);
        assert_eq!(p.lfo().fine_phase(), 0);
        p.write(Level::Low, reg::TEST);
        p.write(Level::High, 0x00);
        run_samples(&mut p, 1);
        assert_eq!(p.lfo().fine_phase(), Lfo::increment(0xFF));
    }
}
