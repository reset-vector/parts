//! The Yamaha YM2612 (OPN2), at its bus.
//!
//! Every item below names the claim it implements.
//!
//! **One [`Ym2612::tick`] is one cycle of `φM`**, 7,670,453 Hz on an NTSC
//! Mega Drive. The consumer shows the part its bus on every call — `A1`,
//! `A0`, `D7`–`D0`, `/CS`, `/WR`, `/RD`, `/IC` — and reads the data bus back.
//! `A1` picks Part I or Part II and `A0` the address or the data
//! (`YM2612-002`); a write is taken when `/CS` and `/WR` stop being both Low,
//! and a read drives the status byte (`YM2612-003`). [`Ym2612::write`] is the
//! same register write without the bus: the address latched, then the data.
//!
//! **One sample every [`CYCLES_PER_SAMPLE`] cycles** (`YM2612-012`): the
//! twenty-four operators are computed on the first cycle of the frame and
//! [`Ym2612::accumulator`] carries the sum of the enabled channels, left and
//! right, each channel a level of −512 to 511 on which one step of the DAC
//! register is four (`YM2612-033`, `YM2612-035`). Six channels at full scale
//! reach ±3072; nothing clips the sum. [`Ym2612::audio`] is the same pair.
//!
//! **Inside**: a phase generator per operator, an envelope generator per
//! operator ([`Envelope`]), one operator with a log-sin and an exponential
//! table, the eight algorithms (`YM2612-010`), the feedback path
//! (`YM2612-031`), the LFO ([`Lfo`], `YM2612-026` to `YM2612-029`),
//! channel 3's four frequencies (`YM2612-018`, `YM2612-019`), the DAC in
//! channel 6's place (`YM2612-034`, `YM2612-035`) and the two timers
//! (`YM2612-036` to `YM2612-038`).
//!
//! **Withheld**: SSG-EG (`YM2612-OPEN-06`) and CSM (`YM2612-OPEN-07`). Their
//! bits are stored and do nothing. **What the crate defines** is each an
//! open item: the bus pins and what initial clear resets (`YM2612-OPEN-01`);
//! the tables (`YM2612-OPEN-02`); busy for one sample, every write taken
//! (`YM2612-OPEN-03`); the envelope's step pattern (`YM2612-OPEN-04`); D1L 15
//! at 93 dB and AR 0 and D2R 0 holding (`YM2612-OPEN-05`); the output
//! converter (`YM2612-OPEN-08`); a channel held to fourteen bits before its
//! ten (`YM2612-OPEN-09`); feedback doubling through 7 (`YM2612-OPEN-10`); a
//! negative detune stopping at zero and key codes past 28
//! (`YM2612-OPEN-12`); the phase modulation's scale and the waves' steps
//! (`YM2612-OPEN-13`); a key on over a sounding operator (`YM2612-OPEN-15`);
//! the supplementary registers' held byte (`YM2612-OPEN-17`); and, over the
//! register manual where the two disagree, the operator blocks, key code and
//! channel 3 registers of `YM2612-008`, `YM2612-017` and `YM2612-019`
//! (`YM2612-OPEN-11`) and the LFO rates of `YM2612-027` (`YM2612-OPEN-16`).
//!
//! # Examples
//!
//! Key channel 1's first operator alone and run a hundred samples:
//!
//! ```
//! use ym2612::{AUDIO_FULL_SCALE, CYCLES_PER_SAMPLE, Inputs, Operator, Select, Ym2612, reg};
//!
//! let mut part = Ym2612::new();
//! let mut poke = |address: u8, data: u8| {
//!     // Part I: `A1` Low, the address with `A0` Low and the data with it High.
//!     part.write(Select::new(0b00), address);
//!     part.write(Select::new(0b01), data);
//! };
//! poke(reg::FB_CONNECT, 0x07); // algorithm 7
//! poke(reg::PAN_AMS_FMS, 0xC0); // both outputs
//! poke(reg::BLOCK_FNUM_UPPER, 4 << 3 | 0x02); // block 4, F-number's top bits
//! poke(reg::FNUM_LOWER, 0x69); // and its low byte, which commits both
//! poke(reg::DT_MUL, 0x01); // operator 1: MUL 1
//! poke(reg::KS_AR, 0x1F); // operator 1: AR 31
//! poke(reg::KEY, Operator::Op1.key_bit());
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
pub use pg::Lfo;
pub use signal::Level;
use units::{Audio, Hertz};

/// `φM` cycles in one sample (`YM2612-012`).
pub const CYCLES_PER_SAMPLE: usize = 144;

/// The typical `φM`, an NTSC Mega Drive's, in hertz: the 53.693175 MHz
/// master clock over seven. Nothing in the crate depends on it.
pub const CLOCK_TYP_HZ: Hertz = Hertz(7_670_453);

/// Channels (`YM2612-001`).
pub const CHANNEL_COUNT: usize = 6;
/// Operators per channel (`YM2612-001`).
pub const OPERATOR_COUNT: usize = 4;

/// `φM` cycles a data write keeps the busy flag up: one sample, the crate's
/// choice (`YM2612-OPEN-03`).
pub const BUSY_CYCLES: usize = CYCLES_PER_SAMPLE;

/// Samples in one cycle of timer B's clock: 288 µs against timer A's
/// 18 (`YM2612-036`).
pub const TIMER_B_SAMPLES_PER_CYCLE: usize = 16;

/// The largest magnitude of one channel's level (`YM2612-033`).
pub const CHANNEL_FULL_SCALE: i32 = 512;

/// The largest magnitude [`Ym2612::audio`] returns on either channel: six
/// channels at −512 (`YM2612-033`).
pub const AUDIO_FULL_SCALE: i32 = 6 * CHANNEL_FULL_SCALE;

/// The half of the register map `A1` selects (`YM2612-002`): Low is Part I,
/// the global registers and channels 1 to 3; High is Part II, channels 4 to 6.
const fn part_of(a1: Level) -> usize {
    a1.is_high() as usize
}

/// The register addresses (`YM2612-002`, `YM2612-007`). Per-channel ones take
/// `+ c` for the channel within its part, per-operator ones
/// [`Operator::slot_offset`]. The names are this part's sheet's but two,
/// which keep the YM2203's for the same fields: `KS_AR`, whose `KS` the
/// sheet calls `RS`, and `FB_CONNECT`, whose connection it calls the
/// algorithm.
pub mod reg {
    /// The LSI test register; nothing heard follows a write (`YM2612-039`).
    pub const TEST: u8 = 0x21;
    /// Bit 3 LFO enable, bits 2–0 its frequency.
    pub const LFO: u8 = 0x22;
    /// Timer A's top eight bits.
    pub const TIMER_A_UPPER: u8 = 0x24;
    /// Timer A's low two bits, in bits 1–0.
    pub const TIMER_A_LOWER: u8 = 0x25;
    /// Timer B's eight bits.
    pub const TIMER_B: u8 = 0x26;
    /// Bits 7–6 channel 3's mode, 5–4 flag reset B A, 3–2 flag enable B A,
    /// 1–0 load B A.
    pub const MODE: u8 = 0x27;
    /// Key on: bits 7–4 operators 4–1, bits 2–0 the channel.
    pub const KEY: u8 = 0x28;
    /// The DAC's byte.
    pub const DAC: u8 = 0x2A;
    /// Bit 7 enables the DAC in channel 6's place.
    pub const DAC_ENABLE: u8 = 0x2B;
    /// Per operator: bits 6–4 DT, 3–0 MUL.
    pub const DT_MUL: u8 = 0x30;
    /// Per operator: bits 6–0 total level.
    pub const TL: u8 = 0x40;
    /// Per operator: bits 7–6 RS, 4–0 AR.
    pub const KS_AR: u8 = 0x50;
    /// Per operator: bit 7 AM, 4–0 D1R.
    pub const AM_D1R: u8 = 0x60;
    /// Per operator: bits 4–0 D2R.
    pub const D2R: u8 = 0x70;
    /// Per operator: bits 7–4 D1L, 3–0 RR.
    pub const D1L_RR: u8 = 0x80;
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
    /// Per channel: bits 5–3 feedback, 2–0 algorithm.
    pub const FB_CONNECT: u8 = 0xB0;
    /// Per channel: bit 7 left, 6 right, 5–4 AMS, 2–0 FMS.
    pub const PAN_AMS_FMS: u8 = 0xB4;
}

/// The bits of the mode register `0x27` (`YM2612-018`, `YM2612-037`).
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
    /// The manual's illegal setting, CSM: stored and withheld.
    pub const CSM: u8 = 0x80;
}

/// The bits of the status byte (`YM2612-003`).
pub mod status {
    /// Timer A overflowed.
    pub const TIMER_A: u8 = 0x01;
    /// Timer B overflowed.
    pub const TIMER_B: u8 = 0x02;
    /// A data write is in progress.
    pub const BUSY: u8 = 0x80;
}

/// The operators as the algorithm diagrams number them (`YM2612-010`).
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
    /// `ch`, within the channel's part: `ch % 3` plus `+0`, `+4`, `+8`, `+C`
    /// for operators 1, 3, 2, 4 (`YM2612-008`).
    ///
    /// # Panics
    ///
    /// Panics if `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn slot_offset(self, ch: usize) -> u8 {
        assert!(ch < CHANNEL_COUNT, "channel out of range");
        (ch % 3) as u8 + 4 * self.block()
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

    /// The key-on bit in `0x28` (`YM2612-009`).
    #[must_use]
    pub const fn key_bit(self) -> u8 {
        0x10 << (self as u8)
    }

    /// The supplementary low-byte register that sets this operator's
    /// frequency on channel 3 in special mode (`YM2612-019`); `None` for
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

/// The levels of the two select pins (`YM2612-002`): `A1` in bit 1, Low
/// selecting Part I and High Part II, and `A0` in bit 0, Low selecting the
/// address and High the data. A select and a data byte are two types, so
/// [`Ym2612::write`] cannot take one for the other.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Select(u8);

impl Select {
    /// The pins at `levels`, `A1` in bit 1 and `A0` in bit 0; higher bits
    /// are dropped.
    #[must_use]
    pub const fn new(levels: u8) -> Self {
        Select(levels & 0b11)
    }

    /// The pins' levels, `A1` in bit 1 and `A0` in bit 0.
    #[must_use]
    pub const fn levels(self) -> u8 {
        self.0
    }
}

/// The bus pins the consumer shows the part on every cycle (`YM2612-002`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Inputs {
    /// `A1` in bit 1, Low selecting Part I and High Part II, and `A0` in
    /// bit 0, Low selecting the address and High the data; higher bits
    /// ignored.
    pub a: u8,
    /// `D7`–`D0` as driven by the bus master; read on a write.
    pub d: u8,
    /// `/CS`: Low selects the part.
    pub cs_n: Level,
    /// `/WR`: Low with `/CS` Low is a write, taken when either rises.
    pub wr_n: Level,
    /// `/RD`: Low with `/CS` Low drives the status byte on the data bus.
    pub rd_n: Level,
    /// `/IC`: Low holds the part in initial clear.
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
            a: 0,
            d: 0,
            cs_n: Level::High,
            wr_n: Level::High,
            rd_n: Level::High,
            ic_n: Level::High,
        }
    }

    /// A write cycle at `a`, as [`Ym2612::write`] takes it: to part `A1`,
    /// the address when `A0` is Low, data when High.
    #[must_use]
    pub const fn write_access(a: Select, d: u8) -> Self {
        Inputs {
            a: a.levels(),
            d,
            cs_n: Level::Low,
            wr_n: Level::Low,
            ..Self::idle()
        }
    }

    /// A read cycle at `a`. The part drives the status byte at any of the
    /// four selects (`YM2612-003`).
    #[must_use]
    pub const fn read_access(a: Select) -> Self {
        Inputs {
            a: a.levels(),
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
pub struct Ym2612 {
    /// Part I and Part II, as written.
    regs: [[u8; 256]; 2],
    /// The address each part last latched.
    address: [u8; 2],
    /// The held high byte of `A4`–`A6` and of `AC`–`AE` (`YM2612-006`,
    /// `YM2612-OPEN-17`).
    fnum_latch: u8,
    supp_latch: u8,
    /// Each channel's committed block and F-number: high byte, low byte.
    freq: [[u8; 2]; CHANNEL_COUNT],
    /// Channel 3's three supplementary frequencies, `A8`–`AA` order.
    supp: [[u8; 2]; 3],
    slots: [[Slot; OPERATOR_COUNT]; CHANNEL_COUNT],
    lfo: Lfo,
    cycle: usize,
    eg_divider: usize,
    eg_steps: u64,
    timer_a: usize,
    timer_b: usize,
    timer_b_sub: usize,
    flag_a: bool,
    flag_b: bool,
    busy: usize,
    write_pending: Option<(u8, u8)>,
    d_out: Option<u8>,
    levels: [i16; CHANNEL_COUNT],
    /// The latest sample's mix, `[left, right]`.
    mix: [i16; 2],
    sample_ready: bool,
}

impl Default for Ym2612 {
    fn default() -> Self {
        Self::new()
    }
}

impl Ym2612 {
    /// The part as initial clear leaves it, the state a board reaches by
    /// holding `/IC` Low: every register zero (`YM2612-OPEN-01`).
    #[must_use]
    pub const fn new() -> Self {
        Ym2612 {
            regs: [[0; 256]; 2],
            address: [0; 2],
            fnum_latch: 0,
            supp_latch: 0,
            freq: [[0; 2]; CHANNEL_COUNT],
            supp: [[0; 2]; 3],
            slots: [[SLOT; OPERATOR_COUNT]; CHANNEL_COUNT],
            lfo: Lfo::new(),
            cycle: 0,
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
            levels: [0; CHANNEL_COUNT],
            mix: [0, 0],
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
        if self.busy > 0 {
            self.busy -= 1;
        }
        if pins.cs_n.is_low() && pins.wr_n.is_low() {
            self.write_pending = Some((pins.a, pins.d));
        } else if let Some((a, d)) = self.write_pending.take() {
            self.write(Select::new(a), d);
        }
        self.d_out = if pins.cs_n.is_low() && pins.rd_n.is_low() {
            Some(self.status())
        } else {
            None
        };
        if self.cycle == 0 {
            self.compute_sample();
            self.sample_ready = true;
        }
        self.cycle = (self.cycle + 1) % CYCLES_PER_SAMPLE;
    }

    /// One bus write, as [`Ym2612::tick`] takes it when `/CS` and `/WR` stop
    /// being both Low, without the bus. `A1` Low is Part I, High Part II;
    /// with `A0` Low, `d` is latched as that part's register address, and
    /// with `A0` High it is written to the address that part last latched
    /// (`YM2612-002`).
    pub fn write(&mut self, a: Select, d: u8) {
        let a = a.levels();
        let part = part_of(Level::from_bool(a & 2 != 0));
        if a & 1 == 0 {
            self.address[part] = d;
        } else {
            self.write_register(part, self.address[part], d);
        }
    }

    fn write_register(&mut self, part: usize, address: u8, data: u8) {
        self.busy = BUSY_CYCLES;
        if part == 1 && address < 0x30 {
            self.regs[1][address as usize] = data;
            return;
        }
        if address == reg::MODE {
            self.write_mode(data);
            return;
        }
        self.regs[part][address as usize] = data;
        match address {
            reg::KEY => self.key(data),
            0xA4..=0xA6 => self.fnum_latch = data,
            0xAC..=0xAE => self.supp_latch = data,
            0xA0..=0xA2 => {
                let ch = 3 * part + (address - 0xA0) as usize;
                self.freq[ch] = [self.fnum_latch, data];
            }
            0xA8..=0xAA if part == 0 => {
                self.supp[(address - 0xA8) as usize] = [self.supp_latch, data];
            }
            _ => {}
        }
    }

    /// `0x28` (`YM2612-009`): channel codes 3 and 7 name no channel.
    fn key(&mut self, data: u8) {
        let code = data & 7;
        if code & 3 == 3 {
            return;
        }
        let ch = if code & 4 != 0 { 3 + (code & 3) } else { code } as usize;
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

    /// `0x27`'s timer bits (`YM2612-037`); the mode bits are read where
    /// they are used.
    fn write_mode(&mut self, data: u8) {
        let was = self.regs[0][reg::MODE as usize];
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
        self.regs[0][reg::MODE as usize] = data & !(mode::RESET_A | mode::RESET_B);
    }

    /// Timer A's period in cycles of its clock, one a sample: `1024 − N`
    /// (`YM2612-036`).
    #[must_use]
    pub const fn timer_a_period_cycles(&self) -> NonZeroUsize {
        let n = ((self.regs[0][reg::TIMER_A_UPPER as usize] as usize) << 2)
            | (self.regs[0][reg::TIMER_A_LOWER as usize] as usize & 3);
        NonZeroUsize::MIN.saturating_add(1023 - n)
    }

    /// Timer B's period in cycles of its clock, `256 − N`, each
    /// [`TIMER_B_SAMPLES_PER_CYCLE`] samples (`YM2612-036`).
    #[must_use]
    pub const fn timer_b_period_cycles(&self) -> NonZeroUsize {
        NonZeroUsize::MIN.saturating_add((u8::MAX - self.regs[0][reg::TIMER_B as usize]) as usize)
    }

    /// The status byte (`YM2612-003`).
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

    /// Whether channel 3 runs its four frequencies (`YM2612-018`).
    #[must_use]
    pub const fn is_special_mode(&self) -> bool {
        self.regs[0][reg::MODE as usize] & mode::SPECIAL != 0
    }

    /// Whether the DAC stands in channel 6's place (`YM2612-034`).
    #[must_use]
    pub const fn is_dac_enabled(&self) -> bool {
        self.regs[0][reg::DAC_ENABLE as usize] & 0x80 != 0
    }

    /// The block and F-number an operator runs at, as committed
    /// (`YM2612-006`, `YM2612-011`, `YM2612-019`).
    ///
    /// # Panics
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
        let part = ch / 3;
        let offset = (ch % 3) as u8 + 4 * op.block();
        self.regs[part][(base + offset) as usize]
    }

    const fn ch_reg(&self, ch: usize, base: u8) -> u8 {
        self.regs[ch / 3][(base + (ch % 3) as u8) as usize]
    }

    /// An operator's key-scaled rates and `D1L` (`YM2612-023`).
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub fn rates(&self, ch: usize, op: Operator) -> Rates {
        let Fnum { block, fnum } = self.fnum(ch, op);
        let kc = pg::key_code(block, fnum);
        let rs_ar = self.op_reg(ch, op, reg::KS_AR);
        let rks = eg::key_scale_offset(kc, rs_ar >> 6);
        let d1l_rr = self.op_reg(ch, op, reg::D1L_RR);
        Rates {
            attack: eg::rate(rs_ar & 0x1F, rks),
            decay1: eg::rate(self.op_reg(ch, op, reg::AM_D1R) & 0x1F, rks),
            decay2: eg::rate(self.op_reg(ch, op, reg::D2R) & 0x1F, rks),
            release: eg::rate(eg::release_r(d1l_rr & 0xF), rks),
            decay1_level_steps: u32::from(eg::first_decay_level(d1l_rr >> 4)),
        }
    }

    /// An operator's phase increment now, under the LFO's present phase
    /// modulation (`YM2612-012` to `YM2612-017`, `YM2612-028`).
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub fn increment_now(&self, ch: usize, op: Operator) -> u32 {
        let Fnum { block, fnum } = self.fnum(ch, op);
        let kc = pg::key_code(block, fnum);
        let fms = self.ch_reg(ch, reg::PAN_AMS_FMS) & 7;
        let fnum = pg::modulated_fnum(fnum, self.lfo.pm_waveform_steps(), fms);
        let dt_mul = self.op_reg(ch, op, reg::DT_MUL);
        pg::increment(block, fnum, kc, (dt_mul >> 4) & 7, dt_mul & 0xF)
    }

    fn compute_sample(&mut self) {
        let m = self.regs[0][reg::MODE as usize];
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

        self.lfo.advance(self.regs[0][reg::LFO as usize]);
        let am_waveform = self.lfo.am_waveform_steps();

        self.eg_divider += 1;
        let eg_tick = self.eg_divider == eg::SAMPLES_PER_STEP;
        if eg_tick {
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

        let mut left: i32 = 0;
        let mut right: i32 = 0;
        for ch in 0..CHANNEL_COUNT {
            let fb_alg = self.ch_reg(ch, reg::FB_CONNECT);
            let alg = &CONNECTIONS[(fb_alg & 7) as usize];
            let fb = (fb_alg >> 3) & 7;
            let pan = self.ch_reg(ch, reg::PAN_AMS_FMS);
            let am = pg::am_attenuation(am_waveform, (pan >> 4) & 3);

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
                let am_on = self.op_reg(ch, op, reg::AM_D1R) & 0x80 != 0;
                let attenuation =
                    s.envelope.attenuation_steps() + tl as u32 + if am_on { am } else { 0 };
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
            let mut level = (sum.clamp(-8192, 8191) >> 4) as i16;
            if ch == 5 && self.is_dac_enabled() {
                level = dac_level(self.regs[0][reg::DAC as usize]);
            }
            self.levels[ch] = level;
            if pan & 0x80 != 0 {
                left += level as i32;
            }
            if pan & 0x40 != 0 {
                right += level as i32;
            }
        }
        self.mix = [left as i16, right as i16];
    }

    /// What the data bus carries this cycle, if the part drives it.
    #[must_use]
    pub const fn d(&self) -> Option<u8> {
        self.d_out
    }

    /// Whether this cycle began a frame and computed a sample.
    #[must_use]
    pub const fn is_sample_ready(&self) -> bool {
        self.sample_ready
    }

    /// The latest sample's accumulators, `[left, right]`: the enabled
    /// channels' levels summed (`YM2612-032`, `YM2612-033`). **On the channel
    /// scale**: one channel spans −512 to 511, on which one step of the DAC
    /// register is 4; six channels at full scale reach ±3072, and nothing
    /// clips the sum. This is not the ±32768 scale of a part that presents a
    /// floating-point serial word.
    #[must_use]
    pub const fn accumulator(&self) -> [i32; 2] {
        [self.mix[0] as i32, self.mix[1] as i32]
    }

    /// The latest sample as `[left, right]`: signed, at rest 0, on the
    /// channel scale, where one channel spans −512 to 511 and six at full
    /// scale reach ±3072 (`YM2612-033`), [`AUDIO_FULL_SCALE`]. The same
    /// levels as [`Ym2612::accumulator`]: the internal sum, since the crate models
    /// no output converter (`YM2612-OPEN-08`).
    #[must_use]
    pub const fn audio(&self) -> [Audio<AUDIO_FULL_SCALE>; 2] {
        [Audio(self.mix[0] as i32), Audio(self.mix[1] as i32)]
    }

    /// Register `i` as written, over both halves of the map as one file:
    /// `i` below 256 is Part I's address `i`, and `256 +` an address is Part
    /// II's, the half `A1` High selects.
    ///
    /// # Panics
    ///
    /// If `i` is not below 512.
    #[must_use]
    pub const fn register(&self, i: usize) -> u8 {
        self.regs[i >> 8][i & 0xFF]
    }

    /// The address the part `a1` selects last latched.
    #[must_use]
    pub const fn address(&self, a1: Level) -> u8 {
        self.address[part_of(a1)]
    }

    /// A channel's latest level, −512 to 511 (`YM2612-033`). With the DAC
    /// on, channel 6 is the DAC's level, which is −512 while `2AH` holds
    /// its reset value of zero (`YM2612-035`).
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn channel_audio(&self, ch: usize) -> Audio<CHANNEL_FULL_SCALE> {
        Audio(self.levels[ch] as i32)
    }

    /// An operator's envelope.
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn envelope(&self, ch: usize, op: Operator) -> Envelope {
        self.slots[ch][op as usize].envelope
    }

    /// The increment an operator last advanced by.
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn increment(&self, ch: usize, op: Operator) -> u32 {
        self.slots[ch][op as usize].increment
    }

    /// An operator's latest output, fourteen bits signed.
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn operator_output(&self, ch: usize, op: Operator) -> i32 {
        self.slots[ch][op as usize].out as i32
    }

    /// An operator's phase accumulator.
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn phase(&self, ch: usize, op: Operator) -> u32 {
        self.slots[ch][op as usize].phase
    }

    /// Whether an operator is keyed on.
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn is_keyed(&self, ch: usize, op: Operator) -> bool {
        self.slots[ch][op as usize].keyed
    }

    /// The LFO.
    #[must_use]
    pub const fn lfo(&self) -> &Lfo {
        &self.lfo
    }

    /// Timer A's count, in cycles of its clock to its overflow, one a sample.
    #[must_use]
    pub const fn timer_a_remaining_cycles(&self) -> usize {
        self.timer_a
    }

    /// Timer B's count, in cycles of its clock to its overflow.
    #[must_use]
    pub const fn timer_b_remaining_cycles(&self) -> usize {
        self.timer_b
    }

    /// `φM` cycles of busy left, of the [`BUSY_CYCLES`] a write starts.
    #[must_use]
    pub const fn busy_remaining_cycles(&self) -> usize {
        self.busy
    }

    /// The `φM` cycle within the sample frame, 0 to 143.
    #[must_use]
    pub const fn frame_cycle(&self) -> usize {
        self.cycle
    }
}

/// The DAC byte's level on a channel's scale: offset binary about `0x80`,
/// one step four (`YM2612-035`).
const fn dac_level(byte: u8) -> i16 {
    (byte as i16 - 128) << 2
}
