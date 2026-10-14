//! The Yamaha YM3526 (OPL), at its pins.
//!
//! Every item below names the claim it implements.
//!
//! **One [`Ym3526::tick`] is one cycle of `φM`** (`YM3526-007`). The
//! consumer shows the part its bus on every call — `A0`, `D7`–`D0`, `/CS`,
//! `/WR`, `/RD` and `/IC` — and reads back the data bus, `/IRQ` and the
//! output word. A write is taken when `/CS` and `/WR` stop being both Low;
//! `A0` Low is an address, High data (`YM3526-003`). [`Ym3526::write`] is
//! the same pair of writes without the bus. The part computes one sample
//! every [`CYCLES_PER_SAMPLE`] cycles (`YM3526-008`).
//!
//! **The output as the part presents it** (`YM3526-039`): one 13-bit word a
//! sample for the YM3014, [`Ym3526::serial_word`], a 10-bit offset-binary mantissa
//! under a 3-bit exponent, and [`Ym3526::output`], the same word decoded to
//! a level on a ±32768 scale, on which one operator at full level swings
//! ±8168. Zero is mantissa 200H under exponent 1.
//!
//! **Inside**: a phase generator per operator, an envelope generator per
//! operator ([`Envelope`]), one operator with a log-sin and an exponential
//! table, the two connections (`YM3526-011`), the feedback path
//! (`YM3526-023`), the AM and vibrato oscillators,
//! the rhythm voices (`YM3526-027` to `YM3526-032`), the two timers and
//! `/IRQ` (`YM3526-033` to `YM3526-036`).
//!
//! **Withheld**: `CSM` (`YM3526-037`, `YM3526-OPEN-07`) and the test
//! register (`YM3526-038`), stored and doing nothing; the serial word's
//! framing on `MO`, `SH` and `φSY` (`YM3526-OPEN-01`). **What the crate
//! defines** is each an open item: the exponent's sense (`YM3526-OPEN-02`);
//! the tables (`YM3526-OPEN-03`); the envelope's step pattern
//! (`YM3526-OPEN-04`); the output's scale and clamp (`YM3526-OPEN-05`); the
//! snare drum's, top cymbal's and hi-hat's waveforms and the noise register
//! (`YM3526-OPEN-06`); the oscillators' rates and where initial clear leaves
//! them (`YM3526-OPEN-08`); feedback doubling through 7 (`YM3526-OPEN-09`);
//! SL 15 at 93 dB (`YM3526-OPEN-10`); a key on over a sounding operator
//! (`YM3526-OPEN-11`); every write taken at once (`YM3526-OPEN-12`); `01H`
//! stored (`YM3526-OPEN-13`); vibrato's offset added before the block's shift
//! (`YM3526-OPEN-14`); a read with `A0` High returning `FFH`
//! (`YM3526-OPEN-15`); and no feedback on the tom-tom (`YM3526-OPEN-16`).
//!
//! # Examples
//!
//! Key channel 0 with its carrier at full level and run a hundred samples:
//!
//! ```
//! use ym3526::{AUDIO_FULL_SCALE, CYCLES_PER_SAMPLE, Inputs, Level, Operator, Ym3526, reg};
//!
//! let mut part = Ym3526::new();
//! let mut poke = |address: u8, data: u8| {
//!     part.write(Level::Low, address);
//!     part.write(Level::High, data);
//! };
//! let carrier = Operator::Op2.slot_offset(0);
//! poke(reg::AM_VIB_EGT_KSR_MUL + carrier, 0x21); // sustained, MUL 1
//! poke(reg::AR_DR + carrier, 0xF0); // AR 15
//! poke(reg::FNUM_LOWER, 0x41);
//! poke(reg::KON_BLOCK_FNUM_UPPER, 0x20 | 4 << 2 | 0x01); // key on, block 4
//! let mut peak = 0;
//! for _ in 0..100 * CYCLES_PER_SAMPLE {
//!     part.tick(Inputs::idle());
//!     peak = peak.max(part.audio().0.abs());
//! }
//! assert!(peak > 0 && peak <= AUDIO_FULL_SCALE);
//! ```

#![no_std]

#[cfg(test)]
extern crate std;

mod eg;
mod lfo;
mod op;
mod pg;

use core::num::NonZeroUsize;
pub use eg::{Envelope, EnvelopeStage, Rates};
use units::{Audio, Hertz};

/// How many pins the package presents (`YM3526-002`).
pub const PIN_COUNT: usize = 24;

/// Cycles of `φM` in one sample (`YM3526-008`).
pub const CYCLES_PER_SAMPLE: usize = 72;

/// The `φM` range the part is specified for, (min, max), in hertz
/// (`YM3526-007`).
pub const CLOCK_RANGE_HZ: (Hertz, Hertz) = (Hertz(2_000_000), Hertz(4_000_000));
/// The typical `φM`, 3.58 MHz, in hertz (`YM3526-007`).
pub const CLOCK_TYP_HZ: Hertz = Hertz(3_580_000);

/// Channels (`YM3526-001`).
pub const CHANNEL_COUNT: usize = 9;
/// Operators per channel (`YM3526-010`).
pub const OPERATOR_COUNT: usize = 2;

/// Samples in one cycle of timer 1's clock (`YM3526-034`).
pub const TIMER_1_SAMPLES_PER_CYCLE: usize = 4;
/// Samples in one cycle of timer 2's clock (`YM3526-034`).
pub const TIMER_2_SAMPLES_PER_CYCLE: usize = 16;

/// What a read with `A0` High returns (`YM3526-005`, `YM3526-OPEN-15`).
pub const READ_DATA: u8 = 0xFF;

/// A pin's logic level.
pub use signal::Level;

/// The largest magnitude [`Ym3526::audio`] returns: the decoded serial
/// level's −32768 (`YM3526-039`).
pub const AUDIO_FULL_SCALE: i32 = 32768;

/// The largest magnitude [`Ym3526::channel_audio`] returns: a rhythm
/// channel's two operators, each doubled, at the 14-bit operator output's
/// 8192 (`YM3526-028` to `YM3526-031`, `YM3526-OPEN-06`).
pub const CHANNEL_FULL_SCALE: i32 = 4 * 8192;

/// The register addresses (`YM3526-009`). Per-channel ones take `+ c`,
/// per-operator ones `+` the operator's offset ([`Operator::slot_offset`]).
pub mod reg {
    /// The LSI test register, "usually set to 0" (`YM3526-038`).
    pub const TEST: u8 = 0x01;
    /// Timer 1's eight bits, `N` of its period `256 − N`.
    pub const TIMER_1: u8 = 0x02;
    /// Timer 2's eight bits, `N` of its period `256 − N`.
    pub const TIMER_2: u8 = 0x03;
    /// Bit 7 resets the flags, 6–5 mask timers 1 and 2, 1–0 start 2 and 1.
    pub const CONTROL: u8 = 0x04;
    /// Bit 7 `CSM`, bit 6 `NOTE SEL`.
    pub const CSM_NOTE_SEL: u8 = 0x08;
    /// Per operator: bits 7–4 AM, VIB, EG-TYP, KSR; 3–0 MULTI.
    pub const AM_VIB_EGT_KSR_MUL: u8 = 0x20;
    /// Per operator: bits 7–6 KSL, 5–0 TL.
    pub const KSL_TL: u8 = 0x40;
    /// Per operator: bits 7–4 AR, 3–0 DR.
    pub const AR_DR: u8 = 0x60;
    /// Per operator: bits 7–4 SL, 3–0 RR.
    pub const SL_RR: u8 = 0x80;
    /// Per channel: the F-number's low eight bits.
    pub const FNUM_LOWER: u8 = 0xA0;
    /// Per channel: bit 5 KON, 4–2 block, 1–0 the F-number's top two.
    pub const KON_BLOCK_FNUM_UPPER: u8 = 0xB0;
    /// AM depth, vibrato depth, rhythm mode and the five rhythm keys.
    pub const RHYTHM: u8 = 0xBD;
    /// Per channel: bits 3–1 FB, bit 0 C.
    pub const FB_C: u8 = 0xC0;
}

/// The bits of `04H` (`YM3526-033`).
pub mod control {
    /// Timer 1 counts while set, loaded for a whole period as it is set.
    pub const START_1: u8 = 0x01;
    /// Timer 2 counts while set, loaded for a whole period as it is set.
    pub const START_2: u8 = 0x02;
    /// Masks timer 2's flag: written set, it clears the flag, and while it
    /// stands an overflow does not set it.
    pub const MASK_2: u8 = 0x20;
    /// Masks timer 1's flag, as [`MASK_2`] does timer 2's.
    pub const MASK_1: u8 = 0x40;
    /// Written set, clears both flags and does nothing else.
    pub const RESET: u8 = 0x80;
}

/// The bits of the status byte (`YM3526-004`).
pub mod status {
    /// What the status byte's low five bits always read (`YM3526-004`).
    pub const FIXED: u8 = 0x06;
    /// Timer 2 overflowed.
    pub const TIMER_2: u8 = 0x20;
    /// Timer 1 overflowed.
    pub const TIMER_1: u8 = 0x40;
    /// Either flag is set, which is when `/IRQ` is Low (`YM3526-036`).
    pub const IRQ: u8 = 0x80;
}

/// The bits of `BDH` (`YM3526-024`, `YM3526-027`).
pub mod rhythm {
    /// Keys the hi-hat.
    pub const HH: u8 = 0x01;
    /// Keys the top cymbal.
    pub const TC: u8 = 0x02;
    /// Keys the tom-tom.
    pub const TOM: u8 = 0x04;
    /// Keys the snare drum.
    pub const SD: u8 = 0x08;
    /// Keys the bass drum.
    pub const BD: u8 = 0x10;
    /// Rhythm mode: channels 6 to 8 (zero-based) sound the five rhythm voices.
    pub const MODE: u8 = 0x20;
    /// The deeper of the two vibrato depths.
    pub const VIB_DEEP: u8 = 0x40;
    /// The deeper of the two amplitude-modulation depths.
    pub const AM_DEEP: u8 = 0x80;
}

/// One of a channel's two operators (`YM3526-010`): the first, which
/// modulates the second under connection 0, and the second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Operator {
    /// The first operator, the modulator under connection 0.
    Op1 = 0,
    /// The second operator, the carrier.
    Op2 = 1,
}

impl Operator {
    /// Both operators, in order.
    pub const ALL: [Operator; 2] = [Operator::Op1, Operator::Op2];

    /// The offset this operator's per-operator registers take on channel
    /// `ch`: `(ch / 3) × 8 + ch % 3`, the second operator three above
    /// (`YM3526-010`).
    ///
    /// # Panics
    ///
    /// If `ch` is 9 or more.
    #[must_use]
    pub const fn slot_offset(self, ch: usize) -> u8 {
        assert!(ch < CHANNEL_COUNT, "no such channel");
        ((ch / 3) * 8 + ch % 3 + 3 * self as usize) as u8
    }
}

/// The log-sin index whose amplitude each of the snare drum, top cymbal
/// and hi-hat sounds at (`YM3526-031`, `YM3526-OPEN-06`).
pub(crate) const SD_INDEX: usize = 157;
pub(crate) const TC_INDEX: usize = 93;
pub(crate) const HH_INDEX: usize = 72;

/// The block and F-number a channel or operator runs at. The register
/// layout closes it, so it is exhaustive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fnum {
    /// The three-bit block (octave).
    pub block: u8,
    /// The F-number.
    pub fnum: u16,
}

/// The pins the consumer shows the part on every cycle (`YM3526-002`,
/// `YM3526-003`).
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

    /// A read cycle: the status when `a0` is Low.
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
}

/// A 13-bit word for the YM3014: bits 9–0 the mantissa in offset binary
/// (bit 9 set for zero and above), bits 12–10 the exponent code `S`, 1 to 7,
/// the level being the mantissa times `2^(S − 1)` (`YM3526-039`,
/// `YM3526-OPEN-02`).
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

/// The noise register's seed and its taps: 23 bits, the crate's
/// (`YM3526-OPEN-06`).
const NOISE_SEED: u32 = 1;
const NOISE_BITS: u32 = 23;

/// The part.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ym3526 {
    /// Every register as written.
    regs: [u8; 256],
    /// The address last latched.
    address: u8,
    slots: [[Slot; OPERATOR_COUNT]; CHANNEL_COUNT],
    cycle: usize,
    eg_steps: u64,
    am_step: u8,
    am_sub: u32,
    vib_step: u8,
    vib_sub: u32,
    noise: u32,
    timer_1: usize,
    timer_1_sub: usize,
    timer_2: usize,
    timer_2_sub: usize,
    flag_1: bool,
    flag_2: bool,
    write_pending: Option<(Level, u8)>,
    d_out: Option<u8>,
    levels: [i32; CHANNEL_COUNT],
    accumulator: i32,
    word: u16,
    sample_ready: bool,
}

impl Default for Ym3526 {
    fn default() -> Self {
        Self::new()
    }
}

impl Ym3526 {
    /// The part as initial clear leaves it, the state a board reaches by
    /// holding `/IC` Low: every register zero (`YM3526-006`), the oscillators at their first steps
    /// (`YM3526-OPEN-08`).
    #[must_use]
    pub const fn new() -> Self {
        Ym3526 {
            regs: [0; 256],
            address: 0,
            slots: [[SLOT; OPERATOR_COUNT]; CHANNEL_COUNT],
            cycle: 0,
            eg_steps: 0,
            am_step: 0,
            am_sub: 0,
            vib_step: 0,
            vib_sub: 0,
            noise: NOISE_SEED,
            timer_1: 0,
            timer_1_sub: 0,
            timer_2: 0,
            timer_2_sub: 0,
            flag_1: false,
            flag_2: false,
            write_pending: None,
            d_out: None,
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
        if pins.cs_n.is_low() && pins.wr_n.is_low() {
            self.write_pending = Some((pins.a0, pins.d));
        } else if let Some((a0, d)) = self.write_pending.take() {
            self.write(a0, d);
        }
        self.d_out = if pins.cs_n.is_low() && pins.rd_n.is_low() {
            Some(if pins.a0.is_high() {
                READ_DATA
            } else {
                self.status()
            })
        } else {
            None
        };
        if self.cycle == 0 {
            self.compute_sample();
            self.sample_ready = true;
        }
        self.cycle += 1;
        if self.cycle >= CYCLES_PER_SAMPLE {
            self.cycle = 0;
        }
    }

    /// One bus write, as [`Ym3526::tick`] takes it when `/CS` and `/WR` stop
    /// being both Low, without the bus: with `a0` Low, `d` is latched as the
    /// register address; with `a0` High, `d` is written to the latched
    /// address (`YM3526-003`). Every write is taken at once
    /// (`YM3526-OPEN-12`).
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
        if address == reg::CONTROL {
            self.write_control(data);
            return;
        }
        self.regs[address as usize] = data;
        if matches!(address, 0xB0..=0xB8 | reg::RHYTHM) {
            self.update_keys();
        }
    }

    /// `04H` (`YM3526-033`, `YM3526-035`): bit 7 resets both flags and the
    /// write does nothing else; otherwise a mask bit clears its flag and a
    /// start bit rising loads its timer for a whole period (`YM3526-034`).
    fn write_control(&mut self, data: u8) {
        if data & control::RESET != 0 {
            self.flag_1 = false;
            self.flag_2 = false;
            return;
        }
        let was = self.regs[reg::CONTROL as usize];
        if data & control::MASK_1 != 0 {
            self.flag_1 = false;
        }
        if data & control::MASK_2 != 0 {
            self.flag_2 = false;
        }
        if data & control::START_1 != 0 && was & control::START_1 == 0 {
            self.timer_1 = self.timer_1_period_cycles().get();
            self.timer_1_sub = TIMER_1_SAMPLES_PER_CYCLE;
        }
        if data & control::START_2 != 0 && was & control::START_2 == 0 {
            self.timer_2 = self.timer_2_period_cycles().get();
            self.timer_2_sub = TIMER_2_SAMPLES_PER_CYCLE;
        }
        self.regs[reg::CONTROL as usize] = data;
    }

    /// Whether the rhythm voices replace channels 6–8 (`YM3526-027`).
    #[must_use]
    pub const fn is_rhythm_mode(&self) -> bool {
        self.regs[reg::RHYTHM as usize] & rhythm::MODE != 0
    }

    /// The rhythm key bit that also keys an operator in rhythm mode: both of
    /// channel 6's for the bass drum, channel 7's first for the hi-hat and
    /// second for the snare drum, channel 8's first for the tom-tom and
    /// second for the top cymbal (`YM3526-029`); 0 for channels 0 to 5.
    const fn rhythm_key(ch: usize, op: usize) -> u8 {
        match (ch, op) {
            (6, _) => rhythm::BD,
            (7, 0) => rhythm::HH,
            (7, _) => rhythm::SD,
            (8, 0) => rhythm::TOM,
            (8, _) => rhythm::TC,
            _ => 0,
        }
    }

    /// Keys each operator on or off: a channel's `KON`, or in rhythm mode
    /// its rhythm key as well (`YM3526-012`, `YM3526-032`).
    fn update_keys(&mut self) {
        let bd = self.regs[reg::RHYTHM as usize];
        let rhythm = self.is_rhythm_mode();
        for ch in 0..CHANNEL_COUNT {
            let kon = self.regs[reg::KON_BLOCK_FNUM_UPPER as usize + ch] & 0x20 != 0;
            for op in 0..OPERATOR_COUNT {
                let on = kon || (rhythm && bd & Self::rhythm_key(ch, op) != 0);
                let was = self.slots[ch][op].keyed;
                if on && !was {
                    let rates = self.rates_at(ch, op);
                    let s = &mut self.slots[ch][op];
                    s.phase = 0;
                    s.envelope.key_on(rates.attack);
                } else if !on && was {
                    self.slots[ch][op].envelope.key_off();
                }
                self.slots[ch][op].keyed = on;
            }
        }
    }

    /// Timer 1's period in cycles of its clock, `256 − N`, each
    /// [`TIMER_1_SAMPLES_PER_CYCLE`] samples (`YM3526-034`).
    #[must_use]
    pub const fn timer_1_period_cycles(&self) -> NonZeroUsize {
        NonZeroUsize::MIN.saturating_add((u8::MAX - self.regs[reg::TIMER_1 as usize]) as usize)
    }

    /// Timer 2's period in cycles of its clock, `256 − N`, each
    /// [`TIMER_2_SAMPLES_PER_CYCLE`] samples (`YM3526-034`).
    #[must_use]
    pub const fn timer_2_period_cycles(&self) -> NonZeroUsize {
        NonZeroUsize::MIN.saturating_add((u8::MAX - self.regs[reg::TIMER_2 as usize]) as usize)
    }

    /// The status byte: `IRQ` and the two flags over the fixed bits
    /// (`YM3526-004`, `YM3526-036`).
    #[must_use]
    pub const fn status(&self) -> u8 {
        let mut s = status::FIXED;
        if self.flag_1 {
            s |= status::TIMER_1;
        }
        if self.flag_2 {
            s |= status::TIMER_2;
        }
        if self.flag_1 || self.flag_2 {
            s |= status::IRQ;
        }
        s
    }

    /// A channel's block and F-number (`YM3526-009`).
    ///
    /// # Panics
    ///
    /// If `ch` is 9 or more.
    #[must_use]
    pub const fn fnum(&self, ch: usize) -> Fnum {
        assert!(ch < CHANNEL_COUNT, "no such channel");
        let hi = self.regs[reg::KON_BLOCK_FNUM_UPPER as usize + ch];
        let lo = self.regs[reg::FNUM_LOWER as usize + ch];
        Fnum {
            block: (hi >> 2) & 7,
            fnum: (((hi & 3) as u16) << 8) | lo as u16,
        }
    }

    const fn op_reg(&self, ch: usize, op: usize, base: u8) -> u8 {
        self.regs[(base + ((ch / 3) * 8 + ch % 3 + 3 * op) as u8) as usize]
    }

    /// An operator's key-scaled rates and `SL` (`YM3526-018`, `YM3526-019`).
    ///
    /// # Panics
    ///
    /// If `ch` is 9 or more.
    #[must_use]
    pub fn rates(&self, ch: usize, op: Operator) -> Rates {
        assert!(ch < CHANNEL_COUNT, "no such channel");
        self.rates_at(ch, op as usize)
    }

    fn rates_at(&self, ch: usize, op: usize) -> Rates {
        let Fnum { block, fnum } = self.fnum(ch);
        let note_sel = self.regs[reg::CSM_NOTE_SEL as usize] & 0x40 != 0;
        let kc = pg::key_code(block, fnum, note_sel);
        let flags = self.op_reg(ch, op, reg::AM_VIB_EGT_KSR_MUL);
        let rks = eg::key_scale_offset(kc, flags & 0x10 != 0);
        let ar_dr = self.op_reg(ch, op, reg::AR_DR);
        let sl_rr = self.op_reg(ch, op, reg::SL_RR);
        let release = eg::rate(sl_rr & 0xF, rks);
        Rates {
            attack: eg::rate(ar_dr >> 4, rks),
            decay: eg::rate(ar_dr & 0xF, rks),
            sustain: if flags & 0x20 != 0 { 0 } else { release },
            release,
            sustain_level_steps: u32::from(eg::sustain_level(sl_rr >> 4)),
        }
    }

    /// An operator's attenuation this sample in steps of 0.09375 dB: its
    /// envelope, `TL`, `KSL` and AM (`YM3526-017`, `YM3526-021`,
    /// `YM3526-025`).
    ///
    /// # Panics
    ///
    /// If `ch` is 9 or more.
    #[must_use]
    pub fn total_attenuation_steps(&self, ch: usize, op: Operator) -> u32 {
        assert!(ch < CHANNEL_COUNT, "no such channel");
        self.attenuation_at(ch, op as usize)
    }

    fn attenuation_at(&self, ch: usize, op: usize) -> u32 {
        let Fnum { block, fnum } = self.fnum(ch);
        let ksl_tl = self.op_reg(ch, op, reg::KSL_TL);
        let flags = self.op_reg(ch, op, reg::AM_VIB_EGT_KSR_MUL);
        let am = if flags & 0x80 != 0 {
            let deep = self.regs[reg::RHYTHM as usize] & rhythm::AM_DEEP != 0;
            lfo::am_attenuation(self.am_step, deep)
        } else {
            0
        };
        let total = self.slots[ch][op].envelope.attenuation_steps()
            + eg::total_level(ksl_tl) as u32
            + eg::key_scale_level(block, fnum, ksl_tl >> 6) as u32
            + am as u32;
        total.min(op::ENVELOPE_MAX as u32)
    }

    /// An operator's phase increment this sample (`YM3526-013`,
    /// `YM3526-014`, `YM3526-026`).
    ///
    /// # Panics
    ///
    /// If `ch` is 9 or more.
    #[must_use]
    pub fn increment_now(&self, ch: usize, op: Operator) -> u32 {
        assert!(ch < CHANNEL_COUNT, "no such channel");
        self.increment_now_at(ch, op as usize)
    }

    fn increment_now_at(&self, ch: usize, op: usize) -> u32 {
        let Fnum { block, fnum } = self.fnum(ch);
        let flags = self.op_reg(ch, op, reg::AM_VIB_EGT_KSR_MUL);
        let vib = if flags & 0x40 != 0 {
            let deep = self.regs[reg::RHYTHM as usize] & rhythm::VIB_DEEP != 0;
            lfo::vib_offset(self.vib_step, fnum, deep)
        } else {
            0
        };
        pg::increment(block, fnum as i32 + vib, flags & 0xF)
    }

    fn compute_sample(&mut self) {
        let c = self.regs[reg::CONTROL as usize];
        if c & control::START_1 != 0 {
            self.timer_1_sub -= 1;
            if self.timer_1_sub == 0 {
                self.timer_1_sub = TIMER_1_SAMPLES_PER_CYCLE;
                if self.timer_1 <= 1 {
                    self.timer_1 = self.timer_1_period_cycles().get();
                    if c & control::MASK_1 == 0 {
                        self.flag_1 = true;
                    }
                } else {
                    self.timer_1 -= 1;
                }
            }
        }
        if c & control::START_2 != 0 {
            self.timer_2_sub -= 1;
            if self.timer_2_sub == 0 {
                self.timer_2_sub = TIMER_2_SAMPLES_PER_CYCLE;
                if self.timer_2 <= 1 {
                    self.timer_2 = self.timer_2_period_cycles().get();
                    if c & control::MASK_2 == 0 {
                        self.flag_2 = true;
                    }
                } else {
                    self.timer_2 -= 1;
                }
            }
        }

        self.am_sub += 1;
        if self.am_sub == lfo::AM_SAMPLES_PER_STEP {
            self.am_sub = 0;
            self.am_step = (self.am_step + 1) % lfo::AM_STEPS;
        }
        self.vib_sub += 1;
        if self.vib_sub == lfo::VIB_SAMPLES_PER_STEP {
            self.vib_sub = 0;
            self.vib_step = (self.vib_step + 1) % lfo::VIB_STEPS;
        }
        let feedback_bit = ((self.noise >> (NOISE_BITS - 1)) ^ (self.noise >> 17)) & 1;
        self.noise = ((self.noise << 1) | feedback_bit) & ((1 << NOISE_BITS) - 1);

        self.eg_steps += 1;
        for ch in 0..CHANNEL_COUNT {
            for op in 0..OPERATOR_COUNT {
                let rates = self.rates_at(ch, op);
                self.slots[ch][op]
                    .envelope
                    .tick(&rates, self.eg_steps as u32);
            }
        }

        let rhythm = self.is_rhythm_mode();
        let mut outs = [[0i16; OPERATOR_COUNT]; CHANNEL_COUNT];
        let mut indices = [[0u32; OPERATOR_COUNT]; CHANNEL_COUNT];
        for (ch, out) in outs.iter_mut().enumerate() {
            let fb_c = self.regs[reg::FB_C as usize + ch];
            let fb = (fb_c >> 1) & 7;
            let fm = fb_c & 1 == 0;
            for op in 0..OPERATOR_COUNT {
                let s = &self.slots[ch][op];
                let modulation = if op == 0 {
                    if rhythm && ch == 8 {
                        0
                    } else {
                        op::feedback_index(s.out_prev, s.out_before, fb)
                    }
                } else if fm {
                    op::modulation_index(out[0] as i32)
                } else {
                    0
                };
                let index = ((s.phase >> pg::PHASE_FRACTION_BITS) as i32 + modulation) as u32;
                indices[ch][op] = s.phase >> pg::PHASE_FRACTION_BITS;
                out[op] = op::operator_output(index, self.attenuation_at(ch, op));
            }
        }

        let mut accumulator: i32 = 0;
        for ch in 0..CHANNEL_COUNT {
            let fm = self.regs[reg::FB_C as usize + ch] & 1 == 0;
            let level = if rhythm && ch >= 6 {
                self.rhythm_level(ch, &outs, &indices)
            } else if fm {
                outs[ch][1] as i32
            } else {
                outs[ch][0] as i32 + outs[ch][1] as i32
            };
            self.levels[ch] = level;
            accumulator += level;
        }

        for (ch, out) in outs.iter().enumerate() {
            for (op, &o) in out.iter().enumerate() {
                let increment = self.increment_now_at(ch, op);
                let s = &mut self.slots[ch][op];
                if op == 0 {
                    s.out_before = s.out_prev;
                    s.out_prev = o;
                }
                s.out = o;
                s.increment = increment;
                s.phase = (s.phase + increment) & (pg::PHASE_MODULUS - 1);
            }
        }

        self.accumulator = accumulator;
        let clamped = accumulator.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        self.word = encode_serial(clamped);
    }

    /// A rhythm channel's level (`YM3526-028` to `YM3526-031`): the bass
    /// drum is channel 6's second operator, the first modulating it at C 0,
    /// at twice a melodic channel's level; the tom-tom is channel 8's first
    /// operator alone, doubled. The snare drum, top cymbal and hi-hat are
    /// the crate's (`YM3526-OPEN-06`): each a two-level wave at its own
    /// operator's attenuation — the snare drum a square at bit 8 of channel
    /// 7's first operator's phase, loud or soft by the noise bit; the hi-hat
    /// that square turned by the noise bit; the top cymbal that square
    /// turned by bit 7 of channel 8's second operator's phase.
    fn rhythm_level(
        &self,
        ch: usize,
        outs: &[[i16; OPERATOR_COUNT]; CHANNEL_COUNT],
        indices: &[[u32; OPERATOR_COUNT]; CHANNEL_COUNT],
    ) -> i32 {
        let square = (indices[7][0] >> 8) & 1 != 0;
        let noise = self.noise & 1 != 0;
        let voice = |op: usize, index: usize, positive: bool, doubled: bool| -> i32 {
            let m = op::amplitude(self.attenuation_at(ch, op), op::LOG_SIN[index]) as i32;
            let m = if doubled { 2 * m } else { m };
            if positive { m } else { -m }
        };
        match ch {
            6 => 2 * outs[6][1] as i32,
            7 => voice(0, HH_INDEX, square ^ noise, true) + voice(1, SD_INDEX, square, noise),
            _ => {
                let cymbal = (indices[8][1] >> 7) & 1 != 0;
                2 * outs[8][0] as i32 + voice(1, TC_INDEX, square ^ cymbal, true)
            }
        }
    }

    /// What the data bus carries this cycle, if the part drives it.
    #[must_use]
    pub const fn d(&self) -> Option<u8> {
        self.d_out
    }

    /// `/IRQ`: Low while a timer's flag is set (`YM3526-036`). The pin is
    /// open drain; `High` is released, the pull-up's level.
    #[must_use]
    pub const fn irq_n(&self) -> Level {
        Level::from_bool(!(self.flag_1 || self.flag_2))
    }

    /// Whether this cycle computed a sample.
    #[must_use]
    pub const fn is_sample_ready(&self) -> bool {
        self.sample_ready
    }

    /// The latest sample's 13-bit serial word for the YM3014 (`YM3526-039`).
    #[must_use]
    pub const fn serial_word(&self) -> u16 {
        self.word
    }

    /// The latest sample, the word decoded to a level on a scale whose full
    /// scale is ±32768, on which one operator at full level swings ±8168
    /// (`YM3526-039`).
    #[must_use]
    pub const fn output(&self) -> i32 {
        decode_serial(self.word) as i32
    }

    /// The latest sample as a signed level: [`Ym3526::output`] widened, the
    /// serial word as the YM3014 would take it, full scale
    /// [`AUDIO_FULL_SCALE`], one operator at full level ±8168, 0 at rest.
    /// Mono.
    #[must_use]
    pub const fn audio(&self) -> Audio<AUDIO_FULL_SCALE> {
        Audio(self.output())
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

    /// The nine channels summed before encoding.
    #[must_use]
    pub const fn accumulator(&self) -> i32 {
        self.accumulator
    }

    /// A channel's latest level, on the scale of [`Ym3526::accumulator`],
    /// within [`CHANNEL_FULL_SCALE`].
    ///
    /// # Panics
    ///
    /// If `ch` is 9 or more.
    #[must_use]
    pub const fn channel_audio(&self, ch: usize) -> Audio<CHANNEL_FULL_SCALE> {
        Audio(self.levels[ch])
    }

    /// An operator's envelope.
    ///
    /// # Panics
    ///
    /// If `ch` is 9 or more.
    #[must_use]
    pub const fn envelope(&self, ch: usize, op: Operator) -> Envelope {
        self.slots[ch][op as usize].envelope
    }

    /// Whether an operator is keyed on.
    ///
    /// # Panics
    ///
    /// If `ch` is 9 or more.
    #[must_use]
    pub const fn is_keyed(&self, ch: usize, op: Operator) -> bool {
        self.slots[ch][op as usize].keyed
    }

    /// An operator's latest output.
    ///
    /// # Panics
    ///
    /// If `ch` is 9 or more.
    #[must_use]
    pub const fn operator_output(&self, ch: usize, op: Operator) -> i32 {
        self.slots[ch][op as usize].out as i32
    }

    /// The increment an operator last advanced by.
    ///
    /// # Panics
    ///
    /// If `ch` is 9 or more.
    #[must_use]
    pub const fn increment(&self, ch: usize, op: Operator) -> u32 {
        self.slots[ch][op as usize].increment
    }

    /// The AM oscillator's phase: its step, 0 to 209.
    #[must_use]
    pub const fn am_phase(&self) -> u8 {
        self.am_step
    }

    /// The vibrato oscillator's phase: its step, 0 to 7.
    #[must_use]
    pub const fn vib_phase(&self) -> u8 {
        self.vib_step
    }

    /// Timer 1's count, in cycles of its clock to its overflow.
    #[must_use]
    pub const fn timer_1_remaining_cycles(&self) -> usize {
        self.timer_1
    }

    /// Timer 2's count, in cycles of its clock to its overflow.
    #[must_use]
    pub const fn timer_2_remaining_cycles(&self) -> usize {
        self.timer_2
    }

    /// The `φM` cycle within the sample frame, 0 to 71; the tick that finds it
    /// 0 computes a sample.
    #[must_use]
    pub const fn frame_cycle(&self) -> usize {
        self.cycle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_codec() {
        assert_eq!(encode_serial(0), 0x200 | 1 << 10);
        for v in [-32768i16, -8176, -1, 0, 1, 511, 512, 8176, 32767] {
            let back = decode_serial(encode_serial(v));
            let step = 1 << ((encode_serial(v) >> 10) - 1);
            assert!(
                (i32::from(back) - i32::from(v)).abs() < step,
                "{v} -> {back}"
            );
        }
    }
}
