//! The Atari C012294 POKEY, at its pins — the audio half.
//!
//! Every item below names the claim it implements.
//!
//! **One [`Pokey::tick`] is one cycle of `Ø2`**, the 1.79 MHz machine clock
//! (`POKEY-005`). The consumer shows the part its bus pins on every call —
//! `A3`–`A0`, `D7`–`D0`, `/CS0`, `CS1`, `R/W` — and reads back the data bus,
//! `/IRQ` and the `AUDIO` level. Addresses are taken on the rising edge and a
//! write's data at the falling edge (`POKEY-003`), so a write lands at the end
//! of the cycle it is presented in and a read's data is on the bus during it.
//!
//! **The four channels** each have an 8-bit divide-by-N counter clocked from
//! 1.79 MHz, from the 64 kHz (÷28) or 15 kHz (÷114) audio clock, or from the
//! channel below it when linked (`POKEY-010`, `POKEY-011`). A counter that
//! underflows fires [`TIMER_PIPELINE_CYCLES`] cycles later, reloads then, and
//! at 1.79 MHz that makes the period `N + 4` unlinked and `N + 7` linked
//! (`POKEY-012`, `POKEY-014`). A fire clocks the channel's output stage
//! (`POKEY-025`, `POKEY-026`), and on channels 1, 2 and 4 asserts the timer's
//! `IRQST` bit the cycle after (`POKEY-017`, `POKEY-018`).
//!
//! **The three noise generators** are XNOR shift registers of 4, 5 and 17
//! (or 9) bits stepped every cycle (`POKEY-020`, `POKEY-021`), sampled by a
//! channel when its timer fires, each channel one cycle behind the last
//! (`POKEY-024`). `RANDOM` is eight bits of the long one, inverted
//! (`POKEY-022`).
//!
//! **The `AUDIO` pin** is four open-drain DACs in parallel;
//! [`Pokey::audio_pin_steps`] is the number of unit devices pulling it down, 0 to
//! [`AUDIO_PIN_FULL_STEPS`] (`POKEY-029`). A board's network turns that into a voltage.
//! [`Pokey::audio`] is the same count as a signed sample, rest 0, one-sided
//! from 0 down to `-`[`AUDIO_FULL_SCALE`].
//!
//! **What the crate defines**, each an open item: the power-up state — every
//! register zero, so the part starts in initialisation mode with `IRQST` all
//! ones (`POKEY-OPEN-01`); a linked pair's second reload at the high
//! channel's fire (`POKEY-OPEN-02`); `SKCTL`'s two-tone bit ignored
//! (`POKEY-OPEN-04`); reads of the pot, keyboard and serial registers return
//! `$FF` (`POKEY-OPEN-05`); the 5-bit generator's per-channel delay
//! (`POKEY-OPEN-06`); a clock-select change taking effect at the next cycle,
//! the count kept (`POKEY-OPEN-07`); `IRQEN` read the cycle after a fire, and
//! a write of 0 blocking a request in flight (`POKEY-OPEN-10`); a filter's
//! flip-flop captured on the cycle of the high channel's fire
//! (`POKEY-OPEN-11`); the NTSC clock, every period in cycles
//! (`POKEY-OPEN-12`); and `STIMER` leaving the filters' flip-flops as they
//! were (`POKEY-OPEN-13`).
//!
//! **Withheld**: the pot scan, the keyboard scan, the serial port and
//! two-tone mode (`POKEY-031` to `POKEY-033` describe them; nothing here runs
//! them).
//!
//! # Examples
//!
//! ```
//! use pokey::{Inputs, Pokey, Select, audc, reg, skctl};
//!
//! let mut pokey = Pokey::new();
//! // Out of initialisation, then channel 1 at volume 8 with its output forced on.
//! pokey.tick(Inputs::write_access(Select::new(reg::SKCTL), skctl::INIT_MASK));
//! pokey.tick(Inputs::write_access(Select::new(reg::AUDC1), audc::VOLUME_ONLY | 8));
//! assert!(!pokey.is_in_init());
//! assert_eq!(pokey.audio_pin_steps(), 8);
//! assert_eq!(pokey.audio().0, -8);
//! ```

#![no_std]

use units::{Audio, Hertz, Microvolts};

/// How many pins the package presents (`POKEY-002`).
pub const PIN_COUNT: usize = 40;

/// The machine clock at `Ø2`, NTSC: the sheet's nominal 1.78979 MHz
/// (`POKEY-005`). Every period here is in cycles, so a PAL machine changes
/// only this (`POKEY-OPEN-12`).
pub const CLOCK_TYP_HZ: Hertz = Hertz(1_789_790);

/// The audio channels.
pub const CHANNEL_COUNT: usize = 4;

/// What the 64 kHz audio clock's divider divides the machine clock by
/// (`POKEY-010`).
pub const CLOCK_64K_DIVIDER_CYCLES: usize = 28;

/// What the 15 kHz audio clock's divider divides the machine clock by
/// (`POKEY-010`).
pub const CLOCK_15K_DIVIDER_CYCLES: usize = 114;

/// Cycles from a counter's underflow to its fire and reload (`POKEY-012`).
pub const TIMER_PIPELINE_CYCLES: usize = 3;

/// An unlinked timer at 1.79 MHz fires every `AUDF + 4` cycles (`POKEY-012`).
pub const FAST_PERIOD_EXTRA_CYCLES: usize = 4;

/// A linked pair at 1.79 MHz fires every `AUDF16 + 7` cycles (`POKEY-014`).
pub const LINKED_PERIOD_EXTRA_CYCLES: usize = 7;

/// Cycles from a `STIMER` write to the reload it causes (`POKEY-015`).
pub const STIMER_RELOAD_DELAY_CYCLES: usize = 3;

/// Cycles from leaving initialisation to the 64 kHz divider's first pulse
/// (`POKEY-008`).
pub const CLOCK_64K_FIRST_PULSE_CYCLES: usize = 19;

/// Cycles from leaving initialisation to the 15 kHz divider's first pulse
/// (`POKEY-008`).
pub const CLOCK_15K_FIRST_PULSE_CYCLES: usize = 78;

/// The most unit devices the `AUDIO` pin can have on, its full scale in
/// steps of one unit device: four channels of fifteen (`POKEY-029`).
pub const AUDIO_PIN_FULL_STEPS: u32 = 60;

/// The largest magnitude [`Pokey::channel_audio`] returns: fifteen unit devices,
/// reached on the negative side only (`POKEY-028`).
pub const CHANNEL_FULL_SCALE: i32 = 15;

/// The largest magnitude [`Pokey::audio`] returns: [`AUDIO_PIN_FULL_STEPS`] unit
/// devices, reached on the negative side only.
pub const AUDIO_FULL_SCALE: i32 = AUDIO_PIN_FULL_STEPS as i32;

/// A pin's level, as a getter named for the pin returns it (`irq_n()`, `channel_level(ch)`).
pub use signal::Level;

/// One row of [`AUDIO_VOL_ROWS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AudioVolRow {
    /// The channel volume the row is stated at: one volume bit set, the
    /// unit devices it turns on, in the steps of one unit device
    /// [`AUDIO_PIN_FULL_STEPS`] counts.
    pub volume_steps: u32,
    /// The `AUDIO` pin's level, at most.
    pub max_uv: Microvolts,
}

/// The sheet's `V_OL` maxima at `AUDIO` with one volume bit of a channel on
/// alone, 10 kΩ to 4.75 V (`POKEY-029`).
pub const AUDIO_VOL_ROWS: [AudioVolRow; 4] = [
    AudioVolRow {
        volume_steps: 1,
        max_uv: Microvolts(4_200_000),
    },
    AudioVolRow {
        volume_steps: 2,
        max_uv: Microvolts(3_400_000),
    },
    AudioVolRow {
        volume_steps: 4,
        max_uv: Microvolts(2_100_000),
    },
    AudioVolRow {
        volume_steps: 8,
        max_uv: Microvolts(1_200_000),
    },
];

/// The sheet's `V_OH` minimum at `AUDIO` with every device off
/// (`POKEY-029`).
pub const AUDIO_VOH_MIN_UV: Microvolts = Microvolts(4_200_000);

/// The register offsets on `A3`–`A0` (`POKEY-004`).
pub mod reg {
    /// Write: audio frequency, channel 1.
    pub const AUDF1: u8 = 0x0;
    /// Write: audio control, channel 1.
    pub const AUDC1: u8 = 0x1;
    /// Write: audio frequency, channel 2.
    pub const AUDF2: u8 = 0x2;
    /// Write: audio control, channel 2.
    pub const AUDC2: u8 = 0x3;
    /// Write: audio frequency, channel 3.
    pub const AUDF3: u8 = 0x4;
    /// Write: audio control, channel 3.
    pub const AUDC3: u8 = 0x5;
    /// Write: audio frequency, channel 4.
    pub const AUDF4: u8 = 0x6;
    /// Write: audio control, channel 4.
    pub const AUDC4: u8 = 0x7;
    /// Write: audio control, shared.
    pub const AUDCTL: u8 = 0x8;
    /// Write: start timers (a strobe).
    pub const STIMER: u8 = 0x9;
    /// Write: reset `SKSTAT` (a strobe).
    pub const SKRES: u8 = 0xA;
    /// Write: start the pot scan (a strobe).
    pub const POTGO: u8 = 0xB;
    /// Write: serial output register.
    pub const SEROUT: u8 = 0xD;
    /// Write: IRQ interrupt enable.
    pub const IRQEN: u8 = 0xE;
    /// Write: serial port and keyboard control.
    pub const SKCTL: u8 = 0xF;

    /// Read: pot 0 to 7 at offsets 0 to 7.
    pub const POT0: u8 = 0x0;
    /// Read: the eight pot lines' states.
    pub const ALLPOT: u8 = 0x8;
    /// Read: keyboard code.
    pub const KBCODE: u8 = 0x9;
    /// Read: random number generator.
    pub const RANDOM: u8 = 0xA;
    /// Read: serial input register.
    pub const SERIN: u8 = 0xD;
    /// Read: IRQ interrupt status.
    pub const IRQST: u8 = 0xE;
    /// Read: serial port and keyboard status.
    pub const SKSTAT: u8 = 0xF;
}

/// The bits of `AUDCTL` (`POKEY-010`).
pub mod audctl {
    /// Bit 7: the 17-bit generator becomes 9 bits.
    pub const POLY9: u8 = 0x80;
    /// Bit 6: channel 1 clocked at 1.79 MHz.
    pub const CH1_FAST: u8 = 0x40;
    /// Bit 5: channel 3 clocked at 1.79 MHz.
    pub const CH3_FAST: u8 = 0x20;
    /// Bit 4: channel 2 clocked by channel 1.
    pub const LINK_12: u8 = 0x10;
    /// Bit 3: channel 4 clocked by channel 3.
    pub const LINK_34: u8 = 0x08;
    /// Bit 2: high-pass filter in channel 1, clocked by channel 3.
    pub const HP1: u8 = 0x04;
    /// Bit 1: high-pass filter in channel 2, clocked by channel 4.
    pub const HP2: u8 = 0x02;
    /// Bit 0: the audio clock is 15 kHz instead of 64 kHz.
    pub const CLOCK_15K: u8 = 0x01;
}

/// The bits of an `AUDC` register (`POKEY-025`).
pub mod audc {
    /// Bit 7: the 5-bit generator does not gate the clock.
    pub const NO_POLY5: u8 = 0x80;
    /// Bit 6: sample the 4-bit generator instead of the 17-bit one.
    pub const POLY4: u8 = 0x40;
    /// Bit 5: toggle on each clock instead of sampling a generator.
    pub const PURE_TONE: u8 = 0x20;
    /// Bit 4: the output is forced on; volume only.
    pub const VOLUME_ONLY: u8 = 0x10;
    /// Bits 3–0: the volume.
    pub const VOLUME_MASK: u8 = 0x0F;
}

/// The bits of `IRQEN` and `IRQST` (`POKEY-017`).
pub mod irq {
    /// Bit 0: timer 1.
    pub const TIMER1: u8 = 0x01;
    /// Bit 1: timer 2.
    pub const TIMER2: u8 = 0x02;
    /// Bit 2: timer 4.
    pub const TIMER4: u8 = 0x04;
    /// Bit 3: serial output transmission finished — not a latch.
    pub const SEROUT_DONE: u8 = 0x08;
    /// Bit 4: serial output data needed.
    pub const SEROUT_NEEDED: u8 = 0x10;
    /// Bit 5: serial input data ready.
    pub const SERIN_READY: u8 = 0x20;
    /// Bit 6: other key.
    pub const OTHER_KEY: u8 = 0x40;
    /// Bit 7: break key.
    pub const BREAK_KEY: u8 = 0x80;
}

/// The bits of `SKCTL` (`POKEY-009`).
pub mod skctl {
    /// Bit 0: enable keyboard debounce.
    pub const DEBOUNCE: u8 = 0x01;
    /// Bit 1: enable keyboard scan.
    pub const KEY_SCAN: u8 = 0x02;
    /// Bit 2: fast pot scan.
    pub const FAST_POT: u8 = 0x04;
    /// Bit 3: two-tone serial output.
    pub const TWO_TONE: u8 = 0x08;
    /// Bits 6–4: serial mode.
    pub const SERIAL_MODE_MASK: u8 = 0x70;
    /// Bit 7: force break.
    pub const FORCE_BREAK: u8 = 0x80;
    /// Bits 1 and 0 both clear: the initialisation state.
    pub const INIT_MASK: u8 = 0x03;
}

/// The levels of the select pins `A3`–`A0`, `A3` in bit 3 and `A0` in bit 0,
/// as [`Inputs::write_access`] and [`Inputs::read_access`] take them: one of
/// the offsets in [`reg`] (`POKEY-004`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Select(u8);

impl Select {
    /// The select at `levels`, masked to `A3`–`A0` (bits 3–0); the other
    /// bits are ignored.
    #[must_use]
    pub const fn new(levels: u8) -> Self {
        Self(levels & 0x0F)
    }

    /// The levels of `A3`–`A0`, in bits 3–0.
    #[must_use]
    pub const fn levels(self) -> u8 {
        self.0
    }
}

/// The bus pins the consumer shows the part on every cycle (`POKEY-003`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Inputs {
    /// `A3`–`A0`, pins 33 to 36; only the low four bits are read.
    pub a: u8,
    /// `D7`–`D0` as driven by the bus master; read on a write.
    pub d: u8,
    /// `/CS0`, pin 30: the part is selected while this is Low …
    pub cs0_n: Level,
    /// `CS1`, pin 31: … and this is High.
    pub cs1: Level,
    /// `R/W`, pin 32: High for a read, Low for a write.
    pub rw: Level,
}

impl Default for Inputs {
    /// [`Inputs::idle`].
    fn default() -> Self {
        Self::idle()
    }
}

impl Inputs {
    /// The bus idle: not selected, `/CS0` High, `CS1` Low, `R/W` High.
    #[must_use]
    pub const fn idle() -> Self {
        Inputs {
            a: 0,
            d: 0,
            cs0_n: Level::High,
            cs1: Level::Low,
            rw: Level::High,
        }
    }

    /// A write cycle to offset `a` with data `d`.
    #[must_use]
    pub const fn write_access(a: Select, d: u8) -> Self {
        Inputs {
            a: a.levels(),
            d,
            cs0_n: Level::Low,
            cs1: Level::High,
            rw: Level::Low,
        }
    }

    /// A read cycle from offset `a`.
    #[must_use]
    pub const fn read_access(a: Select) -> Self {
        Inputs {
            a: a.levels(),
            d: 0,
            cs0_n: Level::Low,
            cs1: Level::High,
            rw: Level::High,
        }
    }

    /// Whether both chip selects agree the part is addressed.
    pub(crate) const fn is_selected(self) -> bool {
        self.cs0_n.is_low() && self.cs1.is_high()
    }
}

/// The part.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Pokey {
    audf: [u8; CHANNEL_COUNT],
    audc: [u8; CHANNEL_COUNT],
    audctl: u8,
    skctl: u8,
    irqen: u8,
    irqst: u8,

    /// Cycles until the 64 kHz and 15 kHz dividers next pulse.
    div64: usize,
    div15: usize,
    /// The dividers' pulses, one cycle behind, as the timers see them.
    pulse64: bool,
    pulse15: bool,

    /// The three XNOR registers, bit 0 the output end.
    poly4: u8,
    poly5: u8,
    poly17: u32,
    /// The last few output bits of each, bit 0 the newest, so channel `k`
    /// reads bit `k`.
    hist4: u8,
    hist5: u8,
    hist17: u8,

    /// The four divide-by-N counters.
    count: [u8; CHANNEL_COUNT],
    /// Fires in flight: bit `k` set means the channel fires in `k` cycles.
    fire_pipe: [u8; CHANNEL_COUNT],
    /// `IRQST` bits to clear on the next cycle.
    irq_pending: u8,
    /// Cycles until a written `STIMER` reloads; 0 when none is pending.
    stimer_pending: usize,

    /// The channels' output flip-flops and the two high-pass flip-flops.
    out_ff: [bool; CHANNEL_COUNT],
    hp_ff: [bool; 2],
    /// Which channels fired on the last cycle.
    fired: [bool; CHANNEL_COUNT],

    /// What the data bus carried on the last cycle, if the part drove it.
    d_out: Option<u8>,
    /// The `AUDIO` level at the end of the last cycle.
    audio: u8,
}

impl Default for Pokey {
    fn default() -> Self {
        Self::new()
    }
}

impl Pokey {
    /// The part in its initialisation state. The part has no reset pin, and
    /// this crate's choice under `POKEY-OPEN-01` is every register zero:
    /// `SKCTL` says initialisation, the generators are held at zero, the audio
    /// clocks are held, and `IRQST` reads all ones, as `IRQEN` at zero locks
    /// it. A program that needs a known state writes `SKCTL` and `IRQEN`
    /// itself.
    #[must_use]
    pub const fn new() -> Self {
        Pokey {
            audf: [0; CHANNEL_COUNT],
            audc: [0; CHANNEL_COUNT],
            audctl: 0,
            skctl: 0,
            irqen: 0,
            irqst: 0xFF,
            div64: CLOCK_64K_FIRST_PULSE_CYCLES,
            div15: CLOCK_15K_FIRST_PULSE_CYCLES,
            pulse64: false,
            pulse15: false,
            poly4: 0,
            poly5: 0,
            poly17: 0,
            hist4: 0,
            hist5: 0xFF,
            hist17: 0,
            count: [0; CHANNEL_COUNT],
            fire_pipe: [0; CHANNEL_COUNT],
            irq_pending: 0,
            stimer_pending: 0,
            out_ff: [false; CHANNEL_COUNT],
            hp_ff: [true; 2],
            fired: [false; CHANNEL_COUNT],
            d_out: None,
            audio: 0,
        }
    }

    /// Whether `SKCTL` bits 1 and 0 are both clear (`POKEY-007`).
    #[must_use]
    pub const fn is_in_init(&self) -> bool {
        self.skctl & skctl::INIT_MASK == 0
    }

    /// What the part drove onto `D7`–`D0` during the last cycle: `Some` on
    /// a read cycle, `None` otherwise.
    #[must_use]
    pub const fn d(&self) -> Option<u8> {
        self.d_out
    }

    /// `/IRQ`, pin 29: [`Level::Low`] while pulled Low — while any `IRQST` bit is
    /// 0 (`POKEY-017`). Open drain; [`Level::High`] is released, not driven High.
    #[must_use]
    pub const fn irq_n(&self) -> Level {
        Level::from_bool(self.irqst == 0xFF)
    }

    /// The `AUDIO` pin, pin 37, in steps of one unit device pulling it down:
    /// the sum over the four channels of the channel's volume where its output
    /// is on, 0 to [`AUDIO_PIN_FULL_STEPS`] (`POKEY-029`).
    #[must_use]
    pub const fn audio_pin_steps(&self) -> u32 {
        self.audio as u32
    }

    /// [`Pokey::audio_pin_steps`] as a signed sample: the count of unit devices
    /// pulling `AUDIO` down, negated, since each one lowers the pin. One-sided:
    /// rest, with no device on, is 0, and the sample runs from there to
    /// `-`[`AUDIO_FULL_SCALE`], one unit device a step.
    #[must_use]
    pub const fn audio(&self) -> Audio<AUDIO_FULL_SCALE> {
        Audio(-(self.audio as i32))
    }

    /// The last value written to `AUDF` for channel `ch` (0 to 3).
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn audf(&self, ch: usize) -> u8 {
        self.audf[ch]
    }

    /// The last value written to `AUDC` for channel `ch` (0 to 3).
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn audc(&self, ch: usize) -> u8 {
        self.audc[ch]
    }

    /// The last value written to `AUDCTL`.
    #[must_use]
    pub const fn audctl(&self) -> u8 {
        self.audctl
    }

    /// The last value written to `SKCTL`.
    #[must_use]
    pub const fn skctl(&self) -> u8 {
        self.skctl
    }

    /// The last value written to `IRQEN`.
    #[must_use]
    pub const fn irqen(&self) -> u8 {
        self.irqen
    }

    /// What a read of `IRQST` returns now (`POKEY-017`).
    #[must_use]
    pub const fn irqst(&self) -> u8 {
        self.irqst
    }

    /// What a read of `RANDOM` returns now: the eight stages after the long
    /// generator's input stage, inverted (`POKEY-022`).
    #[must_use]
    pub const fn random(&self) -> u8 {
        !((self.poly17 >> 8) as u8)
    }

    /// Channel `ch`'s divide-by-N counter (0 to 3), the count it holds now.
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn channel_counter(&self, ch: usize) -> u8 {
        self.count[ch]
    }

    /// Whether channel `ch`'s timer fired on the last cycle — clocked its
    /// output stage, reloaded, and on channels 1, 2 and 4 raised its
    /// interrupt (`POKEY-012`).
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn has_fired(&self, ch: usize) -> bool {
        self.fired[ch]
    }

    /// Channel `ch`'s tone generator's output, its flip-flop (`POKEY-026`).
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn tone_level(&self, ch: usize) -> Level {
        Level::from_bool(self.out_ff[ch])
    }

    /// Channel `ch`'s output as it reaches its DAC: the flip-flop, through
    /// the high-pass exclusive-OR on channels 1 and 2, then the volume-only
    /// override (`POKEY-027`, `POKEY-028`).
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn channel_level(&self, ch: usize) -> Level {
        Level::from_bool({
            let mut out = self.out_ff[ch];
            if ch < 2 {
                out ^= self.hp_ff[ch];
            }
            out || self.audc[ch] & audc::VOLUME_ONLY != 0
        })
    }

    /// Channel `ch`'s contribution to [`Pokey::audio`]: the unit devices it
    /// turns on, its volume if its output is on, else 0 (`POKEY-028`),
    /// negated as `audio` is, so 0 to −[`CHANNEL_FULL_SCALE`].
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn channel_audio(&self, ch: usize) -> Audio<CHANNEL_FULL_SCALE> {
        Audio(-(self.channel_steps(ch) as i32))
    }

    /// Channel `ch`'s unit devices on, as [`Pokey::channel_audio`] counts them.
    const fn channel_steps(&self, ch: usize) -> u8 {
        if self.channel_level(ch).is_high() {
            self.audc[ch] & audc::VOLUME_MASK
        } else {
            0
        }
    }

    /// The 4-bit generator's output bit as channel `ch` sees it
    /// (`POKEY-020`, `POKEY-024`).
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn poly4_level(&self, ch: usize) -> Level {
        Level::from_bool({
            assert!(ch < CHANNEL_COUNT, "channel out of range");
            (self.hist4 >> ch) & 1 != 0
        })
    }

    /// The 5-bit generator's output bit as channel `ch` sees it
    /// (`POKEY-020`, `POKEY-024`, `POKEY-OPEN-06`).
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn poly5_level(&self, ch: usize) -> Level {
        Level::from_bool({
            assert!(ch < CHANNEL_COUNT, "channel out of range");
            (self.hist5 >> ch) & 1 != 0
        })
    }

    /// The 17-bit (or 9-bit) generator's output bit as channel `ch` sees it
    /// (`POKEY-021`, `POKEY-024`).
    ///
    /// # Panics
    /// If `ch` is not below [`CHANNEL_COUNT`].
    #[must_use]
    pub const fn poly17_level(&self, ch: usize) -> Level {
        Level::from_bool({
            assert!(ch < CHANNEL_COUNT, "channel out of range");
            (self.hist17 >> ch) & 1 != 0
        })
    }

    /// What a read of offset `a` returns now (`POKEY-004`, `POKEY-OPEN-05`).
    const fn read(&self, a: u8) -> u8 {
        match a & 0x0F {
            reg::RANDOM => self.random(),
            reg::IRQST => self.irqst,
            _ => 0xFF,
        }
    }

    /// Write `d` to offset `a` as the bus would at the end of a cycle
    /// (`POKEY-004`).
    fn write(&mut self, a: u8, d: u8) {
        match a & 0x0F {
            reg::AUDF1 => self.audf[0] = d,
            reg::AUDC1 => self.audc[0] = d,
            reg::AUDF2 => self.audf[1] = d,
            reg::AUDC2 => self.audc[1] = d,
            reg::AUDF3 => self.audf[2] = d,
            reg::AUDC3 => self.audc[2] = d,
            reg::AUDF4 => self.audf[3] = d,
            reg::AUDC4 => self.audc[3] = d,
            reg::AUDCTL => {
                self.audctl = d;
                if d & audctl::HP1 == 0 {
                    self.hp_ff[0] = true;
                }
                if d & audctl::HP2 == 0 {
                    self.hp_ff[1] = true;
                }
            }
            reg::STIMER => self.stimer_pending = STIMER_RELOAD_DELAY_CYCLES,
            reg::IRQEN => {
                self.irqen = d;
                self.irqst |= !d;
                self.irq_pending &= d;
            }
            reg::SKCTL => {
                let was_init = self.is_in_init();
                self.skctl = d;
                if was_init && !self.is_in_init() {
                    self.div64 = CLOCK_64K_FIRST_PULSE_CYCLES;
                    self.div15 = CLOCK_15K_FIRST_PULSE_CYCLES;
                    self.pulse64 = false;
                    self.pulse15 = false;
                }
            }
            _ => {}
        }
        self.audio = self.level();
    }

    /// One cycle of `Ø2` with the bus pins as given.
    pub fn tick(&mut self, pins: Inputs) {
        let selected = pins.is_selected();
        self.d_out = if selected && pins.rw.is_high() {
            Some(self.read(pins.a))
        } else {
            None
        };

        self.irqst &= !(self.irq_pending & self.irqen);
        self.irq_pending = 0;

        let pulse64 = self.pulse64;
        let pulse15 = self.pulse15;
        self.pulse64 = false;
        self.pulse15 = false;
        if self.is_in_init() {
            self.div64 = CLOCK_64K_FIRST_PULSE_CYCLES;
            self.div15 = CLOCK_15K_FIRST_PULSE_CYCLES;
            self.step_generators(true);
        } else {
            self.div64 -= 1;
            if self.div64 == 0 {
                self.div64 = CLOCK_64K_DIVIDER_CYCLES;
                self.pulse64 = true;
            }
            self.div15 -= 1;
            if self.div15 == 0 {
                self.div15 = CLOCK_15K_DIVIDER_CYCLES;
                self.pulse15 = true;
            }
            self.step_generators(false);
        }

        let mut fires = [false; CHANNEL_COUNT];
        for (fire, pipe) in fires.iter_mut().zip(self.fire_pipe.iter_mut()) {
            *fire = *pipe & 1 != 0;
            *pipe >>= 1;
        }

        let base = if self.audctl & audctl::CLOCK_15K != 0 {
            pulse15
        } else {
            pulse64
        };
        let link12 = self.audctl & audctl::LINK_12 != 0;
        let link34 = self.audctl & audctl::LINK_34 != 0;
        let clocks = [
            self.audctl & audctl::CH1_FAST != 0 || base,
            if link12 { fires[0] } else { base },
            self.audctl & audctl::CH3_FAST != 0 || base,
            if link34 { fires[2] } else { base },
        ];
        for (ch, clocked) in clocks.into_iter().enumerate() {
            if clocked {
                if self.count[ch] == 0 {
                    self.fire_pipe[ch] |= 1 << (TIMER_PIPELINE_CYCLES - 1);
                }
                self.count[ch] = self.count[ch].wrapping_sub(1);
            }
        }

        if self.stimer_pending > 0 {
            self.stimer_pending -= 1;
            if self.stimer_pending == 0 {
                self.count = self.audf;
                self.fire_pipe = [0; CHANNEL_COUNT];
                self.out_ff = [true; CHANNEL_COUNT];
                fires = [false; CHANNEL_COUNT];
            }
        }

        self.fired = fires;
        for (ch, fired) in fires.into_iter().enumerate() {
            if !fired {
                continue;
            }
            let low_of_pair = (ch == 0 && link12) || (ch == 2 && link34);
            let high_of_pair = (ch == 1 && link12) || (ch == 3 && link34);
            if high_of_pair {
                self.count[ch - 1] = self.audf[ch - 1];
                self.count[ch] = self.audf[ch];
            } else if !low_of_pair {
                self.count[ch] = self.audf[ch];
            }
            let c = self.audc[ch];
            let passes = c & audc::NO_POLY5 != 0 || self.poly5_level(ch).is_high();
            if passes {
                if c & audc::PURE_TONE != 0 {
                    self.out_ff[ch] = !self.out_ff[ch];
                } else if c & audc::POLY4 != 0 {
                    self.out_ff[ch] = self.poly4_level(ch).is_high();
                } else {
                    self.out_ff[ch] = self.poly17_level(ch).is_high();
                }
            }
            if ch == 2 && self.audctl & audctl::HP1 != 0 {
                self.hp_ff[0] = self.out_ff[0];
            }
            if ch == 3 && self.audctl & audctl::HP2 != 0 {
                self.hp_ff[1] = self.out_ff[1];
            }
            match ch {
                0 => self.irq_pending |= irq::TIMER1,
                1 => self.irq_pending |= irq::TIMER2,
                3 => self.irq_pending |= irq::TIMER4,
                _ => {}
            }
        }

        if selected && pins.rw.is_low() {
            self.write(pins.a, pins.d);
        }
        self.audio = self.level();
    }

    /// Advance the three generators one cycle (`POKEY-020`, `POKEY-021`,
    /// `POKEY-023`). In initialisation the feedback is replaced by a 0 at
    /// every input, so each register empties from its input end, and the
    /// 5-bit generator's gate is held at 1.
    fn step_generators(&mut self, init: bool) {
        let out4 = self.poly4 & 1;
        let new4 = if init {
            0
        } else {
            1 ^ (out4 ^ ((self.poly4 >> 1) & 1))
        };
        self.poly4 = (self.poly4 >> 1) | (new4 << 3);
        self.hist4 = (self.hist4 << 1) | out4;

        let out5 = self.poly5 & 1;
        let new5 = if init {
            0
        } else {
            1 ^ (out5 ^ ((self.poly5 >> 2) & 1))
        };
        self.poly5 = (self.poly5 >> 1) | (new5 << 4);
        self.hist5 = if init { 0xFF } else { (self.hist5 << 1) | out5 };

        let end = if self.audctl & audctl::POLY9 != 0 {
            8
        } else {
            0
        };
        let out17 = ((self.poly17 >> end) & 1) as u8;
        let tap = ((self.poly17 >> (end + 5)) & 1) as u8;
        let new17 = if init { 0 } else { 1 ^ out17 ^ tap };
        self.poly17 = (self.poly17 >> 1) | (u32::from(new17) << 16);
        self.hist17 = (self.hist17 << 1) | out17;
    }

    /// The `AUDIO` level from the four channels' outputs (`POKEY-029`).
    const fn level(&self) -> u8 {
        self.channel_steps(0)
            + self.channel_steps(1)
            + self.channel_steps(2)
            + self.channel_steps(3)
    }
}
