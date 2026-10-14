//! The Yamaha YM2203 (OPN), at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **One [`Ym2203::tick`] is one cycle of `φM`** (`YM2203-007`). The
//! consumer shows the part its bus on every call — `A0`, `D7`–`D0`, `/CS`,
//! `/WR`, `/RD`, `/IC` and what drives the two ports' pins — and reads back
//! the data bus, `/IRQ`, the ports, the FM half's serial word and the SSG's
//! three channel codes. A write is taken when `/CS` and `/WR` stop being
//! both Low; `A0` Low is an address, High data (`YM2203-003`).
//! [`Ym2203::write`] is the same pair of writes without the bus.
//!
//! **The prescaler** (`YM2203-008`, `YM2203-009`): the part remembers two
//! selects, which an address write alone moves: `2DH` sets the first, `2EH`
//! the second, and `2FH` clears both; initial clear leaves the first set and
//! the second clear. With the first set the dividers are `n` = 6 for the FM
//! half and `m` = 4 for the SSG, or 3 and 2 once the second is set too; with
//! the first clear they are 2 and 1 whatever the second holds, so `2EH`
//! after `2FH` leaves 2 and 1 and `2DH` then makes 3 and 2. The FM half
//! computes one sample every `12 × n` cycles ([`Ym2203::fm_divider_cycles`])
//! and the SSG steps its generators every `4 × m`
//! ([`Ym2203::ssg_divider_cycles`]). `2DH` then `2EH` is 3 and 2.
//!
//! **The output as the part presents it** (`YM2203-026`): one 13-bit word
//! a sample for the YM3014, [`Ym2203::serial_word`], a 10-bit offset-binary
//! mantissa under a 3-bit exponent, and [`Ym2203::output`], the same word
//! decoded to a level on a ±32768 scale; and the three SSG channels
//! separately, as five-bit converter codes ([`Ym2203::ssg_code`]), for the
//! board to sum at its own ratio (`YM2203-037`).
//!
//! **Inside**: a phase generator per operator, an envelope generator per
//! operator ([`Envelope`]), one operator with a log-sin and an exponential
//! table, the eight algorithms (`YM2203-013`), the feedback path
//! (`YM2203-024`), channel 3's four frequencies (`YM2203-018`), the SSG's
//! tone, noise and envelope generators, the two timers and `/IRQ`
//! (`YM2203-027` to `YM2203-030`) and the ports (`YM2203-038`).
//!
//! **One sound sample**: [`Ym2203::audio`] sums the FM half and the three
//! SSG channels, the SSG through a level table the caller supplies.
//!
//! **Withheld**: SSG-EG (`YM2203-OPEN-06`) and `27H`'s bit 7
//! (`YM2203-OPEN-07`), stored and doing nothing; the serial word's framing
//! on `OP-O`, `SH` and `φS` (`YM2203-OPEN-01`). **What the crate defines**
//! is each an open item: the exponent's sense (`YM2203-OPEN-02`); the tables
//! (`YM2203-OPEN-03`); the envelope's step pattern (`YM2203-OPEN-04`); the FM
//! output's scale (`YM2203-OPEN-05`); the prescaler after initial clear
//! (`YM2203-OPEN-08`); a negative detune stopping at zero and key codes 29 to
//! 31 (`YM2203-OPEN-09`); feedback doubling through 7 (`YM2203-OPEN-10`); SL
//! 15 at 93 dB (`YM2203-OPEN-11`); a prescaler select reaching a sounding
//! channel at once (`YM2203-OPEN-12`); the held high bytes of `A4H`–`A6H` and
//! `ACH`–`AEH` (`YM2203-OPEN-13`); `28H`'s codes 3 to 7 keying nothing
//! (`YM2203-OPEN-14`); an undriven input port reading `FFH` (`YM2203-OPEN-15`);
//! the square-wave converter's curve (`YM2203-OPEN-16`); the square-wave
//! generators' grain and phases (`YM2203-OPEN-17`); busy on every data write,
//! every write taken (`YM2203-OPEN-18`); and a key on over a sounding operator
//! (`YM2203-OPEN-19`).
//!
//! # Examples
//!
//! Key channel 1's fourth operator alone and run a hundred samples:
//!
//! ```
//! use ym2203::{AUDIO_FULL_SCALE, Inputs, Level, Operator, Ym2203, reg};
//!
//! let mut part = Ym2203::new();
//! let mut poke = |address: u8, data: u8| {
//!     part.write(Level::Low, address);
//!     part.write(Level::High, data);
//! };
//! let op4 = Operator::Op4.slot_offset(0);
//! poke(reg::FB_CONNECT, 0x07); // channel 1: algorithm 7
//! poke(reg::BLOCK_FNUM_UPPER, 4 << 3 | 0x02); // block 4, F-number's top bits
//! poke(reg::FNUM_LOWER, 0x69); // and its low byte, which commits both
//! poke(reg::DT_MUL + op4, 0x01); // MUL 1
//! poke(reg::KS_AR + op4, 0x1F); // AR 31
//! poke(reg::KEY, Operator::Op4.key_bit());
//! let mut peak = 0;
//! for _ in 0..100 * part.cycles_per_sample().get() {
//!     part.tick(Inputs::idle());
//!     peak = peak.max(part.audio(&[units::Audio(0); 32]).0.abs());
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
mod ssg;

use core::num::NonZeroUsize;
pub use eg::{Envelope, EnvelopeStage, Rates};
use ssg::Ssg;

use units::Audio;

/// How many pins the package presents (`YM2203-002`).
pub const PIN_COUNT: usize = 40;

/// The part computes one FM sample every `12 × n` cycles of `φM` for the FM
/// divider `n` (`YM2203-014`); [`Ym2203::cycles_per_sample`] is the product.
const FM_SAMPLE_CYCLES_PER_DIVIDER: usize = 12;

/// The SSG's generators step once every `4 × m` cycles of `φM` for the SSG
/// divider `m` (`YM2203-032`); [`Ym2203::ssg_cycles_per_edge`] is the product.
const SSG_STEP_CYCLES_PER_DIVIDER: usize = 4;

/// Channels of the FM half (`YM2203-001`).
pub const CHANNEL_COUNT: usize = 3;
/// Operators per channel (`YM2203-001`).
pub const OPERATOR_COUNT: usize = 4;

/// Samples in one cycle of timer B's clock; timer A's clock is the sample
/// (`YM2203-028`).
pub const TIMER_B_SAMPLES_PER_CYCLE: usize = 16;

/// A pin's logic level.
pub use signal::Level;

/// A channel's level is held to fourteen bits (`YM2203-025`,
/// `YM2203-OPEN-05`).
pub const CHANNEL_FULL_SCALE: i32 = 8192;

/// The largest magnitude [`Ym2203::audio`] returns for an `ssg_levels` table
/// whose every level lies within 32768 of code 1's: the FM half's 24576,
/// three channels at [`CHANNEL_FULL_SCALE`]'s −8192, and three SSG channels
/// at 32768 each.
pub const AUDIO_FULL_SCALE: i32 = 3 * CHANNEL_FULL_SCALE + 3 * SSG_LEVEL_SPAN;

/// How far an SSG channel's level may stand from code 1's within
/// [`AUDIO_FULL_SCALE`]: the FM half's decoded scale, ±32768.
const SSG_LEVEL_SPAN: i32 = 32768;

/// The register addresses (`YM2203-010`, `YM2203-031`). Per-channel ones take
/// `+ c`, per-operator ones [`Operator::slot_offset`].
pub mod reg {
    /// The mixer and port directions.
    pub const MIXER: u8 = 0x07;
    /// The envelope's shape; a write restarts it.
    pub const ENVELOPE_SHAPE: u8 = 0x0D;
    /// Port A's latch.
    pub const PORT_A: u8 = 0x0E;
    /// Port B's latch.
    pub const PORT_B: u8 = 0x0F;
    /// The LSI test register, "always set to 0" (`YM2203-040`).
    pub const TEST: u8 = 0x21;
    /// Timer A's top eight bits.
    pub const TIMER_A_UPPER: u8 = 0x24;
    /// Timer A's low two bits, in bits 1–0.
    pub const TIMER_A_LOWER: u8 = 0x25;
    /// Timer B's eight bits.
    pub const TIMER_B: u8 = 0x26;
    /// Bits 7–6 channel 3's mode, 5–4 flag reset B A, 3–2 enable B A, 1–0
    /// load B A.
    pub const MODE: u8 = 0x27;
    /// Key on: bits 7–4 operators 4–1, bits 1–0 the channel.
    pub const KEY: u8 = 0x28;
    /// The first prescaler select, set by the address alone.
    pub const PRESCALE_6: u8 = 0x2D;
    /// The second prescaler select, set by the address alone.
    pub const PRESCALE_3: u8 = 0x2E;
    /// Clears both prescaler selects, on the address alone.
    pub const PRESCALE_2: u8 = 0x2F;
    /// Per operator: bits 6–4 DT, 3–0 MUL.
    pub const DT_MUL: u8 = 0x30;
    /// Per operator: bits 6–0 total level.
    pub const TL: u8 = 0x40;
    /// Per operator: bits 7–6 KS, 4–0 AR.
    pub const KS_AR: u8 = 0x50;
    /// Per operator: bits 4–0 DR.
    pub const DR: u8 = 0x60;
    /// Per operator: bits 4–0 SR.
    pub const SR: u8 = 0x70;
    /// Per operator: bits 7–4 SL, 3–0 RR.
    pub const SL_RR: u8 = 0x80;
    /// Per operator: bits 3–0 SSG-EG, stored and withheld.
    pub const SSG_EG: u8 = 0x90;
    /// Per channel: the F-number's low eight bits.
    pub const FNUM_LOWER: u8 = 0xA0;
    /// Per channel: bits 5–3 block, 2–0 the F-number's top three.
    pub const BLOCK_FNUM_UPPER: u8 = 0xA4;
    /// Channel 3's supplementary low bytes, `A8`–`AA`.
    pub const SUPPLEMENTARY_FNUM_LOWER: u8 = 0xA8;
    /// Channel 3's supplementary block and high bits, `AC`–`AE`.
    pub const SUPPLEMENTARY_BLOCK_FNUM_UPPER: u8 = 0xAC;
    /// Per channel: bits 5–3 feedback, 2–0 connection.
    pub const FB_CONNECT: u8 = 0xB0;
}

/// The bits of the mode register `27H` (`YM2203-018`, `YM2203-027`).
pub mod mode {
    /// Timer A counts while set, reloaded as it is set.
    pub const LOAD_A: u8 = 0x01;
    /// Timer B counts while set, reloaded as it is set.
    pub const LOAD_B: u8 = 0x02;
    /// Timer A's overflow sets its flag.
    pub const ENABLE_A: u8 = 0x04;
    /// Timer B's overflow sets its flag.
    pub const ENABLE_B: u8 = 0x08;
    /// Written set, clears timer A's flag.
    pub const RESET_A: u8 = 0x10;
    /// Written set, clears timer B's flag.
    pub const RESET_B: u8 = 0x20;
    /// Channel 3's four frequencies.
    pub const SPECIAL: u8 = 0x40;
    /// Bit 7, the family's `CSM`: stored and withheld (`YM2203-OPEN-07`).
    pub const CSM: u8 = 0x80;
}

/// The bits of the status byte (`YM2203-004`).
pub mod status {
    /// Timer A overflowed.
    pub const TIMER_A: u8 = 0x01;
    /// Timer B overflowed.
    pub const TIMER_B: u8 = 0x02;
    /// A data write is in progress.
    pub const BUSY: u8 = 0x80;
}

/// The operators as the algorithm diagrams number them (`YM2203-013`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Operator {
    /// Operator 1, the one with the feedback path.
    Op1 = 0,
    /// Operator 2.
    Op2 = 1,
    /// Operator 3.
    Op3 = 2,
    /// Operator 4, a carrier under every algorithm.
    Op4 = 3,
}

impl Operator {
    /// All four, in diagram order.
    pub const ALL: [Operator; 4] = [Operator::Op1, Operator::Op2, Operator::Op3, Operator::Op4];

    /// The offset this operator's per-operator registers take on channel
    /// `ch`: `ch` plus `+0`, `+4`, `+8`, `+C` for operators 1, 3, 2, 4
    /// (`YM2203-011`).
    ///
    /// # Panics
    ///
    /// Panics if `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn slot_offset(self, ch: usize) -> u8 {
        assert!(ch < CHANNEL_COUNT, "channel out of range");
        ch as u8 + 4 * self.block()
    }

    /// The register block, `0` to `3`, of [`Operator::slot_offset`].
    const fn block(self) -> u8 {
        match self {
            Operator::Op1 => 0,
            Operator::Op2 => 2,
            Operator::Op3 => 1,
            Operator::Op4 => 3,
        }
    }

    /// The key-on bit in `28H` (`YM2203-012`).
    #[must_use]
    pub const fn key_bit(self) -> u8 {
        0x10 << (self as u8)
    }

    /// The supplementary low-byte register that sets this operator's
    /// frequency on channel 3 in special mode (`YM2203-018`); `None` for
    /// operator 4, which keeps channel 3's own.
    #[must_use]
    pub const fn supplementary(self) -> Option<u8> {
        match self {
            Operator::Op1 => Some(0xA9),
            Operator::Op2 => Some(0xAA),
            Operator::Op3 => Some(0xA8),
            Operator::Op4 => None,
        }
    }
}

/// The block and F-number a channel or operator runs at. The register
/// layout closes it, so it is exhaustive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fnum {
    /// The three-bit block (octave).
    pub block: u8,
    /// The F-number.
    pub fnum: u16,
}

use Operator::{Op1, Op2, Op3, Op4};
use connections::CONNECTIONS;

/// The pins the consumer shows the part on every cycle (`YM2203-002`,
/// `YM2203-003`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Inputs {
    /// Low selects the address, High the data.
    pub a0: Level,
    /// `D7`–`D0` as driven by the bus master; read on a write.
    pub d: u8,
    /// `/CS`: Low selects the part.
    pub cs_n: Level,
    /// `/WR`: Low with `/CS` Low is a write, taken when either rises.
    pub wr_n: Level,
    /// `/RD`: Low with `/CS` Low drives the data bus.
    pub rd_n: Level,
    /// `/IC`: Low holds the part in initial clear.
    pub ic_n: Level,
    /// What the outside drives on `IOA7`–`IOA0`. The pull-ups make an
    /// undriven pin read 1 (`YM2203-038`), so 0xFF is a port with nothing
    /// connected.
    pub ioa: u8,
    /// What the outside drives on `IOB7`–`IOB0`, as `ioa`.
    pub iob: u8,
}

impl Default for Inputs {
    fn default() -> Self {
        Self::idle()
    }
}

impl Inputs {
    /// The bus idle: nothing selected, `/IC` High, nothing on the ports.
    #[must_use]
    pub const fn idle() -> Self {
        Inputs {
            a0: Level::Low,
            d: 0,
            cs_n: Level::High,
            wr_n: Level::High,
            rd_n: Level::High,
            ic_n: Level::High,
            ioa: 0xFF,
            iob: 0xFF,
        }
    }

    /// A write cycle: the address when `a0` is Low, data when High.
    #[must_use]
    pub const fn write_access(a0: Level, d: u8) -> Self {
        Inputs {
            a0,
            d,
            cs_n: Level::Low,
            wr_n: Level::Low,
            ..Self::idle()
        }
    }

    /// A read cycle: the status when `a0` is Low, a register when High.
    #[must_use]
    pub const fn read_access(a0: Level) -> Self {
        Inputs {
            a0,
            cs_n: Level::Low,
            rd_n: Level::Low,
            ..Self::idle()
        }
    }

    /// The same pins with `/IC` at `ic_n`; Low holds the part in initial
    /// clear.
    #[must_use]
    pub const fn with_ic_n(self, ic_n: Level) -> Self {
        Inputs { ic_n, ..self }
    }

    /// The same pins with `IOA7`–`IOA0` driven to `ioa`.
    #[must_use]
    pub const fn with_ioa(self, ioa: u8) -> Self {
        Inputs { ioa, ..self }
    }

    /// The same pins with `IOB7`–`IOB0` driven to `iob`.
    #[must_use]
    pub const fn with_iob(self, iob: u8) -> Self {
        Inputs { iob, ..self }
    }
}

/// A 13-bit word for the YM3014: bits 9–0 the mantissa in offset binary
/// (bit 9 set for zero and above), bits 12–10 the exponent code `S`, 1 to 7,
/// the level being the mantissa times `2^(S − 1)` (`YM2203-026`,
/// `YM2203-OPEN-02`).
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

/// The level a word stands for, on the ±32768 scale.
const fn decode_serial(word: u16) -> i16 {
    let m = (word & 0x3FF) as i32 - 512;
    let s = ((word >> 10) & 7) as u32;
    let s = if s == 0 { 1 } else { s };
    (m << (s - 1)) as i16
}

/// One operator's state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Slot {
    phase: u32,
    envelope: Envelope,
    keyed: bool,
    /// The last two outputs, for the feedback path.
    out_prev: i16,
    out_before: i16,
    out: i16,
    increment: u32,
}

const SLOT: Slot = Slot {
    phase: 0,
    envelope: Envelope::new(),
    keyed: false,
    out_prev: 0,
    out_before: 0,
    out: 0,
    increment: 0,
};

/// The part.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ym2203 {
    /// Every register as written.
    regs: [u8; 256],
    /// The address last latched.
    address: u8,
    /// The two prescaler selects: `2DH` sets the first, `2EH` the second,
    /// `2FH` clears both (`YM2203-009`).
    select_6: bool,
    select_3: bool,
    /// The held high byte of `A4`–`A6` and of `AC`–`AE` (`YM2203-OPEN-13`).
    fnum_latch: u8,
    supp_latch: u8,
    /// Each channel's committed block and F-number: high byte, low byte.
    freq: [[u8; 2]; CHANNEL_COUNT],
    /// Channel 3's three supplementary frequencies, `A8`–`AA` order.
    supp: [[u8; 2]; 3],
    slots: [[Slot; OPERATOR_COUNT]; CHANNEL_COUNT],
    ssg: Ssg,
    fm_cycle: usize,
    ssg_cycle: usize,
    eg_divider: usize,
    /// Envelope generator clocks since the last initial clear.
    eg_steps: u64,
    timer_a: usize,
    timer_b: usize,
    timer_b_sub: usize,
    flag_a: bool,
    flag_b: bool,
    busy: usize,
    write_pending: Option<(Level, u8)>,
    d_out: Option<u8>,
    ports_in: [u8; 2],
    levels: [i16; CHANNEL_COUNT],
    accumulator: i32,
    word: u16,
    sample_ready: bool,
}

impl Default for Ym2203 {
    fn default() -> Self {
        Self::new()
    }
}

impl Ym2203 {
    /// The part as initial clear leaves it, the state a board reaches by
    /// holding `/IC` Low: every register zero (`YM2203-006`) and the
    /// prescaler at `2DH`'s (`YM2203-OPEN-08`).
    #[must_use]
    pub const fn new() -> Self {
        Ym2203 {
            regs: [0; 256],
            address: 0,
            select_6: true,
            select_3: false,
            fnum_latch: 0,
            supp_latch: 0,
            freq: [[0; 2]; CHANNEL_COUNT],
            supp: [[0; 2]; 3],
            slots: [[SLOT; OPERATOR_COUNT]; CHANNEL_COUNT],
            ssg: Ssg {
                tone: [ssg::ToneState {
                    counter: 0,
                    level: false,
                }; 3],
                noise: ssg::NoiseState {
                    counter: 0,
                    phase: false,
                    shift_register: 1,
                },
                envelope: ssg::EnvelopeState {
                    counter: 0,
                    shape: 0,
                    step: 31,
                    attack: false,
                    holding: false,
                },
            },
            fm_cycle: 0,
            ssg_cycle: 0,
            eg_divider: 0,
            eg_steps: 0,
            timer_a: 0,
            timer_b: 0,
            timer_b_sub: 0,
            flag_a: false,
            flag_b: false,
            busy: 0,
            write_pending: None,
            d_out: None,
            ports_in: [0xFF; 2],
            levels: [0; CHANNEL_COUNT],
            accumulator: 0,
            word: 512 | (1 << 10),
            sample_ready: false,
        }
    }

    /// Initial clear.
    fn initial_clear(&mut self) {
        *self = Self::new();
    }

    /// One cycle of `φM`.
    pub fn tick(&mut self, pins: Inputs) {
        self.sample_ready = false;
        if pins.ic_n.is_low() {
            self.initial_clear();
            return;
        }
        self.ports_in = [pins.ioa, pins.iob];
        if self.busy > 0 {
            self.busy -= 1;
        }
        if pins.cs_n.is_low() && pins.wr_n.is_low() {
            self.write_pending = Some((pins.a0, pins.d));
        } else if let Some((a0, d)) = self.write_pending.take() {
            self.write(a0, d);
        }
        self.d_out = if pins.cs_n.is_low() && pins.rd_n.is_low() {
            Some(if pins.a0.is_high() {
                self.data_read(self.address)
            } else {
                self.status()
            })
        } else {
            None
        };

        self.ssg_cycle += 1;
        if self.ssg_cycle >= self.ssg_cycles_per_edge().get() {
            self.ssg_cycle = 0;
            self.ssg.step(&self.regs[..16]);
        }
        if self.fm_cycle == 0 {
            self.compute_sample();
            self.sample_ready = true;
        }
        self.fm_cycle += 1;
        if self.fm_cycle >= FM_SAMPLE_CYCLES_PER_DIVIDER * self.fm_divider_cycles().get() {
            self.fm_cycle = 0;
        }
    }

    /// One bus write, as [`Ym2203::tick`] takes it when `/CS` and `/WR` stop
    /// being both Low, without the bus: with `a0` Low, `d` is latched as the
    /// register address, a prescaler select taking effect on it
    /// (`YM2203-009`) and no busy set (`YM2203-005`); with `a0` High, `d` is
    /// written to the latched address (`YM2203-003`) and busy is held for one
    /// sample (`YM2203-005`).
    pub fn write(&mut self, a0: Level, d: u8) {
        if a0.is_high() {
            self.write_data(d);
        } else {
            self.write_address(d);
        }
    }

    /// An address write alone.
    fn write_address(&mut self, address: u8) {
        self.address = address;
        match address {
            reg::PRESCALE_6 => self.select_6 = true,
            reg::PRESCALE_3 => self.select_3 = true,
            reg::PRESCALE_2 => {
                self.select_6 = false;
                self.select_3 = false;
            }
            _ => {}
        }
        let last = self.cycles_per_sample().get() - 1;
        self.fm_cycle = self.fm_cycle.min(last);
    }

    /// A data write to the latched address.
    fn write_data(&mut self, data: u8) {
        self.busy = FM_SAMPLE_CYCLES_PER_DIVIDER * self.fm_divider_cycles().get();
        let address = self.address;
        if address == reg::MODE {
            self.write_mode(data);
            return;
        }
        self.regs[address as usize] = data;
        match address {
            reg::ENVELOPE_SHAPE => self.ssg.envelope = ssg::EnvelopeState::triggered(data),
            reg::KEY => self.key(data),
            0xA4..=0xA6 => self.fnum_latch = data,
            0xAC..=0xAE => self.supp_latch = data,
            0xA0..=0xA2 => {
                self.freq[(address - 0xA0) as usize] = [self.fnum_latch, data];
            }
            0xA8..=0xAA => {
                self.supp[(address - 0xA8) as usize] = [self.supp_latch, data];
            }
            _ => {}
        }
    }

    /// `28H` (`YM2203-012`): channel codes 3 to 7 name no channel
    /// (`YM2203-OPEN-14`).
    fn key(&mut self, data: u8) {
        let ch = (data & 7) as usize;
        if ch >= CHANNEL_COUNT {
            return;
        }
        for op in Operator::ALL {
            let on = data & op.key_bit() != 0;
            let was = self.slots[ch][op as usize].keyed;
            if on && !was {
                let rates = self.rates(ch, op);
                let s = &mut self.slots[ch][op as usize];
                s.phase = 0;
                s.envelope.key_on(rates.attack);
            } else if !on && was {
                self.slots[ch][op as usize].envelope.key_off();
            }
            self.slots[ch][op as usize].keyed = on;
        }
    }

    /// `27H`'s timer bits (`YM2203-027`, `YM2203-029`); the mode bits are
    /// read where they are used.
    fn write_mode(&mut self, data: u8) {
        let was = self.regs[reg::MODE as usize];
        if data & mode::LOAD_A != 0 && was & mode::LOAD_A == 0 {
            self.timer_a = self.timer_a_period_cycles().get();
        }
        if data & mode::LOAD_B != 0 && was & mode::LOAD_B == 0 {
            self.timer_b = self.timer_b_period_cycles().get();
            self.timer_b_sub = TIMER_B_SAMPLES_PER_CYCLE;
        }
        if data & mode::RESET_A != 0 {
            self.flag_a = false;
        }
        if data & mode::RESET_B != 0 {
            self.flag_b = false;
        }
        self.regs[reg::MODE as usize] = data & !(mode::RESET_A | mode::RESET_B);
    }

    /// What the FM section divides the clock by, `n`: 6, 3 or 2 (`YM2203-009`).
    #[must_use]
    pub const fn fm_divider_cycles(&self) -> NonZeroUsize {
        match (self.select_6, self.select_3) {
            (true, false) => const { NonZeroUsize::new(6).unwrap() },
            (true, true) => const { NonZeroUsize::new(3).unwrap() },
            _ => const { NonZeroUsize::new(2).unwrap() },
        }
    }

    /// What the SSG divides the clock by, `m`: 4, 2 or 1 (`YM2203-009`).
    #[must_use]
    pub const fn ssg_divider_cycles(&self) -> NonZeroUsize {
        match (self.select_6, self.select_3) {
            (true, false) => const { NonZeroUsize::new(4).unwrap() },
            (true, true) => const { NonZeroUsize::new(2).unwrap() },
            _ => NonZeroUsize::MIN,
        }
    }

    /// `φM` cycles in one FM sample: `12 × n` (`YM2203-014`).
    #[must_use]
    pub const fn cycles_per_sample(&self) -> NonZeroUsize {
        self.fm_divider_cycles()
            .saturating_mul(const { NonZeroUsize::new(FM_SAMPLE_CYCLES_PER_DIVIDER).unwrap() })
    }

    /// `φM` cycles between the divided edges the SSG's generators step on:
    /// `4 × m`
    /// (`YM2203-032`).
    #[must_use]
    pub const fn ssg_cycles_per_edge(&self) -> NonZeroUsize {
        self.ssg_divider_cycles()
            .saturating_mul(const { NonZeroUsize::new(SSG_STEP_CYCLES_PER_DIVIDER).unwrap() })
    }

    /// Timer A's period in cycles of its clock, one a sample: `1024 − NA`
    /// (`YM2203-028`).
    #[must_use]
    pub const fn timer_a_period_cycles(&self) -> NonZeroUsize {
        let n = ((self.regs[reg::TIMER_A_UPPER as usize] as usize) << 2)
            | (self.regs[reg::TIMER_A_LOWER as usize] as usize & 3);
        NonZeroUsize::MIN.saturating_add(1023 - n)
    }

    /// Timer B's period in cycles of its clock, `256 − NB`, each
    /// [`TIMER_B_SAMPLES_PER_CYCLE`] samples (`YM2203-028`).
    #[must_use]
    pub const fn timer_b_period_cycles(&self) -> NonZeroUsize {
        NonZeroUsize::MIN.saturating_add((u8::MAX - self.regs[reg::TIMER_B as usize]) as usize)
    }

    /// The status byte: busy and the two flags, the other bits 0
    /// (`YM2203-004`).
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

    /// What a read with `A0` High returns at `address` (`YM2203-003`,
    /// `YM2203-038`, `YM2203-039`): the SSG's registers as written, a port
    /// set to output its latch and one set to input its pins, and 0 at any
    /// FM address.
    const fn data_read(&self, address: u8) -> u8 {
        match address {
            reg::PORT_A if self.regs[reg::MIXER as usize] & 0x40 == 0 => self.ports_in[0],
            reg::PORT_B if self.regs[reg::MIXER as usize] & 0x80 == 0 => self.ports_in[1],
            0x00..=0x0F => self.regs[address as usize],
            _ => 0,
        }
    }

    /// What the part drives on `IOA7`–`IOA0`, if port A is set to output
    /// (`YM2203-038`).
    #[must_use]
    pub const fn ioa(&self) -> Option<u8> {
        if self.regs[reg::MIXER as usize] & 0x40 != 0 {
            Some(self.regs[reg::PORT_A as usize])
        } else {
            None
        }
    }

    /// What the part drives on `IOB7`–`IOB0`, if port B is set to output
    /// (`YM2203-038`).
    #[must_use]
    pub const fn iob(&self) -> Option<u8> {
        if self.regs[reg::MIXER as usize] & 0x80 != 0 {
            Some(self.regs[reg::PORT_B as usize])
        } else {
            None
        }
    }

    /// Whether channel 3 runs its four frequencies (`YM2203-018`).
    #[must_use]
    pub const fn is_special_mode(&self) -> bool {
        self.regs[reg::MODE as usize] & mode::SPECIAL != 0
    }

    /// The block and F-number an operator runs at, as committed
    /// (`YM2203-011`, `YM2203-018`).
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn fnum(&self, ch: usize, op: Operator) -> Fnum {
        let [hi, lo] = if ch == 2 && self.is_special_mode() {
            match op.supplementary() {
                Some(r) => self.supp[(r - 0xA8) as usize],
                None => self.freq[2],
            }
        } else {
            self.freq[ch]
        };
        Fnum {
            block: (hi >> 3) & 7,
            fnum: (((hi & 7) as u16) << 8) | lo as u16,
        }
    }

    const fn op_reg(&self, ch: usize, op: Operator, base: u8) -> u8 {
        self.regs[(base + ch as u8 + 4 * op.block()) as usize]
    }

    const fn ch_reg(&self, ch: usize, base: u8) -> u8 {
        self.regs[(base + ch as u8) as usize]
    }

    /// An operator's key-scaled rates and `SL` (`YM2203-021`).
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub fn rates(&self, ch: usize, op: Operator) -> Rates {
        let Fnum { block, fnum } = self.fnum(ch, op);
        let kc = pg::key_code(block, fnum);
        let ks_ar = self.op_reg(ch, op, reg::KS_AR);
        let rks = eg::key_scale_offset(kc, ks_ar >> 6);
        let sl_rr = self.op_reg(ch, op, reg::SL_RR);
        Rates {
            attack: eg::rate(ks_ar & 0x1F, rks),
            decay: eg::rate(self.op_reg(ch, op, reg::DR) & 0x1F, rks),
            sustain: eg::rate(self.op_reg(ch, op, reg::SR) & 0x1F, rks),
            release: eg::rate(eg::release_r(sl_rr & 0xF), rks),
            sustain_level_steps: u32::from(eg::sustain_level(sl_rr >> 4)),
        }
    }

    /// An operator's phase increment from its registers as they stand
    /// (`YM2203-014` to `YM2203-017`).
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub fn increment_now(&self, ch: usize, op: Operator) -> u32 {
        let Fnum { block, fnum } = self.fnum(ch, op);
        let dt_mul = self.op_reg(ch, op, reg::DT_MUL);
        pg::increment(block, fnum, (dt_mul >> 4) & 7, dt_mul & 0xF)
    }

    fn compute_sample(&mut self) {
        let m = self.regs[reg::MODE as usize];
        if m & mode::LOAD_A != 0 {
            if self.timer_a <= 1 {
                self.timer_a = self.timer_a_period_cycles().get();
                if m & mode::ENABLE_A != 0 {
                    self.flag_a = true;
                }
            } else {
                self.timer_a -= 1;
            }
        }
        if m & mode::LOAD_B != 0 {
            self.timer_b_sub -= 1;
            if self.timer_b_sub == 0 {
                self.timer_b_sub = TIMER_B_SAMPLES_PER_CYCLE;
                if self.timer_b <= 1 {
                    self.timer_b = self.timer_b_period_cycles().get();
                    if m & mode::ENABLE_B != 0 {
                        self.flag_b = true;
                    }
                } else {
                    self.timer_b -= 1;
                }
            }
        }

        self.eg_divider += 1;
        if self.eg_divider == eg::SAMPLES_PER_STEP {
            self.eg_divider = 0;
            self.eg_steps += 1;
            for ch in 0..CHANNEL_COUNT {
                for op in Operator::ALL {
                    let rates = self.rates(ch, op);
                    self.slots[ch][op as usize]
                        .envelope
                        .tick(&rates, self.eg_steps as u32);
                }
            }
        }

        let mut accumulator: i32 = 0;
        for ch in 0..CHANNEL_COUNT {
            let fb_connect = self.ch_reg(ch, reg::FB_CONNECT);
            let alg = &CONNECTIONS[(fb_connect & 7) as usize];
            let fb = (fb_connect >> 3) & 7;

            let mut outs = [0i32; OPERATOR_COUNT];
            for op in Operator::ALL {
                let increment = self.increment_now(ch, op);
                let sources: &[Operator] = match op {
                    Op1 => &[],
                    Op2 => alg.op2_from,
                    Op3 => alg.op3_from,
                    Op4 => alg.op4_from,
                };
                let s = &self.slots[ch][op as usize];
                let modulation: i32 = if op == Op1 {
                    op::feedback_index(s.out_prev, s.out_before, fb)
                } else {
                    sources
                        .iter()
                        .map(|src| op::modulation_index(outs[*src as usize]))
                        .sum()
                };
                let tl = eg::total_level(self.op_reg(ch, op, reg::TL));
                let attenuation = s.envelope.attenuation_steps() + tl as u32;
                let index = ((s.phase >> pg::PHASE_FRACTION_BITS) as i32 + modulation) as u32;
                let out = op::operator_output(index, attenuation);
                outs[op as usize] = out as i32;

                let s = &mut self.slots[ch][op as usize];
                s.out_before = s.out_prev;
                s.out_prev = out;
                s.out = out;
                s.increment = increment;
                s.phase = (s.phase + increment) & (pg::PHASE_MODULUS - 1);
            }

            let sum: i32 = alg.carriers.iter().map(|c| outs[*c as usize]).sum();
            let level = sum.clamp(-CHANNEL_FULL_SCALE, CHANNEL_FULL_SCALE - 1) as i16;
            self.levels[ch] = level;
            accumulator += level as i32;
        }
        self.accumulator = accumulator;
        let word =
            i16::try_from(accumulator).unwrap_or(if accumulator < 0 { i16::MIN } else { i16::MAX });
        self.word = encode_serial(word);
    }

    /// What the data bus carries this cycle, if the part drives it.
    #[must_use]
    pub const fn d(&self) -> Option<u8> {
        self.d_out
    }

    /// `/IRQ`: Low while a timer's flag is set (`YM2203-030`). The pin is
    /// open drain; `High` is released, the pull-up's level.
    #[must_use]
    pub const fn irq_n(&self) -> Level {
        Level::from_bool(!(self.flag_a || self.flag_b))
    }

    /// Whether this cycle computed an FM sample.
    #[must_use]
    pub const fn is_sample_ready(&self) -> bool {
        self.sample_ready
    }

    /// The latest FM sample's 13-bit serial word for the YM3014
    /// (`YM2203-026`).
    #[must_use]
    pub const fn serial_word(&self) -> u16 {
        self.word
    }

    /// The latest FM sample, [`Ym2203::serial_word`] decoded to a level on a
    /// ±32768 scale: full scale is −32768 to 32767, and silence is 0. The SSG
    /// channels are not in it; they leave on their own pins
    /// ([`Ym2203::ssg_code`]). One sample stands for
    /// [`Ym2203::cycles_per_sample`] cycles of `φM`, so the sample rate moves
    /// when a prescaler select is written (`YM2203-009`).
    #[must_use]
    pub const fn output(&self) -> i32 {
        decode_serial(self.word) as i32
    }

    /// An SSG channel's five-bit converter code, 0 (A) to 2 (C)
    /// (`YM2203-035`).
    ///
    /// # Panics
    ///
    /// If `ch` is not below 3.
    #[must_use]
    pub fn ssg_code(&self, ch: usize) -> u8 {
        assert!(ch < 3, "the SSG has channels 0 to 2");
        self.ssg
            .code(ch, self.regs[reg::MIXER as usize], self.regs[8 + ch])
    }

    /// The part's sound now, one signed sample: [`Ym2203::output`] plus
    /// `ssg_levels[code]` for each SSG channel's [`Ym2203::ssg_code`].
    ///
    /// The unit is the FM half's decoded level, whose full scale is ±32768.
    /// The part leaves the ratio at which a board sums the SSG channels with
    /// the FM half to the board (`YM2203-037`), so `ssg_levels` is the
    /// caller's: each five-bit code's level on that scale. Each channel is
    /// measured from code 1's level, the code a reset leaves (fixed level 0,
    /// gate open or closed), so the part at rest reads 0 and an all-zero
    /// table gives the FM half alone. A table whose levels lie within 32768
    /// of code 1's keeps the sum within [`AUDIO_FULL_SCALE`], and any sum
    /// saturates at `i32`'s range. The FM half is its serial word as the
    /// YM3014 would take it, and the SSG its three pins' codes. The
    /// sample rate is the FM half's ([`Ym2203::cycles_per_sample`]).
    #[must_use]
    pub fn audio(&self, ssg_levels: &[Audio<AUDIO_FULL_SCALE>; 32]) -> Audio<AUDIO_FULL_SCALE> {
        let rest = i64::from(ssg_levels[1].0);
        let sum: i64 = (0..3)
            .map(|ch| i64::from(ssg_levels[self.ssg_code(ch) as usize].0) - rest)
            .sum::<i64>()
            + i64::from(self.output());
        Audio(sum.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
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

    /// The address last latched.
    #[must_use]
    pub const fn address(&self) -> u8 {
        self.address
    }

    /// The three FM channels summed before encoding.
    #[must_use]
    pub const fn accumulator(&self) -> i32 {
        self.accumulator
    }

    /// A channel's latest level, fourteen bits signed, −8192 to 8191
    /// (`YM2203-025`).
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn channel_audio(&self, ch: usize) -> Audio<CHANNEL_FULL_SCALE> {
        Audio(self.levels[ch] as i32)
    }

    /// An operator's envelope.
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn envelope(&self, ch: usize, op: Operator) -> Envelope {
        self.slots[ch][op as usize].envelope
    }

    /// The increment an operator last advanced by.
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn increment(&self, ch: usize, op: Operator) -> u32 {
        self.slots[ch][op as usize].increment
    }

    /// An operator's latest output, fourteen bits signed.
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn operator_output(&self, ch: usize, op: Operator) -> i32 {
        self.slots[ch][op as usize].out as i32
    }

    /// Whether an operator is keyed on.
    ///
    /// # Panics
    ///
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn is_keyed(&self, ch: usize, op: Operator) -> bool {
        self.slots[ch][op as usize].keyed
    }

    /// Timer A's count, in cycles of its clock to its overflow, one a sample.
    #[must_use]
    pub const fn timer_a_remaining_cycles(&self) -> usize {
        self.timer_a
    }

    /// Timer B's count, in cycles of its clock to its overflow, each
    /// [`TIMER_B_SAMPLES_PER_CYCLE`] samples.
    #[must_use]
    pub const fn timer_b_remaining_cycles(&self) -> usize {
        self.timer_b
    }

    /// `φM` cycles of busy left of the wait a write starts.
    #[must_use]
    pub const fn busy_remaining_cycles(&self) -> usize {
        self.busy
    }

    /// The `φM` cycle within the FM sample frame, 0 to one less than 12 ×
    /// [`fm_divider_cycles`](Self::fm_divider_cycles); the tick that finds it
    /// 0 computes a sample.
    #[must_use]
    pub const fn frame_cycle(&self) -> usize {
        self.fm_cycle
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    #[test]
    fn serial_codec() {
        assert_eq!(encode_serial(0), 512 | 1 << 10);
        assert_eq!(encode_serial(511), 1023 | 1 << 10);
        assert_eq!(encode_serial(-512), 1 << 10);
        assert_eq!(encode_serial(24_576) >> 10, 7);
        for v in [-24_576i16, -1000, -1, 0, 1, 1000, 24_575] {
            let back = decode_serial(encode_serial(v));
            assert!((back as i32 - v as i32).abs() < 64, "{v}: {back}");
        }
    }

    #[test]
    fn a_shorter_prescale_keeps_frame_cycle_in_range_and_the_next_sample_where_it_was() {
        let mut part = Ym2203::new();
        assert_eq!(part.fm_divider_cycles().get(), 6);
        for _ in 0..50 {
            part.tick(Inputs::idle());
        }
        assert_eq!(part.frame_cycle(), 50);
        part.write(Level::Low, reg::PRESCALE_2);
        assert_eq!(part.fm_divider_cycles().get(), 2);
        assert_eq!(
            part.frame_cycle(),
            23,
            "folded to the 24-cycle frame's last"
        );
        part.tick(Inputs::idle());
        assert!(!part.is_sample_ready());
        assert_eq!(part.frame_cycle(), 0);
        part.tick(Inputs::idle());
        assert!(
            part.is_sample_ready(),
            "the second tick computes the sample"
        );
        for _ in 0..100 {
            part.tick(Inputs::idle());
            assert!(part.frame_cycle() < 24);
        }
    }

    #[test]
    fn audio_rests_at_zero() {
        let levels = core::array::from_fn(|c| Audio(1000 * c as i32));
        assert_eq!(Ym2203::new().audio(&levels), Audio(0));
    }

    fn ssg_part(regs: &[(u8, u8)]) -> Ym2203 {
        let mut part = Ym2203::new();
        part.write(Level::Low, reg::MIXER);
        part.write(Level::High, 0x3F);
        for c in 0..3u8 {
            for r in 0..4u8 {
                part.write(Level::Low, reg::TL + c + 4 * r);
                part.write(Level::High, 0x7F);
                part.write(Level::Low, reg::SL_RR + c + 4 * r);
                part.write(Level::High, 0x0F);
            }
        }
        for &(r, v) in regs {
            part.write(Level::Low, r);
            part.write(Level::High, v);
        }
        part
    }

    fn run_cycles(part: &mut Ym2203, cycles: u64, mut each: impl FnMut(&Ym2203)) {
        for _ in 0..cycles {
            part.tick(Inputs::idle());
            each(part);
        }
    }

    #[test]
    fn noise_rate_and_recurrence() {
        const PHI_M_HZ: u64 = 3_000_000;
        const NULL_HZ_NP16: u64 = 5855;
        let mut part = ssg_part(&[(6, 16), (8, 15), (7, 0x37)]);
        let mut shifts = 0u64;
        let mut last = part.ssg.noise.shift_register;
        let n = 3_000_000u64;
        run_cycles(&mut part, n, |p| {
            if p.ssg.noise.shift_register != last {
                shifts += 1;
                last = p.ssg.noise.shift_register;
            }
        });
        let hz = shifts * PHI_M_HZ / n;
        assert!(hz.abs_diff(NULL_HZ_NP16) <= 30, "{hz}");
        let mut part = ssg_part(&[(6, 1), (8, 15), (7, 0x37)]);
        let mut bits = Vec::new();
        let mut last = part.ssg.noise.shift_register;
        run_cycles(&mut part, 32 * 3000, |p| {
            if p.ssg.noise.shift_register != last {
                last = p.ssg.noise.shift_register;
                bits.push(p.ssg.noise.level());
            }
        });
        assert!(bits.len() > 2900);
        assert!((17..bits.len()).all(|n| bits[n] == (bits[n - 14] ^ bits[n - 17])));
    }

    #[test]
    fn tone_starts_low() {
        let part = ssg_part(&[(8, 5), (7, 0x3E)]);
        assert!(!part.ssg.tone[0].level);
        assert_eq!(part.ssg_code(0), 1);
    }
}
